# SPDX-License-Identifier: AGPL-3.0-or-later
#
# Distributed Tracing Pipeline - Python WASM
#
# OTLP span ingestion → trace assembly (out-of-order) → tail-based sampling
# (error-biased, latency-biased) → service graph construction.
# Leader/worker with shard-group placement, scatter/gather,
# compute vs coordination metrics tracking.
#
# Real-world analog: Jaeger, Zipkin, Datadog APM, AWS X-Ray, Honeycomb

import json
import math
import time

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


# ─── Span Generation ─────────────────────────────────────────────────────────

SERVICES = [
    {"name": "api-gateway", "operations": ["POST /orders", "GET /users", "POST /auth", "GET /health"]},
    {"name": "user-service", "operations": ["getUser", "validateToken", "listUsers", "updateProfile"]},
    {"name": "order-service", "operations": ["createOrder", "getOrder", "listOrders", "cancelOrder"]},
    {"name": "payment-service", "operations": ["processPayment", "refund", "getBalance", "validateCard"]},
    {"name": "inventory-service", "operations": ["checkStock", "reserveItem", "releaseItem", "updateStock"]},
    {"name": "notification-service", "operations": ["sendEmail", "sendSMS", "pushNotify", "logEvent"]},
]

# call graph: parent_service_idx -> list of (child_service_idx, probability)
CALL_GRAPH = {
    0: [(1, 0.8), (2, 0.9)],               # api-gateway -> user-service, order-service
    1: [],                                    # user-service (leaf)
    2: [(3, 0.7), (4, 0.85)],               # order-service -> payment, inventory
    3: [(5, 0.3)],                           # payment-service -> notification
    4: [],                                    # inventory-service (leaf)
    5: [],                                    # notification-service (leaf)
}

# base latency per service (ms)
SERVICE_LATENCY = [5, 8, 15, 25, 10, 3]


def generate_span_id(rng):
    rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
    return rng, f"{rng:08x}{((rng * 7 + 3) & 0xFFFFFFFF):08x}"


