# SPDX-License-Identifier: AGPL-3.0-or-later
"""RegressionDetectorActor — compare trajectories across eval runs.

Demonstrates: reading from TupleSpace blackboard, diff logic,
and the eval feedback loop (run → score → compare → diagnose → fix → rerun).
"""

import json
from plexspaces import actor, state, init_handler, handler, host


@actor
class RegressionDetectorActor:
    """
    Detects regressions by comparing trajectory scores across eval runs.

    The eval feedback loop:
    1. Run eval (EvalRunnerActor)
    2. Score trajectories (ScorerActor)
    3. Detect regressions (this actor) — flag scenarios that got worse
    4. Diagnose: inspect trajectories to understand WHY
    5. Fix harness config, policy, or prompt
    6. Rerun: compare new scores against baseline

    No model changes needed to fix many regressions — harness changes are cheaper.
    """

    actor_id: str = state(default="")
    total_comparisons: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv.put("svc:regression_detector", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="regression_detector")
        except Exception:
            pass
        host.info(f"RegressionDetectorActor init actor_id={self.actor_id}")

    @handler("compare")
    def compare(self, eval_run_id: str = "", scores: list = None) -> dict:
        """Compare current eval scores against baseline stored in KV."""
        if not eval_run_id:
            return {"error": "eval_run_id is required"}
        if not scores:
            return {"regressions": [], "improvements": [], "unchanged": []}

        # Load baseline
        baseline = self._load_baseline()
        if not baseline:
            # No baseline — store current as baseline and return clean
            self._store_baseline(eval_run_id, scores)
            return {
                "regressions": [],
                "improvements": [],
                "unchanged": [],
                "message": f"Stored as baseline (eval_run_id={eval_run_id})",
            }

        regressions = []
        improvements = []
        unchanged = []

        for current in scores:
            traj_id = current.get("trajectory_id", "")
            scenario_id = traj_id  # Use trajectory_id as scenario key
            current_score = current.get("score", 0.0)

            baseline_score = baseline.get(scenario_id, {}).get("score")
            if baseline_score is None:
                # New scenario not in baseline
                unchanged.append({"trajectory_id": traj_id, "current": current_score, "baseline": None})
                continue

            delta = current_score - baseline_score
            entry = {
                "trajectory_id": traj_id,
                "current": current_score,
                "baseline": baseline_score,
                "delta": round(delta, 3),
            }

            threshold = 0.05  # 5% regression threshold
            if delta < -threshold:
                entry["severity"] = "high" if delta < -0.15 else "medium"
                regressions.append(entry)
            elif delta > threshold:
                improvements.append(entry)
            else:
                unchanged.append(entry)

        self.total_comparisons += 1
        host.incr_counter("regression_comparisons_total", 1)

        if regressions:
            host.warn(f"Regressions detected: {len(regressions)} scenarios degraded in eval_run={eval_run_id}")

        return {
            "regressions": regressions,
            "improvements": improvements,
            "unchanged": unchanged,
            "regression_count": len(regressions),
            "improvement_count": len(improvements),
            "eval_run_id": eval_run_id,
        }

    @handler("set_baseline")
    def set_baseline(self, eval_run_id: str = "", scores: list = None) -> dict:
        """Explicitly set a baseline from an eval run."""
        if not scores:
            return {"error": "scores is required"}
        self._store_baseline(eval_run_id, scores)
        return {"status": "ok", "baseline_eval_run_id": eval_run_id, "scenarios": len(scores)}

    @handler("get_baseline")
    def get_baseline(self) -> dict:
        baseline = self._load_baseline()
        return {"status": "ok", "baseline": baseline, "count": len(baseline) if baseline else 0}

    @handler("replay_diff")
    def replay_diff(self, traj_id_a: str = "", traj_id_b: str = "") -> dict:
        """
        Compare two trajectories step-by-step.

        Use this to diagnose WHY scores diverged between two eval runs.
        Shows where the agent behavior changed.
        """
        traj_a = self._load_trajectory(traj_id_a)
        traj_b = self._load_trajectory(traj_id_b)

        if not traj_a or not traj_b:
            return {"error": "one or both trajectories not found"}

        steps_a = traj_a.get("steps", [])
        steps_b = traj_b.get("steps", [])

        diffs = []
        max_steps = max(len(steps_a), len(steps_b))

        for i in range(max_steps):
            if i >= len(steps_a):
                diffs.append({"step": i, "type": "added", "b": steps_b[i]})
            elif i >= len(steps_b):
                diffs.append({"step": i, "type": "removed", "a": steps_a[i]})
            else:
                step_a = steps_a[i]
                step_b = steps_b[i]
                if step_a.get("kind") != step_b.get("kind") or step_a.get("success") != step_b.get("success"):
                    diffs.append({
                        "step": i,
                        "type": "changed",
                        "a_kind": step_a.get("kind"),
                        "b_kind": step_b.get("kind"),
                        "a_success": step_a.get("success"),
                        "b_success": step_b.get("success"),
                    })

        return {
            "trajectory_id_a": traj_id_a,
            "trajectory_id_b": traj_id_b,
            "steps_a": len(steps_a),
            "steps_b": len(steps_b),
            "score_a": traj_a.get("score", 0),
            "score_b": traj_b.get("score", 0),
            "diff_count": len(diffs),
            "diffs": diffs[:20],  # Cap at 20 diffs
        }

    @handler("get_stats")
    def get_stats(self) -> dict:
        return {"status": "ok", "total_comparisons": self.total_comparisons}

    # ------------------------------------------------------------------

    def _load_baseline(self) -> dict:
        try:
            raw = host.kv.get("regression_baseline")
            return json.loads(raw) if raw else {}
        except Exception:
            return {}

    def _store_baseline(self, eval_run_id: str, scores: list) -> None:
        baseline = {}
        for s in scores:
            traj_id = s.get("trajectory_id", "")
            baseline[traj_id] = {"score": s.get("score", 0.0), "eval_run_id": eval_run_id}
        try:
            host.kv.put("regression_baseline", json.dumps(baseline))
            host.kv.put("regression_baseline_eval_run", eval_run_id)
        except Exception as e:
            host.warn(f"Failed to store baseline: {e}")

    def _load_trajectory(self, traj_id: str) -> dict:
        try:
            raw = host.kv.get(f"trajectory:{traj_id}")
            return json.loads(raw) if raw else {}
        except Exception:
            return {}
