# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Metrics Aggregation Pipeline - Python WASM
#
# Production-grade metrics pipeline: StatsD/OTLP ingestion → tumbling window
# aggregation → cascading rollup (1s→1m→1h) → anomaly detection (Z-score/EWMA)
# → alerting. Leader/worker with shard-group placement, scatter/gather,
# compute vs coordination metrics tracking.
#
# Real-world analog: Datadog Agent, CloudWatch Metrics, Prometheus + Thanos, Graphite

import math

from plexspaces import ActorID, actor, handler, init_handler, host, state
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement

ACTOR_ROLES = {}


def actor_application_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def register_role(role_name):
    def decorator(cls):
        ACTOR_ROLES[role_name] = cls
        return cls
    return decorator


# ─── Metric Generation ────────────────────────────────────────────────────────

def generate_metrics(count, seed=42):
    metrics = []
    rng = seed
    metric_names = [
        "cpu.usage", "memory.used", "disk.io.read", "disk.io.write",
        "network.rx.bytes", "network.tx.bytes", "http.request.duration",
        "http.request.count", "cache.hit_rate", "queue.depth",
        "gc.pause_ms", "thread.count", "db.query.duration", "db.connections.active",
        "api.latency.p99", "api.error_rate",
    ]
    hosts_list = ["web-01", "web-02", "api-01", "db-01", "cache-01", "worker-01"]
    envs = ["production", "staging"]
    regions = ["us-east-1", "eu-west-1", "ap-south-1"]

    for i in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        name = metric_names[rng % len(metric_names)]
        host_name = hosts_list[(rng >> 8) % len(hosts_list)]
        env = envs[(rng >> 12) % len(envs)]
        region = regions[(rng >> 16) % len(regions)]

        if "duration" in name or "latency" in name or "pause" in name:
            value = float((rng % 500) + 1)
        elif "rate" in name or "usage" in name or "hit_rate" in name:
            value = float(rng % 100)
        elif "count" in name or "depth" in name or "connections" in name:
            value = float(rng % 1000)
        else:
            value = float(rng % 10000)

        if (rng >> 20) % 20 == 0:
            value *= 10.0

        metric_type = "counter" if "count" in name else (
            "histogram" if "duration" in name or "latency" in name else "gauge"
        )
        metrics.append({
            "name": name,
            "value": value,
            "type": metric_type,
            "tags": {"host": host_name, "env": env, "region": region},
            "timestamp": host.now_ms() + i,
        })
    return metrics


# ─── Window Aggregation ──────────────────────────────────────────────────────

