// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Log Pipeline - Rust WASM app
//
// Production-grade observability pipeline: ingest syslog/HTTP/security/infra events,
// apply configurable pipeline functions (parse, mask PII, enrich, rename, drop),
// route by rules, format for sinks (Splunk HEC, Datadog, S3 Parquet).
//
// Roles (set via args.role in app-config.toml):
//   leader  — orchestrates pipeline, creates shard groups, runs benchmarks
//   worker  — processes event batches through function chain

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use plexspaces_proto::actor::v1::{
    CreateShardGroupRequest, CreateShardGroupResponse, NodePlacement,
    NodePlacementStrategy, PartitionStrategy, RebalancePolicy, ScatterGatherRequest,
    ScatterGatherResponse,
};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use prost::Message as ProstMessage;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_logging::now_ms;
use plexspaces::actor::host_shard::{create_shard_group, scatter_gather};
use plexspaces::actor::host_metrics::application_metrics_add;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AppState {
    actor_id: String,
    application_id: String,
    role: String,
    // worker fields
    pipeline_functions: String,
    events_processed: u64,
    compute_ms: u64,
}

#[derive(Debug, Deserialize)]
struct InitConfig {
    actor_id: Option<String>,
    #[serde(default)]
    args: HashMap<String, Value>,
}

static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

fn state() -> &'static Mutex<AppState> {
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn application_id_from_actor(actor_id: &str) -> String {
    actor_id.split('/').next().unwrap_or("").to_string()
}

fn node_id_from_actor(actor_id: &str) -> String {
    actor_id.split('/').last().unwrap_or("").to_string()
}

// ---------------------------------------------------------------------------
// Log Event types
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LogEvent {
    timestamp: String,
    source: String,
    source_type: String,
    severity: String,
    host: String,
    message: String,
    #[serde(default)]
    fields: HashMap<String, Value>,
}

// ---------------------------------------------------------------------------
// Pipeline Functions
// ---------------------------------------------------------------------------

fn fn_json_parse(events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.into_iter().map(|mut evt| {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, Value>>(&evt.message) {
            for (k, v) in parsed {
                evt.fields.insert(k, v);
            }
        } else {
            evt.fields.insert("parse_error".into(), json!(true));
        }
        evt
    }).collect()
}

fn fn_regex_extract(events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.into_iter().map(|mut evt| {
        // Simple IP extraction without regex crate (WASM-friendly)
        let msg = &evt.message;
        if let Some(pos) = msg.find(|c: char| c.is_ascii_digit()) {
            let slice = &msg[pos..];
            let parts: Vec<&str> = slice.split(|c: char| !c.is_ascii_digit() && c != '.').next().unwrap_or("").split('.').collect();
            if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
                evt.fields.insert("ip_address".into(), json!(parts.join(".")));
            }
        }
        for level in &["DEBUG", "INFO", "WARN", "ERROR", "FATAL"] {
            if msg.contains(level) {
                evt.fields.insert("log_level".into(), json!(level.to_lowercase()));
                break;
            }
        }
        evt
    }).collect()
}

fn fn_mask_pii(events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.into_iter().map(|mut evt| {
        let mut msg = evt.message.clone();
        // Mask emails (simple pattern)
        while let Some(at_pos) = msg.find('@') {
            let start = msg[..at_pos].rfind(|c: char| c.is_whitespace() || c == '"' || c == ',').map(|p| p + 1).unwrap_or(0);
            let end = msg[at_pos..].find(|c: char| c.is_whitespace() || c == '"' || c == ',').map(|p| at_pos + p).unwrap_or(msg.len());
            msg = format!("{}***@***.***{}", &msg[..start], &msg[end..]);
        }
        // Mask SSN-like patterns (###-##-####)
        let mut masked = String::with_capacity(msg.len());
        let chars: Vec<char> = msg.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            if i + 10 < chars.len()
                && chars[i].is_ascii_digit() && chars[i+1].is_ascii_digit() && chars[i+2].is_ascii_digit()
                && chars[i+3] == '-'
                && chars[i+4].is_ascii_digit() && chars[i+5].is_ascii_digit()
                && chars[i+6] == '-'
                && chars[i+7].is_ascii_digit() && chars[i+8].is_ascii_digit() && chars[i+9].is_ascii_digit() && chars[i+10].is_ascii_digit()
            {
                masked.push_str("***-**-****");
                i += 11;
            } else {
                masked.push(chars[i]);
                i += 1;
            }
        }
        evt.message = masked;
        evt.fields.insert("pii_masked".into(), json!(true));
        evt
    }).collect()
}

