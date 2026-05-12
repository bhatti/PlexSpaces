# SPDX-License-Identifier: AGPL-3.0-or-later
"""Parallel AI Inference - Demonstrates all 4 parallelization mechanisms.

Mechanisms demonstrated:
1. ShardGroup scatter-gather (shard-based parallel inference)
2. Elastic pool checkout/checkin (dynamic worker scaling)
3. MPI collectives (BroadcastShardGroup, ReduceShardGroup, AllReduceShardGroup, BarrierShardGroup)
4. Process groups for coordination
"""

from typing import Any, Dict, List

from plexspaces import (
    ActorID,
    actor,
    event_actor,
    fsm_actor,
    gen_server_actor,
    handler,
    host,
    init_handler,
    query_handler,
    run_handler,
    signal_handler,
    state,
    workflow_actor,
)


def actor_application_id(actor_id: str) -> str:
    """Extract the application namespace from a canonical actor id."""
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def actor_node_id(actor_id: str) -> str:
    """Extract the runtime node id from a canonical actor id."""
    try:
        return ActorID.parse(actor_id).node_id or "local"
    except ValueError:
        return "local"


# ─────────────────────────────────────────────────────────────────────────────
# MetricsEventActor
# Role: "metrics_event"
# GenEvent actor: fire-and-forget inference metrics events for observability.
# ─────────────────────────────────────────────────────────────────────────────


@event_actor
class MetricsEventActor:
    """GenEvent actor: fire-and-forget inference metrics events for observability."""

    events_received: int = state(default=0)
    last_event: dict = state(default_factory=dict)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        host.process_groups.join("metrics-events")

    @handler("inference_completed", "cast")
    def on_inference_completed(
        self,
        worker_id: str = "",
        latency_ms: int = 0,
        model_type: str = "",
        from_actor: str = "",
    ) -> None:
        self.events_received += 1
        self.last_event = {
            "worker_id": worker_id,
            "latency_ms": latency_ms,
            "model_type": model_type,
        }
        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "events_received": 1,
                        f"model_{model_type}_events": 1,
                    },
                    "latency_totals_ms": {"event_latency": latency_ms},
                    "latency_max_ms": {"event_latency": latency_ms},
                    "latency_samples": {"event_latency": 1},
                },
            )
        except Exception:
            pass

    @handler("get_stats")
    def get_stats(self, from_actor: str = "") -> dict:
        return {
            "events_received": self.events_received,
            "last_event": self.last_event,
        }


# ─────────────────────────────────────────────────────────────────────────────
# WorkerCircuitBreakerFSM
# Role: "circuit_breaker"
# FSM actor: circuit-breaker state machine for inference worker health.
# ─────────────────────────────────────────────────────────────────────────────


@fsm_actor(states=["closed", "open", "half_open"], initial="closed")
class WorkerCircuitBreakerFSM:
    """FSM actor: circuit-breaker state machine for inference worker health."""

    failure_count: int = state(default=0)
    success_count: int = state(default=0)
    threshold: int = state(default=3)
    fsm_state: str = state(default="closed")
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        args = config.get("args", {})
        self.threshold = int(args.get("threshold", 3))

    @handler("record_success")
    def record_success(self, from_actor: str = "") -> dict:
        if self.fsm_state == "half_open":
            self.success_count += 1
            if self.success_count >= 2:
                self.fsm_state = "closed"
                self.failure_count = 0
                self.success_count = 0
        elif self.fsm_state == "closed":
            self.failure_count = max(0, self.failure_count - 1)
        return {"state": self.fsm_state, "failures": self.failure_count}

    @handler("record_failure")
    def record_failure(self, from_actor: str = "") -> dict:
        if self.fsm_state == "closed":
            self.failure_count += 1
            if self.failure_count >= self.threshold:
                self.fsm_state = "open"
                # Schedule transition to half_open after 5 seconds
                host.send_after(5000, "try_reset", {})
        elif self.fsm_state == "half_open":
            self.fsm_state = "open"
            host.send_after(5000, "try_reset", {})
        return {"state": self.fsm_state, "failures": self.failure_count}

    @handler("try_reset")
    def try_reset(self, from_actor: str = "") -> dict:
        if self.fsm_state == "open":
            self.fsm_state = "half_open"
            self.success_count = 0
        return {"state": self.fsm_state}

    @handler("is_allowed")
    def is_allowed(self, from_actor: str = "") -> dict:
        return {"allowed": self.fsm_state != "open", "state": self.fsm_state}

    @handler("get_state")
    def get_state(self, from_actor: str = "") -> dict:
        return {
            "fsm_state": self.fsm_state,
            "failures": self.failure_count,
            "successes": self.success_count,
        }


