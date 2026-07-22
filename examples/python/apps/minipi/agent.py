# SPDX-License-Identifier: AGPL-3.0-or-later
"""AgentActor — OODA-loop agent with durable execution trace capture.

Demonstrates the harness layer: loop control, tool calling, token budget,
crash recovery via DurabilityFacet, and execution trace export via
ExecutionTraceFacet.

Harness = agent - model (everything except the LLM).
"""

import json
from plexspaces import workflow_actor, state, init_handler, run_handler, signal_handler, query_handler, host
from plexspaces.agent import AgentLoop, AgentConfig

_MAX_ITER = 10
_TOKEN_BUDGET = 4096


@workflow_actor
class AgentActor:
    """
    OODA-loop agent: Observe → Orient → Decide → Act.

    Durable: DurabilityFacet journals every step. Crash at step 7?
    Restart brings back all prior steps from journal — no re-burned tokens.

    Execution Trace: ExecutionTraceFacet captures ordered step sequence for eval,
    writes trace:{id} and trace_index:{actor_id} to KV on completion.
    """

    actor_id: str = state(default="")
    task: str = state(default="")
    iterations_done: int = state(default=0)
    total_tool_calls: int = state(default=0)
    eval_run_id: str = state(default="")
    scenario_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.eval_run_id = args.get("eval_run_id", "")
        self.scenario_id = args.get("scenario_id", "")
        try:
            host.kv.put("svc:agent_runner", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="agent_runner")
        except Exception:
            pass
        host.info(f"AgentActor init actor_id={self.actor_id} eval_run={self.eval_run_id}")

    @run_handler
    def run(self, task: str = "", eval_run_id: str = "", scenario_id: str = "") -> dict:
        """Main OODA loop — durable: each step is journaled."""
        # SDK may pass the full payload dict as `task` when run_fn == instance.run
        if isinstance(task, dict):
            payload = task
            task = payload.get("task", "")
            eval_run_id = eval_run_id or payload.get("eval_run_id", "")
            scenario_id = scenario_id or payload.get("scenario_id", "")
        if not task:
            return {"error": "task is required"}

        self.task = task
        if eval_run_id:
            self.eval_run_id = eval_run_id
        if scenario_id:
            self.scenario_id = scenario_id

        host.info(f"AgentActor starting task: {str(task)[:80]}")

        loop = AgentLoop(
            actor_id=self.actor_id or host.actor_id(),
            config=AgentConfig(max_iterations=_MAX_ITER, token_budget=_TOKEN_BUDGET),
            eval_run_id=self.eval_run_id,
            scenario_id=self.scenario_id,
        )

        # OODA loop — runs until done, budget exceeded, or suspended
        while not loop.iteration_limit_reached():
            if loop.budget_exceeded():
                host.incr_counter("agent_budget_exceeded", 1)
                traj = loop.finalize_trajectory("budget_exceeded", f"Token budget {_TOKEN_BUDGET} exceeded")
                return {"status": "budget_exceeded", "trajectory": traj}

            if loop.is_suspended:
                traj = loop.get_trajectory().to_dict()
                return {"status": "suspended", "trajectory": traj}

            # --- OBSERVE: fetch context, memory, environment ---
            observations = self._do_observe(loop, task)

            # --- ORIENT: analyze observations with LLM ---
            plan = self._do_orient(loop, observations)

            # --- DECIDE: pick next action ---
            action = self._do_decide(loop, plan)

            if action.get("done"):
                break

            # Check for approval-required actions (human-in-the-loop)
            if action.get("needs_approval"):
                loop.suspend(f"action_needs_approval:{action.get('tool_name', 'unknown')}")
                traj = loop.get_trajectory().to_dict()
                return {"status": "suspended", "trajectory": traj}

            # --- ACT: execute the chosen tool ---
            result = self._do_act(loop, action)
            self.total_tool_calls += 1
            self.iterations_done += 1

            host.incr_counter("agent_iterations", 1)
            loop.increment_iteration()

        traj = loop.finalize_trajectory("completed", f"Completed {self.iterations_done} iterations")
        self._export_trajectory(traj)
        host.incr_counter("agent_runs_completed", 1)
        return {"status": "success", "task": task, "iterations": self.iterations_done, "trajectory": traj}

    @signal_handler("resume")
    def on_resume(self, data: dict = None) -> None:
        """Resume after human-in-the-loop approval."""
        host.info(f"AgentActor resumed: {data}")

    @query_handler("execution_trace")
    def on_query_execution_trace(self) -> dict:
        """Live execution trace inspection mid-run (returns most recent KV trace)."""
        try:
            index_raw = host.kv.get(f"trace_index:{self.actor_id}")
            if index_raw:
                trace_ids = json.loads(index_raw)
                if trace_ids:
                    raw = host.kv.get(f"trace:{trace_ids[-1]}")
                    if raw:
                        return json.loads(raw)
        except Exception:
            pass
        return {"actor_id": self.actor_id, "steps": [], "outcome": "running"}

    @query_handler("status")
    def on_query_status(self) -> dict:
        return {
            "actor_id": self.actor_id,
            "task": self.task[:80] if self.task else "",
            "iterations_done": self.iterations_done,
            "total_tool_calls": self.total_tool_calls,
        }

    # ------------------------------------------------------------------
    # OODA implementation helpers

    def _do_observe(self, loop: AgentLoop, task: str) -> dict:
        """OBSERVE: gather context from memory and environment."""
        memory_key = f"agent_memory:{self.actor_id}"
        prior_context = {}
        try:
            raw = host.kv.get(memory_key)
            if raw:
                prior_context = json.loads(raw)
        except Exception:
            pass

        observations = {
            "task": task,
            "prior_context": prior_context,
            "iteration": self.iterations_done,
        }
        return loop.observe(observations)

    def _do_orient(self, loop: AgentLoop, observations: dict) -> dict:
        """ORIENT: analyze observations with LLM to build a plan."""
        llm_id = self._find_service("llm_gateway")
        if not llm_id:
            plan = {
                "analysis": f"Processing task: {observations.get('task', '')}",
                "next_tool": "calculator",
                "arguments": {"expression": observations.get('task', '1+1')},
                "done": False,
            }
        else:
            messages = [
                {"role": "system", "content": "You are a helpful agent. Analyze the task and decide what to do next."},
                {"role": "user", "content": f"Task: {observations.get('task', '')}\nIteration: {observations.get('iteration', 0)}"},
            ]
            resp = self._ask(llm_id, "completion", {"messages": messages}, timeout_ms=10000)
            if not resp or "error" in resp:
                plan = {"done": True, "result": "LLM unavailable"}
            else:
                response = resp.get("response", {})
                plan = {
                    "analysis": response.get("content", ""),
                    "next_tool": response.get("tool_name", "calculator"),
                    "arguments": response.get("arguments", {}),
                    "input_tokens": resp.get("input_tokens", 0),
                    "output_tokens": resp.get("output_tokens", 0),
                    "model": resp.get("model", ""),
                    "done": response.get("stop_reason") == "end_turn" and not response.get("tool_calls"),
                }

        return loop.orient(plan)

    def _do_decide(self, loop: AgentLoop, plan: dict) -> dict:
        """DECIDE: select the next concrete action."""
        action = {
            "tool_name": plan.get("next_tool", "calculator"),
            "arguments": plan.get("arguments", {}),
            "done": plan.get("done", False),
            "needs_approval": plan.get("needs_approval", False),
        }
        return loop.decide(action)

    def _do_act(self, loop: AgentLoop, action: dict) -> dict:
        """ACT: execute the tool call."""
        tool_name = action.get("tool_name", "")
        arguments = action.get("arguments", {})

        tool_id = self._find_service("tool_registry")
        if not tool_id:
            result = {"error": "tool_registry unavailable", "tool": tool_name}
        else:
            result = self._ask(tool_id, tool_name, arguments) or {}

        return loop.tool_call(
            name=tool_name,
            args=arguments,
            result=result,
            input_tokens=result.get("input_tokens", 0),
            output_tokens=result.get("output_tokens", 0),
        )

    def _export_trajectory(self, traj: dict) -> None:
        """Store completed trajectory in KV (ExecutionTraceFacet also exports its own trace)."""
        try:
            key = f"agent_trajectory:{traj.get('trajectory_id', '')}"
            host.kv.put(key, json.dumps(traj))

            # Update per-agent index for easy retrieval by eval runner
            index_key = f"agent_trajectory_index:{self.actor_id}"
            try:
                existing = json.loads(host.kv.get(index_key) or "[]")
            except Exception:
                existing = []
            existing.append(traj.get("trajectory_id", ""))
            host.kv.put(index_key, json.dumps(existing))
        except Exception as e:
            host.warn(f"Failed to export trajectory: {e}")

    def _find_service(self, service_type: str) -> str:
        """Discover service actor ID via object registry; falls back to peer ID on same node."""
        try:
            regs = host.registry.discover(None, object_category=service_type, limit=1)
            if regs:
                return regs[0]["object_id"]
        except Exception:
            pass
        idx = self.actor_id.find("//")
        if idx >= 0:
            return service_type + self.actor_id[idx:]
        return service_type

    def _ask(self, actor_id: str, op: str, payload: dict, timeout_ms: int = 5000) -> dict:
        """Send a request-reply message to another actor."""
        try:
            result = host.ask(actor_id, op, payload, timeout_ms)
            return result or {}
        except Exception as e:
            return {"error": str(e)}
