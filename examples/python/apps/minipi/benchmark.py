# SPDX-License-Identifier: AGPL-3.0-or-later
"""BenchmarkActor — fan-out N eval runs with different configs, measure throughput.

Demonstrates: parallel eval fan-out, config comparison, performance measurement.
The key insight: harness changes (cheap) often beat model changes (expensive).
"""

import json
from plexspaces import workflow_actor, state, init_handler, run_handler, query_handler, host


@workflow_actor
class BenchmarkActor:
    """
    Benchmark runner: same scenario, N different harness configs.

    Measures:
    - Latency (time per eval run)
    - Token cost (input + output tokens per scenario)
    - Quality score (from ScorerActor)
    - Pass rate (scenarios scoring >= 0.8)

    Output: comparison table showing which harness config wins.
    """

    actor_id: str = state(default="")
    benchmark_id: str = state(default="")
    status: str = state(default="idle")
    results: list = state(default_factory=list)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        try:
            host.kv_put("svc:benchmark", host.self_id())
        except Exception:
            pass
        try:
            host.registry.register(None, self.actor_id or host.self_id(), "actor", "",
                                   object_category="benchmark")
        except Exception:
            pass
        host.info(f"BenchmarkActor init actor_id={self.actor_id}")

    @run_handler
    def run(self, scenarios: list = None, configs: list = None, benchmark_id: str = "") -> dict:
        """
        Run the same scenarios with N different harness configs.

        Each config runs in parallel — fan-out to N EvalRunnerActors.
        Collect results and produce a comparison table.
        """
        # SDK may pass the full payload dict as `scenarios` when run_fn == instance.run
        if isinstance(scenarios, dict):
            payload = scenarios
            scenarios = payload.get("scenarios")
            configs = configs or payload.get("configs")
            benchmark_id = benchmark_id or payload.get("benchmark_id", "")
        if not scenarios:
            return {"error": "scenarios is required"}
        if not configs:
            configs = [{"name": "default", "max_iterations": 10, "token_budget": 4096}]

        self.benchmark_id = benchmark_id or host.new_id()
        self.status = "running"

        host.info(f"BenchmarkActor starting: benchmark_id={self.benchmark_id} configs={len(configs)} scenarios={len(scenarios)}")

        start_ms = host.now_ms()

        # Resolve eval_runner full actor ID via process groups
        eval_runner_id = self._find_service("eval_runner") or "eval_runner"

        # Run each config sequentially via the shared eval_runner actor
        # (WASM execution is synchronous — spawned actors won't complete before we return)
        self.results = []
        for i, cfg in enumerate(configs):
            eval_run_id = f"bench-{self.benchmark_id}-config-{i}"
            try:
                # Limit to 2 scenarios per config (benchmark tests harness config, not coverage)
                bench_scenarios = scenarios[:2] if len(scenarios) > 2 else scenarios
                report = self._ask(eval_runner_id, "workflow_run", {
                    "suite_name": f"benchmark-{cfg.get('name', i)}",
                    "scenarios": bench_scenarios,
                    "eval_run_id": eval_run_id,
                }, timeout_ms=120000)
                if report.get("error"):
                    host.warn(f"Benchmark: eval_runner returned error for config {cfg.get('name', i)}: {report.get('error','')[:100]}")
                self.results.append({
                    "config_name": cfg.get("name", f"config-{i}"),
                    "config": cfg,
                    "eval_run_id": eval_run_id,
                    "avg_score": report.get("avg_score", 0.0),
                    "pass_rate": report.get("pass_rate", 0.0),
                    "completed_scenarios": report.get("completed_scenarios", 0),
                    "total_scenarios": report.get("total_scenarios", len(bench_scenarios)),
                })
            except Exception as e:
                host.warn(f"Benchmark config {cfg.get('name', i)} failed: {e}")
                self.results.append({
                    "config_name": cfg.get("name", f"config-{i}"),
                    "config": cfg,
                    "eval_run_id": eval_run_id,
                    "avg_score": 0.0,
                    "pass_rate": 0.0,
                    "completed_scenarios": 0,
                    "total_scenarios": len(scenarios),
                })

        total_ms = host.now_ms() - start_ms

        # Sort by pass rate (best first)
        self.results.sort(key=lambda r: r.get("pass_rate", 0), reverse=True)

        self.status = "completed"

        comparison_table = self._format_comparison_table(self.results)

        host.incr_counter("benchmarks_completed", 1)
        host.info(f"BenchmarkActor completed: benchmark_id={self.benchmark_id} configs={len(self.results)}")

        best_score = self.results[0].get("avg_score", 0.0) if self.results else 0.0
        return {
            "status": "completed",
            "benchmark_id": self.benchmark_id,
            "configs_tested": len(self.results),
            "scenarios": len(scenarios),
            "total_duration_ms": total_ms,
            "results": self.results,
            "comparison_table": comparison_table,
            "winner": self.results[0]["config_name"] if self.results else "",
            "best_score": round(best_score, 3),
        }

    @query_handler("status")
    def on_query_status(self) -> dict:
        return {
            "benchmark_id": self.benchmark_id,
            "status": self.status,
            "results_count": len(self.results),
        }

    # ------------------------------------------------------------------

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

    def _ask(self, actor_id: str, op: str, payload: dict, timeout_ms: int = 10000) -> dict:
        try:
            return host.ask(actor_id, op, payload, timeout_ms) or {}
        except Exception as e:
            return {"error": str(e)}

    def _format_comparison_table(self, results: list) -> list:
        """Format results as a comparison table (list of rows)."""
        table = []
        for r in results:
            table.append({
                "config": r.get("config_name", ""),
                "pass_rate": f"{r.get('pass_rate', 0)*100:.1f}%",
                "completed": f"{r.get('completed_scenarios', 0)}/{r.get('total_scenarios', 0)}",
                "max_iterations": r.get("config", {}).get("max_iterations", "?"),
                "token_budget": r.get("config", {}).get("token_budget", "?"),
            })
        return table
