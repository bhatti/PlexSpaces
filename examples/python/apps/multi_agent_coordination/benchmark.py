# SPDX-License-Identifier: AGPL-3.0-or-later
"""BenchmarkAgent — micro-benchmarks for all 10 coordination patterns.

Demonstrates: Two-Phase Commit / Barrier (benchmark synchronization),
all pattern micro-benchmarks.
"""

from plexspaces import actor, state, handler, init_handler, host


def _percentile(sorted_vals: list, pct: float) -> float:
    if not sorted_vals:
        return 0.0
    idx = int(len(sorted_vals) * pct / 100.0)
    idx = min(idx, len(sorted_vals) - 1)
    return sorted_vals[idx]


def _bench_blackboard(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        host.ts.write(["bench", "bb", i, f"data-{i}"])
        host.ts.read(["bench", "bb", i, None])
        times.append(host.now_ms() - t0)
    return _stats("blackboard", times)


def _bench_scatter_gather(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        for j in range(3):
            host.ts.write(["scatter", i, j, f"subtask-{j}"])
        results = host.ts.read_all(["scatter", i, None, None])
        times.append(host.now_ms() - t0)
    return _stats("scatter_gather", times)


def _bench_generator_verifier(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        host.ts.write(["gen", i, "draft", 0.5])
        draft = host.ts.read(["gen", i, None, None])
        host.ts.write(["verify", i, "approved", 0.8])
        times.append(host.now_ms() - t0)
    return _stats("generator_verifier", times)


def _bench_pipeline(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        host.ts.write(["pipe", "stage1", i, "raw"])
        s1 = host.ts.read(["pipe", "stage1", i, None])
        host.ts.write(["pipe", "stage2", i, "processed"])
        s2 = host.ts.read(["pipe", "stage2", i, None])
        host.ts.write(["pipe", "stage3", i, "final"])
        times.append(host.now_ms() - t0)
    return _stats("pipeline", times)


def _bench_pubsub(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        host.ts.write(["event", "pubsub", i, f"msg-{i}"])
        host.ts.read(["event", "pubsub", i, None])
        times.append(host.now_ms() - t0)
    return _stats("pubsub", times)


def _bench_voting(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        for v in range(3):
            decision = "approve" if v < 2 else "reject"
            host.ts.write(["bench_vote", f"prop-{i}", f"v{v}", decision, host.now_ms()])
        votes = host.ts.read_all(["bench_vote", f"prop-{i}", None, None, None])
        times.append(host.now_ms() - t0)
    return _stats("voting", times)


def _bench_task_delegation(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        tid = f"bench-task-{host.now_ms()}-{i}"
        host.ts.write(["task", tid, "pending", f"bench-{i}", 0])
        claimed = host.ts.take(["task", tid, "pending", None, None])
        times.append(host.now_ms() - t0)
    return _stats("task_delegation", times)


def _bench_veto(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        aid = f"bench-analysis-{i}"
        host.ts.write(["veto", aid, "bench reason", host.now_ms()])
        host.ts.read(["veto", aid, None, None])
        times.append(host.now_ms() - t0)
    return _stats("veto", times)


def _bench_barrier(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        for role in ("research", "analysis", "verifier"):
            host.ts.write(["bench_ready", role, f"actor-{role}", host.now_ms()])
        ready = host.ts.read_all(["bench_ready", None, None, None])
        if len(ready) >= 3:
            host.ts.write(["bench_signal", "COMMIT", "coordinator", f"phase-{i}", host.now_ms()])
        times.append(host.now_ms() - t0)
    return _stats("barrier", times)


def _bench_capability_discovery(iterations: int) -> dict:
    times = []
    for i in range(iterations):
        t0 = host.now_ms()
        host.ts.write(["svc", f"bench-svc-{i}", f"actor-{i}"])
        host.ts.read(["svc", f"bench-svc-{i}", None])
        times.append(host.now_ms() - t0)
    return _stats("capability_discovery", times)


_PATTERN_RUNNERS = {
    "blackboard": _bench_blackboard,
    "scatter_gather": _bench_scatter_gather,
    "generator_verifier": _bench_generator_verifier,
    "pipeline": _bench_pipeline,
    "pubsub": _bench_pubsub,
    "voting": _bench_voting,
    "task_delegation": _bench_task_delegation,
    "veto": _bench_veto,
    "barrier": _bench_barrier,
    "capability_discovery": _bench_capability_discovery,
}


def _stats(pattern: str, times: list) -> dict:
    if not times:
        return {"pattern": pattern, "iterations": 0, "avg_ms": 0}
    sorted_t = sorted(times)
    total = sum(sorted_t)
    avg = total / len(sorted_t)
    tps = (1000.0 / avg) if avg > 0 else 0
    return {
        "pattern": pattern,
        "iterations": len(sorted_t),
        "avg_ms": round(avg, 1),
        "min_ms": sorted_t[0],
        "max_ms": sorted_t[-1],
        "p50_ms": _percentile(sorted_t, 50),
        "p95_ms": _percentile(sorted_t, 95),
        "tps": round(tps, 0),
    }


@actor
class BenchmarkAgent:
    """GenServer: runs micro-benchmarks for all 10 coordination patterns."""

    last_results: list = state(default=[])
    actor_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        host.info(f"BenchmarkAgent init actor_id={self.actor_id}")

    @handler("run_pattern_benchmark")
    def run_pattern_benchmark(self, pattern: str = "", iterations: int = 10) -> dict:
        runner = _PATTERN_RUNNERS.get(pattern)
        if not runner:
            return {"error": f"unknown pattern: {pattern}", "available": list(_PATTERN_RUNNERS.keys())}
        result = runner(iterations)
        self.last_results = [result]
        return result

    @handler("run_all_benchmarks")
    def run_all_benchmarks(self, iterations: int = 10) -> dict:
        results = []
        for name, runner in _PATTERN_RUNNERS.items():
            try:
                results.append(runner(iterations))
            except Exception as e:
                results.append({"pattern": name, "error": str(e)})
        self.last_results = results
        return {"results": results, "patterns_tested": len(results)}

    @handler("get_results")
    def get_results(self) -> dict:
        return {"results": self.last_results}