# ─────────────────────────────────────────────────────────────────────────────
# InferenceWorkerActor
# Role: "inference_worker"
# Simulates ML model inference with configurable model sizes.
# ─────────────────────────────────────────────────────────────────────────────

# Model latency profiles (simulated compute iterations)
_MODEL_LATENCY_ITERATIONS = {
    "small": 5000,
    "medium": 20000,
    "large": 50000,
}


def _synthetic_worker_coordination_ms(actor_id: str, model_type: str) -> int:
    base = {"small": 2, "medium": 3, "large": 4}.get(model_type, 2)
    return base + (sum(actor_id.encode("utf-8")) % 3)


def _round2(value: float) -> float:
    return round(value, 2)


def _percentile(sorted_values: List[int], percentile: float) -> int:
    if not sorted_values:
        return 0
    index = min(len(sorted_values) - 1, max(0, int(len(sorted_values) * percentile + 0.999999) - 1))
    return int(sorted_values[index])


def _default_scaling_shards() -> List[int]:
    return [2, 4, 6, 8, 16, 32, 64, 128]


@actor
class InferenceWorkerActor:
    worker_id: str = state(default="")
    application_id: str = state(default="")
    model_type: str = state(default="small")
    requests_processed: int = state(default=0)
    total_latency_ms: int = state(default=0)
    error_count: int = state(default=0)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.worker_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.worker_id)
        args = config.get("args", {})
        self.model_type = str(args.get("model_type", "small"))
        self.requests_processed = 0
        self.total_latency_ms = 0
        self.error_count = 0

    @handler("infer")
    def infer(
        self,
        request_id: str = "",
        input: str = "",
        model_type: str = "",
        work_multiplier: int = 1,
        batch_size: int = 1,
        from_actor: str = "",
    ) -> dict:
        start_ms = host.now_ms()
        if not isinstance(work_multiplier, int):
            work_multiplier = int(work_multiplier)
        if not isinstance(batch_size, int):
            batch_size = int(batch_size)
        batch_size = max(1, batch_size)
        effective_model_type = str(model_type or self.model_type)

        # Run one model pass regardless of batch_size — batched inference fuses N items into
        # a single forward pass, so compute grows sub-linearly. Coordination is paid once.
        iterations = _MODEL_LATENCY_ITERATIONS.get(effective_model_type, 5000) * max(1, work_multiplier)
        acc = 0
        for i in range(iterations):
            acc += i

        compute_time_ms = host.now_ms() - start_ms
        # One coordination RTT regardless of batch size — this is the batching benefit
        coordination_time_ms = _synthetic_worker_coordination_ms(self.worker_id, effective_model_type)
        latency_ms = compute_time_ms + coordination_time_ms
        self.requests_processed += batch_size
        self.total_latency_ms += latency_ms

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "requests_processed": 1,
                        "inference_count": 1,
                    },
                    "latency_totals_ms": {
                        "inference": latency_ms,
                        "compute": compute_time_ms,
                        "coordination": coordination_time_ms,
                        "worker.compute": compute_time_ms,
                        "worker.coordination": coordination_time_ms,
                    },
                    "latency_max_ms": {
                        "inference": latency_ms,
                        "compute": compute_time_ms,
                        "coordination": coordination_time_ms,
                        "worker.compute": compute_time_ms,
                        "worker.coordination": coordination_time_ms,
                    },
                    "latency_samples": {
                        "inference": 1,
                        "compute": 1,
                        "coordination": 1,
                        "worker.compute": 1,
                        "worker.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        # Fire inference_completed event (cast = fire-and-forget)
        try:
            members = host.process_groups.members("metrics-events")
            event_actor_id = members[0] if members else "metrics_event"
            host.send(event_actor_id, "inference_completed", {
                "worker_id": self.worker_id,
                "latency_ms": latency_ms,
                "model_type": effective_model_type,
            })
        except Exception:
            pass

        return {
            "status": "ok",
            "result": f"inference-result-{request_id}",
            "model": effective_model_type,
            "batch_size": batch_size,
            "latency_ms": latency_ms,
            "compute_time_ms": compute_time_ms,
            "coordination_time_ms": coordination_time_ms,
            "worker_id": self.worker_id,
            "node_id": actor_node_id(self.worker_id),
            "acc": acc % 1000,
        }

    @handler("get_metrics")
    def get_metrics(self, from_actor: str = "") -> dict:
        avg = (
            self.total_latency_ms / self.requests_processed
            if self.requests_processed > 0
            else 0
        )
        return {
            "status": "ok",
            "requests_processed": self.requests_processed,
            "avg_latency_ms": avg,
            "error_count": self.error_count,
            "worker_id": self.worker_id,
            "model_type": self.model_type,
        }

    @handler("get_numeric_stats")
    def get_numeric_stats(self, from_actor: str = "") -> dict:
        """Return only numeric fields — safe to use as map_function for ReduceShardGroup/AllReduceShardGroup."""
        avg = (
            self.total_latency_ms / self.requests_processed
            if self.requests_processed > 0
            else 0
        )
        return {
            "requests_processed": self.requests_processed,
            "avg_latency_ms": avg,
            "error_count": self.error_count,
            "total_latency_ms": self.total_latency_ms,
        }

    @handler("reset")
    def reset(self, from_actor: str = "") -> dict:
        self.requests_processed = 0
        self.total_latency_ms = 0
        self.error_count = 0
        return {"status": "ok", "worker_id": self.worker_id}


# ─────────────────────────────────────────────────────────────────────────────
# BenchmarkActor
# Role: "benchmark"
# Runs benchmarks across all 4 parallelization mechanisms.
# ─────────────────────────────────────────────────────────────────────────────


def _extract_shard_responses(response: Any) -> List[Dict[str, Any]]:
    """Normalize shard_responses from a scatter-gather response."""
    if not isinstance(response, dict):
        return []
    return response.get("shard_responses", [])


def _unwrap_payload(payload: Any) -> Dict[str, Any]:
    """Unwrap nested payload structures from shard responses."""
    current = payload
    while isinstance(current, dict):
        if "status" in current or "result" in current or "requests_processed" in current:
            return current
        for key in ("payload", "result", "response", "data"):
            nested = current.get(key)
            if isinstance(nested, dict):
                current = nested
                break
        else:
            return current
    return {}


@actor
class BenchmarkActor:
    application_id: str = state(default="")
    actor_id: str = state(default="")
    results: list = state(default_factory=list)
    benchmark_running: bool = state(default=False)

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)
        self.results = []
        self.benchmark_running = False

    def _run_shard_suite(
        self,
        shard_counts: List[int],
        requests_per_shard: int,
        warmup_requests: int,
        logical_actor_count: int,
        payload_size_bytes: int,
        model_type: str,
        work_multiplier: int,
        benchmark_name: str,
        weak_scaling: bool = False,
    ) -> dict:
        benchmark_results = []
        leader_node_id = actor_node_id(self.actor_id)
        baseline = None

        for num_shards in shard_counts:
            if not isinstance(num_shards, int):
                num_shards = int(num_shards)

            group_id = f"bench-shard-{num_shards}-{host.now_ms()}"
            group = host.create_shard_group(
                {
                    "group_id": group_id,
                    "actor_type": "inference_worker",
                    "shard_count": num_shards,
                    "partition_strategy": "hash",
                    "rebalance_policy": "manual",
                    "placement": {"strategy": "from_registry"},
                    "initial_state": {},
                }
            )
            shard_actor_ids = group.get("shard_actor_ids", [])
            if not shard_actor_ids:
                benchmark_results.append({"shards": num_shards, "error": "failed to create shard group"})
                continue

            # Strong scaling: fixed total work split across shards (batch shrinks as shards grow).
            # Weak scaling: fixed work per shard (batch constant, total grows with shards).
            actors_per_shard = max(1, logical_actor_count if logical_actor_count else requests_per_shard)
            batch_size = actors_per_shard if weak_scaling else max(1, actors_per_shard // max(1, num_shards))
            scatter_gather_rounds = max(1, requests_per_shard)
            total_requests = num_shards * batch_size * scatter_gather_rounds

            try:
                for warmup_index in range(warmup_requests):
                    host.scatter_gather(
                        {
                            "group_id": group_id,
                            "query": {
                                "op": "infer",
                                "request_id": f"warmup-{num_shards}-{warmup_index}",
                                "input": "x" * max(1, payload_size_bytes),
                                "model_type": model_type,
                                "work_multiplier": work_multiplier,
                                "batch_size": batch_size,
                            },
                            "aggregation": "concat",
                            "min_responses": num_shards,
                            "timeout_ms": 30000,
                        }
                    )
            except Exception as exc:
                benchmark_results.append(
                    {
                        "shards": num_shards,
                        "logical_actor_count": logical_actor_count,
                        "payload_size_bytes": payload_size_bytes,
                        "model_type": model_type,
                        "work_multiplier": work_multiplier,
                        "error": f"warmup failed: {exc}",
                    }
                )
                continue

            latencies: List[int] = []
            total_compute_ms = 0
            total_coordination_ms = 0
            total_errors = 0
            worker_nodes = set()
            remote_nodes = set()
            bench_start = host.now_ms()

            for i in range(scatter_gather_rounds):
                request_start = host.now_ms()
                try:
                    response = host.scatter_gather(
                        {
                            "group_id": group_id,
                            "query": {
                                "op": "infer",
                                "request_id": f"bench-{num_shards}-{i}",
                                "input": "x" * max(1, payload_size_bytes),
                                "model_type": model_type,
                                "work_multiplier": work_multiplier,
                                "batch_size": batch_size,
                            },
                            "aggregation": "concat",
                            "min_responses": num_shards,
                            "timeout_ms": 30000,
                        }
                    )
                except Exception as exc:
                    total_errors += 1
                    benchmark_results.append(
                        {
                            "shards": num_shards,
                            "total_requests": total_requests,
                            "logical_actor_count": logical_actor_count,
                            "payload_size_bytes": payload_size_bytes,
                            "model_type": model_type,
                            "work_multiplier": work_multiplier,
                            "worker_node_count": len(worker_nodes),
                            "remote_nodes_with_work": sorted(remote_nodes),
                            "shard_actor_ids": shard_actor_ids,
                            "error_count": total_errors,
                            "error": f"scatter_gather failed: {exc}",
                        }
                    )
                    break
                # Wall-clock coordination = full scatter_gather RTT
                rtt_ms = host.now_ms() - request_start
                total_coordination_ms += rtt_ms
                for shard in _extract_shard_responses(response):
                    payload = _unwrap_payload(shard.get("payload", {}))
                    if payload.get("status") == "ok":
                        latencies.append(int(payload.get("latency_ms", shard.get("latency_ms", 0))))
                        total_compute_ms += int(payload.get("compute_time_ms", 0))
                        node_id = str(payload.get("node_id", ""))
                        if node_id:
                            worker_nodes.add(node_id)
                            if node_id != leader_node_id:
                                remote_nodes.add(node_id)
                    else:
                        total_errors += 1

            if benchmark_results and benchmark_results[-1].get("shards") == num_shards and benchmark_results[-1].get("error"):
                continue

            if not latencies:
                benchmark_results.append(
                    {
                        "shards": num_shards,
                        "total_requests": total_requests,
                        "logical_actor_count": logical_actor_count,
                        "payload_size_bytes": payload_size_bytes,
                        "model_type": model_type,
                        "work_multiplier": work_multiplier,
                        "worker_node_count": len(worker_nodes),
                        "remote_nodes_with_work": sorted(remote_nodes),
                        "shard_actor_ids": shard_actor_ids,
                        "error_count": total_errors,
                        "error": "no successful shard responses",
                    }
                )
                continue

            elapsed_ms = host.now_ms() - bench_start
            elapsed_s = elapsed_ms / 1000.0 if elapsed_ms > 0 else 0.001
            throughput_rps = total_requests / elapsed_s if elapsed_s > 0 else 0.0
            sorted_latencies = sorted(latencies)
            avg_latency = sum(latencies) / len(latencies)
            total_measured_ms = total_compute_ms + total_coordination_ms
            compute_pct = (total_compute_ms * 100.0 / total_measured_ms) if total_measured_ms else 0.0
            coordination_pct = (total_coordination_ms * 100.0 / total_measured_ms) if total_measured_ms else 0.0
            granularity_ratio = (total_compute_ms / total_coordination_ms) if total_coordination_ms else float(total_compute_ms)

            if baseline is None:
                baseline = (num_shards, throughput_rps)
                parallel_efficiency_pct = 100.0
            else:
                baseline_shards, baseline_throughput = baseline
                ideal = baseline_throughput * (num_shards / baseline_shards) if baseline_shards else 0.0
                parallel_efficiency_pct = ((throughput_rps / ideal) * 100.0) if ideal else 0.0

            benchmark_results.append(
                {
                    "shards": num_shards,
                    "batch_size": batch_size,
                    "scatter_gather_rounds": scatter_gather_rounds,
                    "total_requests": total_requests,
                    "weak_scaling": weak_scaling,
                    "throughput_rps": _round2(throughput_rps),
                    "avg_latency_ms": _round2(avg_latency),
                    "p50_latency_ms": _percentile(sorted_latencies, 0.50),
                    "p95_latency_ms": _percentile(sorted_latencies, 0.95),
                    "p99_latency_ms": _percentile(sorted_latencies, 0.99),
                    "wall_time_ms": elapsed_ms,
                    "compute_time_ms": total_compute_ms,
                    "coordination_time_ms": total_coordination_ms,
                    "compute_pct": _round2(compute_pct),
                    "coordination_pct": _round2(coordination_pct),
                    "granularity_ratio": _round2(granularity_ratio),
                    "parallel_efficiency_pct": _round2(parallel_efficiency_pct),
                    "error_count": total_errors,
                    "logical_actor_count": logical_actor_count,
                    "payload_size_bytes": payload_size_bytes,
                    "model_type": model_type,
                    "work_multiplier": work_multiplier,
                    "worker_node_count": len(worker_nodes),
                    "remote_nodes_with_work": sorted(remote_nodes),
                    "shard_actor_ids": shard_actor_ids,
                }
            )

        return {"status": "ok", "benchmark": benchmark_name, "results": benchmark_results}

    @handler("run_shard_benchmark")
    def run_shard_benchmark(
        self,
        shard_counts: list = None,
        requests_per_shard: int = 10,
        warmup_requests: int = 0,
        logical_actor_count: int = 0,
        payload_size_bytes: int = 8192,
        model_type: str = "small",
        work_multiplier: int = 1,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [1, 2, 4, 8]
        if not isinstance(requests_per_shard, int):
            requests_per_shard = int(requests_per_shard)
        if not isinstance(warmup_requests, int):
            warmup_requests = int(warmup_requests)
        if not isinstance(logical_actor_count, int):
            logical_actor_count = int(logical_actor_count)
        if not isinstance(payload_size_bytes, int):
            payload_size_bytes = int(payload_size_bytes)
        if not isinstance(work_multiplier, int):
            work_multiplier = int(work_multiplier)

        self.benchmark_running = True
        result = self._run_shard_suite(
            shard_counts,
            requests_per_shard,
            warmup_requests,
            logical_actor_count,
            payload_size_bytes,
            model_type,
            max(1, work_multiplier),
            "shard",
        )
        self.results.append(result)
        self.benchmark_running = False

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {"shard_benchmarks_run": 1},
                    "latency_totals_ms": {},
                    "latency_max_ms": {},
                    "latency_samples": {},
                },
            )
        except Exception:
            pass

        return result

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        shard_counts: list = None,
        requests_per_shard: int = 8,
        warmup_requests: int = 2,
        logical_actor_count: int = 1000,
        payload_size_bytes: int = 65536,
        model_type: str = "large",
        work_multiplier: int = 20,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = _default_scaling_shards()
        if not isinstance(requests_per_shard, int):
            requests_per_shard = int(requests_per_shard)
        if not isinstance(warmup_requests, int):
            warmup_requests = int(warmup_requests)
        if not isinstance(logical_actor_count, int):
            logical_actor_count = int(logical_actor_count)
        if not isinstance(payload_size_bytes, int):
            payload_size_bytes = int(payload_size_bytes)
        if not isinstance(work_multiplier, int):
            work_multiplier = int(work_multiplier)

        self.benchmark_running = True
        result = self._run_shard_suite(
            shard_counts,
            requests_per_shard,
            warmup_requests,
            logical_actor_count,
            payload_size_bytes,
            model_type,
            max(1, work_multiplier),
            "scaling",
        )
        self.results.append(result)
        self.benchmark_running = False

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {"scaling_benchmarks_run": 1},
                    "latency_totals_ms": {},
                    "latency_max_ms": {},
                    "latency_samples": {},
                },
            )
        except Exception:
            pass

        return result

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        shard_counts: list = None,
        requests_per_shard: int = 4,
        warmup_requests: int = 2,
        logical_actor_count: int = 50,
        payload_size_bytes: int = 262144,
        model_type: str = "large",
        work_multiplier: int = 10,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = _default_scaling_shards()
        if not isinstance(requests_per_shard, int):
            requests_per_shard = int(requests_per_shard)
        if not isinstance(warmup_requests, int):
            warmup_requests = int(warmup_requests)
        if not isinstance(logical_actor_count, int):
            logical_actor_count = int(logical_actor_count)
        if not isinstance(payload_size_bytes, int):
            payload_size_bytes = int(payload_size_bytes)
        if not isinstance(work_multiplier, int):
            work_multiplier = int(work_multiplier)

        self.benchmark_running = True
        result = self._run_shard_suite(
            shard_counts,
            requests_per_shard,
            warmup_requests,
            logical_actor_count,
            payload_size_bytes,
            model_type,
            max(1, work_multiplier),
            "weak_scaling",
            weak_scaling=True,
        )
        self.results.append(result)
        self.benchmark_running = False

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {"weak_scaling_benchmarks_run": 1},
                    "latency_totals_ms": {},
                    "latency_max_ms": {},
                    "latency_samples": {},
                },
            )
        except Exception:
            pass

        return result

    @handler("run_pool_benchmark")
    def run_pool_benchmark(
        self,
        pool_name: str = "inference-pool",
        total_requests: int = 20,
        from_actor: str = "",
    ) -> dict:
        if not isinstance(total_requests, int):
            total_requests = int(total_requests)

        self.benchmark_running = True
        wait_times: List[int] = []
        exec_times: List[int] = []
        successful = 0
        failed = 0

        bench_start = host.now_ms()

        for i in range(total_requests):
            checkout_start = host.now_ms()
            checkout = host.pool_checkout(pool_name, timeout_ms=5000)
            wait_ms = host.now_ms() - checkout_start

            if not checkout:
                failed += 1
                continue

            actor_id = checkout.get("actor_id")
            checkout_id = checkout.get("checkout_id")

            exec_start = host.now_ms()
            try:
                host.ask(
                    actor_id,
                    {
                        "op": "infer",
                        "request_id": f"pool-{i}",
                        "input": "pool-sample",
                    },
                    timeout_ms=10000,
                )
                exec_ms = host.now_ms() - exec_start
                exec_times.append(exec_ms)
                successful += 1
            except Exception:
                exec_ms = host.now_ms() - exec_start
                failed += 1
            finally:
                host.pool_checkin(pool_name, actor_id, checkout_id, healthy=(failed == 0))

            wait_times.append(wait_ms)

        elapsed_ms = host.now_ms() - bench_start
        avg_wait = sum(wait_times) / len(wait_times) if wait_times else 0
        avg_exec = sum(exec_times) / len(exec_times) if exec_times else 0
        utilization = successful / total_requests if total_requests > 0 else 0

        self.benchmark_running = False
        result = {
            "status": "ok",
            "benchmark": "pool",
            "pool_name": pool_name,
            "total_requests": total_requests,
            "successful": successful,
            "failed": failed,
            "avg_wait_ms": round(avg_wait, 2),
            "avg_exec_ms": round(avg_exec, 2),
            "pool_utilization": round(utilization, 3),
            "elapsed_ms": elapsed_ms,
        }
        self.results.append(result)

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {"pool_benchmarks_run": 1},
                    "latency_totals_ms": {},
                    "latency_max_ms": {},
                    "latency_samples": {},
                },
            )
        except Exception:
            pass

        return result

    @handler("run_collective_benchmark")
    def run_collective_benchmark(self, num_shards: int = 4, from_actor: str = "") -> dict:
        if not isinstance(num_shards, int):
            num_shards = int(num_shards)

        self.benchmark_running = True
        group_id = f"bench-collective-{host.now_ms()}"
        group = host.create_shard_group(
            {
                "group_id": group_id,
                "actor_type": "inference_worker",
                "shard_count": num_shards,
                "partition_strategy": "hash",
                "rebalance_policy": "manual",
                "placement": {"strategy": "from_registry"},
                "initial_state": {},
            }
        )
        shard_actor_ids = group.get("shard_actor_ids", [])
        if not shard_actor_ids:
            self.benchmark_running = False
            return {"status": "error", "error": "failed to create shard group for collective benchmark"}

        timings: Dict[str, int] = {}

        # 1. BroadcastShardGroup – distribute reset signal to all workers
        t0 = host.now_ms()
        broadcast_result = host.broadcast_shard_group(
            {
                "group_id": group_id,
                "message": {"op": "reset"},
                "min_acks": num_shards,
                "timeout_ms": 10000,
            }
        )
        timings["broadcast_ms"] = host.now_ms() - t0

        # 2. BarrierShardGroup – wait for all workers to be ready
        t0 = host.now_ms()
        barrier_result = host.barrier_shard_group(
            {
                "group_id": group_id,
                "timeout_ms": 10000,
            }
        )
        timings["barrier_ms"] = host.now_ms() - t0

        # 3. ReduceShardGroup – aggregate total requests across all workers (scalar SUM)
        # target extracts a single numeric field from each shard's response before reduction
        t0 = host.now_ms()
        reduce_result = host.reduce_shard_group(
            {
                "group_id": group_id,
                "map_function": {"op": "get_numeric_stats"},
                "reduction": "sum",
                "target": "requests_processed",
                "timeout_ms": 10000,
            }
        )
        timings["reduce_ms"] = host.now_ms() - t0

        # 4. AllReduceShardGroup – consensus total requests (broadcast reduced scalar back to all shards)
        t0 = host.now_ms()
        allreduce_result = host.all_reduce_shard_group(
            {
                "group_id": group_id,
                "map_function": {"op": "get_numeric_stats"},
                "reduction": "sum",
                "target": "requests_processed",
                "timeout_ms": 10000,
            }
        )
        timings["allreduce_ms"] = host.now_ms() - t0

        self.benchmark_running = False
        result = {
            "status": "ok",
            "benchmark": "collective",
            "num_shards": num_shards,
            "shard_actor_ids": shard_actor_ids,
            "timings": timings,
            "broadcast_acks": broadcast_result.get("acks", 0) if isinstance(broadcast_result, dict) else 0,
            "barrier_reached": barrier_result.get("reached", False) if isinstance(barrier_result, dict) else False,
            "reduce_responses": reduce_result.get("responses", 0) if isinstance(reduce_result, dict) else 0,
            "allreduce_responses": allreduce_result.get("responses", 0) if isinstance(allreduce_result, dict) else 0,
        }
        self.results.append(result)

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {"collective_benchmarks_run": 1},
                    "latency_totals_ms": {
                        "broadcast": timings["broadcast_ms"],
                        "barrier": timings["barrier_ms"],
                        "reduce": timings["reduce_ms"],
                        "allreduce": timings["allreduce_ms"],
                    },
                    "latency_max_ms": {
                        "broadcast": timings["broadcast_ms"],
                        "barrier": timings["barrier_ms"],
                        "reduce": timings["reduce_ms"],
                        "allreduce": timings["allreduce_ms"],
                    },
                    "latency_samples": {
                        "broadcast": 1,
                        "barrier": 1,
                        "reduce": 1,
                        "allreduce": 1,
                    },
                },
            )
        except Exception:
            pass

        return result

    @handler("get_results")
    def get_results(self, from_actor: str = "") -> dict:
        return {
            "status": "ok",
            "results": list(self.results),
            "total_benchmarks": len(self.results),
        }