fn fn_enrich(events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.into_iter().map(|mut evt| {
        let ip = evt.fields.get("ip_address")
            .and_then(|v| v.as_str())
            .unwrap_or(&evt.host);
        let prefix = ip.split('.').next().unwrap_or("");
        let region = match prefix {
            "10" => "us-east-1",
            "172" => "eu-west-1",
            "192" => "ap-south-1",
            _ => "unknown",
        };
        evt.fields.insert("geo_region".into(), json!(region));
        evt.fields.insert("enriched_at".into(), json!(evt.timestamp));
        let criticality = if evt.source_type == "security" { "high" } else { "normal" };
        evt.fields.insert("asset_criticality".into(), json!(criticality));
        evt
    }).collect()
}

fn fn_rename_fields(events: Vec<LogEvent>) -> Vec<LogEvent> {
    let renames: Vec<(&str, &str)> = vec![
        ("host", "_host"), ("source", "_source"), ("message", "_raw"),
        ("severity", "level"), ("timestamp", "_time"),
    ];
    events.into_iter().map(|mut evt| {
        for (from, to) in &renames {
            if let Some(val) = evt.fields.remove(*from) {
                evt.fields.insert(to.to_string(), val);
            }
        }
        evt
    }).collect()
}

fn fn_drop(events: Vec<LogEvent>) -> Vec<LogEvent> {
    events.into_iter().filter(|evt| {
        evt.severity != "debug" && evt.source_type != "health_check"
    }).collect()
}

const ORDERED_FUNCTIONS: &[&str] = &[
    "json_parse", "regex_extract", "mask_pii", "enrich", "rename_fields", "drop",
];

fn apply_pipeline(events: Vec<LogEvent>, func_names: &[&str]) -> Vec<LogEvent> {
    let mut result = events;
    for name in func_names {
        result = match *name {
            "json_parse" => fn_json_parse(result),
            "regex_extract" => fn_regex_extract(result),
            "mask_pii" => fn_mask_pii(result),
            "enrich" => fn_enrich(result),
            "rename_fields" => fn_rename_fields(result),
            "drop" => fn_drop(result),
            _ => result,
        };
    }
    result
}

// ---------------------------------------------------------------------------
// Sink Formatters
// ---------------------------------------------------------------------------

fn format_splunk_hec(evt: &LogEvent) -> Value {
    json!({
        "event": { "message": evt.message, "fields": evt.fields },
        "sourcetype": evt.source_type,
        "source": evt.source,
        "host": evt.host,
        "index": if evt.severity == "error" { "main_errors" } else { "main" },
        "time": evt.timestamp,
    })
}

fn format_datadog(evt: &LogEvent) -> Value {
    json!({
        "ddsource": evt.source_type,
        "ddtags": format!("severity:{},host:{}", evt.severity, evt.host),
        "hostname": evt.host,
        "message": evt.message,
        "service": evt.source,
        "status": evt.severity,
        "attributes": evt.fields,
    })
}

fn format_s3_parquet(evt: &LogEvent) -> Value {
    let date = if evt.timestamp.len() >= 10 { &evt.timestamp[..10] } else { "1970-01-01" };
    json!({
        "bucket": "logs-archive",
        "key": format!("logs/{}/{}/{}.parquet", date, evt.source_type, evt.host),
        "partition": format!("dt={}/src={}", date, evt.source_type),
        "columns": evt.fields.keys().collect::<Vec<_>>(),
        "row": { "_time": evt.timestamp, "_raw": evt.message },
    })
}

fn format_event(evt: &LogEvent, sink: &str) -> Value {
    match sink {
        "splunk_hec" => format_splunk_hec(evt),
        "datadog" => format_datadog(evt),
        "s3_parquet" => format_s3_parquet(evt),
        _ => format_splunk_hec(evt),
    }
}

// ---------------------------------------------------------------------------
// Event Generator
// ---------------------------------------------------------------------------

const SEVERITIES: &[&str] = &["debug", "info", "warn", "error"];
const HOSTS: &[&str] = &[
    "web-01.prod", "web-02.prod", "api-01.prod", "db-01.prod",
    "cache-01.prod", "worker-01.prod", "lb-01.prod", "monitor-01.prod",
];
const SOURCE_TYPES: &[&str] = &["syslog", "app_json", "security", "infra"];

