# SPDX-License-Identifier: AGPL-3.0-or-later
"""AgentActor — self-improving agent loop.

Conversation turn: restore KV history → compress if >75% → inject skills →
fetch tool schemas → LLM loop (max 8 iterations) → persist history →
evaluate for skill learning.
"""

import json
from plexspaces import actor, state, handler, init_handler, host
from helpers import registry_first, fire_audit, write_actor_info, ask, truncate_str

_MAX_ITER = 8
_MAX_HISTORY = 50
_TOKEN_BUDGET = 8192
_COMPRESS_THRESHOLD = 0.75


@actor
class AgentActor:
    """Self-improving agent: chat, tool calling, skill learning, cron execution."""

    system_prompt: str = state(default="You are Hermes, a self-improving AI assistant. You learn from experience, create reusable skills, and automate recurring tasks.")
    messages: list = state(default_factory=list)
    max_history: int = state(default=_MAX_HISTORY)
    max_iterations: int = state(default=_MAX_ITER)
    token_budget: int = state(default=_TOKEN_BUDGET)
    total_chats: int = state(default=0)
    total_tool_calls: int = state(default=0)
    skills_learned: int = state(default=0)
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        args = config.get("args", {})
        self.system_prompt = args.get("system_prompt", self.system_prompt)
        if args.get("max_iterations"):
            self.max_iterations = int(args["max_iterations"])
        if args.get("token_budget"):
            self.token_budget = int(args["token_budget"])
        host.process_groups.join("svc:agent")
        # Register in object registry for capability-aware discovery
        try:
            host.registry.register(
                ctx="",
                object_id=self.actor_id,
                object_type="actor",
                grpc_address="",
                object_category="agent",
                capabilities=["chat", "tool_use", "skill_learning", "cron_execution"],
            )
        except Exception:
            pass
        write_actor_info(self.actor_id, "hermes-agent", "Self-improving agent with skill learning and cron automation",
                         ["chat", "tool_use", "skill_learning", "cron_execution"])
        host.info(f"AgentActor init actor_id={self.actor_id}")

    @handler("chat")
    def chat(self, message: str = "", session_id: str = "") -> dict:
        if not message:
            return {"error": "message is required"}

        # Restore history from KV if session_id provided
        if session_id:
            raw = host.kv.get(f"session_history:{session_id}")
            if raw:
                try:
                    self.messages = json.loads(raw)
                except Exception:
                    pass

        self.messages.append({"role": "user", "content": message})

        # Compress if over budget
        if len(self.messages) > 10:
            self._maybe_compress(session_id)

        # Inject relevant skills
        skill_context = self._fetch_skill_context(message)

        # Get tool schemas
        tools = self._fetch_tool_schemas()

        final_response = ""
        tool_call_count = 0

        for i in range(self.max_iterations):
            llm_id, _ = registry_first("llm_gateway", fallback_group="svc:llm_gateway")
            if not llm_id:
                final_response = f"[no LLM gateway] Processed: {truncate_str(message, 60)}"
                break

            msgs_with_system = [{"role": "system", "content": self.system_prompt + skill_context}] + self.messages
            llm_resp = ask(llm_id, "completion", {"messages": msgs_with_system, "tools": tools}, 15000)
            if not llm_resp or "error" in llm_resp:
                final_response = "LLM unavailable, please try again"
                break

            response = llm_resp.get("response", {})
            stop_reason = response.get("stop_reason", "end_turn")
            content = response.get("content", "")
            tool_calls = response.get("tool_calls", [])

            assistant_msg: dict = {"role": "assistant", "content": content, "stop_reason": stop_reason}
            if tool_calls:
                assistant_msg["tool_calls"] = tool_calls
            self.messages.append(assistant_msg)

            if stop_reason == "end_turn":
                final_response = content
                break

            if stop_reason == "tool_use":
                for tc in tool_calls:
                    tc_name = tc.get("name", "")
                    tc_input = tc.get("input", {})
                    tc_id = tc.get("id", "")

                    # Guardrails check
                    if not self._check_guardrails(tc_name, tc_input):
                        self.messages.append({
                            "role": "tool",
                            "tool_call_id": tc_id,
                            "content": f"Tool {tc_name} was denied by guardrails policy.",
                        })
                        fire_audit("tool_denied", f"tool={tc_name} session={session_id}")
                        continue

                    # Execute tool
                    tool_id, _ = registry_first("tool_executor", fallback_group="svc:tools")
                    tool_output = {}
                    if not tool_id:
                        tool_output = {"error": "tool executor unavailable"}
                    else:
                        result = ask(tool_id, "execute", {"name": tc_name, "input": tc_input})
                        tool_output = result or {"error": "tool execution failed"}

                    self.messages.append({
                        "role": "tool",
                        "tool_call_id": tc_id,
                        "content": json.dumps(tool_output),
                    })
                    tool_call_count += 1
                    fire_audit("tool_called", f"tool={tc_name} session={session_id}")

                final_response = f"Completed {len(tool_calls)} tool call(s) in iteration {i + 1}"
            else:
                final_response = content
                break

        # Persist history
        if session_id and self.messages:
            try:
                host.kv.put(f"session_history:{session_id}", json.dumps(self.messages[-self.max_history:]))
            except Exception:
                pass

        # Trim in-memory history
        if len(self.messages) > self.max_history:
            self.messages = self.messages[-self.max_history:]

        self.total_chats += 1
        self.total_tool_calls += tool_call_count

        # Evaluate for skill learning
        if tool_call_count >= 3:
            self._maybe_learn_skill(session_id, tool_call_count)

        host.incr_counter("agent_chats", 1)
        fire_audit("agent_chat", f"session={session_id} tools={tool_call_count}")
        return {
            "status": "ok",
            "response": final_response,
            "session_id": session_id,
            "tool_calls": tool_call_count,
            "messages_count": len(self.messages),
        }

    @handler("process_cron")
    def process_cron(self, job_id: str = "", prompt: str = "") -> dict:
        if not job_id:
            return {"error": "job_id is required"}
        # Save and isolate conversation context
        saved_messages = list(self.messages)
        self.messages = []
        result = self.chat(message=prompt or f"Execute scheduled task: {job_id}", session_id=f"cron:{job_id}")
        self.messages = saved_messages

        import hashlib
        run_id = hashlib.md5(f"{job_id}{host.now_ms()}".encode()).hexdigest()[:8]
        fire_audit("cron_executed", f"job_id={job_id} run_id={run_id}")
        return {"status": "ok", "job_id": job_id, "run_id": run_id, "result": result.get("response", "")}

    @handler("get_history")
    def get_history(self) -> dict:
        return {"status": "ok", "messages": self.messages, "count": len(self.messages)}

    @handler("clear_history")
    def clear_history(self, session_id: str = "") -> dict:
        self.messages = []
        if session_id:
            host.kv.delete(f"session_history:{session_id}")
        return {"status": "ok"}

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {
            "status": "ok",
            "total_chats": self.total_chats,
            "total_tool_calls": self.total_tool_calls,
            "skills_learned": self.skills_learned,
            "messages_count": len(self.messages),
        }

    # ------------------------------------------------------------------

    def _fetch_skill_context(self, message: str) -> str:
        skill_id, _ = registry_first("skill_store", fallback_group="svc:skills")
        if not skill_id:
            return ""
        resp = ask(skill_id, "match_skills", {"query": message, "limit": 2})
        if not resp or not resp.get("skills"):
            return ""
        parts = ["\n\n--- Relevant skills from experience ---"]
        for s in resp["skills"][:2]:
            parts.append(f"Skill: {s.get('name', '')}\n{truncate_str(s.get('procedure', ''), 200)}")
        return "\n".join(parts)

    def _fetch_tool_schemas(self) -> list:
        tool_id, _ = registry_first("tool_executor", fallback_group="svc:tools")
        if not tool_id:
            return []
        resp = ask(tool_id, "list_tools", {})
        return resp.get("tools", []) if resp else []

    def _check_guardrails(self, tool_name: str, tool_input: dict) -> bool:
        guard_id, _ = registry_first("guardrails", fallback_group="svc:guardrails")
        if not guard_id:
            return True  # Allow if guardrails unavailable
        resp = ask(guard_id, "check", {"tool": tool_name, "input": tool_input})
        if not resp:
            return True
        decision = resp.get("decision", "allow")
        return decision == "allow"

    def _maybe_compress(self, session_id: str) -> None:
        comp_id, _ = registry_first("context_compressor", fallback_group="svc:compressor")
        if not comp_id:
            return
        resp = ask(comp_id, "compress", {
            "session_id": session_id or "default",
            "messages": json.dumps(self.messages),
            "keep_last": 4,
        })
        if resp and resp.get("compressed_messages"):
            try:
                self.messages = json.loads(resp["compressed_messages"])
            except Exception:
                pass

    def _maybe_learn_skill(self, session_id: str, tool_call_count: int) -> None:
        skill_id, _ = registry_first("skill_store", fallback_group="svc:skills")
        if not skill_id:
            return
        resp = ask(skill_id, "evaluate_for_learning", {
            "session_id": session_id or "default",
            "tool_call_count": tool_call_count,
            "messages": json.dumps(self.messages[-10:]),
        })
        if resp and resp.get("action") == "learned":
            self.skills_learned += 1
