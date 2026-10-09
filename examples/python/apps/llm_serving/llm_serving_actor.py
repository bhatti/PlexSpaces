# SPDX-License-Identifier: AGPL-3.0-or-later
#
# LLM Serving Pipeline - Python WASM
#
# Production-grade LLM serving: multi-model tiers (small/medium/large),
# cost-aware routing, dynamic request batching, health monitoring,
# supervision for model actors, compute vs coordination metrics.
#
# Real-world analog: Ray Serve, vLLM, TGI, SageMaker endpoints

import json
import math
import time
from typing import Any, Dict, List, Optional

from plexspaces import (
    ActorID,
    actor,
    handler,
    host,
    init_handler,
    state,
)
from plexspaces import CreateShardGroupRequest, ScatterGatherRequest, NodePlacement

ACTOR_ROLES = {}


def actor_application_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).namespace
    except ValueError:
        return ""


def actor_node_id(actor_id: str) -> str:
    try:
        return ActorID.parse(actor_id).node_id or "local"
    except ValueError:
        return "local"


def register_role(role_name):
    def decorator(cls):
        ACTOR_ROLES[role_name] = cls
        return cls
    return decorator


# ─── Model Tier Definitions ──────────────────────────────────────────────────

MODEL_TIERS = {
    "small": {
        "name": "plx-small-7b",
        "params": "7B",
        "latency_ms": 10,
        "tokens_per_sec": 500,
        "cost_per_1k_tokens": 0.02,
        "max_context": 4096,
        "tasks": ["classification", "extraction", "simple_qa"],
    },
    "medium": {
        "name": "plx-medium-30b",
        "params": "30B",
        "latency_ms": 50,
        "tokens_per_sec": 200,
        "cost_per_1k_tokens": 0.10,
        "max_context": 8192,
        "tasks": ["summarization", "translation", "reasoning"],
    },
    "large": {
        "name": "plx-large-70b",
        "params": "70B",
        "latency_ms": 200,
        "tokens_per_sec": 80,
        "cost_per_1k_tokens": 0.50,
        "max_context": 32768,
        "tasks": ["generation", "code", "complex_reasoning", "creative"],
    },
}

TASK_TO_TIER = {
    "classification": "small",
    "extraction": "small",
    "simple_qa": "small",
    "summarization": "medium",
    "translation": "medium",
    "reasoning": "medium",
    "generation": "large",
    "code": "large",
    "complex_reasoning": "large",
    "creative": "large",
}


# ─── Request Generation ─────────────────────────────────────────────────────

def generate_requests(count, seed=42):
    tasks = list(TASK_TO_TIER.keys())
    prompts = [
        "Classify this email as spam or not.",
        "Extract the key entities from this document.",
        "What is the capital of France?",
        "Summarize the following 2000-word article.",
        "Translate this paragraph to Spanish.",
        "What are the logical implications of this argument?",
        "Write a 500-word blog post about distributed systems.",
        "Implement a binary search in Python.",
        "Given the following premises, what can we conclude?",
        "Write a creative short story about time travel.",
    ]
    users = [f"user-{i}" for i in range(20)]

    requests = []
    rng = seed
    for i in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        task = tasks[rng % len(tasks)]
        prompt = prompts[(rng >> 4) % len(prompts)]
        user = users[(rng >> 8) % len(users)]
        input_tokens = 50 + (rng % 200)
        max_output_tokens = 50 + ((rng >> 12) % 500)

        tier = None
        if (rng >> 16) % 5 == 0:
            tier = TASK_TO_TIER[task]

        requests.append({
            "request_id": f"req-{i}",
            "task": task,
            "prompt": prompt,
            "user_id": user,
            "input_tokens": input_tokens,
            "max_output_tokens": max_output_tokens,
            "tier": tier,
            "priority": "high" if (rng >> 20) % 10 == 0 else "normal",
        })
    return requests


# ─── Synthetic Token Generation ──────────────────────────────────────────────