def generate_traces(count, seed=42):
    traces = []
    rng = seed
    for _ in range(count):
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        trace_id = f"{rng:08x}{((rng * 13 + 5) & 0xFFFFFFFF):08x}{((rng * 17 + 11) & 0xFFFFFFFF):08x}{((rng * 23 + 7) & 0xFFFFFFFF):08x}"
        base_time = int(host.now_ms()) + rng % 10000

        spans = []
        # generate root span at api-gateway
        rng, root_span_id = generate_span_id(rng)
        is_error = (rng % 20) == 0  # ~5% error rate

        def build_subtree(svc_idx, parent_span_id, depth, start_time):
            nonlocal rng, is_error
            svc = SERVICES[svc_idx]
            rng, span_id = generate_span_id(rng)
            op = svc["operations"][rng % len(svc["operations"])]

            latency_base = SERVICE_LATENCY[svc_idx]
            rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
            jitter = (rng % (latency_base + 1))
            duration = latency_base + jitter

            # error spans get elevated duration
            status = "OK"
            if is_error and depth >= 2:
                rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
                if (rng % 3) == 0:
                    status = "ERROR"
                    duration *= 5

            span = {
                "trace_id": trace_id,
                "span_id": span_id,
                "parent_span_id": parent_span_id,
                "service_name": svc["name"],
                "operation_name": op,
                "status_code": status,
                "duration_ms": duration,
                "start_time_ms": start_time,
                "tags": {
                    "service.version": f"1.{svc_idx}.0",
                    "deployment.environment": "production",
                },
            }
            spans.append(span)

            child_time = start_time + (duration // 4)
            for child_idx, prob in CALL_GRAPH.get(svc_idx, []):
                rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
                if (rng % 100) < int(prob * 100):
                    build_subtree(child_idx, span_id, depth + 1, child_time)
                    child_time += SERVICE_LATENCY[child_idx] + 2

        build_subtree(0, "", 0, base_time)

        # shuffle spans to simulate out-of-order arrival
        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        for i in range(len(spans) - 1, 0, -1):
            rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
            j = rng % (i + 1)
            spans[i], spans[j] = spans[j], spans[i]

        traces.append(spans)
    return traces


# ─── Trace Assembly ──────────────────────────────────────────────────────────

def assemble_trace(spans):
    by_id = {s["span_id"]: s for s in spans}
    roots = [s for s in spans if not s.get("parent_span_id")]

    trace_duration = 0
    span_count = len(spans)
    has_error = any(s["status_code"] == "ERROR" for s in spans)
    services = list(set(s["service_name"] for s in spans))

    if roots:
        root = roots[0]
        trace_duration = root["duration_ms"]
    elif spans:
        min_start = min(s["start_time_ms"] for s in spans)
        max_end = max(s["start_time_ms"] + s["duration_ms"] for s in spans)
        trace_duration = max_end - min_start

    return {
        "trace_id": spans[0]["trace_id"] if spans else "",
        "span_count": span_count,
        "duration_ms": trace_duration,
        "has_error": has_error,
        "services": services,
        "service_count": len(services),
        "root_service": roots[0]["service_name"] if roots else "",
        "root_operation": roots[0]["operation_name"] if roots else "",
    }


# ─── Tail-Based Sampling ────────────────────────────────────────────────────

def tail_sample(assembled_traces, error_keep=1.0, latency_percentile=99, random_rate=0.01, seed=42):
    if not assembled_traces:
        return {"sampled": [], "stats": {}}

    durations = sorted(t["duration_ms"] for t in assembled_traces)
    p_idx = min(int(len(durations) * latency_percentile / 100), len(durations) - 1)
    latency_threshold = durations[p_idx] if durations else 0

    sampled = []
    reasons = {"error": 0, "latency": 0, "random": 0, "dropped": 0}
    rng = seed

    for trace in assembled_traces:
        if trace["has_error"]:
            rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
            if (rng % 100) < int(error_keep * 100):
                sampled.append({**trace, "sample_reason": "error"})
                reasons["error"] += 1
                continue

        if trace["duration_ms"] >= latency_threshold:
            sampled.append({**trace, "sample_reason": "latency"})
            reasons["latency"] += 1
            continue

        rng = (rng * 1103515245 + 12345) & 0x7FFFFFFF
        if (rng % 10000) < int(random_rate * 10000):
            sampled.append({**trace, "sample_reason": "random"})
            reasons["random"] += 1
            continue

        reasons["dropped"] += 1

    return {
        "sampled": sampled,
        "stats": {
            "total_traces": len(assembled_traces),
            "sampled_count": len(sampled),
            "sample_rate": round(len(sampled) / len(assembled_traces) * 100, 1) if assembled_traces else 0,
            "latency_threshold_ms": latency_threshold,
            "reasons": reasons,
        },
    }


# ─── Service Graph ───────────────────────────────────────────────────────────

def build_service_graph(all_spans):
    edges = {}
    node_stats = {}

    for span in all_spans:
        svc = span["service_name"]
        if svc not in node_stats:
            node_stats[svc] = {"calls": 0, "errors": 0, "total_duration_ms": 0}
        node_stats[svc]["calls"] += 1
        node_stats[svc]["total_duration_ms"] += span["duration_ms"]
        if span["status_code"] == "ERROR":
            node_stats[svc]["errors"] += 1

    # build parent->child edges
    spans_by_id = {}
    for span in all_spans:
        spans_by_id[span["span_id"]] = span

    for span in all_spans:
        parent_id = span.get("parent_span_id", "")
        if parent_id and parent_id in spans_by_id:
            parent_svc = spans_by_id[parent_id]["service_name"]
            child_svc = span["service_name"]
            edge_key = f"{parent_svc}->{child_svc}"
            if edge_key not in edges:
                edges[edge_key] = {
                    "source": parent_svc,
                    "target": child_svc,
                    "call_count": 0,
                    "error_count": 0,
                    "total_duration_ms": 0,
                    "durations": [],
                }
            e = edges[edge_key]
            e["call_count"] += 1
            e["total_duration_ms"] += span["duration_ms"]
            e["durations"].append(span["duration_ms"])
            if span["status_code"] == "ERROR":
                e["error_count"] += 1

    # compute percentiles for edges
    edge_list = []
    for key, e in edges.items():
        durs = sorted(e["durations"])
        p50 = durs[len(durs) // 2] if durs else 0
        p99_idx = min(int(len(durs) * 0.99), len(durs) - 1) if durs else 0
        edge_list.append({
            "source": e["source"],
            "target": e["target"],
            "call_count": e["call_count"],
            "error_count": e["error_count"],
            "error_rate": round(e["error_count"] / e["call_count"] * 100, 1) if e["call_count"] > 0 else 0,
            "avg_duration_ms": round(e["total_duration_ms"] / e["call_count"], 1) if e["call_count"] > 0 else 0,
            "p50_duration_ms": p50,
            "p99_duration_ms": durs[p99_idx] if durs else 0,
        })

    nodes = []
    for svc, stats in node_stats.items():
        nodes.append({
            "service": svc,
            "calls": stats["calls"],
            "errors": stats["errors"],
            "error_rate": round(stats["errors"] / stats["calls"] * 100, 1) if stats["calls"] > 0 else 0,
            "avg_duration_ms": round(stats["total_duration_ms"] / stats["calls"], 1) if stats["calls"] > 0 else 0,
        })

    return {"nodes": nodes, "edges": edge_list}


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
    def run(self, trace_count: int = 1000, worker_count: int = 8, batch_size: int = 100,
            error_keep: float = 1.0, latency_percentile: int = 99, random_rate: float = 0.01,
            from_actor: str = "") -> dict:

        # Create shard group
        coord_start = host.now_ms()
        group_id = f"tracing-py-{host.now_ms()}"
        group = host.create_shard_group(CreateShardGroupRequest(
            group_id=group_id,
            actor_type="WorkerActor",
            shard_count=worker_count,
            partition_strategy="hash",
            rebalance_policy="manual",
            placement=NodePlacement(strategy="from_registry"),
        ))
        shard_group_id = group.group_id
        if not shard_group_id:
            return {"status": "error", "error": "failed to create worker shard group"}
        coord_create = host.now_ms() - coord_start

        # Generate traces (each trace = list of shuffled spans)
        traces = generate_traces(trace_count, seed=int(host.now_ms()) % 100000)

        # Flatten all spans for distribution
        all_spans = []
        for trace_spans in traces:
            all_spans.extend(trace_spans)

        # Batch spans for scatter-gather
        batches = [all_spans[i:i + batch_size] for i in range(0, len(all_spans), batch_size)]

        total_compute_ms = 0
        total_coord_ms = coord_create
        all_assembled = []
        all_graph_spans = []
        error_count = 0

        for batch in batches:
            sg_start = host.now_ms()
            sg_result = host.scatter_gather(ScatterGatherRequest(
                group_id=shard_group_id,
                query={
                    "op": "process_spans",
                    "spans": batch,
                },
                timeout_ms=30000,
            ))
            sg_elapsed = host.now_ms() - sg_start
            total_coord_ms += sg_elapsed

            for resp in sg_result.shard_responses:
                result = resp.get("payload", resp)
                if "error" in result:
                    error_count += 1
                    continue
                total_compute_ms += result.get("compute_ms", 0)
                all_assembled.extend(result.get("assembled_traces", []))
                all_graph_spans.extend(result.get("spans_for_graph", []))

        # Tail-based sampling
        sample_start = host.now_ms()
        sample_result = tail_sample(all_assembled, error_keep, latency_percentile, random_rate)
        sample_compute = host.now_ms() - sample_start
        total_compute_ms += sample_compute

        # Build service graph
        graph_start = host.now_ms()
        service_graph = build_service_graph(all_graph_spans)
        graph_compute = host.now_ms() - graph_start
        total_compute_ms += graph_compute

        wall_time = total_compute_ms + total_coord_ms
        total = total_compute_ms + total_coord_ms or 1
        granularity = round(total_compute_ms / total_coord_ms, 1) if total_coord_ms > 0 else 0
        spans_per_sec = round(len(all_spans) * 1000 / wall_time) if wall_time > 0 else 0

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "leader_traces_in": trace_count,
                        "leader_spans_total": len(all_spans),
                        "leader_assembled": len(all_assembled),
                    },
                    "latency_totals_ms": {
                        "leader.compute": total_compute_ms,
                        "leader.coordination": total_coord_ms,
                    },
                    "latency_max_ms": {
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
            "trace_count": trace_count,
            "total_spans": len(all_spans),
            "worker_count": worker_count,
            "wall_time_ms": wall_time,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coord_ms,
            "granularity_ratio": granularity,
            "spans_per_sec": spans_per_sec,
            "assembled_traces": len(all_assembled),
            "sampling": sample_result["stats"],
            "service_graph": {
                "node_count": len(service_graph["nodes"]),
                "edge_count": len(service_graph["edges"]),
                "nodes": service_graph["nodes"],
                "edges": service_graph["edges"][:10],
            },
            "error_count": error_count,
        }

    @handler("run_scaling_benchmark")
    def run_scaling_benchmark(self, trace_count: int = 1000, shard_counts: list = None,
                              batch_size: int = 100, warmup_rounds: int = 1,
                              benchmark_rounds: int = 2, from_actor: str = "") -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        # Each worker generates spans locally — num_passes repeats full assembly
        # so compute dominates the fixed scatter-gather coordination overhead.
        NUM_PASSES = 4
        TIMING_SG = benchmark_rounds
        results = []
        baseline_wall = 0

        for shard_count in shard_counts:
            coord_start = host.now_ms()
            group_id = f"tracing-bench-{shard_count}-{host.now_ms()}"
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=group_id,
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            shard_group_id = group.group_id
            coord_create = host.now_ms() - coord_start

            traces_per_shard = max(10, trace_count // shard_count)

            # Warmup: one SG call
            host.scatter_gather(ScatterGatherRequest(
                group_id=shard_group_id,
                query={"op": "benchmark_run", "trace_count": max(5, traces_per_shard // 4),
                       "seed": 1, "num_passes": 1},
                timeout_ms=60000,
            ))

            total_compute = 0
            total_coord = coord_create
            total_sps = 0

            for round_i in range(TIMING_SG):
                sg_start = host.now_ms()
                sg_result = host.scatter_gather(ScatterGatherRequest(
                    group_id=shard_group_id,
                    query={"op": "benchmark_run", "trace_count": traces_per_shard,
                           "seed": round_i + 100, "num_passes": NUM_PASSES},
                    timeout_ms=60000,
                ))
                sg_elapsed = host.now_ms() - sg_start
                total_coord += sg_elapsed

                round_spans = 0
                for resp in sg_result.shard_responses:
                    result = resp.get("payload", resp)
                    total_compute += result.get("compute_ms", 0)
                    round_spans += result.get("total_spans", 0)

                total_sps += round(round_spans * 1000 / sg_elapsed) if sg_elapsed > 0 else 0

            avg_compute = total_compute // TIMING_SG
            avg_coord = total_coord // TIMING_SG
            avg_sps = total_sps // TIMING_SG
            avg_wall = avg_coord

            if baseline_wall == 0:
                baseline_wall = avg_wall
            speedup = baseline_wall / avg_wall if avg_wall > 0 else 1.0
            efficiency = speedup / (shard_count / shard_counts[0]) * 100 if shard_counts[0] > 0 else 100

            results.append({
                "shards": shard_count,
                "spans_per_sec": avg_sps,
                "wall_time_ms": avg_wall,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": round(avg_compute / avg_coord, 1) if avg_coord > 0 else 0,
                "speedup": round(speedup, 2),
                "efficiency_pct": round(efficiency, 1),
                "error_count": 0,
            })

        return {
            "status": "ok",
            "trace_count": trace_count,
            "results": results,
        }

    @handler("run_weak_scaling_benchmark")
    def run_weak_scaling_benchmark(self, traces_per_shard: int = 500, shard_counts: list = None,
                                   batch_size: int = 100, warmup_rounds: int = 1,
                                   benchmark_rounds: int = 2, from_actor: str = "") -> dict:
        if shard_counts is None:
            shard_counts = [2, 4, 8, 16]

        NUM_PASSES = 4
        TIMING_SG = benchmark_rounds
        results = []
        baseline_sps = 0

        for shard_count in shard_counts:
            coord_start = host.now_ms()
            group_id = f"tracing-weak-{shard_count}-{host.now_ms()}"
            group = host.create_shard_group(CreateShardGroupRequest(
                group_id=group_id,
                actor_type="WorkerActor",
                shard_count=shard_count,
                partition_strategy="hash",
                rebalance_policy="manual",
                placement=NodePlacement(strategy="from_registry"),
            ))
            shard_group_id = group.group_id
            coord_create = host.now_ms() - coord_start

            # Warmup
            host.scatter_gather(ScatterGatherRequest(
                group_id=shard_group_id,
                query={"op": "benchmark_run", "trace_count": max(5, traces_per_shard // 4),
                       "seed": 1, "num_passes": 1},
                timeout_ms=60000,
            ))

            total_sps = 0
            total_compute = 0
            total_coord = coord_create

            for round_i in range(TIMING_SG):
                sg_start = host.now_ms()
                sg_result = host.scatter_gather(ScatterGatherRequest(
                    group_id=shard_group_id,
                    query={"op": "benchmark_run", "trace_count": traces_per_shard,
                           "seed": round_i + 200, "num_passes": NUM_PASSES},
                    timeout_ms=60000,
                ))
                sg_elapsed = host.now_ms() - sg_start
                total_coord += sg_elapsed

                round_spans = 0
                for resp in sg_result.shard_responses:
                    result = resp.get("payload", resp)
                    total_compute += result.get("compute_ms", 0)
                    round_spans += result.get("total_spans", 0)

                total_sps += round(round_spans * 1000 / sg_elapsed) if sg_elapsed > 0 else 0

            avg_sps = total_sps // TIMING_SG
            avg_compute = total_compute // TIMING_SG
            avg_coord = total_coord // TIMING_SG

            if baseline_sps == 0:
                baseline_sps = avg_sps
            efficiency = avg_sps / baseline_sps * 100 if baseline_sps > 0 else 100

            results.append({
                "shards": shard_count,
                "total_traces": traces_per_shard * shard_count,
                "spans_per_sec": avg_sps,
                "wall_time_ms": avg_coord,
                "compute_time_ms": avg_compute,
                "coordination_time_ms": avg_coord,
                "granularity_ratio": round(avg_compute / avg_coord, 1) if avg_coord > 0 else 0,
                "efficiency_pct": round(efficiency, 1),
                "error_count": 0,
            })

        return {
            "status": "ok",
            "traces_per_shard": traces_per_shard,
            "results": results,
        }


# ─── Worker Actor ─────────────────────────────────────────────────────────────

@register_role("worker")
@actor
class WorkerActor:
    spans_processed: int = state(default=0)
    traces_assembled: int = state(default=0)
    actor_id: str = state(default="")
    application_id: str = state(default="")

    @init_handler
    def on_init(self, config: dict) -> None:
        self.actor_id = config.get("actor_id", "")
        self.application_id = actor_application_id(self.actor_id)

    @handler("benchmark_run")
    def benchmark_run(self, trace_count: int = 100, seed: int = 42, num_passes: int = 1,
                      from_actor: str = "") -> dict:
        comp_start = host.now_ms()
        total_spans = 0
        total_assembled = 0
        for _pass in range(num_passes):
            traces = generate_traces(trace_count, seed=seed + _pass * 7919)
            all_spans = [s for t in traces for s in t]
            by_trace = {}
            for span in all_spans:
                by_trace.setdefault(span.get("trace_id", ""), []).append(span)
            assembled = [assemble_trace(s) for s in by_trace.values()]
            total_spans += len(all_spans)
            total_assembled += len(assembled)
        compute_ms = host.now_ms() - comp_start
        self.spans_processed += total_spans
        self.traces_assembled += total_assembled
        return {
            "total_spans": total_spans,
            "assembled_count": total_assembled,
            "compute_ms": compute_ms,
        }

    @handler("process_spans")
    def process_spans(self, spans: list = None, from_actor: str = "") -> dict:
        if spans is None:
            spans = []
        comp_start = host.now_ms()

        # Group spans by trace_id for assembly
        by_trace = {}
        for span in spans:
            tid = span.get("trace_id", "")
            by_trace.setdefault(tid, []).append(span)

        # Assemble each trace
        assembled = []
        for tid, trace_spans in by_trace.items():
            assembled.append(assemble_trace(trace_spans))

        compute_ms = host.now_ms() - comp_start
        self.spans_processed += len(spans)
        self.traces_assembled += len(assembled)

        try:
            host.application_metrics_add(
                self.application_id,
                {
                    "message_count": 1,
                    "counter_metrics": {
                        "worker_spans_processed": len(spans),
                        "worker_traces_assembled": len(assembled),
                    },
                    "latency_totals_ms": {"worker.compute": compute_ms},
                    "latency_max_ms": {"worker.compute": compute_ms},
                    "latency_samples": {"worker.compute": 1},
                },
            )
        except Exception:
            pass

        return {
            "spans_in": len(spans),
            "assembled_traces": assembled,
            "spans_for_graph": spans,
            "compute_ms": compute_ms,
        }
