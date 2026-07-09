# SPDX-License-Identifier: AGPL-3.0-or-later
"""DashboardActor — aggregates eval metrics and exposes query handlers.

Demonstrates: read-only aggregation pattern, query-only actor.
"""

import json
from plexspaces import actor, state, init_handler, handler, host


@actor
class DashboardActor:
    """
    Eval dashboard: aggregates results from all eval runs.

    Query this actor for:
    - Eval run summaries
    - Pass rate trends over time
    - Token cost trends
    - Regression alerts
    """

    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv_put("svc:dashboard", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="dashboard")
        except Exception:
            pass
        host.info(f"DashboardActor init actor_id={self.actor_id}")

    @handler("report_eval")
    def report_eval(self, eval_run_id: str = "", report: dict = None, **kwargs) -> dict:
        """Accept an eval report directly (used when KV is not shared across WASM instances)."""
        if not eval_run_id:
            return {"error": "eval_run_id is required"}
        data = report if report else kwargs
        try:
            host.kv_put(f"eval_report:{eval_run_id}", json.dumps(data))
            host.info(f"DashboardActor: stored eval report eval_run_id={eval_run_id}")
            return {"status": "ok", "eval_run_id": eval_run_id}
        except Exception as e:
            return {"error": str(e)}

    @handler("get_eval_report")
    def get_eval_report(self, eval_run_id: str = "") -> dict:
        """Get the full report for an eval run."""
        if not eval_run_id:
            return {"error": "eval_run_id is required"}
        raw = host.kv_get(f"eval_report:{eval_run_id}")
        if not raw:
            return {"error": f"eval run {eval_run_id} not found"}
        try:
            return json.loads(raw)
        except Exception:
            return {"error": "failed to parse report"}

    @handler("list_eval_runs")
    def list_eval_runs(self, limit: int = 10) -> dict:
        """List recent eval runs."""
        try:
            raw_keys = host.kv_list("eval_report:")
            keys = json.loads(raw_keys) if raw_keys and not raw_keys.startswith("ERROR:") else []
            run_ids = [k.replace("eval_report:", "") for k in keys[:limit]]
            reports = []
            for run_id in run_ids:
                raw = host.kv_get(f"eval_report:{run_id}")
                if raw:
                    try:
                        report = json.loads(raw)
                        reports.append({
                            "eval_run_id": run_id,
                            "suite_name": report.get("suite_name", ""),
                            "avg_score": report.get("avg_score", 0.0),
                            "pass_rate": report.get("pass_rate", 0.0),
                            "completed": report.get("completed_scenarios", 0),
                            "total": report.get("total_scenarios", 0),
                            "status": report.get("status", ""),
                        })
                    except Exception:
                        pass
            return {"status": "ok", "runs": reports, "count": len(reports)}
        except Exception as e:
            return {"error": str(e)}

    @handler("get_trajectory")
    def get_trajectory(self, trajectory_id: str = "") -> dict:
        """Get a specific trajectory by ID."""
        if not trajectory_id:
            return {"error": "trajectory_id is required"}
        raw = host.kv_get(f"trajectory:{trajectory_id}")
        if not raw:
            return {"error": f"trajectory {trajectory_id} not found"}
        try:
            return json.loads(raw)
        except Exception:
            return {"error": "failed to parse trajectory"}

    @handler("get_regressions")
    def get_regressions(self) -> dict:
        """Get regression baseline info."""
        baseline_run = host.kv_get("regression_baseline_eval_run") or ""
        baseline = host.kv_get("regression_baseline") or "{}"
        try:
            baseline_data = json.loads(baseline)
            return {
                "status": "ok",
                "baseline_eval_run": baseline_run,
                "baseline_scenario_count": len(baseline_data),
            }
        except Exception:
            return {"error": "failed to parse baseline"}

    @handler("summary")
    def summary(self) -> dict:
        """High-level system summary with aggregate stats."""
        try:
            raw_keys = host.kv_list("eval_report:")
            keys = json.loads(raw_keys) if raw_keys and not raw_keys.startswith("ERROR:") else []
            total_evals = 0
            score_sum = 0.0
            for key in keys:
                run_id = key.replace("eval_report:", "")
                raw = host.kv_get(f"eval_report:{run_id}")
                if raw:
                    try:
                        report = json.loads(raw)
                        total_evals += 1
                        score_sum += float(report.get("avg_score", 0.0))
                    except Exception:
                        pass
            avg_score = round(score_sum / total_evals, 3) if total_evals > 0 else 0.0
            return {
                "status": "ok",
                "actor_id": self.actor_id,
                "total_evals": total_evals,
                "avg_score": avg_score,
                "message": "Use get_eval_report, list_eval_runs, get_trajectory for details.",
            }
        except Exception as e:
            return {
                "status": "ok",
                "actor_id": self.actor_id,
                "total_evals": 0,
                "avg_score": 0.0,
                "message": str(e),
            }