def generate_tokens(count, seed=0):
    words = [
        "the", "a", "is", "was", "are", "been", "will", "have", "has", "do",
        "data", "model", "system", "process", "network", "cloud", "scale",
        "distributed", "pipeline", "actor", "message", "state", "event",
        "function", "service", "query", "result", "response", "request",
        "latency", "throughput", "batch", "stream", "cache", "index",
    ]
    rng = seed
    tokens = []
    for _ in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        tokens.append(words[rng % len(words)])
    return " ".join(tokens)


# ─── Router Actor (Leader) ───────────────────────────────────────────────────

@register_role("leader")
@actor
class RouterActor:
    total_compute_ms: int = state(default=0)
    total_coord_ms: int = state(default=0)
    model_health: dict = state(default_factory=dict)
    routing_stats: dict = state(default_factory=lambda: {"small": 0, "medium": 0, "large": 0})
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    def _route_request(self, request):
        if request.get("tier"):
            return request["tier"]
        task = request.get("task", "generation")
        tier = TASK_TO_TIER.get(task, "large")
        input_tokens = request.get("input_tokens", 100)
        if input_tokens > 8192 and tier == "small":
            tier = "medium"
        if input_tokens > 16384 and tier == "medium":
            tier = "large"
        return tier

    @handler("run")
    def run(
        self,
        request_count: int = 1000,
        worker_count: int = 6,
        batch_size: int = 16,
        max_wait_ms: int = 50,
        from_actor: str = "",
    ) -> dict:
        coord_start = host.now_ms()
        group_name = f"llm-models-{host.now_ms()}"
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=group_name,
            actor_type="ModelActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        shard_group_id = group.group_id
        shard_ids = group.shard_actor_ids
        if not shard_ids:
            return {"status": "error", "error": "failed to create model shard group"}
        coord_create = host.now_ms() - coord_start

        requests = generate_requests(request_count, seed=host.now_ms() % 100000)

        total_compute_ms = 0
        total_coord_ms = coord_create
        total_tokens_generated = 0
        total_requests_served = 0
        total_cost = 0.0
        error_count = 0
        tier_counts = {"small": 0, "medium": 0, "large": 0}
        tier_latencies = {"small": [], "medium": [], "large": []}
        batch_counts = 0

        tier_assignments = {"small": [], "medium": [], "large": []}
        for req in requests:
            tier = self._route_request(req)
            tier_assignments[tier].append(req)
            tier_counts[tier] += 1

        for tier, tier_reqs in tier_assignments.items():
            for batch_start in range(0, len(tier_reqs), batch_size):
                batch = tier_reqs[batch_start:batch_start + batch_size]
                batch_counts += 1

                sg_start = host.now_ms()
                sg_result = host.scatter_gather(ScatterGatherRequest(
                    group_id=shard_group_id,
                    query={
                        "op": "process_batch",
                        "batch": batch,
                        "tier": tier,
                        "batch_size": batch_size,
                        "max_wait_ms": max_wait_ms,
                    },
                    timeout_ms=30000,
                ))
                sg_elapsed = host.now_ms() - sg_start
                total_coord_ms += sg_elapsed

                for resp in sg_result.shard_responses:
                    result = resp
                    for key in ["payload", "result", "response", "data"]:
                        if isinstance(result, dict) and key in result:
                            result = result[key]
                    if isinstance(result, dict):
                        if "error" in result:
                            error_count += 1
                            continue
                        total_compute_ms += result.get("compute_ms", 0)
                        total_tokens_generated += result.get("tokens_generated", 0)
                        total_requests_served += result.get("requests_served", 0)
                        total_cost += result.get("cost", 0.0)
                        for lat in result.get("latencies", []):
                            t = lat.get("tier", tier)
                            if t in tier_latencies:
                                tier_latencies[t].append(lat.get("latency_ms", 0))

        wall_time = total_compute_ms + total_coord_ms
        total = total_compute_ms + total_coord_ms or 1
        granularity = round(total_compute_ms / total_coord_ms, 1) if total_coord_ms > 0 else 0
        requests_per_sec = round(total_requests_served * 1000 / wall_time) if wall_time > 0 else 0
        tokens_per_sec = round(total_tokens_generated * 1000 / wall_time) if wall_time > 0 else 0
        batch_efficiency = round(total_requests_served / batch_counts, 2) if batch_counts > 0 else 0

        tier_stats = {}
        for tier, lats in tier_latencies.items():
            if lats:
                lats_sorted = sorted(lats)
                tier_stats[tier] = {
                    "count": tier_counts.get(tier, 0),
                    "avg_latency_ms": round(sum(lats) / len(lats), 1),
                    "p50_latency_ms": lats_sorted[len(lats_sorted) // 2],
                    "p99_latency_ms": lats_sorted[min(int(len(lats_sorted) * 0.99), len(lats_sorted) - 1)],
                }
            else:
                tier_stats[tier] = {"count": tier_counts.get(tier, 0), "avg_latency_ms": 0}

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "router_requests_served": total_requests_served,
                        "router_tokens_generated": total_tokens_generated,
                        "router_errors": error_count,
                    },
                    "latency_totals_ms": {
                        "router.compute": total_compute_ms,
                        "router.coordination": total_coord_ms,
                    },
                    "latency_max_ms": {
                        "router.compute": total_compute_ms,
                        "router.coordination": total_coord_ms,
                    },
                    "latency_samples": {
                        "router.compute": 1,
                        "router.coordination": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "status": "ok",
            "request_count": request_count,
            "worker_count": worker_count,
            "batch_size": batch_size,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "requests_per_sec": requests_per_sec,
            "tokens_per_sec": tokens_per_sec,
            "total_tokens_generated": total_tokens_generated,
            "total_requests_served": total_requests_served,
            "total_cost": round(total_cost, 4),
            "batch_count": batch_counts,
            "batch_efficiency": batch_efficiency,
            "tier_distribution": tier_counts,
            "tier_stats": tier_stats,
            "error_count": error_count,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(
        self,
        request_count: int = 1000,
        shard_counts: list = None,
        batch_size: int = 16,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion: 2 scatter-gather calls per timing pass
        TIMING_SG = 2

        results = []
        baseline_rps = 0

        for shard_count in shard_counts:
            # Strong scaling: each shard processes request_count // shard_count requests.
            # Total work = request_count (fixed), divided evenly so per-shard work shrinks with N.
            reqs_per_shard = max(1, request_count // shard_count)
            bench_reqs = [
                {
                    "request_id": f"bench-{i}",
                    "task": "summarization",
                    "input_tokens": 100 + (i % 50),
                    "max_output_tokens": 80 + (i % 40),
                    "tier": "medium",
                }
                for i in range(reqs_per_shard)
            ]

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"llm-bench-{shard_count}-{host.now_ms()}",
                actor_type="ModelActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            # 1 warmup scatter-gather (discard timing)
            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_batch", "batch": bench_reqs[:min(4, len(bench_reqs))],
                       "tier": "medium", "batch_size": batch_size, "max_wait_ms": 50},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0
            total_tokens = 0
            total_served = 0

            for _bi in range(benchmark_rounds):
                iter_start = host.now_ms()
                iter_compute = 0
                iter_tokens = 0
                iter_served = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_batch", "batch": bench_reqs,
                               "tier": "medium", "batch_size": batch_size, "max_wait_ms": 50},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)
                            iter_tokens += r.get("tokens_generated", 0)
                            iter_served += r.get("requests_served", 0)

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute
                total_tokens += iter_tokens
                total_served += iter_served

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)
            avg_tokens = total_tokens // benchmark_rounds if benchmark_rounds > 0 else total_tokens

            # Aggregate throughput: TIMING_SG × reqs_per_shard × shard_count per benchmark iteration
            actual_reqs = TIMING_SG * reqs_per_shard * shard_count
            avg_rps = actual_reqs * 1000 // avg_wall if avg_wall > 0 else 0
            avg_tps = avg_tokens * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_rps == 0:
                baseline_rps = avg_rps or 1

            speedup_100 = avg_rps * 100 // baseline_rps if baseline_rps > 0 else 100
            speedup = speedup_100 / 100
            eff_100 = speedup_100 * shard_counts[0] // shard_count if shard_count > 0 else 100

            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "replicas": shard_count,
                "requests_per_sec": avg_rps,
                "tokens_per_sec": avg_tps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran,
                "speedup": speedup,
                "efficiency_pct": eff_100,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "request_count": request_count,
            "batch_size": batch_size,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(
        self,
        requests_per_replica: int = 500,
        shard_counts: list = None,
        batch_size: int = 16,
        warmup_rounds: int = 1,
        benchmark_rounds: int = 2,
        from_actor: str = "",
    ) -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Cap to prevent WASM heap exhaustion
        TIMING_SG = 2

        results = []
        baseline_rps = 0

        for shard_count in shard_counts:
            # Weak scaling: fixed requests_per_replica per shard; total work grows with N
            bench_reqs = [
                {
                    "request_id": f"bench-{i}",
                    "task": "summarization",
                    "input_tokens": 100 + (i % 50),
                    "max_output_tokens": 80 + (i % 40),
                    "tier": "medium",
                }
                for i in range(requests_per_replica)
            ]

            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=f"llm-weak-{shard_count}-{host.now_ms()}",
                actor_type="ModelActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            group_id = group.group_id

            host.scatter_gather(ScatterGatherRequest(
                group_id=group_id,
                query={"op": "process_batch", "batch": bench_reqs[:min(4, len(bench_reqs))],
                       "tier": "medium", "batch_size": batch_size, "max_wait_ms": 50},
                timeout_ms=30000,
            ))

            total_wall = 0
            total_compute = 0
            total_tokens = 0
            total_served = 0

            for _bi in range(benchmark_rounds):
                iter_start = host.now_ms()
                iter_compute = 0
                iter_tokens = 0
                iter_served = 0

                for _sg in range(TIMING_SG):
                    sg_result = host.scatter_gather(ScatterGatherRequest(
                        group_id=group_id,
                        query={"op": "process_batch", "batch": bench_reqs,
                               "tier": "medium", "batch_size": batch_size, "max_wait_ms": 50},
                        timeout_ms=30000,
                    ))
                    for resp in sg_result.shard_responses:
                        r = resp
                        for key in ["payload", "result", "response", "data"]:
                            if isinstance(r, dict) and key in r:
                                r = r[key]
                        if isinstance(r, dict):
                            iter_compute += r.get("compute_ms", 0)
                            iter_tokens += r.get("tokens_generated", 0)
                            iter_served += r.get("requests_served", 0)

                iter_wall = host.now_ms() - iter_start
                total_wall += iter_wall
                total_compute += iter_compute
                total_tokens += iter_tokens
                total_served += iter_served

            avg_wall = total_wall // benchmark_rounds if benchmark_rounds > 0 else total_wall
            avg_compute = total_compute // benchmark_rounds if benchmark_rounds > 0 else total_compute
            avg_coord = max(1, avg_wall - avg_compute)
            avg_tokens = total_tokens // benchmark_rounds if benchmark_rounds > 0 else total_tokens

            actual_reqs = TIMING_SG * requests_per_replica * shard_count
            avg_rps = actual_reqs * 1000 // avg_wall if avg_wall > 0 else 0
            avg_tps = avg_tokens * 1000 // avg_wall if avg_wall > 0 else 0

            if baseline_rps == 0:
                baseline_rps = avg_rps or 1

            eff_100 = avg_rps * 100 // baseline_rps if baseline_rps > 0 else 100

            gran_10 = avg_compute * 10 // avg_coord if avg_coord > 0 else 0
            gran = gran_10 / 10

            results.append({
                "replicas": shard_count,
                "total_requests": TIMING_SG * requests_per_replica * shard_count,
                "requests_per_sec": avg_rps,
                "tokens_per_sec": avg_tps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": gran,
                "efficiency_pct": eff_100,
                "error_count": 0,
            })

        return {
            "status": "ok",
            "requests_per_replica": requests_per_replica,
            "batch_size": batch_size,
            "results": results,
        }


