# SPDX-License-Identifier: LGPL-2.1-or-later
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
    def infer(self, request_id: str = "", input: str = "", from_actor: str = "") -> dict:
        start_ms = host.now_ms()

        # Simulate compute work proportional to model size
        iterations = _MODEL_LATENCY_ITERATIONS.get(self.model_type, 5000)
        acc = 0
        for i in range(iterations):
            acc += i

        latency_ms = host.now_ms() - start_ms
        self.requests_processed += 1
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
                    "latency_totals_ms": {"inference": latency_ms},
                    "latency_max_ms": {"inference": latency_ms},
                    "latency_samples": {"inference": 1},
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
                "model_type": self.model_type,
            })
        except Exception:
            pass

        return {
            "status": "ok",
            "result": f"inference-result-{request_id}",
            "model": self.model_type,
            "latency_ms": latency_ms,
            "worker_id": self.worker_id,
            "acc": acc % 1000,  # Include to prevent optimizer from eliminating the loop
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

    @handler("run_shard_benchmark")
    def run_shard_benchmark(
        self,
        shard_counts: list = None,
        requests_per_shard: int = 10,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [1, 2, 4, 8]
        if not isinstance(requests_per_shard, int):
            requests_per_shard = int(requests_per_shard)

        self.benchmark_running = True
        benchmark_results = []

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
                benchmark_results.append(
                    {
                        "shards": num_shards,
                        "error": "failed to create shard group",
                    }
                )
                continue

            total_requests = num_shards * requests_per_shard
            latencies: List[int] = []
            bench_start = host.now_ms()

            for i in range(requests_per_shard):
                response = host.scatter_gather(
                    {
                        "group_id": group_id,
                        "query": {
                            "op": "infer",
                            "request_id": f"bench-{num_shards}-{i}",
                            "input": "sample-data",
                        },
                        "aggregation": "concat",
                        "min_responses": num_shards,
                        "timeout_ms": 30000,
                    }
                )
                for shard in _extract_shard_responses(response):
                    payload = _unwrap_payload(shard.get("payload", {}))
                    if payload.get("status") == "ok":
                        latencies.append(int(payload.get("latency_ms", 0)))

            elapsed_ms = host.now_ms() - bench_start
            elapsed_s = elapsed_ms / 1000.0 if elapsed_ms > 0 else 0.001
            throughput_rps = total_requests / elapsed_s if elapsed_s > 0 else 0

            avg_latency = sum(latencies) / len(latencies) if latencies else 0
            sorted_latencies = sorted(latencies)
            p99_idx = max(0, int(len(sorted_latencies) * 0.99) - 1)
            p99_latency = sorted_latencies[p99_idx] if sorted_latencies else 0

            benchmark_results.append(
                {
                    "shards": num_shards,
                    "total_requests": total_requests,
                    "throughput_rps": round(throughput_rps, 2),
                    "avg_latency_ms": round(avg_latency, 2),
                    "p99_latency_ms": p99_latency,
                    "elapsed_ms": elapsed_ms,
                }
            )

        self.benchmark_running = False
        result = {"status": "ok", "benchmark": "shard", "results": benchmark_results}
        self.results.append(result)

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


ACTOR_ROLES = {
    "inference_worker": InferenceWorkerActor,
    "benchmark": BenchmarkActor,
    "orchestrator": OrchestratorWorkflow,
    "metrics_event": MetricsEventActor,
    "circuit_breaker": WorkerCircuitBreakerFSM,
}
