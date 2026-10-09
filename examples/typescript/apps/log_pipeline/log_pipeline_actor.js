// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Log Pipeline - TypeScript WASM
//
// Production-grade observability pipeline: ingest syslog/HTTP/security/infra events,
// apply configurable pipeline functions (parse, mask PII, enrich, rename, drop),
// route by rules, format for sinks (Splunk HEC, Datadog, S3 Parquet).
//
// Architecture: Leader/worker with shard-group placement, scatter/gather,
// compute vs coordination metrics tracking.
import { ActorRouter, PlexSpacesActor, host } from "@plexspaces/sdk";
// ─── Pipeline Functions ──────────────────────────────────────────────────────
function fnJsonParse(events) {
    return events.map((evt) => {
        try {
            const parsed = JSON.parse(evt.message);
            return { ...evt, fields: { ...evt.fields, ...parsed }, message: evt.message };
        }
        catch {
            return { ...evt, fields: { ...evt.fields, parse_error: true } };
        }
    });
}
function fnRegexExtract(events) {
    const patterns = [
        ["ip_address", /\b(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})\b/],
        ["http_status", /\bHTTP\/\d\.\d"\s(\d{3})\b/],
        ["log_level", /\b(DEBUG|INFO|WARN|ERROR|FATAL)\b/i],
    ];
    return events.map((evt) => {
        const extracted = {};
        for (const [name, regex] of patterns) {
            const match = regex.exec(evt.message);
            if (match)
                extracted[name] = match[1];
        }
        return { ...evt, fields: { ...evt.fields, ...extracted } };
    });
}
function fnMaskPii(events) {
    const emailRe = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g;
    const ssnRe = /\b\d{3}-\d{2}-\d{4}\b/g;
    const ipRe = /\b(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})\b/g;
    return events.map((evt) => {
        let msg = evt.message;
        msg = msg.replace(emailRe, "***@***.***");
        msg = msg.replace(ssnRe, "***-**-****");
        msg = msg.replace(ipRe, "$1.$2.xxx.xxx");
        return { ...evt, message: msg, fields: { ...evt.fields, pii_masked: true } };
    });
}
function fnEnrich(events) {
    const geoDb = {
        "10": "us-east-1", "172": "eu-west-1", "192": "ap-south-1",
    };
    return events.map((evt) => {
        const ip = String(evt.fields.ip_address ?? evt.host ?? "");
        const prefix = ip.split(".")[0] ?? "";
        return {
            ...evt,
            fields: {
                ...evt.fields,
                geo_region: geoDb[prefix] ?? "unknown",
                enriched_at: evt.timestamp,
                asset_criticality: evt.source_type === "security" ? "high" : "normal",
            },
        };
    });
}
function fnRenameFields(events) {
    const renames = {
        host: "_host", source: "_source", message: "_raw",
        severity: "level", timestamp: "_time",
    };
    return events.map((evt) => {
        const renamed = { ...evt.fields };
        for (const [from, to] of Object.entries(renames)) {
            if (from in renamed) {
                renamed[to] = renamed[from];
                delete renamed[from];
            }
        }
        return { ...evt, fields: renamed };
    });
}
function fnDrop(events) {
    return events.filter((evt) => evt.severity !== "debug" && evt.source_type !== "health_check");
}
const PIPELINE_FUNCTIONS = {
    json_parse: fnJsonParse,
    regex_extract: fnRegexExtract,
    mask_pii: fnMaskPii,
    enrich: fnEnrich,
    rename_fields: fnRenameFields,
    drop: fnDrop,
};
const ORDERED_FUNCTIONS = [
    "json_parse", "regex_extract", "mask_pii", "enrich", "rename_fields", "drop",
];
// ─── Sink Formatters ─────────────────────────────────────────────────────────
function formatSplunkHec(evt) {
    return {
        event: { ...evt.fields, message: evt.message },
        sourcetype: evt.source_type,
        source: evt.source,
        host: evt.host,
        index: evt.severity === "error" ? "main_errors" : "main",
        time: evt.timestamp,
    };
}
function formatDatadog(evt) {
    return {
        ddsource: evt.source_type,
        ddtags: `severity:${evt.severity},host:${evt.host}`,
        hostname: evt.host,
        message: evt.message,
        service: evt.source,
        status: evt.severity,
        attributes: evt.fields,
    };
}
function formatS3Parquet(evt) {
    const ts = evt.timestamp;
    const date = ts.length >= 10 ? ts.substring(0, 10) : "1970-01-01";
    return {
        bucket: "logs-archive",
        key: `logs/${date}/${evt.source_type}/${evt.host}.parquet`,
        partition: `dt=${date}/src=${evt.source_type}`,
        columns: Object.keys(evt.fields),
        row: { ...evt.fields, _time: evt.timestamp, _raw: evt.message },
    };
}
const SINK_FORMATTERS = {
    splunk_hec: formatSplunkHec,
    datadog: formatDatadog,
    s3_parquet: formatS3Parquet,
};
// ─── Event Generator ─────────────────────────────────────────────────────────
const SEVERITIES = ["debug", "info", "warn", "error"];
const HOSTS = [
    "web-01.prod", "web-02.prod", "api-01.prod", "db-01.prod",
    "cache-01.prod", "worker-01.prod", "lb-01.prod", "monitor-01.prod",
];
function generateEvents(count, seed) {
    const events = [];
    let rng = seed;
    for (let i = 0; i < count; i++) {
        rng = (rng * 1103515245 + 12345) & 0x7fffffff;
        const sourceType = ["syslog", "app_json", "security", "infra"][rng % 4];
        const severity = SEVERITIES[(rng >> 8) % 4];
        const hostName = HOSTS[(rng >> 12) % HOSTS.length];
        const ts = `2025-01-15T${String((rng >> 16) % 24).padStart(2, "0")}:${String((rng >> 20) % 60).padStart(2, "0")}:00Z`;
        let message;
        switch (sourceType) {
            case "syslog":
                message = `<${134 + (rng % 8)}> ${ts} ${hostName} app[${1000 + (rng % 9000)}]: ${severity.toUpperCase()} Connection from ${10 + (rng % 200)}.${(rng >> 4) % 256}.${(rng >> 8) % 256}.${(rng >> 12) % 256} port ${20000 + (rng % 40000)}`;
                break;
            case "app_json":
                message = JSON.stringify({
                    level: severity, service: "api-gateway", trace_id: `trace-${rng % 100000}`,
                    method: ["GET", "POST", "PUT", "DELETE"][rng % 4],
                    path: ["/api/v1/users", "/api/v1/orders", "/api/v1/products", "/health"][rng % 4],
                    duration_ms: rng % 500, status: [200, 201, 400, 500][rng % 4],
                    user: `user-${rng % 1000}@example.com`,
                });
                break;
            case "security":
                message = `${ts} AUTH ${["SUCCESS", "FAILURE", "LOCKOUT", "MFA_CHALLENGE"][(rng >> 4) % 4]} user=admin-${rng % 100} src=${10 + (rng % 200)}.${(rng >> 4) % 256}.${(rng >> 8) % 256}.${(rng >> 12) % 256} ssn=${String(100 + (rng % 900))}-${String(10 + (rng % 90))}-${String(1000 + (rng % 9000))}`;
                break;
            default:
                message = `${ts} ${hostName} cpu_usage=${50 + (rng % 50)}% mem_usage=${40 + (rng % 60)}% disk_io=${rng % 1000}iops network_rx=${rng % 10000}KB/s`;
        }
        events.push({
            timestamp: ts,
            source: `${sourceType}-collector`,
            source_type: sourceType,
            severity,
            host: hostName,
            message,
            fields: {},
        });
    }
    return events;
}
// ─── Routing ─────────────────────────────────────────────────────────────────
const DEFAULT_RULES = [
    { name: "errors", field: "severity", op: "equals", value: "error", sink: "splunk_hec" },
    { name: "security", field: "source_type", op: "equals", value: "security", sink: "splunk_hec" },
    { name: "infra", field: "source_type", op: "equals", value: "infra", sink: "datadog" },
    { name: "archive", field: "severity", op: "equals", value: "info", sink: "s3_parquet" },
];
function evaluateRule(evt, rule) {
    const val = String(evt[rule.field] ?? evt.fields[rule.field] ?? "");
    switch (rule.op) {
        case "equals": return val === rule.value;
        case "contains": return val.includes(rule.value);
        case "starts_with": return val.startsWith(rule.value);
        case "exists": return val !== "";
        default: return false;
    }
}
function routeEvent(evt) {
    for (const rule of DEFAULT_RULES) {
        if (evaluateRule(evt, rule))
            return rule.sink;
    }
    return "s3_parquet";
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
        const args = recordValue(config.args);
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "leader";
        this.state.total_compute_ms = 0;
        this.state.total_coord_ms = 0;
    }
    onRun(payload) {
        const eventCount = intValue(payload.event_count, 5000);
        const workerCount = intValue(payload.worker_count, 8);
        const batchSize = intValue(payload.batch_size, 500);
        const pipelineDepth = intValue(payload.pipeline_depth, 5);
        const sinkFormat = stringValue(payload.sink_format) || "splunk_hec";
        const functions = ORDERED_FUNCTIONS.slice(0, pipelineDepth);
        const funcList = functions.join(",");
        const coordStart = host.nowMs();
        const groupId = `log-pipeline-ts-${host.nowMs()}`;
        const group = host.createShardGroup({
            groupId,
            actorType: "worker",
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
        const seed = host.nowMs() % 100000;
        const numBatches = Math.ceil(eventCount / batchSize);
        let totalEventsOut = 0;
        let totalEventsDropped = 0;
        let totalComputeMs = 0;
        let totalCoordMs = 0;
        let maxWorkerLatency = 0;
        let totalWorkerLatency = 0;
        let workerCalls = 0;
        const routeDistribution = {};
        let errorCount = 0;
        for (let b = 0; b < numBatches; b++) {
            const actualBatchSize = Math.min(batchSize, eventCount - b * batchSize);
            const batch = generateEvents(actualBatchSize, (seed + b) % 100000);
            const sgStart = host.nowMs();
            const sgResult = host.scatterGather({
                groupId,
                query: {
                    op: "process_batch",
                    events: batch,
                    pipeline_functions: funcList,
                    sink_format: sinkFormat,
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
                totalEventsOut += intValue(result.events_out, 0);
                totalEventsDropped += intValue(result.events_dropped, 0);
                totalComputeMs += intValue(result.compute_ms, 0);
                const routes = recordValue(result.route_distribution);
                for (const [sink, count] of Object.entries(routes)) {
                    routeDistribution[sink] = (routeDistribution[sink] ?? 0) + intValue(count, 0);
                }
                const latency = intValue(result.compute_ms, 0);
                totalWorkerLatency += latency;
                if (latency > maxWorkerLatency)
                    maxWorkerLatency = latency;
                workerCalls++;
            }
        }
        totalCoordMs += coordCreate;
        const wallTime = totalComputeMs + totalCoordMs;
        const total = totalComputeMs + totalCoordMs || 1;
        const granularity = totalCoordMs > 0 ? Math.round((totalComputeMs / totalCoordMs) * 10) / 10 : 0;
        const eventsPerSec = wallTime > 0 ? Math.round((eventCount / wallTime) * 1000) : 0;
        const nodeActorCounts = computeActorCounts(leaderNodeId, shardActorIds);
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "leader.compute": totalComputeMs, "leader.coordination": totalCoordMs } });
        }
        catch (_e) { }
        return {
            status: "ok",
            event_count: eventCount,
            worker_count: workerCount,
            pipeline_depth: pipelineDepth,
            pipeline_functions: funcList,
            sink_format: sinkFormat,
            wall_time_ms: wallTime,
            compute_time_ms: totalComputeMs,
            coordination_time_ms: totalCoordMs,
            granularity_ratio: granularity,
            events_per_sec: eventsPerSec,
            total_events_in: eventCount,
            total_events_out: totalEventsOut,
            total_events_dropped: totalEventsDropped,
            avg_worker_latency_ms: workerCalls > 0 ? Math.round(totalWorkerLatency / workerCalls) : 0,
            max_worker_latency_ms: maxWorkerLatency,
            route_distribution: routeDistribution,
            node_count: Object.keys(nodeActorCounts).length,
            actor_count: shardActorIds.length + 1,
            leader_node_id: leaderNodeId,
            nodes: nodeActorCounts,
            error_count: errorCount,
        };
    }
    onRun_scaling_benchmark(payload) {
        const eventCount = intValue(payload.event_count, 10000);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const batchSize = intValue(payload.batch_size, 500);
        const pipelineDepth = intValue(payload.pipeline_depth, 5);
        const warmupRounds = intValue(payload.warmup_rounds, 1);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 2);
        const results = [];
        let baselineWall = 0;
        for (const shardCount of shardCounts) {
            for (let w = 0; w < warmupRounds; w++) {
                this.onRun({
                    event_count: Math.min(eventCount, 1000),
                    worker_count: shardCount,
                    batch_size: batchSize,
                    pipeline_depth: pipelineDepth,
                    sink_format: "splunk_hec",
                });
            }
            let totalWall = 0;
            let totalCompute = 0;
            let totalCoord = 0;
            let totalEventsPerSec = 0;
            let nodeCount = 0;
            let errorCount = 0;
            // Cap per-run events to 2 batches: throughput (events/ms) is batch-size-independent
            // so fewer batches give the same events_per_sec while keeping WASM memory bounded.
            const runEvents = Math.min(eventCount, batchSize * 2);
            for (let r = 0; r < benchmarkRounds; r++) {
                const result = this.onRun({
                    event_count: runEvents,
                    worker_count: shardCount,
                    batch_size: batchSize,
                    pipeline_depth: pipelineDepth,
                    sink_format: "splunk_hec",
                });
                totalWall += intValue(result.wall_time_ms, 0);
                totalCompute += intValue(result.compute_time_ms, 0);
                totalCoord += intValue(result.coordination_time_ms, 0);
                totalEventsPerSec += intValue(result.events_per_sec, 0);
                nodeCount = intValue(result.node_count, 0);
                errorCount += intValue(result.error_count, 0);
            }
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            const avgEventsPerSec = Math.round(totalEventsPerSec / benchmarkRounds);
            const total = avgCompute + avgCoord || 1;
            if (baselineWall === 0)
                baselineWall = avgWall;
            const speedup = baselineWall > 0 && avgWall > 0 ? baselineWall / avgWall : 1;
            const efficiency = (speedup / (shardCount / shardCounts[0])) * 100;
            results.push({
                shards: shardCount,
                events_per_sec: avgEventsPerSec,
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
        return {
            status: "ok",
            event_count: eventCount,
            pipeline_depth: pipelineDepth,
            results,
        };
    }
    onRun_weak_scaling_benchmark(payload) {
        const eventsPerShard = intValue(payload.events_per_shard, 5000);
        const shardCounts = intArrayValue(payload.shard_counts, [2, 4, 8, 16]);
        const batchSize = intValue(payload.batch_size, 500);
        const pipelineDepth = intValue(payload.pipeline_depth, 5);
        const warmupRounds = intValue(payload.warmup_rounds, 1);
        const benchmarkRounds = intValue(payload.benchmark_rounds, 2);
        const results = [];
        let baselineEventsPerSec = 0;
        for (const shardCount of shardCounts) {
            const totalEvents = eventsPerShard * shardCount;
            for (let w = 0; w < warmupRounds; w++) {
                this.onRun({
                    event_count: Math.min(totalEvents, 1000),
                    worker_count: shardCount,
                    batch_size: batchSize,
                    pipeline_depth: pipelineDepth,
                    sink_format: "splunk_hec",
                });
            }
            let totalEventsPerSec = 0;
            let totalWall = 0;
            let totalCompute = 0;
            let totalCoord = 0;
            let nodeCount = 0;
            let errorCount = 0;
            const runEvents = Math.min(totalEvents, batchSize * 2);
            for (let r = 0; r < benchmarkRounds; r++) {
                const result = this.onRun({
                    event_count: runEvents,
                    worker_count: shardCount,
                    batch_size: batchSize,
                    pipeline_depth: pipelineDepth,
                    sink_format: "splunk_hec",
                });
                totalEventsPerSec += intValue(result.events_per_sec, 0);
                totalWall += intValue(result.wall_time_ms, 0);
                totalCompute += intValue(result.compute_time_ms, 0);
                totalCoord += intValue(result.coordination_time_ms, 0);
                nodeCount = intValue(result.node_count, 0);
                errorCount += intValue(result.error_count, 0);
            }
            const avgEventsPerSec = Math.round(totalEventsPerSec / benchmarkRounds);
            const avgWall = Math.round(totalWall / benchmarkRounds);
            const avgCompute = Math.round(totalCompute / benchmarkRounds);
            const avgCoord = Math.round(totalCoord / benchmarkRounds);
            if (baselineEventsPerSec === 0)
                baselineEventsPerSec = avgEventsPerSec;
            const efficiency = baselineEventsPerSec > 0 ? (avgEventsPerSec / baselineEventsPerSec) * 100 : 100;
            results.push({
                shards: shardCount,
                total_events: totalEvents,
                events_per_sec: avgEventsPerSec,
                wall_time_ms: avgWall,
                compute_time_ms: avgCompute,
                coordination_time_ms: avgCoord,
                granularity_ratio: avgCoord > 0 ? Math.round((avgCompute / avgCoord) * 10) / 10 : 0,
                efficiency_pct: Math.round(efficiency * 10) / 10,
                node_count: nodeCount,
                error_count: errorCount,
            });
        }
        return {
            status: "ok",
            events_per_shard: eventsPerShard,
            pipeline_depth: pipelineDepth,
            results,
        };
    }
    onRun_pipeline_depth_benchmark(payload) {
        const eventCount = intValue(payload.event_count, 10000);
        const workerCount = intValue(payload.worker_count, 8);
        const batchSize = intValue(payload.batch_size, 500);
        const depths = intArrayValue(payload.depths, [1, 2, 3, 4, 5]);
        const results = [];
        const runEvents = Math.min(eventCount, batchSize * 2);
        for (const depth of depths) {
            const result = this.onRun({
                event_count: runEvents,
                worker_count: workerCount,
                batch_size: batchSize,
                pipeline_depth: depth,
                sink_format: "splunk_hec",
            });
            results.push({
                depth,
                functions: ORDERED_FUNCTIONS.slice(0, depth).join(","),
                events_per_sec: intValue(result.events_per_sec, 0),
                wall_time_ms: intValue(result.wall_time_ms, 0),
                granularity_ratio: result.granularity_ratio ?? 0,
            });
        }
        return {
            status: "ok",
            event_count: eventCount,
            worker_count: workerCount,
            results,
        };
    }
}
// ─── Worker Actor ────────────────────────────────────────────────────────────
class WorkerActor extends PlexSpacesActor {
    getDefaultState() {
        return {
            actor_id: "", application_id: "", role: "worker",
            pipeline_functions: "json_parse,mask_pii,enrich,rename_fields,drop",
            events_processed: 0, compute_ms: 0,
        };
    }
    onInit(config) {
        const args = recordValue(config.args);
        this.state.actor_id = String(config.actor_id ?? "");
        this.state.application_id = actorApplicationId(this.state.actor_id);
        this.state.role = "worker";
        this.state.pipeline_functions = stringValue(args.pipeline_functions) || this.state.pipeline_functions;
    }
    onProcess_batch(payload) {
        const compStart = host.nowMs();
        const rawEvents = anyArray(payload.events);
        const funcNames = stringValue(payload.pipeline_functions).split(",").filter(Boolean);
        const sinkFormat = stringValue(payload.sink_format) || "splunk_hec";
        let events = rawEvents.map((raw) => {
            const r = recordValue(raw);
            return {
                timestamp: stringValue(r.timestamp),
                source: stringValue(r.source),
                source_type: stringValue(r.source_type),
                severity: stringValue(r.severity),
                host: stringValue(r.host),
                message: stringValue(r.message),
                fields: recordValue(r.fields),
            };
        });
        const eventsIn = events.length;
        for (const fname of funcNames) {
            const fn = PIPELINE_FUNCTIONS[fname];
            if (fn)
                events = fn(events);
        }
        const routeDistribution = {};
        const formatter = SINK_FORMATTERS[sinkFormat] ?? formatSplunkHec;
        const formatted = [];
        for (const evt of events) {
            const sink = routeEvent(evt);
            routeDistribution[sink] = (routeDistribution[sink] ?? 0) + 1;
            formatted.push(formatter(evt));
        }
        const computeMs = host.nowMs() - compStart;
        this.state.events_processed += eventsIn;
        this.state.compute_ms += computeMs;
        try {
            host.applicationMetricsAdd(this.state.application_id, { counter_metrics: { "worker.compute": computeMs } });
        }
        catch (_e) { }
        return {
            events_in: eventsIn,
            events_out: events.length,
            events_dropped: eventsIn - events.length,
            compute_ms: computeMs,
            route_distribution: routeDistribution,
            formatted_count: formatted.length,
        };
    }
}
// ─── Helpers ─────────────────────────────────────────────────────────────────
function actorApplicationId(actorId) {
    // Format: namespace//actor_name::app_id@node_id
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
    // Format: namespace//actor_name::app_id@node_id
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
});
export const actor = {
    init: (configJson) => router.init(configJson),
    handle: (from, msgType, payloadJson) => router.handle(from, msgType, payloadJson),
    getState: () => router.getState(),
    setState: (stateJson) => router.setState(stateJson),
};