# ─── Model Actor (Worker) ───────────────────────────────────────────────────

@register_role("worker")
@actor
class ModelActor:
    requests_processed: int = state(default=0)
    tokens_generated: int = state(default=0)
    compute_ms: int = state(default=0)
    failures: int = state(default=0)
    health_status: str = state(default="healthy")
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    def _simulate_inference(self, request, tier_info, seed):
        rng = seed
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF

        # Simulate failure (~2%)
        if rng % 50 == 0:
            return {
                "request_id": request["request_id"],
                "status": "failed",
                "error": "OOM" if rng % 2 == 0 else "timeout",
                "latency_ms": 0,
                "tokens": 0,
            }

        base_latency = tier_info["latency_ms"]
        jitter = (rng % 20) - 10
        latency = max(1, base_latency + jitter)

        # Higher latency for larger inputs
        input_tokens = request.get("input_tokens", 100)
        if input_tokens > tier_info["max_context"] // 2:
            latency = int(latency * 1.5)

        max_output = request.get("max_output_tokens", 100)
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        output_tokens = min(max_output, 20 + (rng % max(1, max_output - 20)))

        cost = output_tokens * tier_info["cost_per_1k_tokens"] / 1000

        return {
            "request_id": request["request_id"],
            "status": "ok",
            "tier": request.get("tier_used", "unknown"),
            "latency_ms": latency,
            "tokens": output_tokens,
            "cost": round(cost, 6),
            "model": tier_info["name"],
        }

    @handler("process_batch")
    def process_batch(
        self,
        batch: list = None,
        tier: str = "medium",
        batch_size: int = 16,
        max_wait_ms: int = 50,
        from_actor: str = "",
    ) -> dict:
        if batch is None:
            batch = []
        comp_start = host.now_ms()

        tier_info = MODEL_TIERS.get(tier, MODEL_TIERS["medium"])

        results = []
        tokens_generated = 0
        total_cost = 0.0
        latencies = []
        failures = 0

        for i, req in enumerate(batch):
            req["tier_used"] = tier
            seed = (host.now_ms() + i * 7919) % 2147483647
            result = self._simulate_inference(req, tier_info, seed)

            if result["status"] == "ok":
                tokens_generated += result["tokens"]
                total_cost += result["cost"]
                latencies.append({"tier": tier, "latency_ms": result["latency_ms"]})
                results.append(result)
            else:
                failures += 1
                self.failures += 1

        compute_ms = host.now_ms() - comp_start
        self.requests_processed += len(batch) - failures
        self.tokens_generated += tokens_generated
        self.compute_ms += compute_ms

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "model_requests_processed": len(batch) - failures,
                        "model_tokens_generated": tokens_generated,
                        "model_failures": failures,
                    },
                    "latency_totals_ms": {
                        "model.compute": compute_ms,
                    },
                    "latency_max_ms": {
                        "model.compute": compute_ms,
                    },
                    "latency_samples": {
                        "model.compute": 1,
                    },
                },
            )
        except Exception:
            pass

        return {
            "requests_served": len(batch) - failures,
            "tokens_generated": tokens_generated,
            "cost": round(total_cost, 6),
            "compute_ms": compute_ms,
            "latencies": latencies,
            "failures": failures,
        }

    @handler("health_check")
    def health_check(self, from_actor: str = "") -> dict:
        return {
            "status": self.health_status,
            "requests_processed": self.requests_processed,
            "tokens_generated": self.tokens_generated,
            "total_compute_ms": self.compute_ms,
            "failures": self.failures,
        }
