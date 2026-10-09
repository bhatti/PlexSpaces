// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Streaming Pipeline - TypeScript WASM
//
// Leader/worker streaming pipeline with shard-group placement, scatter/gather,
// tuple-space stage summaries, and application-metrics-backed reporting.
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
const STREAM_PREFIX = "streaming-pipeline-ts";
class LeaderActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "",
            application_id: "",
            role: "leader",
            worker_count: 8,
            batch_count: 18,
            events_per_batch: 1200,
            drop_rate: 0.08,
            enrich_fields: 6,
            total_compute_ms: 0,
            total_coord_ms: 0,
        };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = String(args.role ?? "leader");
        this.state.worker_count = intValue(args.worker_count, this.state.worker_count);
        this.state.batch_count = intValue(args.batch_count, this.state.batch_count);
        this.state.events_per_batch = intValue(args.events_per_batch, this.state.events_per_batch);
        this.state.drop_rate = floatValue(args.drop_rate, this.state.drop_rate);
        this.state.enrich_fields = intValue(args.enrich_fields, this.state.enrich_fields);
        this.state.total_compute_ms = 0;
        this.state.total_coord_ms = 0;
    }
    onRun(payload) {
        const request = this.requestFromPayload(payload);
        if (request.worker_count <= 0 || request.batch_count <= 0 || request.events_per_batch <= 0) {
            return { error: "worker_count, batch_count, and events_per_batch must be positive" };
        }
        const wallStart = host.nowMs();
        const groupId = `${STREAM_PREFIX}-${host.nowMs()}`;
        const group = host.createShardGroup({
            groupId,
            actorType: "streaming-worker",
            shardCount: request.worker_count,
            partitionStrategy: "hash",
            rebalancePolicy: "manual",
            placement: { strategy: "from_registry" },
            initialState: {},
        });
        const shardActorIds = group.shardActorIds;
        if (shardActorIds.length === 0) {
            return { error: "failed to create worker shard group" };
        }
        const leaderNodeId = actorNodeId(this.state.actor_id);
        const nodeAddresses = {};
        const startLeaderMetrics = host.applicationGetMetrics(this.state.application_id, leaderNodeId);
        const nodeMetrics = {};
        const roleMetrics = {};
        const results = [];
        const remoteNodesWithWork = new Set();
        let totalErrors = 0;
        let totalWorkerLatencyMs = 0;
        let totalWorkerResponses = 0;
        let maxWorkerLatencyMs = 0;
        let totalEventCount = 0;
        let totalFilteredEvents = 0;
        let totalEnrichedEvents = 0;
        let totalTransformedEvents = 0;
        let totalDroppedEvents = 0;
        let totalBytesProcessed = 0;
        let totalTupleOperations = 0;
        for (let batchIndex = 0; batchIndex < request.batch_count; batchIndex++) {
            const runId = `${groupId}-batch-${batchIndex}`;
            const coordStart = host.nowMs();
            const response = host.scatterGather({
                groupId,
                query: {
                    op: "process_batch",
                    run_id: runId,
                    batch_index: batchIndex,
                    events_per_batch: request.events_per_batch,
                    drop_rate: request.drop_rate,
                    enrich_fields: request.enrich_fields,
                },
                aggregation: "concat",
                minResponses: request.worker_count,
                timeoutMs: 30000,
            });
            const coordMs = host.nowMs() - coordStart;
            const candidates = [];
            const shardResponses = response.shardResponses;
            let iterationErrors = 0;
            let iterationResponses = 0;
            let iterationLatencyMs = 0;
            let iterationMaxLatencyMs = 0;
            let iterationEvents = 0;
            let iterationFiltered = 0;
            let iterationEnriched = 0;
            let iterationTransformed = 0;
            let iterationDropped = 0;
            let iterationBytes = 0;
            let iterationTupleOps = 0;
            for (const shard of shardResponses) {
                const payloadMap = normalizeWorkerPayload(shard.payload);
                if (stringValue(payloadMap.status) === "ok") {
                    iterationResponses += 1;
                    totalWorkerResponses += 1;
                    const actorId = stringValue(payloadMap.actor_id);
                    const nodeId = stringValue(payloadMap.node_id) || actorNodeId(actorId);
                    const latencyMs = intValue(payloadMap.latency_ms, 0);
                    const tupleOperations = intValue(payloadMap.tuple_operations, 0);
                    iterationLatencyMs += latencyMs;
                    totalWorkerLatencyMs += latencyMs;
                    iterationMaxLatencyMs = Math.max(iterationMaxLatencyMs, latencyMs);
                    maxWorkerLatencyMs = Math.max(maxWorkerLatencyMs, latencyMs);
                    iterationEvents += intValue(payloadMap.event_count, 0);
                    iterationFiltered += intValue(payloadMap.filtered_events, 0);
                    iterationEnriched += intValue(payloadMap.enriched_events, 0);
                    iterationTransformed += intValue(payloadMap.transformed_events, 0);
                    iterationDropped += intValue(payloadMap.dropped_events, 0);
                    iterationBytes += intValue(payloadMap.bytes_processed, 0);
                    iterationTupleOps += tupleOperations;
                    if (nodeId && nodeId !== leaderNodeId) {
                        remoteNodesWithWork.add(nodeId);
                    }
                    const workerNode = ensureNodeMetric(nodeMetrics, nodeId);
                    workerNode.messages += 1;
                    workerNode.worker_messages += 1;
                    workerNode.event_count += intValue(payloadMap.event_count, 0);
                    workerNode.filtered_events += intValue(payloadMap.filtered_events, 0);
                    workerNode.enriched_events += intValue(payloadMap.enriched_events, 0);
                    workerNode.transformed_events += intValue(payloadMap.transformed_events, 0);
                    workerNode.dropped_events += intValue(payloadMap.dropped_events, 0);
                    workerNode.bytes_processed += intValue(payloadMap.bytes_processed, 0);
                    workerNode.tuple_operations += tupleOperations;
                    workerNode.compute_time_ms += latencyMs;
                    workerNode.total_latency_ms += latencyMs;
                    workerNode.max_latency_ms = Math.max(workerNode.max_latency_ms, latencyMs);
                    workerNode.responses += 1;
                    const workerRole = ensureRoleMetric(roleMetrics, "worker");
                    workerRole.messages += 1;
                    workerRole.event_count += intValue(payloadMap.event_count, 0);
                    workerRole.filtered_events += intValue(payloadMap.filtered_events, 0);
                    workerRole.enriched_events += intValue(payloadMap.enriched_events, 0);
                    workerRole.transformed_events += intValue(payloadMap.transformed_events, 0);
                    workerRole.dropped_events += intValue(payloadMap.dropped_events, 0);
                    workerRole.bytes_processed += intValue(payloadMap.bytes_processed, 0);
                    workerRole.tuple_operations += tupleOperations;
                    workerRole.compute_time_ms += latencyMs;
                    workerRole.total_latency_ms += latencyMs;
                    workerRole.max_latency_ms = Math.max(workerRole.max_latency_ms, latencyMs);
                    workerRole.responses += 1;
                    for (const streamCount of anyArray(payloadMap.top_streams)) {
                        const streamMap = recordValue(streamCount);
                        candidates.push({
                            stream: stringValue(streamMap.stream),
                            count: intValue(streamMap.count, 0),
                        });
                    }
                }
                else {
                    iterationErrors += 1;
                    totalErrors += 1;
                }
            }
            const stageTuples = host.ts.readAll([STREAM_PREFIX, runId, "stage_summary", null, null, null]);
            const computeStart = host.nowMs();
            const mergedTopStreams = mergeTopStreams(candidates, 5);
            let computeMs = host.nowMs() - computeStart;
            if (iterationResponses > 0 && computeMs <= 0) {
                computeMs = 1;
            }
            this.state.total_coord_ms += coordMs;
            this.state.total_compute_ms += computeMs;
            totalEventCount += iterationEvents;
            totalFilteredEvents += iterationFiltered;
            totalEnrichedEvents += iterationEnriched;
            totalTransformedEvents += iterationTransformed;
            totalDroppedEvents += iterationDropped;
            totalBytesProcessed += iterationBytes;
            totalTupleOperations += iterationTupleOps + stageTuples.length;
            results.push({
                batch_index: batchIndex,
                responses: iterationResponses,
                errors: iterationErrors,
                event_count: iterationEvents,
                filtered_events: iterationFiltered,
                enriched_events: iterationEnriched,
                transformed_events: iterationTransformed,
                dropped_events: iterationDropped,
                bytes_processed: iterationBytes,
                tuple_operations: iterationTupleOps + stageTuples.length,
                coord_ms: coordMs,
                compute_ms: computeMs,
                avg_latency_ms: iterationResponses > 0 ? iterationLatencyMs / iterationResponses : 0,
                max_latency_ms: iterationMaxLatencyMs,
                top_streams: mergedTopStreams,
            });
        }
        host.applicationMetricsAdd(this.state.application_id, {
            message_count: 1,
            counter_metrics: {
                leader_messages: request.batch_count + 1,
                leader_runs: 1,
                streaming_rounds: request.batch_count,
                leader_tuple_operations: request.batch_count,
            },
            latency_totals_ms: {
                leader: this.state.total_compute_ms + this.state.total_coord_ms,
                "leader.compute": this.state.total_compute_ms,
                "leader.coordination": this.state.total_coord_ms,
            },
            latency_max_ms: {
                leader: this.state.total_compute_ms + this.state.total_coord_ms,
                "leader.compute": this.state.total_compute_ms,
                "leader.coordination": this.state.total_coord_ms,
            },
            latency_samples: {
                leader: 1,
                "leader.compute": 1,
                "leader.coordination": 1,
            },
        });
        const endLeaderMetrics = host.applicationGetMetrics(this.state.application_id, leaderNodeId);
        applyMetricsDelta(nodeMetrics, roleMetrics, startLeaderMetrics, endLeaderMetrics, leaderNodeId);
        for (const [nodeId, counts] of Object.entries(computeActorCounts(leaderNodeId, shardActorIds))) {
            const node = ensureNodeMetric(nodeMetrics, nodeId);
            node.actors += counts.actors;
            node.leader_actors += counts.leader_actors;
            node.worker_actors += counts.worker_actors;
        }
        ensureRoleMetric(roleMetrics, "leader").actors = 1;
        ensureRoleMetric(roleMetrics, "worker").actors = shardActorIds.length;
        let totalMessages = 0;
        let totalComputeMs = 0;
        let totalCoordinationMs = 0;
        const actorCounts = [];
        for (const [nodeId, metrics] of Object.entries(nodeMetrics)) {
            totalMessages += metrics.messages;
            totalComputeMs += metrics.compute_time_ms;
            totalCoordinationMs += metrics.coordination_time_ms;
            actorCounts.push(metrics.actors);
        }
        const actorDistributionSkew = actorCounts.length > 0
            ? Math.max(...actorCounts) - Math.min(...actorCounts)
            : 0;
        const nodes = {};
        for (const [nodeId, metrics] of Object.entries(nodeMetrics)) {
            nodes[nodeId] = {
                actors: metrics.actors,
                leader_actors: metrics.leader_actors,
                worker_actors: metrics.worker_actors,
                messages: metrics.messages,
                leader_messages: metrics.leader_messages,
                worker_messages: metrics.worker_messages,
                event_count: metrics.event_count,
                filtered_events: metrics.filtered_events,
                enriched_events: metrics.enriched_events,
                transformed_events: metrics.transformed_events,
                dropped_events: metrics.dropped_events,
                bytes_processed: metrics.bytes_processed,
                tuple_operations: metrics.tuple_operations,
                compute_time_ms: metrics.compute_time_ms,
                coordination_time_ms: metrics.coordination_time_ms,
                avg_latency_ms: averageLatency(metrics),
                max_latency_ms: metrics.max_latency_ms,
                errors: metrics.errors,
            };
        }
        const roles = {};
        for (const [role, metrics] of Object.entries(roleMetrics)) {
            roles[role] = {
                actors: metrics.actors,
                messages: metrics.messages,
                event_count: metrics.event_count,
                filtered_events: metrics.filtered_events,
                enriched_events: metrics.enriched_events,
                transformed_events: metrics.transformed_events,
                dropped_events: metrics.dropped_events,
                bytes_processed: metrics.bytes_processed,
                tuple_operations: metrics.tuple_operations,
                compute_time_ms: metrics.compute_time_ms,
                coordination_time_ms: metrics.coordination_time_ms,
                avg_latency_ms: averageLatency(metrics),
                max_latency_ms: metrics.max_latency_ms,
                errors: metrics.errors,
            };
        }
        return {
            status: "ok",
            worker_count: request.worker_count,
            batch_count: request.batch_count,
            events_per_batch: request.events_per_batch,
            drop_rate: request.drop_rate,
            enrich_fields: request.enrich_fields,
            stream_rounds: request.batch_count,
            leader_node_id: leaderNodeId,
            node_addresses: nodeAddresses,
            shard_actor_ids: shardActorIds,
            node_count: Object.keys(nodeMetrics).length,
            worker_node_count: remoteNodesWithWork.size,
            actor_count: shardActorIds.length + 1,
            message_count: totalMessages,
            event_count: totalEventCount,
            filtered_event_count: totalFilteredEvents,
            enriched_event_count: totalEnrichedEvents,
            transformed_event_count: totalTransformedEvents,
            dropped_event_count: totalDroppedEvents,
            bytes_processed: totalBytesProcessed,
            tuple_operation_count: totalTupleOperations,
            compute_time_ms: totalComputeMs,
            coordination_time_ms: totalCoordinationMs,
            total_time_ms: totalComputeMs + totalCoordinationMs,
            granularity_ratio: ratio(totalComputeMs, totalCoordinationMs),
            avg_worker_latency_ms: totalWorkerResponses > 0 ? totalWorkerLatencyMs / totalWorkerResponses : 0,
            max_worker_latency_ms: maxWorkerLatencyMs,
            error_count: totalErrors,
            remote_nodes_with_work: Array.from(remoteNodesWithWork).sort(),
            actor_distribution_skew: actorDistributionSkew,
            wall_time_ms: host.nowMs() - wallStart,
            events_per_sec: Math.round(totalEventCount * 1000 / Math.max(1, host.nowMs() - wallStart)),
            results,
            nodes,
            roles,
        };
    }
    onRun_scaling_benchmark(payload) {
        const eventsPerBatch = intValue(payload.events_per_batch, 1200);
        const batchCount = intValue(payload.batch_count, 18);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 1);
        const dropRate = floatValue(payload.drop_rate, 0.08);
        const enrichFields = intValue(payload.enrich_fields, 6);
        const results = [];
        let baselineEvtPerSec = 0;
        for (const shardCount of shardCounts) {
            let totalWall = 0, totalCompute = 0, totalCoord = 0, totalEvents = 0, errorCount = 0;
            for (let r = 0; r < benchmarkRounds; r++) {
                const result = this.onRun({ worker_count: shardCount, batch_count: batchCount, events_per_batch: eventsPerBatch, drop_rate: dropRate, enrich_fields: enrichFields });
                totalWall += intValue(result.wall_time_ms, 0);
                totalCompute += intValue(result.compute_time_ms, 0);
                totalCoord += intValue(result.coordination_time_ms, 0);
                totalEvents += intValue(result.event_count, 0);
                errorCount += intValue(result.error_count, 0);
            }
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            const avgEvents = Math.round(totalEvents / benchmarkRounds);
            const evtPerSec = avgWall > 0 ? Math.round(avgEvents * 1000 / avgWall) : 0;
            if (baselineEvtPerSec === 0)
                baselineEvtPerSec = evtPerSec;
            const speedup = baselineEvtPerSec > 0 && evtPerSec > 0 ? evtPerSec / baselineEvtPerSec : 1;
            const efficiency = (speedup / (shardCount / shardCounts[0])) * 100;
            results.push({
                shards: shardCount,
                events_per_sec: evtPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
                speedup: Math.round(speedup * 100) / 100,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                error_count: errorCount,
            });
        }
        return { status: "ok", events_per_batch: eventsPerBatch, batch_count: batchCount, results };
    }
    onRun_weak_scaling_benchmark(payload) {
        const eventsPerWorker = intValue(payload.events_per_worker, 1200);
        const numPasses = intValue(payload.num_passes, 4);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 1);
        const results = [];
        let baselineEvtPerSec = 0;
        for (const shardCount of shardCounts) {
            const groupId = `${STREAM_PREFIX}-bench-${host.nowMs()}`;
            host.createShardGroup({
                groupId,
                actorType: "streaming-worker",
                shardCount,
                partitionStrategy: "hash",
                rebalancePolicy: "manual",
                placement: { strategy: "from_registry" },
                initialState: {},
            });
            let totalCompute = 0, totalCoord = 0, totalEvents = 0, errorCount = 0;
            for (let r = 0; r < benchmarkRounds; r++) {
                const wallStart = host.nowMs();
                // Single broadcast SG — all workers process their own events in parallel
                const sgResult = host.scatterGather({
                    groupId,
                    query: { op: "benchmark_batch", events_per_worker: eventsPerWorker, num_passes: numPasses, seed: r + shardCount * 100 },
                    timeoutMs: 60000,
                });
                const wallMs = host.nowMs() - wallStart;
                let roundCompute = 0;
                let roundEvents = 0;
                for (const resp of sgResult.shardResponses) {
                    const result = recordValue(resp.payload ?? resp);
                    if (result.error) {
                        errorCount++;
                        continue;
                    }
                    roundCompute += intValue(result.compute_ms, 0);
                    roundEvents += intValue(result.events_processed, 0);
                }
                totalCoord += wallMs;
                totalCompute += roundCompute;
                totalEvents += roundEvents;
            }
            const avgWall = Math.round(totalCoord / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.max(avgWall - Math.round(avgCompute / shardCount), 1);
            const avgEvents = Math.round(totalEvents / benchmarkRounds);
            const evtPerSec = avgWall > 0 ? Math.round(avgEvents * 1000 / avgWall) : 0;
            const granularity = avgCoord > 0 ? Math.round((Math.round(avgCompute / shardCount) / avgCoord) * 10) / 10 : 0;
            if (baselineEvtPerSec === 0)
                baselineEvtPerSec = evtPerSec;
            const efficiency = baselineEvtPerSec > 0 ? (evtPerSec / baselineEvtPerSec) * 100 : 100;
            results.push({
                shards: shardCount,
                total_events: avgEvents,
                events_per_sec: evtPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                granularity_ratio: granularity,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                error_count: errorCount,
            });
        }
        return { status: "ok", events_per_worker: eventsPerWorker, num_passes: numPasses, results };
    }
    requestFromPayload(payload) {
        return {
            worker_count: intValue(payload.worker_count, this.state.worker_count),
            batch_count: intValue(payload.batch_count, this.state.batch_count),
            events_per_batch: intValue(payload.events_per_batch, this.state.events_per_batch),
            drop_rate: floatValue(payload.drop_rate, this.state.drop_rate),
            enrich_fields: intValue(payload.enrich_fields, this.state.enrich_fields),
        };
    }
}
class WorkerActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "",
            application_id: "",
            role: "worker",
        };
    }
    onInit(config) {
        const args = config.args ?? {};
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = String(args.role ?? "worker");
    }
    onProcess_batch(payload) {
        const runId = stringValue(payload.run_id);
        const batchIndex = intValue(payload.batch_index, 0);
        const eventsPerBatch = intValue(payload.events_per_batch, 1200);
        const dropRate = floatValue(payload.drop_rate, 0.08);
        const enrichFields = intValue(payload.enrich_fields, 6);
        const filteredEvents = Math.max(0, Math.floor(eventsPerBatch * (1 - dropRate)));
        const droppedEvents = Math.max(0, eventsPerBatch - filteredEvents);
        const enrichedEvents = filteredEvents;
        const transformedEvents = filteredEvents;
        const bytesProcessed = transformedEvents * (180 + enrichFields * 24);
        const computeMs = Math.max(1, Math.floor(eventsPerBatch / 150));
        const latencyMs = computeMs;
        const topStreams = topStreamCounts(workerSeed(this.state.actor_id), batchIndex, transformedEvents);
        host.ts.write([
            STREAM_PREFIX,
            runId,
            "stage_summary",
            batchIndex,
            actorRoleId(this.state.actor_id),
            {
                filtered_events: filteredEvents,
                enriched_events: enrichedEvents,
                transformed_events: transformedEvents,
                dropped_events: droppedEvents,
            },
        ]);
        host.applicationMetricsAdd(this.state.application_id, {
            message_count: 1,
            counter_metrics: {
                worker_messages: 1,
                event_count: eventsPerBatch,
                filtered_events: filteredEvents,
                enriched_events: enrichedEvents,
                transformed_events: transformedEvents,
                dropped_events: droppedEvents,
                bytes_processed: bytesProcessed,
                worker_tuple_operations: 1,
            },
            latency_totals_ms: {
                worker: latencyMs,
                "worker.compute": computeMs,
                "worker.coordination": Math.max(0, latencyMs - computeMs),
            },
            latency_max_ms: {
                worker: latencyMs,
                "worker.compute": computeMs,
                "worker.coordination": Math.max(0, latencyMs - computeMs),
            },
            latency_samples: {
                worker: 1,
                "worker.compute": 1,
                "worker.coordination": 1,
            },
        });
        return {
            status: "ok",
            actor_id: this.state.actor_id,
            node_id: actorNodeId(this.state.actor_id),
            latency_ms: latencyMs,
            event_count: eventsPerBatch,
            filtered_events: filteredEvents,
            enriched_events: enrichedEvents,
            transformed_events: transformedEvents,
            dropped_events: droppedEvents,
            bytes_processed: bytesProcessed,
            tuple_operations: 1,
            top_streams: topStreams,
        };
    }
    // Worker-local benchmark: processes events locally for num_passes without cross-shard data.
    // Leader sends one broadcast SG — enables true weak scaling with one round trip.
    onBenchmark_batch(payload) {
        const eventsPerWorker = intValue(payload.events_per_worker, 1200);
        const numPasses = intValue(payload.num_passes, 4);
        const seed = intValue(payload.seed, 42);
        const compStart = host.nowMs();
        let totalEvents = 0;
        for (let p = 0; p < numPasses; p++) {
            const filteredEvents = Math.max(0, Math.floor(eventsPerWorker * 0.92));
            const enrichedEvents = filteredEvents;
            const transformedEvents = filteredEvents;
            const bytesProcessed = transformedEvents * (180 + 6 * 24);
            // Drive some computation proportional to event count
            const streams = topStreamCounts(seed + p, p, transformedEvents);
            void streams;
            void bytesProcessed;
            totalEvents += eventsPerWorker;
        }
        const computeMs = host.nowMs() - compStart;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.bench_compute": computeMs } });
        }
        catch (_e) { }
        return {
            events_processed: totalEvents,
            passes: numPasses,
            compute_ms: computeMs,
        };
    }
}
function actorNodeId(actorId) {
    const parts = actorId.split("@");
    return parts.length > 1 ? parts[parts.length - 1] : "local";
}
function actorRoleId(actorId) {
    if (!actorId) {
        return "";
    }
    const canonicalSep = actorId.indexOf("//");
    if (canonicalSep >= 0 && canonicalSep + 2 < actorId.length) {
        const rest = actorId.substring(canonicalSep + 2);
        const behaviorSep = rest.indexOf("::");
        if (behaviorSep >= 0) {
            return rest.substring(0, behaviorSep);
        }
        const nodeSep = rest.indexOf("@");
        return nodeSep >= 0 ? rest.substring(0, nodeSep) : rest;
    }
    const childSep = actorId.indexOf(":");
    if (childSep >= 0) {
        return actorId.substring(0, childSep);
    }
    const nodeSep = actorId.indexOf("@");
    return nodeSep >= 0 ? actorId.substring(0, nodeSep) : actorId;
}
function actorApplicationId(actorId) {
    if (!actorId) {
        return "";
    }
    if (actorId.includes("//") && actorId.includes("::")) {
        const rest = actorId.split("//", 2)[1];
        const qualified = rest.split("@", 1)[0];
        const parts = qualified.split("::", 2);
        return parts.length === 2 ? parts[1] : "";
    }
    if (actorId.includes(":") && actorId.includes("@")) {
        return actorId.split(":", 2)[1].split("@", 1)[0];
    }
    return "";
}
function mergeTopStreams(values, topK) {
    const counts = {};
    for (const value of values) {
        counts[value.stream] = (counts[value.stream] ?? 0) + value.count;
    }
    return Object.entries(counts)
        .map(([stream, count]) => ({ stream, count }))
        .sort((a, b) => b.count - a.count)
        .slice(0, topK);
}
function topStreamCounts(seed, batchIndex, events) {
    const streams = ["auth", "edge", "api", "dns", "audit"];
    const results = [];
    let remaining = events;
    for (let index = 0; index < streams.length; index++) {
        const divisor = streams.length - index;
        const count = index === streams.length - 1
            ? remaining
            : Math.max(0, Math.floor(((seed + batchIndex * 17 + index * 11) % 100) / 100 * remaining / divisor));
        results.push({ stream: streams[index], count });
        remaining = Math.max(0, remaining - count);
    }
    return results.sort((a, b) => intValue(b.count, 0) - intValue(a.count, 0));
}
function normalizeWorkerPayload(payload) {
    let current = recordValue(payload);
    while (current && !("status" in current)) {
        let progressed = false;
        for (const key of ["payload", "result", "response", "data"]) {
            if (recordValue(current[key])) {
                current = recordValue(current[key]);
                progressed = true;
                break;
            }
        }
        if (!progressed) {
            break;
        }
    }
    return current;
}
function ensureNodeMetric(metrics, nodeId) {
    if (!metrics[nodeId]) {
        metrics[nodeId] = {
            actors: 0,
            leader_actors: 0,
            worker_actors: 0,
            messages: 0,
            leader_messages: 0,
            worker_messages: 0,
            event_count: 0,
            filtered_events: 0,
            enriched_events: 0,
            transformed_events: 0,
            dropped_events: 0,
            bytes_processed: 0,
            tuple_operations: 0,
            compute_time_ms: 0,
            coordination_time_ms: 0,
            total_latency_ms: 0,
            max_latency_ms: 0,
            responses: 0,
            errors: 0,
        };
    }
    return metrics[nodeId];
}
function ensureRoleMetric(metrics, role) {
    if (!metrics[role]) {
        metrics[role] = {
            actors: 0,
            messages: 0,
            event_count: 0,
            filtered_events: 0,
            enriched_events: 0,
            transformed_events: 0,
            dropped_events: 0,
            bytes_processed: 0,
            tuple_operations: 0,
            compute_time_ms: 0,
            coordination_time_ms: 0,
            total_latency_ms: 0,
            max_latency_ms: 0,
            responses: 0,
            errors: 0,
        };
    }
    return metrics[role];
}
function applyMetricsDelta(nodeMetrics, roleMetrics, startMetrics, endMetrics, nodeId) {
    const counterDelta = saturatingMapDelta(metricsMap(endMetrics, "counter_metrics"), metricsMap(startMetrics, "counter_metrics"));
    const latencyTotalsDelta = saturatingMapDelta(metricsMap(endMetrics, "latency_totals_ms"), metricsMap(startMetrics, "latency_totals_ms"));
    const latencyMaxEnd = metricsMap(endMetrics, "latency_max_ms");
    const latencyMaxStart = metricsMap(startMetrics, "latency_max_ms");
    const latencySamplesDelta = saturatingMapDelta(metricsMap(endMetrics, "latency_samples"), metricsMap(startMetrics, "latency_samples"));
    const messageDelta = Math.max(intValue(endMetrics.message_count, 0) - intValue(startMetrics.message_count, 0), 0);
    const errorDelta = Math.max(intValue(endMetrics.error_count, 0) - intValue(startMetrics.error_count, 0), 0);
    const node = ensureNodeMetric(nodeMetrics, nodeId);
    node.messages += messageDelta;
    node.leader_messages += counterDelta.leader_messages ?? 0;
    node.worker_messages += counterDelta.worker_messages ?? 0;
    node.event_count += counterDelta.event_count ?? 0;
    node.filtered_events += counterDelta.filtered_events ?? 0;
    node.enriched_events += counterDelta.enriched_events ?? 0;
    node.transformed_events += counterDelta.transformed_events ?? 0;
    node.dropped_events += counterDelta.dropped_events ?? 0;
    node.bytes_processed += counterDelta.bytes_processed ?? 0;
    node.tuple_operations += (counterDelta.worker_tuple_operations ?? 0) + (counterDelta.leader_tuple_operations ?? 0);
    node.compute_time_ms += (latencyTotalsDelta["worker.compute"] ?? 0) + (latencyTotalsDelta["leader.compute"] ?? 0);
    node.coordination_time_ms += (latencyTotalsDelta["worker.coordination"] ?? 0) + (latencyTotalsDelta["leader.coordination"] ?? 0);
    node.total_latency_ms += (latencyTotalsDelta.worker ?? 0) + (latencyTotalsDelta.leader ?? 0);
    node.max_latency_ms = Math.max(node.max_latency_ms, latencyMaxEnd.worker ?? 0, latencyMaxStart.worker ?? 0, latencyMaxEnd.leader ?? 0, latencyMaxStart.leader ?? 0);
    node.responses += (latencySamplesDelta.worker ?? 0) + (latencySamplesDelta.leader ?? 0);
    node.errors += errorDelta;
    const leader = ensureRoleMetric(roleMetrics, "leader");
    leader.messages += counterDelta.leader_messages ?? 0;
    leader.tuple_operations += counterDelta.leader_tuple_operations ?? 0;
    leader.compute_time_ms += latencyTotalsDelta["leader.compute"] ?? 0;
    leader.coordination_time_ms += latencyTotalsDelta["leader.coordination"] ?? 0;
    leader.total_latency_ms += latencyTotalsDelta.leader ?? 0;
    leader.max_latency_ms = Math.max(leader.max_latency_ms, latencyMaxEnd.leader ?? 0, latencyMaxStart.leader ?? 0);
    leader.responses += latencySamplesDelta.leader ?? 0;
    leader.errors += errorDelta;
    const worker = ensureRoleMetric(roleMetrics, "worker");
    worker.messages += counterDelta.worker_messages ?? 0;
    worker.event_count += counterDelta.event_count ?? 0;
    worker.filtered_events += counterDelta.filtered_events ?? 0;
    worker.enriched_events += counterDelta.enriched_events ?? 0;
    worker.transformed_events += counterDelta.transformed_events ?? 0;
    worker.dropped_events += counterDelta.dropped_events ?? 0;
    worker.bytes_processed += counterDelta.bytes_processed ?? 0;
    worker.tuple_operations += counterDelta.worker_tuple_operations ?? 0;
    worker.compute_time_ms += latencyTotalsDelta["worker.compute"] ?? 0;
    worker.coordination_time_ms += latencyTotalsDelta["worker.coordination"] ?? 0;
    worker.total_latency_ms += latencyTotalsDelta.worker ?? 0;
    worker.max_latency_ms = Math.max(worker.max_latency_ms, latencyMaxEnd.worker ?? 0, latencyMaxStart.worker ?? 0);
    worker.responses += latencySamplesDelta.worker ?? 0;
    worker.errors += errorDelta;
}
function metricsMap(metrics, field) {
    const value = recordValue(metrics[field]);
    const result = {};
    for (const [key, raw] of Object.entries(value)) {
        result[key] = intValue(raw, 0);
    }
    return result;
}
function saturatingMapDelta(end, start) {
    const result = {};
    for (const [key, endValue] of Object.entries(end)) {
        const startValue = start[key] ?? 0;
        result[key] = endValue > startValue ? endValue - startValue : 0;
    }
    for (const key of Object.keys(start)) {
        if (!(key in result)) {
            result[key] = 0;
        }
    }
    return result;
}
function computeActorCounts(leaderNodeId, shardActorIds) {
    const nodes = {
        [leaderNodeId]: {
            actors: 1,
            leader_actors: 1,
            worker_actors: 0,
        },
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
function averageLatency(metric) {
    return metric.responses > 0 ? metric.total_latency_ms / metric.responses : 0;
}
function ratio(computeMs, coordinationMs) {
    return coordinationMs > 0 ? computeMs / coordinationMs : 0;
}
function workerSeed(actorId) {
    let value = 0;
    for (let index = 0; index < actorId.length; index++) {
        value = (value * 31 + actorId.charCodeAt(index)) % 1000;
    }
    return value;
}
function intArrayValue(value, fallback) {
    if (!Array.isArray(value))
        return fallback;
    const result = [];
    for (const v of value) {
        const n = typeof v === "number" ? Math.trunc(v) : Number.parseInt(String(v), 10);
        result.push(Number.isFinite(n) ? n : 0);
    }
    return result.length > 0 ? result : fallback;
}
function intValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value)) {
        return Math.trunc(value);
    }
    if (typeof value === "string") {
        const parsed = Number.parseInt(value, 10);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
}
function floatValue(value, fallback) {
    if (typeof value === "number" && Number.isFinite(value)) {
        return value;
    }
    if (typeof value === "string") {
        const parsed = Number.parseFloat(value);
        return Number.isFinite(parsed) ? parsed : fallback;
    }
    return fallback;
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
const router = new ActorRouter({
    leader: () => new LeaderActor(),
    worker: () => new WorkerActor(),
    "streaming-leader": () => new LeaderActor(),
    "streaming-worker": () => new WorkerActor(),
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