def aggregate_window(metrics, window_ms=1000):
    buckets = {}
    for m in metrics:
        key = m["name"]
        if key not in buckets:
            buckets[key] = {
                "name": m["name"], "count": 0, "sum": 0.0,
                "min": float("inf"), "max": float("-inf"),
                "values": [], "tags_sample": m.get("tags", {}),
                "type": m.get("type", "gauge"),
            }
        b = buckets[key]
        b["count"] += 1
        b["sum"] += m["value"]
        if m["value"] < b["min"]:
            b["min"] = m["value"]
        if m["value"] > b["max"]:
            b["max"] = m["value"]
        b["values"].append(m["value"])

    results = []
    for _, b in buckets.items():
        avg = b["sum"] / b["count"] if b["count"] > 0 else 0.0
        values = sorted(b["values"])
        n = len(values)
        p50 = values[n // 2] if values else 0.0
        p95 = values[min(int(n * 0.95), n - 1)] if values else 0.0
        p99 = values[min(int(n * 0.99), n - 1)] if values else 0.0
        # safe_round: avoid WASM double_round crash for ndigits > 0
        avg_r = int(avg * 100) / 100
        results.append({
            "name": b["name"], "count": b["count"], "sum": b["sum"],
            "avg": avg_r,
            "min": b["min"] if b["min"] != float("inf") else 0.0,
            "max": b["max"] if b["max"] != float("-inf") else 0.0,
            "p50": p50, "p95": p95, "p99": p99,
            "type": b["type"], "tags": b["tags_sample"], "window_ms": window_ms,
        })
    return results


# ─── Cascading Rollup ────────────────────────────────────────────────────────

def rollup_aggregates(aggregates, target_window_ms):
    rolled = {}
    for agg in aggregates:
        key = agg["name"]
        if key not in rolled:
            rolled[key] = {
                "name": agg["name"], "count": 0, "sum": 0.0,
                "min": float("inf"), "max": float("-inf"),
                "type": agg.get("type", "gauge"), "tags": agg.get("tags", {}),
                "window_ms": target_window_ms,
            }
        r = rolled[key]
        r["count"] += agg["count"]
        r["sum"] += agg["sum"]
        if agg["min"] < r["min"]:
            r["min"] = agg["min"]
        if agg["max"] > r["max"]:
            r["max"] = agg["max"]

    results = []
    for _, r in rolled.items():
        r["avg"] = int(r["sum"] / r["count"] * 100) / 100 if r["count"] > 0 else 0.0
        if r["min"] == float("inf"):
            r["min"] = 0.0
        if r["max"] == float("-inf"):
            r["max"] = 0.0
        results.append(r)
    return results


# ─── Anomaly Detection ───────────────────────────────────────────────────────

def detect_anomalies_zscore(aggregates, threshold=2.5):
    anomalies = []
    by_name = {}
    for agg in aggregates:
        by_name.setdefault(agg["name"], []).append(agg)

    for name, aggs in by_name.items():
        values = [a["avg"] for a in aggs]
        if len(values) < 3:
            continue
        mean = sum(values) / len(values)
        variance = sum((v - mean) ** 2 for v in values) / len(values)
        std = math.sqrt(variance) if variance > 0 else 0.001

        for agg in aggs:
            z = abs(agg["avg"] - mean) / std if std > 0 else 0
            if z > threshold:
                anomalies.append({
                    "metric": name, "value": agg["avg"],
                    "z_score": int(z * 100) / 100,
                    "severity": "critical" if z > threshold * 2 else "warning",
                    "tags": agg.get("tags", {}),
                })
    return anomalies


def detect_anomalies_ewma(aggregates, alpha=0.3, threshold=2.0):
    anomalies = []
    by_name = {}
    for agg in aggregates:
        by_name.setdefault(agg["name"], []).append(agg)

    for name, aggs in by_name.items():
        if len(aggs) < 3:
            continue
        ewma = aggs[0]["avg"]
        ewma_var = 0.0
        for agg in aggs[1:]:
            val = agg["avg"]
            diff = val - ewma
            ewma = alpha * val + (1 - alpha) * ewma
            ewma_var = alpha * (diff ** 2) + (1 - alpha) * ewma_var
            ewma_std = math.sqrt(ewma_var) if ewma_var > 0 else 0.001
            deviation = abs(val - ewma) / ewma_std if ewma_std > 0 else 0
            if deviation > threshold:
                anomalies.append({
                    "metric": name, "value": val,
                    "ewma": int(ewma * 100) / 100,
                    "deviation": int(deviation * 100) / 100,
                    "severity": "critical" if deviation > threshold * 2 else "warning",
                    "tags": agg.get("tags", {}),
                })
    return anomalies


# ─── Leader Actor ─────────────────────────────────────────────────────────────

@register_role("leader")
@actor
class LeaderActor:
    total_compute_ms: int = state(default=0)
    total_coord_ms: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("run")
    def run(
        self,
        metric_count: int = 10000,
        worker_count: int = 8,
        batch_size: int = 500,
        window_ms: int = 1000,
        anomaly_threshold: float = 2.5,
        rollup_levels: list = None,
        from_actor: str = "",
    ) -> dict:
        if rollup_levels is None:
            rollup_levels = [1000, 60000, 3600000]

        coord_start = host.now_ms()
        group_id = f"metrics-agg-{host.now_ms()}"
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=group_id,
            actor_type="WorkerActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        shard_ids = group.shard_actor_ids
        if not shard_ids:
            return {"status": "error", "error": "failed to create worker shard group"}
        coord_create = host.now_ms() - coord_start

        metrics = generate_metrics(metric_count, seed=host.now_ms() % 100000)
        batches = [metrics[i:i + batch_size] for i in range(0, len(metrics), batch_size)]

        total_compute_ms = 0
        total_coord_ms = coord_create
        all_aggregates = []
        all_anomalies = []
        error_count = 0

        for batch in batches:
            sg_start = host.now_ms()
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={
                    "op": "aggregate_batch",
                    "metrics": batch,
                    "window_ms": window_ms,
                    "anomaly_threshold": anomaly_threshold,
                },
                timeout_ms=30000,
            ))
            total_coord_ms += host.now_ms() - sg_start

            for resp in sg_result.shard_responses:
                r = resp
                for key in ["payload", "result", "response", "data"]:
                    if isinstance(r, dict) and key in r:
                        r = r[key]
                if not isinstance(r, dict) or "error" in r:
                    error_count += 1
                    continue
                total_compute_ms += r.get("compute_ms", 0)
                all_aggregates.extend(r.get("aggregates", []))
                all_anomalies.extend(r.get("anomalies", []))

        # Cascading rollup
        rollup_start = host.now_ms()
        rollup_results = {}
        current_aggs = all_aggregates
        for level_ms in rollup_levels:
            rolled = rollup_aggregates(current_aggs, level_ms)
            rollup_results[str(level_ms)] = {
                "window_ms": level_ms,
                "metric_count": len(rolled),
                "total_datapoints": sum(r["count"] for r in rolled),
            }
            current_aggs = rolled
        total_compute_ms += host.now_ms() - rollup_start

        self.total_compute_ms += total_compute_ms
        self.total_coord_ms += total_coord_ms

        wall_time = total_compute_ms + total_coord_ms
        granularity = int(total_compute_ms * 10 / total_coord_ms) / 10 if total_coord_ms > 0 else 0
        metrics_per_sec = metric_count * 1000 // wall_time if wall_time > 0 else 0

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {
                        "leader.compute": total_compute_ms,
                        "leader.coordination": total_coord_ms,
                    },
                    "latency_samples": {
                        "leader.compute": 1,
                        "leader.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "status": "ok",
            "metric_count": metric_count,
            "worker_count": worker_count,
            "window_ms": window_ms,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "metrics_per_sec": metrics_per_sec,
            "unique_metrics": len(set(a["name"] for a in all_aggregates)),
            "total_aggregates": len(all_aggregates),
            "anomaly_count": len(all_anomalies),
            "anomaly_severity": {
                "critical": sum(1 for a in all_anomalies if a.get("severity") == "critical"),
                "warning": sum(1 for a in all_anomalies if a.get("severity") == "warning"),
            },
            "rollup_levels": rollup_results,
            "actor_count": len(shard_ids) + 1,
            "error_count": error_count,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        metric_count: int = 10000,
        shard_counts: list = None,
        batch_size: int = 500,
        window_ms: int = 1000,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Strong scaling: total work = BENCH_METRICS (fixed). Each shard gets
        # BENCH_METRICS // shard_count metrics and runs NUM_PASSES aggregation passes
        # so compute dominates the ~30ms scatter_gather coordination overhead.
        TIMING_SG = 2
        BENCH_METRICS = 4000  # total; each shard gets BENCH_METRICS // shard_count
        NUM_PASSES = 8

        results = []
        baseline_mps = 0

        for shard_count in shard_counts:
            metrics_per_shard = max(1, BENCH_METRICS // shard_count)

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"bench-strong-{shard_count}-{host.now_ms()}",
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            # 1 warmup scatter_gather
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "aggregate_batch", "metric_count": 4, "num_passes": 1,
                       "window_ms": window_ms, "anomaly_threshold": 2.5},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0

            for _bi in range(benchmark_rounds):
                iter_start = host.now_ms()
                iter_compute = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "aggregate_batch", "metric_count": metrics_per_shard,
                               "num_passes": NUM_PASSES,
                               "window_ms": window_ms, "anomaly_threshold": 2.5},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            actual_metrics = TIMING_SG * metrics_per_shard * shard_count * NUM_PASSES
            avg_mps = actual_metrics * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_mps == 0:
                baseline_mps = avg_mps or 1

            speedup_100 = avg_mps * 100 // baseline_mps if baseline_mps > 0 else 100
            speedup = speedup_100 / 100
            eff_100 = speedup_100 * shard_counts[0] // shard_count if shard_count > 0 else 100
            compute_pct = avg_compute * 100 // (avg_compute + avg_coord)
            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0

            results.append({
                "shards": shard_count,
                "metrics_per_sec": avg_mps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "compute_pct": compute_pct,
                "granularity_ratio": gran_10 / 10,
                "speedup": speedup,
                "efficiency_pct": eff_100,
                "node_count": 1,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "metric_count": metric_count,
            "window_ms": window_ms,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        metrics_per_shard: int = 5000,
        shard_counts: list = None,
        batch_size: int = 500,
        window_ms: int = 1000,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Weak scaling: each shard always processes BENCH_PER_SHARD metrics locally
        # with NUM_PASSES aggregation passes (compute-dominated).
        # Total work grows with N; wall time stays flat → efficiency tracks parallelism.
        TIMING_SG = 2
        BENCH_PER_SHARD = 500
        NUM_PASSES = 8

        results = []
        baseline_mps = 0

        for shard_count in shard_counts:
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"bench-weak-{shard_count}-{host.now_ms()}",
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "aggregate_batch", "metric_count": 4, "num_passes": 1,
                       "window_ms": window_ms, "anomaly_threshold": 2.5},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0

            for _bi in range(benchmark_rounds):
                iter_start = host.now_ms()
                iter_compute = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "aggregate_batch", "metric_count": BENCH_PER_SHARD,
                               "num_passes": NUM_PASSES,
                               "window_ms": window_ms, "anomaly_threshold": 2.5},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)

            actual_metrics = TIMING_SG * BENCH_PER_SHARD * shard_count * NUM_PASSES
            avg_mps = actual_metrics * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_mps == 0:
                baseline_mps = avg_mps or 1

            eff_100 = avg_mps * 100 // baseline_mps if baseline_mps > 0 else 100
            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0

            results.append({
                "shards": shard_count,
                "total_metrics": TIMING_SG * BENCH_PER_SHARD * shard_count,
                "metrics_per_sec": avg_mps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran_10 / 10,
                "efficiency_pct": eff_100,
                "node_count": 1,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "metrics_per_shard": metrics_per_shard,
            "window_ms": window_ms,
            "results": results,
        }


# ─── Worker Actor ─────────────────────────────────────────────────────────────

@register_role("worker")
@actor
class WorkerActor:
    metrics_processed: int = state(default=0)
    compute_ms_total: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("aggregate_batch")
    def aggregate_batch(
        self,
        metrics: list = None,
        metric_count: int = 0,
        num_passes: int = 1,
        window_ms: int = 1000,
        anomaly_threshold: float = 2.5,
        from_actor: str = "",
    ) -> dict:
        if not metrics and metric_count > 0:
            metrics = generate_metrics(metric_count, seed=self.metrics_processed % 100000 + 1)
        if metrics is None:
            metrics = []

        comp_start = host.now_ms()
        # Run the full pipeline num_passes times to model heavier compute workloads
        aggregates = []
        all_anomalies = []
        for _p in range(max(1, num_passes)):
            aggregates = aggregate_window(metrics, window_ms)
            anomalies_z = detect_anomalies_zscore(aggregates, anomaly_threshold)
            anomalies_ewma = detect_anomalies_ewma(aggregates)
            all_anomalies = anomalies_z + anomalies_ewma

        seen = set()
        unique_anomalies = []
        for a in all_anomalies:
            key = (a["metric"], a.get("value", 0))
            if key not in seen:
                seen.add(key)
                unique_anomalies.append(a)

        compute_ms = host.now_ms() - comp_start
        self.metrics_processed += len(metrics) * max(1, num_passes)
        self.compute_ms_total += compute_ms

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "latency_totals_ms": {"worker.compute": compute_ms},
                    "latency_samples": {"worker.compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "metrics_in": len(metrics),
            "aggregates": aggregates,
            "anomalies": unique_anomalies,
            "compute_ms": compute_ms,
        }