fn generate_events(count: usize, seed: u64) -> Vec<LogEvent> {
    let mut events = Vec::with_capacity(count);
    let mut rng = seed;
    for _ in 0..count {
        rng = (rng.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
        let source_type = SOURCE_TYPES[(rng as usize) % 4];
        let severity = SEVERITIES[((rng >> 8) as usize) % 4];
        let host = HOSTS[((rng >> 12) as usize) % HOSTS.len()];
        let hour = (rng >> 16) % 24;
        let minute = (rng >> 20) % 60;
        let ts = format!("2025-01-15T{:02}:{:02}:00Z", hour, minute);

        let message = match source_type {
            "syslog" => format!(
                "<{}> {} {}[{}]: {} Connection from {}.{}.{}.{} port {}",
                134 + (rng % 8), ts, host, 1000 + (rng % 9000), severity.to_uppercase(),
                10 + (rng % 200), (rng >> 4) % 256, (rng >> 8) % 256, (rng >> 12) % 256,
                20000 + (rng % 40000)
            ),
            "app_json" => serde_json::to_string(&json!({
                "level": severity,
                "service": "api-gateway",
                "trace_id": format!("trace-{}", rng % 100000),
                "method": ["GET", "POST", "PUT", "DELETE"][(rng as usize) % 4],
                "path": ["/api/v1/users", "/api/v1/orders", "/api/v1/products", "/health"][(rng as usize) % 4],
                "duration_ms": rng % 500,
                "status": [200, 201, 400, 500][(rng as usize) % 4],
                "user": format!("user-{}@example.com", rng % 1000),
            })).unwrap_or_default(),
            "security" => format!(
                "{} AUTH {} user=admin-{} src={}.{}.{}.{} ssn={}-{}-{}",
                ts, ["SUCCESS", "FAILURE", "LOCKOUT", "MFA_CHALLENGE"][((rng >> 4) as usize) % 4],
                rng % 100, 10 + (rng % 200), (rng >> 4) % 256, (rng >> 8) % 256, (rng >> 12) % 256,
                100 + (rng % 900), 10 + (rng % 90), 1000 + (rng % 9000)
            ),
            _ => format!(
                "{} {} cpu_usage={}% mem_usage={}% disk_io={}iops network_rx={}KB/s",
                ts, host, 50 + (rng % 50), 40 + (rng % 60), rng % 1000, rng % 10000
            ),
        };

        events.push(LogEvent {
            timestamp: ts,
            source: format!("{}-collector", source_type),
            source_type: source_type.to_string(),
            severity: severity.to_string(),
            host: host.to_string(),
            message,
            fields: HashMap::new(),
        });
    }
    events
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

fn route_event(evt: &LogEvent) -> &'static str {
    if evt.severity == "error" { return "splunk_hec"; }
    if evt.source_type == "security" { return "splunk_hec"; }
    if evt.source_type == "infra" { return "datadog"; }
    if evt.severity == "info" { return "s3_parquet"; }
    "s3_parquet"
}

// ---------------------------------------------------------------------------
// Leader logic
// ---------------------------------------------------------------------------

fn handle_run(payload: &Value) -> Value {
    let event_count = payload["event_count"].as_u64().unwrap_or(5000) as usize;
    let worker_count = payload["worker_count"].as_u64().unwrap_or(8) as u32;
    let batch_size = payload["batch_size"].as_u64().unwrap_or(500) as usize;
    let pipeline_depth = payload["pipeline_depth"].as_u64().unwrap_or(5) as usize;
    let sink_format = payload["sink_format"].as_str().unwrap_or("splunk_hec");

    let functions: Vec<&str> = ORDERED_FUNCTIONS.iter().take(pipeline_depth).copied().collect();
    let func_list = functions.join(",");

    let coord_start = now_ms();
    let group_id = format!("log-pipeline-rs-{}", now_ms());

    let sg_req = CreateShardGroupRequest {
        group_id: group_id.clone(),
        actor_type: "worker".to_string(),
        shard_count: worker_count,
        partition_strategy: PartitionStrategy::Hash as i32,
        rebalance_policy: RebalancePolicy::Manual as i32,
        placement: Some(NodePlacement {
            strategy: NodePlacementStrategy::FromRegistry as i32,
            ..Default::default()
        }),
        ..Default::default()
    };
    let sg_bytes = sg_req.encode_to_vec();
    let sg_resp_bytes = create_shard_group(&sg_bytes);
    let sg_resp = CreateShardGroupResponse::decode(sg_resp_bytes.as_slice()).unwrap_or_default();
    let shard_actor_ids: Vec<String> = sg_resp.shard_actor_ids.clone();
    if shard_actor_ids.is_empty() {
        return json!({"status": "error", "error": "failed to create worker shard group"});
    }
    let coord_create = now_ms() - coord_start;

    let st = state().lock().unwrap();
    let leader_node_id = node_id_from_actor(&st.actor_id);
    let app_id = st.application_id.clone();
    drop(st);

    let events = generate_events(event_count, now_ms() % 100000);
    let batches: Vec<Vec<LogEvent>> = events.chunks(batch_size).map(|c| c.to_vec()).collect();

    let mut total_events_out: u64 = 0;
    let mut total_events_dropped: u64 = 0;
    let mut total_compute_ms: u64 = 0;
    let mut total_coord_ms: u64 = coord_create;
    let mut max_worker_latency: u64 = 0;
    let mut total_worker_latency: u64 = 0;
    let mut worker_calls: u64 = 0;
    let mut route_distribution: HashMap<String, u64> = HashMap::new();
    let mut error_count: u64 = 0;

    for batch in &batches {
        let batch_payload = json!({
            "op": "process_batch",
            "events": batch,
            "pipeline_functions": func_list,
            "sink_format": sink_format,
        });

        let sg_scatter = ScatterGatherRequest {
            group_id: group_id.clone(),
            payload: serde_json::to_vec(&batch_payload).unwrap_or_default(),
            timeout_ms: 30000,
            ..Default::default()
        };
        let sg_start = now_ms();
        let sg_scatter_bytes = sg_scatter.encode_to_vec();
        let sg_result_bytes = scatter_gather(&sg_scatter_bytes);
        let sg_elapsed = now_ms() - sg_start;
        total_coord_ms += sg_elapsed;

        if let Ok(sg_result) = ScatterGatherResponse::decode(sg_result_bytes.as_slice()) {
            for resp in &sg_result.responses {
                if let Ok(result) = serde_json::from_slice::<Value>(&resp.payload) {
                    if result.get("error").is_some() {
                        error_count += 1;
                        continue;
                    }
                    total_events_out += result["events_out"].as_u64().unwrap_or(0);
                    total_events_dropped += result["events_dropped"].as_u64().unwrap_or(0);
                    let comp = result["compute_ms"].as_u64().unwrap_or(0);
                    total_compute_ms += comp;
                    total_worker_latency += comp;
                    if comp > max_worker_latency { max_worker_latency = comp; }
                    worker_calls += 1;

                    if let Some(routes) = result["route_distribution"].as_object() {
                        for (sink, count) in routes {
                            *route_distribution.entry(sink.clone()).or_insert(0) += count.as_u64().unwrap_or(0);
                        }
                    }
                }
            }
        }
    }

    let wall_time = total_compute_ms + total_coord_ms;
    let total = total_compute_ms + total_coord_ms;
    let granularity = if total_coord_ms > 0 { (total_compute_ms as f64 / total_coord_ms as f64 * 10.0).round() / 10.0 } else { 0.0 };
    let events_per_sec = if wall_time > 0 { (event_count as u64 * 1000) / wall_time } else { 0 };

    application_metrics_add(&app_id, "leader.compute", total_compute_ms);
    application_metrics_add(&app_id, "leader.coordination", total_coord_ms);

    let mut nodes: HashMap<String, Value> = HashMap::new();
    nodes.insert(leader_node_id.clone(), json!({"actors": 1, "leader_actors": 1, "worker_actors": 0}));
    for aid in &shard_actor_ids {
        let nid = node_id_from_actor(aid);
        let entry = nodes.entry(nid).or_insert_with(|| json!({"actors": 0, "leader_actors": 0, "worker_actors": 0}));
        if let Some(obj) = entry.as_object_mut() {
            *obj.entry("actors").or_insert(json!(0)) = json!(obj["actors"].as_u64().unwrap_or(0) + 1);
            *obj.entry("worker_actors").or_insert(json!(0)) = json!(obj["worker_actors"].as_u64().unwrap_or(0) + 1);
        }
    }

    json!({
        "status": "ok",
        "event_count": event_count,
        "worker_count": worker_count,
        "pipeline_depth": pipeline_depth,
        "pipeline_functions": func_list,
        "sink_format": sink_format,
        "wall_time_ms": wall_time,
        "compute_time_ms": total_compute_ms,
        "coordination_time_ms": total_coord_ms,
        "granularity_ratio": granularity,
        "events_per_sec": events_per_sec,
        "total_events_in": event_count,
        "total_events_out": total_events_out,
        "total_events_dropped": total_events_dropped,
        "avg_worker_latency_ms": if worker_calls > 0 { total_worker_latency / worker_calls } else { 0 },
        "max_worker_latency_ms": max_worker_latency,
        "route_distribution": route_distribution,
        "node_count": nodes.len(),
        "actor_count": shard_actor_ids.len() + 1,
        "leader_node_id": leader_node_id,
        "nodes": nodes,
        "error_count": error_count,
    })
}

fn handle_scaling_benchmark(payload: &Value) -> Value {
    let event_count = payload["event_count"].as_u64().unwrap_or(10000) as usize;
    let shard_counts: Vec<u64> = payload["shard_counts"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_else(|| vec![2, 4, 8, 16]);
    let batch_size = payload["batch_size"].as_u64().unwrap_or(500);
    let pipeline_depth = payload["pipeline_depth"].as_u64().unwrap_or(5);
    let warmup_rounds = payload["warmup_rounds"].as_u64().unwrap_or(1);
    let benchmark_rounds = payload["benchmark_rounds"].as_u64().unwrap_or(2);

    let mut results = Vec::new();
    let mut baseline_wall: u64 = 0;

    for &shard_count in &shard_counts {
        for _ in 0..warmup_rounds {
            handle_run(&json!({
                "event_count": std::cmp::min(event_count, 1000),
                "worker_count": shard_count,
                "batch_size": batch_size,
                "pipeline_depth": pipeline_depth,
                "sink_format": "splunk_hec",
            }));
        }

        let mut total_wall: u64 = 0;
        let mut total_compute: u64 = 0;
        let mut total_coord: u64 = 0;
        let mut total_eps: u64 = 0;
        let mut node_count: u64 = 0;
        let mut error_count: u64 = 0;

        for _ in 0..benchmark_rounds {
            let r = handle_run(&json!({
                "event_count": event_count,
                "worker_count": shard_count,
                "batch_size": batch_size,
                "pipeline_depth": pipeline_depth,
                "sink_format": "splunk_hec",
            }));
            total_wall += r["wall_time_ms"].as_u64().unwrap_or(0);
            total_compute += r["compute_time_ms"].as_u64().unwrap_or(0);
            total_coord += r["coordination_time_ms"].as_u64().unwrap_or(0);
            total_eps += r["events_per_sec"].as_u64().unwrap_or(0);
            node_count = r["node_count"].as_u64().unwrap_or(0);
            error_count += r["error_count"].as_u64().unwrap_or(0);
        }

        let avg_wall = total_wall / benchmark_rounds;
        let avg_compute = total_compute / benchmark_rounds;
        let avg_coord = total_coord / benchmark_rounds;
        let avg_eps = total_eps / benchmark_rounds;
        let total = avg_compute + avg_coord;

        if baseline_wall == 0 { baseline_wall = avg_wall; }
        let speedup = if avg_wall > 0 { baseline_wall as f64 / avg_wall as f64 } else { 1.0 };
        let base_shards = shard_counts[0];
        let efficiency = speedup / (shard_count as f64 / base_shards as f64) * 100.0;

        results.push(json!({
            "shards": shard_count,
            "events_per_sec": avg_eps,
            "wall_time_ms": avg_wall,
            "compute_time_ms": avg_compute,
            "coordination_time_ms": avg_coord,
            "compute_pct": if total > 0 { avg_compute as f64 * 100.0 / total as f64 } else { 0.0 },
            "granularity_ratio": if avg_coord > 0 { (avg_compute as f64 / avg_coord as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "speedup": (speedup * 100.0).round() / 100.0,
            "efficiency_pct": (efficiency * 10.0).round() / 10.0,
            "node_count": node_count,
            "error_count": error_count,
        }));
    }

    json!({
        "status": "ok",
        "event_count": event_count,
        "pipeline_depth": pipeline_depth,
        "results": results,
    })
}

fn handle_weak_scaling(payload: &Value) -> Value {
    let events_per_shard = payload["events_per_shard"].as_u64().unwrap_or(5000);
    let shard_counts: Vec<u64> = payload["shard_counts"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_else(|| vec![2, 4, 8, 16]);
    let batch_size = payload["batch_size"].as_u64().unwrap_or(500);
    let pipeline_depth = payload["pipeline_depth"].as_u64().unwrap_or(5);
    let warmup_rounds = payload["warmup_rounds"].as_u64().unwrap_or(1);
    let benchmark_rounds = payload["benchmark_rounds"].as_u64().unwrap_or(2);

    let mut results = Vec::new();
    let mut baseline_eps: u64 = 0;

    for &shard_count in &shard_counts {
        let total_events = events_per_shard * shard_count;

        for _ in 0..warmup_rounds {
            handle_run(&json!({
                "event_count": std::cmp::min(total_events, 1000),
                "worker_count": shard_count,
                "batch_size": batch_size,
                "pipeline_depth": pipeline_depth,
                "sink_format": "splunk_hec",
            }));
        }

        let mut total_eps: u64 = 0;
        let mut total_wall: u64 = 0;
        let mut total_compute: u64 = 0;
        let mut total_coord: u64 = 0;
        let mut node_count: u64 = 0;
        let mut error_count: u64 = 0;

        for _ in 0..benchmark_rounds {
            let r = handle_run(&json!({
                "event_count": total_events,
                "worker_count": shard_count,
                "batch_size": batch_size,
                "pipeline_depth": pipeline_depth,
                "sink_format": "splunk_hec",
            }));
            total_eps += r["events_per_sec"].as_u64().unwrap_or(0);
            total_wall += r["wall_time_ms"].as_u64().unwrap_or(0);
            total_compute += r["compute_time_ms"].as_u64().unwrap_or(0);
            total_coord += r["coordination_time_ms"].as_u64().unwrap_or(0);
            node_count = r["node_count"].as_u64().unwrap_or(0);
            error_count += r["error_count"].as_u64().unwrap_or(0);
        }

        let avg_eps = total_eps / benchmark_rounds;
        let avg_wall = total_wall / benchmark_rounds;
        let avg_compute = total_compute / benchmark_rounds;
        let avg_coord = total_coord / benchmark_rounds;

        if baseline_eps == 0 { baseline_eps = avg_eps; }
        let efficiency = if baseline_eps > 0 { avg_eps as f64 / baseline_eps as f64 * 100.0 } else { 100.0 };

        results.push(json!({
            "shards": shard_count,
            "total_events": total_events,
            "events_per_sec": avg_eps,
            "wall_time_ms": avg_wall,
            "compute_time_ms": avg_compute,
            "coordination_time_ms": avg_coord,
            "granularity_ratio": if avg_coord > 0 { (avg_compute as f64 / avg_coord as f64 * 10.0).round() / 10.0 } else { 0.0 },
            "efficiency_pct": (efficiency * 10.0).round() / 10.0,
            "node_count": node_count,
            "error_count": error_count,
        }));
    }

    json!({
        "status": "ok",
        "events_per_shard": events_per_shard,
        "pipeline_depth": pipeline_depth,
        "results": results,
    })
}

fn handle_pipeline_depth(payload: &Value) -> Value {
    let event_count = payload["event_count"].as_u64().unwrap_or(10000);
    let worker_count = payload["worker_count"].as_u64().unwrap_or(8);
    let batch_size = payload["batch_size"].as_u64().unwrap_or(500);
    let depths: Vec<u64> = payload["depths"].as_array()
        .map(|a| a.iter().filter_map(|v| v.as_u64()).collect())
        .unwrap_or_else(|| vec![1, 2, 3, 4, 5]);

    let mut results = Vec::new();
    for depth in &depths {
        let r = handle_run(&json!({
            "event_count": event_count,
            "worker_count": worker_count,
            "batch_size": batch_size,
            "pipeline_depth": depth,
            "sink_format": "splunk_hec",
        }));

        let funcs: Vec<&str> = ORDERED_FUNCTIONS.iter().take(*depth as usize).copied().collect();
        results.push(json!({
            "depth": depth,
            "functions": funcs.join(","),
            "events_per_sec": r["events_per_sec"].as_u64().unwrap_or(0),
            "wall_time_ms": r["wall_time_ms"].as_u64().unwrap_or(0),
            "granularity_ratio": r["granularity_ratio"],
        }));
    }

    json!({
        "status": "ok",
        "event_count": event_count,
        "worker_count": worker_count,
        "results": results,
    })
}

// ---------------------------------------------------------------------------
// Worker logic
// ---------------------------------------------------------------------------

fn handle_process_batch(payload: &Value) -> Value {
    let comp_start = now_ms();
    let events_raw: Vec<LogEvent> = payload["events"].as_array()
        .map(|arr| arr.iter().filter_map(|v| serde_json::from_value(v.clone()).ok()).collect())
        .unwrap_or_default();
    let func_names_str = payload["pipeline_functions"].as_str().unwrap_or("json_parse,mask_pii,enrich,rename_fields,drop");
    let sink_format = payload["sink_format"].as_str().unwrap_or("splunk_hec");

    let func_names: Vec<&str> = func_names_str.split(',').filter(|s| !s.is_empty()).collect();
    let events_in = events_raw.len() as u64;
    let processed = apply_pipeline(events_raw, &func_names);

    let mut route_distribution: HashMap<String, u64> = HashMap::new();
    let mut formatted_count: u64 = 0;
    for evt in &processed {
        let sink = route_event(evt);
        *route_distribution.entry(sink.to_string()).or_insert(0) += 1;
        let _ = format_event(evt, sink_format);
        formatted_count += 1;
    }

    let compute_ms = now_ms() - comp_start;

    let st = state().lock().unwrap();
    let app_id = st.application_id.clone();
    drop(st);

    application_metrics_add(&app_id, "worker.compute", compute_ms);

    json!({
        "events_in": events_in,
        "events_out": processed.len(),
        "events_dropped": events_in - processed.len() as u64,
        "compute_ms": compute_ms,
        "route_distribution": route_distribution,
        "formatted_count": formatted_count,
    })
}

// ---------------------------------------------------------------------------
// WIT Guest implementation
// ---------------------------------------------------------------------------

struct Component;

impl Guest for Component {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let config_str = String::from_utf8_lossy(&config);
        let parsed: InitConfig = serde_json::from_str(&config_str).unwrap_or(InitConfig {
            actor_id: None,
            args: HashMap::new(),
        });
        let actor_id = parsed.actor_id.unwrap_or_default();
        let role = parsed.args.get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("worker")
            .to_string();
        let pipeline_functions = parsed.args.get("pipeline_functions")
            .and_then(|v| v.as_str())
            .unwrap_or("json_parse,mask_pii,enrich,rename_fields,drop")
            .to_string();

        let mut st = state().lock().unwrap();
        st.application_id = application_id_from_actor(&actor_id);
        st.actor_id = actor_id;
        st.role = role;
        st.pipeline_functions = pipeline_functions;
        Ok(())
    }

    fn handle(_from: String, _msg_type: String, payload: Vec<u8>) -> Vec<u8> {
        let payload_str = String::from_utf8_lossy(&payload);
        let parsed: Value = serde_json::from_str(&payload_str).unwrap_or(json!({}));
        let op = parsed["op"].as_str().unwrap_or("");

        let role = {
            let st = state().lock().unwrap();
            st.role.clone()
        };

        let result = match role.as_str() {
            "leader" => match op {
                "run" => handle_run(&parsed),
                "run_scaling_benchmark" => handle_scaling_benchmark(&parsed),
                "run_weak_scaling_benchmark" => handle_weak_scaling(&parsed),
                "run_pipeline_depth_benchmark" => handle_pipeline_depth(&parsed),
                _ => json!({"error": format!("unknown leader op: {}", op)}),
            },
            "worker" => match op {
                "process_batch" => handle_process_batch(&parsed),
                _ => json!({"error": format!("unknown worker op: {}", op)}),
            },
            _ => json!({"error": format!("unknown role: {}", role)}),
        };

        serde_json::to_vec(&result).unwrap_or_default()
    }

    fn get_state() -> Vec<u8> {
        let st = state().lock().unwrap();
        serde_json::to_vec(&*st).unwrap_or_default()
    }

    fn set_state(data: Vec<u8>) {
        if let Ok(new_state) = serde_json::from_slice::<AppState>(&data) {
            let mut st = state().lock().unwrap();
            *st = new_state;
        }
    }
}

export!(Component);
