# SPDX-License-Identifier: AGPL-3.0-or-later
"""ScorerActor — trajectory scoring for eval pipelines.

Demonstrates: LLM-as-judge pattern, heuristic scoring, rubric evaluation.
"""

import json
from plexspaces import actor, state, init_handler, handler, host


@actor
class ScorerActor:
    """
    Scores agent trajectories against rubrics.

    Supports two scoring modes:
    - heuristic: rule-based scoring (fast, deterministic)
    - llm_judge: LLM-as-judge (expensive, flexible) — uses LLMGateway
    """

    actor_id: str = state(default="")
    total_scored: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv.put("svc:scorer", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="scorer")
        except Exception:
            pass
        host.info(f"ScorerActor init actor_id={self.actor_id}")

    @handler("score")
    def score(self, trajectory: dict = None, rubric = None) -> dict:
        """Score a trajectory against a rubric. Returns score 0.0–1.0."""
        if not trajectory:
            return {"error": "trajectory is required", "score": 0.0}
        if not rubric:
            rubric = {"type": "task_completion"}
        if isinstance(rubric, str):
            rubric = {"type": rubric}

        rubric_type = rubric.get("type", "task_completion")
        score = 0.0
        detail = ""

        if rubric_type == "task_completion":
            score, detail = self._score_task_completion(trajectory, rubric)
        elif rubric_type == "tool_use":
            score, detail = self._score_tool_use(trajectory, rubric)
        elif rubric_type == "efficiency":
            score, detail = self._score_efficiency(trajectory, rubric)
        elif rubric_type == "llm_judge":
            score, detail = self._score_llm_judge(trajectory, rubric)
        else:
            score, detail = self._score_task_completion(trajectory, rubric)

        self.total_scored += 1
        host.incr_counter("trajectories_scored", 1)

        return {
            "status": "ok",
            "trajectory_id": trajectory.get("trajectory_id", ""),
            "score": round(score, 3),
            "rubric_type": rubric_type,
            "detail": detail,
        }

    @handler("batch_score")
    def batch_score(self, trajectories: list = None, rubric: dict = None) -> dict:
        """Score multiple trajectories against the same rubric."""
        if not trajectories:
            return {"error": "trajectories is required", "scores": []}
        results = [self.score(t, rubric) for t in trajectories]
        scores = [r.get("score", 0.0) for r in results]
        return {
            "status": "ok",
            "scores": results,
            "mean_score": sum(scores) / max(len(scores), 1),
            "pass_rate": sum(1 for s in scores if s >= 0.8) / max(len(scores), 1),
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "total_scored": self.total_scored}

    # ------------------------------------------------------------------

    def _score_task_completion(self, traj: dict, rubric: dict) -> tuple:
        """Score based on outcome and expected keywords."""
        outcome = traj.get("outcome", "")
        steps = traj.get("steps", [])
        expected_keywords = rubric.get("expected_keywords", [])

        if outcome in ("success", "completed"):
            base_score = 0.7
        elif outcome == "budget_exceeded":
            base_score = 0.3
        elif outcome == "suspended":
            base_score = 0.5
        else:
            base_score = 0.4

        # Bonus for completing in fewer steps
        max_steps = rubric.get("max_steps", 20)
        step_count = len(steps)
        if step_count <= max_steps // 2:
            base_score = min(1.0, base_score + 0.15)

        # Check for expected keywords in final output
        all_outputs = json.dumps([s.get("output", "") for s in steps])
        keyword_matches = sum(1 for kw in expected_keywords if kw.lower() in all_outputs.lower())
        if expected_keywords:
            keyword_bonus = 0.15 * (keyword_matches / len(expected_keywords))
            base_score = min(1.0, base_score + keyword_bonus)

        detail = f"outcome={outcome} steps={step_count} keywords_matched={keyword_matches}/{len(expected_keywords)}"
        return base_score, detail

    def _score_tool_use(self, traj: dict, rubric: dict) -> tuple:
        """Score based on correct tool usage."""
        steps = traj.get("steps", [])
        tool_calls = [s for s in steps if s.get("kind") == "tool_call"]
        expected_tools = rubric.get("expected_tools", [])

        used_tools = {s.get("method", "").replace("tool:", "") for s in tool_calls}
        if not expected_tools:
            score = 0.8 if tool_calls else 0.4
        else:
            matches = len(set(expected_tools) & used_tools)
            score = matches / len(expected_tools)

        detail = f"tool_calls={len(tool_calls)} used_tools={list(used_tools)} expected={expected_tools}"
        return score, detail

    def _score_efficiency(self, traj: dict, rubric: dict) -> tuple:
        """Score based on token efficiency (fewer tokens = better)."""
        total_tokens = traj.get("total_input_tokens", 0) + traj.get("total_output_tokens", 0)
        budget = rubric.get("token_budget", 4096)

        if total_tokens == 0:
            return 0.5, "no token data"

        # Score inversely proportional to token usage
        efficiency = max(0.0, 1.0 - (total_tokens / budget))
        outcome = traj.get("outcome", "")
        if outcome != "success":
            efficiency *= 0.5  # Penalize failed runs

        detail = f"tokens={total_tokens} budget={budget} outcome={outcome}"
        return round(efficiency, 3), detail

    def _score_llm_judge(self, traj: dict, rubric: dict) -> tuple:
        """Use LLM-as-judge for flexible scoring."""
        llm_id = self._find_service("llm_gateway")
        if not llm_id:
            # Fall back to heuristic
            return self._score_task_completion(traj, rubric)

        criteria = rubric.get("criteria", "Did the agent successfully complete the task?")
        traj_summary = {
            "outcome": traj.get("outcome"),
            "step_count": len(traj.get("steps", [])),
            "total_tokens": traj.get("total_input_tokens", 0) + traj.get("total_output_tokens", 0),
        }

        prompt = f"""Rate this agent trajectory on a scale of 0.0 to 1.0.

Criteria: {criteria}

Trajectory summary: {json.dumps(traj_summary)}

Respond with ONLY a JSON object: {{"score": 0.0-1.0, "reasoning": "brief explanation"}}"""

        try:
            resp = host.ask(llm_id, "completion", {
                "messages": [{"role": "user", "content": prompt}]
            }, 15000)

            if resp and "error" not in resp:
                content = resp.get("response", {}).get("content", "")
                parsed = json.loads(content)
                return parsed.get("score", 0.5), parsed.get("reasoning", "")
        except Exception:
            pass

        return self._score_task_completion(traj, rubric)

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
