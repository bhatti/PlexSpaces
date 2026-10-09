// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Distributed Tracing Pipeline - TypeScript WASM
//
// Production-grade tracing pipeline: OTLP span ingestion → trace assembly
// (out-of-order spans) → tail-based sampling (error-biased, latency-biased)
// → service graph construction.
//
// Architecture: Leader/worker with shard-group placement (partition by traceId),
// scatter/gather, compute vs coordination metrics tracking.
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
// ─── Span Generator ─────────────────────────────────────────────────────────
const SERVICES = [
    "api-gateway", "user-service", "order-service",
    "payment-service", "inventory-service", "notification-service",
];
const OPERATIONS = {
    "api-gateway": ["route_request", "authenticate", "rate_limit", "proxy_forward"],
    "user-service": ["get_user", "update_profile", "validate_session", "list_users"],
    "order-service": ["create_order", "get_order", "update_status", "list_orders"],
    "payment-service": ["charge_card", "refund", "validate_payment", "get_balance"],
    "inventory-service": ["check_stock", "reserve_item", "release_item", "update_count"],
    "notification-service": ["send_email", "send_sms", "send_push", "queue_notification"],
};
// Trace templates: which services call which
const CALL_CHAINS = [
    ["api-gateway", "user-service"],
    ["api-gateway", "order-service", "payment-service", "inventory-service"],
    ["api-gateway", "order-service", "notification-service"],
    ["api-gateway", "user-service", "notification-service"],
    ["api-gateway", "inventory-service"],
];
function generateSpans(count, seed) {
    const spans = [];
    let rng = seed;
    let traceIndex = 0;
    while (spans.length < count) {
        rng = (rng * 1103515245 + 12345) & 0x7fffffff;
        const chain = CALL_CHAINS[rng % CALL_CHAINS.length];
        const traceId = `trace-${String(traceIndex).padStart(8, "0")}`;
        traceIndex++;
        let parentSpanId = "";
        const traceBaseTime = 1700000000000 + (rng % 86400000);
        // ~5% error rate
        const isErrorTrace = (rng >> 16) % 20 === 0;
        // ~3% high-latency trace
        const isSlowTrace = (rng >> 20) % 33 === 0;
        let cumulativeOffset = 0;
        for (let depth = 0; depth < chain.length && spans.length < count; depth++) {
            rng = (rng * 1103515245 + 12345) & 0x7fffffff;
            const serviceName = chain[depth];
            const ops = OPERATIONS[serviceName];
            const operationName = ops[rng % ops.length];
            const spanId = `span-${traceId}-${depth}`;
            // Normal: p50=10ms, p95=50ms, p99=200ms
            // Slow: 500-2000ms
            let durationMs;
            if (isSlowTrace && depth === chain.length - 1) {
                durationMs = 500 + (rng % 1500);
            }
            else {
                const roll = rng % 100;
                if (roll < 50)
                    durationMs = 2 + (rng % 18); // p50 ~10ms
                else if (roll < 95)
                    durationMs = 20 + (rng % 30); // p95 ~50ms
                else
                    durationMs = 100 + (rng % 100); // p99 ~200ms
            }
            // Error on last span of error traces
            const statusCode = (isErrorTrace && depth === chain.length - 1) ? 2 : 0;
            const tags = {
                "service.version": `1.${rng % 10}.0`,
                "deployment.environment": "production",
            };
            if (statusCode === 2) {
                tags["error.type"] = ["timeout", "connection_refused", "internal_error", "not_found"][(rng >> 8) % 4];
            }
            spans.push({
                trace_id: traceId,
                span_id: spanId,
                parent_span_id: parentSpanId,
                service_name: serviceName,
                operation_name: operationName,
                status_code: statusCode,
                duration_ms: durationMs,
                start_time_ms: traceBaseTime + cumulativeOffset,
                tags,
            });
            parentSpanId = spanId;
            cumulativeOffset += durationMs;
        }
    }
    // Shuffle to simulate out-of-order arrival
    for (let i = spans.length - 1; i > 0; i--) {
        rng = (rng * 1103515245 + 12345) & 0x7fffffff;
        const j = rng % (i + 1);
        [spans[i], spans[j]] = [spans[j], spans[i]];
    }
    return spans.slice(0, count);
}
// ─── Trace Assembly ─────────────────────────────────────────────────────────
function assembleTraces(spans) {
    const traces = {};
    for (const span of spans) {
        if (!traces[span.trace_id]) {
            traces[span.trace_id] = {
                trace_id: span.trace_id,
                spans: [],
                root_service: "",
                root_operation: "",
                total_duration_ms: 0,
                has_error: false,
                span_count: 0,
                services: [],
            };
        }
        const trace = traces[span.trace_id];
        trace.spans.push(span);
        trace.span_count++;
        if (span.parent_span_id === "") {
            trace.root_service = span.service_name;
            trace.root_operation = span.operation_name;
        }
        if (span.status_code !== 0) {
            trace.has_error = true;
        }
    }
    // Compute total duration and service list
    for (const trace of Object.values(traces)) {
        const serviceSet = new Set();
        let minStart = Infinity;
        let maxEnd = 0;
        for (const span of trace.spans) {
            serviceSet.add(span.service_name);
            if (span.start_time_ms < minStart)
                minStart = span.start_time_ms;
            const end = span.start_time_ms + span.duration_ms;
            if (end > maxEnd)
                maxEnd = end;
        }
        trace.total_duration_ms = maxEnd - minStart;
        trace.services = Array.from(serviceSet);
    }
    return traces;
}
// ─── Tail-Based Sampling ────────────────────────────────────────────────────
function tailSample(traces, latencyThresholdMs, randomRate, seed) {
    const sampled = [];
    let errorSampled = 0;
    let latencySampled = 0;
    let randomSampled = 0;
    let rng = seed;
    for (const trace of Object.values(traces)) {
        // Error-biased: keep all error traces
        if (trace.has_error) {
            sampled.push(trace);
            errorSampled++;
            continue;
        }
        // Latency-biased: keep high-latency traces
        if (trace.total_duration_ms > latencyThresholdMs) {
            sampled.push(trace);
            latencySampled++;
            continue;
        }
        // Random: sample a percentage of normal traces
        rng = (rng * 1103515245 + 12345) & 0x7fffffff;
        if ((rng % 1000) < (randomRate * 1000)) {
            sampled.push(trace);
            randomSampled++;
        }
    }
    const totalTraces = Object.keys(traces).length;
    return {
        sampled,
        stats: {
            total_traces: totalTraces,
            sampled_traces: sampled.length,
            error_sampled: errorSampled,
            latency_sampled: latencySampled,
            random_sampled: randomSampled,
            sample_rate: totalTraces > 0 ? sampled.length / totalTraces : 0,
        },
    };
}
// ─── Service Graph ──────────────────────────────────────────────────────────
function buildServiceGraph(traces) {
    const edgeMap = new Map();
    for (const trace of Object.values(traces)) {
        // Build spanId → span lookup
        const spanMap = new Map();
        for (const span of trace.spans) {
            spanMap.set(span.span_id, span);
        }
        for (const span of trace.spans) {
            if (span.parent_span_id === "")
                continue;
            const parent = spanMap.get(span.parent_span_id);
            if (!parent)
                continue;
            const key = `${parent.service_name}->${span.service_name}`;
            const existing = edgeMap.get(key);
            if (existing) {
                existing.call_count++;
                existing.total_latency_ms += span.duration_ms;
                if (span.status_code !== 0)
                    existing.error_count++;
            }
            else {
                edgeMap.set(key, {
                    from_service: parent.service_name,
                    to_service: span.service_name,
                    call_count: 1,
                    total_latency_ms: span.duration_ms,
                    error_count: span.status_code !== 0 ? 1 : 0,
                });
            }
        }
    }
    return Array.from(edgeMap.values());
}
// ─── Leader Actor ────────────────────────────────────────────────────────────
class LeaderActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "", application_id: "", role: "leader",
            total_compute_ms: 0, total_coord_ms: 0,
        };
    }
    onInit(config) {
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "leader";
        this.state.total_compute_ms = 0;
        this.state.total_coord_ms = 0;
    }
    onRun(payload) {
        const spanCount = intValue(payload.span_count, 10000);
        const workerCount = intValue(payload.worker_count, 8);
        const batchSize = intValue(payload.batch_size, 500);
        const latencyThreshold = intValue(payload.latency_threshold_ms, 500);
        const randomSampleRate = floatValue(payload.random_sample_rate, 0.01);
        const numPasses = intValue(payload.num_passes, 1);
        // Create shard group
        const coordStart = host.nowMs();
        const groupId = `tracing-pipeline-ts-${host.nowMs()}`;
        const group = host.createShardGroup({
            groupId,
            actorType: "tracing-worker",
            shardCount: workerCount,
            partitionStrategy: "hash",
            rebalancePolicy: "manual",
            placement: { strategy: "from_registry" },
            initialState: {},
        });
        const shardActorIds = group.shardActorIds;
        if (shardActorIds.length === 0) {
            return { status: "error", error: "failed to create worker shard group" };
        }
        const coordCreate = host.nowMs() - coordStart;
        const leaderNodeId = actorNodeId(this.state.actor_id);
        // Generate spans
        const spans = generateSpans(spanCount, host.nowMs() % 100000);
        // Partition spans by traceId hash → shard
        const shardBatches = Array.from({ length: workerCount }, () => []);
        for (const span of spans) {
            const hash = simpleHash(span.trace_id);
            const shardIdx = hash % workerCount;
            shardBatches[shardIdx].push(span);
        }
        let totalComputeMs = 0;
        let totalCoordMs = coordCreate;
        let totalTraces = 0;
        let totalSampled = 0;
        let totalErrorSampled = 0;
        let totalLatencySampled = 0;
        let totalRandomSampled = 0;
        let maxWorkerLatency = 0;
        let totalWorkerLatency = 0;
        let workerCalls = 0;
        let errorCount = 0;
        const allEdges = [];
        // Send batches to workers via scatter/gather
        for (let shardIdx = 0; shardIdx < workerCount; shardIdx++) {
            const shardSpans = shardBatches[shardIdx];
            if (shardSpans.length === 0)
                continue;
            for (let i = 0; i < shardSpans.length; i += batchSize) {
                const batch = shardSpans.slice(i, i + batchSize);
                const sgStart = host.nowMs();
                const sgResult = host.scatterGather({
                    groupId,
                    query: {
                        op: "process_spans",
                        spans: batch,
                        latency_threshold_ms: latencyThreshold,
                        random_sample_rate: randomSampleRate,
                        num_passes: numPasses,
                    },
                    timeoutMs: 30000,
                });
                const sgElapsed = host.nowMs() - sgStart;
                totalCoordMs += sgElapsed;
                for (const resp of sgResult.shardResponses) {
                    const result = recordValue(resp.payload ?? resp);
                    if (result.error) {
                        errorCount++;
                        continue;
                    }
                    totalComputeMs += intValue(result.compute_ms, 0);
                    totalTraces += intValue(result.traces_assembled, 0);
                    const sampling = recordValue(result.sampling);
                    totalSampled += intValue(sampling.sampled_traces, 0);
                    totalErrorSampled += intValue(sampling.error_sampled, 0);
                    totalLatencySampled += intValue(sampling.latency_sampled, 0);
                    totalRandomSampled += intValue(sampling.random_sampled, 0);
                    const edges = anyArray(result.service_edges);
                    for (const edge of edges) {
                        allEdges.push(recordValue(edge));
                    }
                    const latency = intValue(result.compute_ms, 0);
                    totalWorkerLatency += latency;
                    if (latency > maxWorkerLatency)
                        maxWorkerLatency = latency;
                    workerCalls++;
                }
            }
        }
        // Merge service edges
        const mergedEdges = mergeServiceEdges(allEdges);
        const wallTime = totalComputeMs + totalCoordMs;
        const total = totalComputeMs + totalCoordMs || 1;
        const granularity = totalCoordMs > 0 ? Math.round((totalComputeMs / totalCoordMs) * 10) / 10 : 0;
        const spansPerSec = wallTime > 0 ? Math.round((spanCount / wallTime) * 1000) : 0;
        const nodeActorCounts = computeActorCounts(leaderNodeId, shardActorIds);
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "leader.compute": totalComputeMs, "leader.coordination": totalCoordMs } });
        }
        catch (_e) { }
        return {
            status: "ok",
            span_count: spanCount,
            worker_count: workerCount,
            wall_time_ms: wallTime,
            compute_time_ms: totalComputeMs,
            coordination_time_ms: totalCoordMs,
            granularity_ratio: granularity,
            spans_per_sec: spansPerSec,
            traces_assembled: totalTraces,
            traces_sampled: totalSampled,
            error_sampled: totalErrorSampled,
            latency_sampled: totalLatencySampled,
            random_sampled: totalRandomSampled,
            sample_rate: totalTraces > 0 ? Math.round((totalSampled / totalTraces) * 1000) / 1000 : 0,
            service_edges: mergedEdges.map(e => ({
                from: e.from_service,
                to: e.to_service,
                calls: e.call_count,
                avg_latency_ms: e.call_count > 0 ? Math.round(e.total_latency_ms / e.call_count) : 0,
                error_count: e.error_count,
            })),
            service_edge_count: mergedEdges.length,
            avg_worker_latency_ms: workerCalls > 0 ? Math.round(totalWorkerLatency / workerCalls) : 0,
            max_worker_latency_ms: maxWorkerLatency,
            node_count: Object.keys(nodeActorCounts).length,
            actor_count: shardActorIds.length + 1,
            leader_node_id: leaderNodeId,
            nodes: nodeActorCounts,
            error_count: errorCount,
        };
    }
    onRun_scaling_benchmark(payload) {
        const spanCount = intValue(payload.span_count, 10000);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const batchSize = intValue(payload.batch_size, 500);
        const warmupRounds = intValue(payload.warmup_rounds, 1);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 2);
        const results = [];
        let baselineWall = 0;
        for (const shardCount of shardCounts) {
            for (let w = 0; w < warmupRounds; w++) {
                this.onRun({
                    span_count: Math.min(spanCount, 1000),
                    worker_count: shardCount,
                    batch_size: batchSize,
                });
            }
            let totalWall = 0;
            let totalCompute = 0;
            let totalCoord = 0;
            let totalSpansPerSec = 0;
            let nodeCount = 0;
            let errorCount = 0;
            const runSpans = Math.min(spanCount, batchSize * 2);
            for (let r = 0; r < benchmarkRounds; r++) {
                const result = this.onRun({
                    span_count: runSpans,
                    worker_count: shardCount,
                    batch_size: batchSize,
                });
                totalWall += intValue(result.wall_time_ms, 0);
                totalCompute += intValue(result.compute_time_ms, 0);
                totalCoord += intValue(result.coordination_time_ms, 0);
                totalSpansPerSec += intValue(result.spans_per_sec, 0);
                nodeCount = intValue(result.node_count, 0);
                errorCount += intValue(result.error_count, 0);
            }
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            const avgSpansPerSec = Math.round(totalSpansPerSec / benchmarkRounds);
            const total = avgCompute + avgCoord || 1;
            if (baselineWall === 0)
                baselineWall = avgWall;
            const speedup = baselineWall > 0 && avgWall > 0 ? baselineWall / avgWall : 1;
            const efficiency = (speedup / (shardCount / shardCounts[0])) * 100;
            results.push({
                shards: shardCount,
                spans_per_sec: avgSpansPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                compute_pct: (avgCompute * 100) / total,
                granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
                speedup: Math.round(speedup * 100) / 100,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                node_count: nodeCount,
                error_count: errorCount,
            });
        }
        return { status: "ok", span_count: spanCount, results };
    }
    onRun_weak_scaling_benchmark(payload) {
        const spansPerShard = intValue(payload.spans_per_shard, 500);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const numPasses = intValue(payload.num_passes, 4);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 1);
        const results = [];
        let baselineSpansPerSec = 0;
        const leaderNodeId = actorNodeId(this.state.actor_id);
        for (const shardCount of shardCounts) {
            const totalSpans = spansPerShard * shardCount;
            const coordStart = host.nowMs();
            const groupId = `tracing-ws-${shardCount}-${host.nowMs()}`;
            const group = host.createShardGroup({
                groupId,
                actorType: "tracing-worker",
                shardCount,
                partitionStrategy: "hash",
                rebalancePolicy: "manual",
                placement: { strategy: "from_registry" },
                initialState: {},
            });
            if (group.shardActorIds.length === 0)
                continue;
            const coordCreate = host.nowMs() - coordStart;
            let totalWall = 0;
            let totalCompute = 0;
            let totalCoord = coordCreate;
            let totalSpansAgg = 0;
            let errorCount = 0;
            for (let r = 0; r < benchmarkRounds; r++) {
                const sgStart = host.nowMs();
                // Single broadcast scatter-gather: each worker generates and processes its own spans
                const sgResult = host.scatterGather({
                    groupId,
                    query: {
                        op: "benchmark_spans",
                        span_count: spansPerShard,
                        num_passes: numPasses,
                        seed: r + shardCount * 100,
                    },
                    timeoutMs: 30000,
                });
                const sgElapsed = host.nowMs() - sgStart;
                totalCoord += sgElapsed;
                let roundCompute = 0;
                for (const resp of sgResult.shardResponses) {
                    const result = recordValue(resp.payload ?? resp);
                    if (result.error) {
                        errorCount++;
                        continue;
                    }
                    roundCompute += intValue(result.compute_ms, 0);
                    totalSpansAgg += intValue(result.spans_processed, 0);
                }
                totalCompute += roundCompute;
                totalWall += roundCompute + sgElapsed;
            }
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            const spansPerSec = avgWall > 0 ? Math.round((totalSpans / avgWall) * 1000) : 0;
            const nodeActorCounts = computeActorCounts(leaderNodeId, group.shardActorIds);
            if (baselineSpansPerSec === 0)
                baselineSpansPerSec = spansPerSec;
            const efficiency = baselineSpansPerSec > 0 ? (spansPerSec / baselineSpansPerSec) * 100 : 100;
            results.push({
                shards: shardCount,
                total_spans: totalSpans,
                spans_per_sec: spansPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                node_count: nodeActorCounts.nodeCount,
                error_count: errorCount,
            });
        }
        return { status: "ok", spans_per_shard: spansPerShard, results };
    }
}
// ─── Worker Actor ────────────────────────────────────────────────────────────
class WorkerActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "", application_id: "", role: "worker",
            traces: {}, spans_ingested: 0, compute_ms: 0,
        };
    }
    onInit(config) {
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "worker";
    }
    onProcess_spans(payload) {
        const compStart = host.nowMs();
        const rawSpans = anyArray(payload.spans);
        const latencyThreshold = intValue(payload.latency_threshold_ms, 500);
        const randomSampleRate = floatValue(payload.random_sample_rate, 0.01);
        const numPasses = intValue(payload.num_passes, 1);
        const spans = rawSpans.map((raw) => {
            const r = recordValue(raw);
            return {
                trace_id: stringValue(r.trace_id),
                span_id: stringValue(r.span_id),
                parent_span_id: stringValue(r.parent_span_id),
                service_name: stringValue(r.service_name),
                operation_name: stringValue(r.operation_name),
                status_code: intValue(r.status_code, 0),
                duration_ms: intValue(r.duration_ms, 0),
                start_time_ms: intValue(r.start_time_ms, 0),
                tags: recordValue(r.tags),
            };
        });
        // Assemble traces from spans — repeat num_passes times to increase compute/coordination ratio
        let traces = assembleTraces(spans);
        let sampled = [];
        let stats = { total_traces: 0, sampled_traces: 0, error_sampled: 0, latency_sampled: 0, random_sampled: 0, sample_rate: 0 };
        let edges = [];
        for (let p = 0; p < numPasses; p++) {
            traces = assembleTraces(spans);
            const result = tailSample(traces, latencyThreshold, randomSampleRate, compStart);
            sampled = result.sampled;
            stats = result.stats;
            edges = buildServiceGraph(traces);
        }
        const traceCount = Object.keys(traces).length;
        const computeMs = host.nowMs() - compStart;
        this.state.spans_ingested += spans.length;
        this.state.compute_ms += computeMs;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.compute": computeMs, "worker.spans_ingested": spans.length } });
        }
        catch (_e) { }
        return {
            spans_processed: spans.length,
            traces_assembled: traceCount,
            compute_ms: computeMs,
            sampling: {
                total_traces: stats.total_traces,
                sampled_traces: stats.sampled_traces,
                error_sampled: stats.error_sampled,
                latency_sampled: stats.latency_sampled,
                random_sampled: stats.random_sampled,
                sample_rate: Math.round(stats.sample_rate * 1000) / 1000,
            },
            service_edges: edges.map(e => ({
                from: e.from_service,
                to: e.to_service,
                calls: e.call_count,
                avg_latency_ms: e.call_count > 0 ? Math.round(e.total_latency_ms / e.call_count) : 0,
                error_count: e.error_count,
            })),
        };
    }
    // Worker-local benchmark: generates spans locally so the leader can use a single
    // broadcast scatter-gather instead of N sequential per-shard calls. This enables
    // true weak scaling — one SG round trip regardless of shard count.
    onBenchmark_spans(payload) {
        const spanCount = intValue(payload.span_count, 500);
        const numPasses = intValue(payload.num_passes, 4);
        const latencyThreshold = intValue(payload.latency_threshold_ms, 500);
        const randomSampleRate = floatValue(payload.random_sample_rate, 0.01);
        const seed = intValue(payload.seed, 42);
        const compStart = host.nowMs();
        const spans = generateSpans(spanCount, seed);
        let traces = assembleTraces(spans);
        let stats = { total_traces: 0, sampled_traces: 0, error_sampled: 0, latency_sampled: 0, random_sampled: 0, sample_rate: 0 };
        let edges = [];
        for (let p = 0; p < numPasses; p++) {
            traces = assembleTraces(spans);
            const result = tailSample(traces, latencyThreshold, randomSampleRate, compStart + p);
            stats = result.stats;
            edges = buildServiceGraph(traces);
        }
        const computeMs = host.nowMs() - compStart;
        this.state.compute_ms += computeMs;
        return {
            spans_processed: spanCount,
            traces_assembled: Object.keys(traces).length,
            compute_ms: computeMs,
            passes: numPasses,
            sampling: stats,
        };
    }
}
// ─── Helpers ─────────────────────────────────────────────────────────────────
function simpleHash(s) {
    let hash = 0;
    for (let i = 0; i < s.length; i++) {
        hash = ((hash * 31) + s.charCodeAt(i)) & 0x7fffffff;
    }
    return hash;
}
function mergeServiceEdges(edges) {
    const map = new Map();
    for (const e of edges) {
        const key = `${e.from_service}->${e.to_service}`;
        const existing = map.get(key);
        if (existing) {
            existing.call_count += e.call_count;
            existing.total_latency_ms += e.total_latency_ms;
            existing.error_count += e.error_count;
        }
        else {
            map.set(key, { ...e });
        }
    }
    return Array.from(map.values());
}
function actorApplicationId(actorId) {
    const doubleSlash = actorId.indexOf("//");
    if (doubleSlash < 0)
        return "";
    const rest = actorId.slice(doubleSlash + 2);
    const colonColon = rest.indexOf("::");
    if (colonColon < 0)
        return "";
    const afterColon = rest.slice(colonColon + 2);
    const atSign = afterColon.indexOf("@");
    return atSign >= 0 ? afterColon.slice(0, atSign) : afterColon;
}
function actorNodeId(actorId) {
    const atSign = actorId.lastIndexOf("@");
    return atSign >= 0 ? actorId.slice(atSign + 1) : "";
}
function intValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value))
        return Math.trunc(value);
    if (typeof value === "string") {
        const parsed = Number.parseInt(value, 10);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
}
function floatValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value))
        return value;
    if (typeof value === "string") {
        const parsed = Number.parseFloat(value);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
}
function intArrayValue(value, fallback) {
    if (!Array.isArray(value))
        return fallback;
    return value.map((v) => intValue(v, 0)).filter((v) => v > 0);
}
function stringValue(value) {
    return typeof value === "string" ? value : "";
}
function recordValue(value) {
    return value !== null && typeof value === "object" && !Array.isArray(value)
        ? value
        : {};
}
function anyArray(value) {
    return Array.isArray(value) ? value : [];
}
function stringArray(value) {
    return anyArray(value).map((item) => String(item)).filter((item) => item.length > 0);
}
function computeActorCounts(leaderNodeId, shardActorIds) {
    const nodes = {
        [leaderNodeId]: { actors: 1, leader_actors: 1, worker_actors: 0 },
    };
    for (const actorId of shardActorIds) {
        const nodeId = actorNodeId(actorId);
        if (!nodes[nodeId]) {
            nodes[nodeId] = { actors: 0, leader_actors: 0, worker_actors: 0 };
        }
        nodes[nodeId].actors += 1;
        nodes[nodeId].worker_actors += 1;
    }
    return nodes;
}
// ─── Router & Export ─────────────────────────────────────────────────────────
const router = new ActorRouter({
    leader: () => new LeaderActor(),
    worker: () => new WorkerActor(),
    "tracing-leader": () => new LeaderActor(),
    "tracing-worker": () => new WorkerActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