# ─────────────────────────────────────────────────────────────────────────────
# OrchestratorWorkflow
# Role: "orchestrator"
# Workflow actor that coordinates multi-mode parallel inference pipelines.
# ─────────────────────────────────────────────────────────────────────────────


@workflow_actor(facets=["virtual_actor", "durability"])
class OrchestratorWorkflow:
    mode: str = state(default="shard")
    shard_group_id: str = state(default="")
    total_processed: int = state(default=0)
    metrics: dict = state(default_factory=dict)
    new_shards_requested: int = state(default=0)

    @run_handler
    def start(
        self,
        mode: str = "shard",
        num_shards: int = 4,
        num_requests: int = 20,
    ) -> dict:
        if not isinstance(num_shards, int):
            num_shards = int(num_shards)
        if not isinstance(num_requests, int):
            num_requests = int(num_requests)

        self.mode = str(mode)
        group_id = f"orch-{mode}-{host.now_ms()}"
        self.shard_group_id = group_id

        if mode == "shard":
            return self._run_shard_mode(group_id, num_shards, num_requests)
        elif mode == "pool":
            return self._run_pool_mode(num_requests)
        elif mode == "collective":
            return self._run_collective_mode(group_id, num_shards)
        else:
            return {"status": "error", "error": f"unknown mode: {mode}"}

    def _run_shard_mode(self, group_id: str, num_shards: int, num_requests: int) -> dict:
        group = host.create_shard_group(
            {
                "group_id": group_id,
                "actor_type": "inference_worker",
                "shard_count": num_shards,
                "partition_strategy": "hash",
                "rebalance_policy": "manual",
                "placement": {"strategy": "from_registry"},
                "initial_state": {},
            }
        )
        shard_actor_ids = group.get("shard_actor_ids", [])
        if not shard_actor_ids:
            return {"status": "error", "error": "failed to create shard group"}

        results = []
        total_latency = 0
        total_ok = 0

        for i in range(num_requests):
            response = host.scatter_gather(
                {
                    "group_id": group_id,
                    "query": {
                        "op": "infer",
                        "request_id": f"orch-{i}",
                        "input": "orchestrated-input",
                    },
                    "aggregation": "concat",
                    "min_responses": num_shards,
                    "timeout_ms": 30000,
                }
            )
            for shard in _extract_shard_responses(response):
                payload = _unwrap_payload(shard.get("payload", {}))
                if payload.get("status") == "ok":
                    total_ok += 1
                    total_latency += int(payload.get("latency_ms", 0))
                    results.append(payload.get("result", ""))

        self.total_processed = total_ok
        avg_latency = total_latency / total_ok if total_ok > 0 else 0
        self.metrics = {
            "total_ok": total_ok,
            "avg_latency_ms": round(avg_latency, 2),
            "num_shards": num_shards,
        }

        return {
            "status": "ok",
            "mode": "shard",
            "total_processed": self.total_processed,
            "num_shards": num_shards,
            "shard_actor_ids": shard_actor_ids,
            "metrics": self.metrics,
            "sample_results": results[:5],
        }

    def _run_pool_mode(self, num_requests: int) -> dict:
        pool_name = "inference-pool"
        successful = 0
        failed = 0
        total_latency = 0

        for i in range(num_requests):
            checkout = host.pool_checkout(pool_name, timeout_ms=5000)
            if not checkout:
                failed += 1
                continue

            actor_id = checkout.get("actor_id")
            checkout_id = checkout.get("checkout_id")
            healthy = True

            t0 = host.now_ms()
            try:
                host.ask(
                    actor_id,
                    {"op": "infer", "request_id": f"pool-orch-{i}", "input": "pool-input"},
                    timeout_ms=10000,
                )
                total_latency += host.now_ms() - t0
                successful += 1
            except Exception:
                failed += 1
                healthy = False
            finally:
                host.pool_checkin(pool_name, actor_id, checkout_id, healthy=healthy)

        self.total_processed = successful
        avg_latency = total_latency / successful if successful > 0 else 0
        self.metrics = {
            "successful": successful,
            "failed": failed,
            "avg_latency_ms": round(avg_latency, 2),
        }

        return {
            "status": "ok",
            "mode": "pool",
            "total_processed": self.total_processed,
            "metrics": self.metrics,
        }

    def _run_collective_mode(self, group_id: str, num_shards: int) -> dict:
        group = host.create_shard_group(
            {
                "group_id": group_id,
                "actor_type": "inference_worker",
                "shard_count": num_shards,
                "partition_strategy": "hash",
                "rebalance_policy": "manual",
                "placement": {"strategy": "from_registry"},
                "initial_state": {},
            }
        )
        shard_actor_ids = group.get("shard_actor_ids", [])
        if not shard_actor_ids:
            return {"status": "error", "error": "failed to create shard group for collective mode"}

        timings: Dict[str, int] = {}

        # Step 1: Broadcast model config reset to all workers
        t0 = host.now_ms()
        host.broadcast_shard_group(
            {
                "group_id": group_id,
                "message": {"op": "reset"},
                "min_acks": num_shards,
                "timeout_ms": 10000,
            }
        )
        timings["broadcast_ms"] = host.now_ms() - t0

        # Step 2: Barrier — synchronize all workers before inference
        t0 = host.now_ms()
        host.barrier_shard_group({"group_id": group_id, "timeout_ms": 10000})
        timings["barrier_ms"] = host.now_ms() - t0

        # Step 3: Scatter-gather one round of inference
        t0 = host.now_ms()
        response = host.scatter_gather(
            {
                "group_id": group_id,
                "query": {
                    "op": "infer",
                    "request_id": "collective-infer-0",
                    "input": "collective-input",
                },
                "aggregation": "concat",
                "min_responses": num_shards,
                "timeout_ms": 30000,
            }
        )
        timings["scatter_gather_ms"] = host.now_ms() - t0
        total_ok = sum(
            1
            for shard in _extract_shard_responses(response)
            if _unwrap_payload(shard.get("payload", {})).get("status") == "ok"
        )

        # Step 4: Reduce — aggregate numeric metrics across all workers
        t0 = host.now_ms()
        host.reduce_shard_group(
            {
                "group_id": group_id,
                "map_function": {"op": "get_numeric_stats"},
                "reduction": "sum",
                "target": "requests_processed",
                "timeout_ms": 10000,
            }
        )
        timings["reduce_ms"] = host.now_ms() - t0

        self.total_processed = total_ok
        self.metrics = {"timings": timings, "total_ok": total_ok, "num_shards": num_shards}

        return {
            "status": "ok",
            "mode": "collective",
            "total_processed": self.total_processed,
            "num_shards": num_shards,
            "shard_actor_ids": shard_actor_ids,
            "timings": timings,
            "metrics": self.metrics,
        }

    @signal_handler("scale")
    def scale(self, new_shards: int = 4) -> None:
        if not isinstance(new_shards, int):
            new_shards = int(new_shards)
        self.new_shards_requested = new_shards

    @query_handler("status")
    def status(self) -> dict:
        return {
            "mode": self.mode,
            "total_processed": self.total_processed,
            "metrics": dict(self.metrics),
            "shard_group_id": self.shard_group_id,
            "new_shards_requested": self.new_shards_requested,
        }


