// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Heat Diffusion - Rust WASM app
//
// Leader actor:
// - Creates a shard group of worker actors using the actor-world WIT host.
// - Initializes workers and runs iterative scatter-gather.
// - Returns benchmark metrics with compute vs coordination split.
//
// Worker actor:
// - Maintains one region of the grid.
// - Exchanges ghost boundaries via TupleSpace.
// - Returns per-iteration compute and coordination timing.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use prost::Message;
use plexspaces_proto::actor::v1::{
    CreateShardGroupRequest, CreateShardGroupResponse, DataParallelConfig, NodePlacement,
    NodePlacementStrategy, PartitionStrategy, RebalancePolicy, ScatterGatherRequest,
    ScatterGatherResponse, ShardGroupAggregationStrategy,
};
use plexspaces_proto::application::v1::{
    ApplicationInfo, ApplicationMetrics, ApplicationStatus, GetApplicationStatusResponse,
};
use plexspaces_proto::tuplespace::v1::{
    tuple_field::Value as ProtoTupleValue, ReadRequest, ReadResponse, Tuple,
    TupleField as ProtoTupleField, WriteRequest,
};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use plexspaces_proto::prost_types;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::self_id;
use plexspaces::actor::host_logging::{log, now_ms};
use plexspaces::actor::host_shard::{application_get_metrics, application_get_status, application_metrics_add};
use plexspaces_sdk::simple_actor::ActorWorldHandlers;

mod leader;
mod worker;

const DEFAULT_WIDTH: usize = 16000;
const DEFAULT_REGIONS: usize = 16;
const DEFAULT_ITERATIONS: usize = 40;
const DEFAULT_TOLERANCE: f64 = 0.0005;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct WorkerRegion {
    region_id: usize,
    width: usize,
    num_regions: usize,
    data: Vec<f64>,
    fixed_boundary: Vec<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AppState {
    actor_id: String,
    application_id: String,
    role: String,
    last_result: Option<serde_json::Value>,
    worker: Option<WorkerRegion>,
}

#[derive(Debug, Deserialize)]
struct InitConfig {
    actor_id: Option<String>,
    role: Option<String>,
    args: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    #[serde(default = "default_width")]
    width: usize,
    #[serde(default = "default_regions")]
    num_regions: usize,
    #[serde(default = "default_iterations")]
    max_iterations: usize,
    #[serde(default = "default_tolerance")]
    tolerance: f64,
}

#[derive(Debug, Deserialize)]
struct ComputeRequest {
    iteration: usize,
    tolerance: f64,
}

#[derive(Default)]
struct NodeMetrics {
    actors: u64,
    leader_actors: u64,
    worker_actors: u64,
    messages: u64,
    leader_messages: u64,
    worker_messages: u64,
    tuple_operations: u64,
    compute_time_ms: u64,
    coordination_time_ms: u64,
    total_latency_ms: u64,
    max_latency_ms: u64,
    responses: u64,
    errors: u64,
}

#[derive(Default)]
struct RoleMetrics {
    actors: u64,
    messages: u64,
    tuple_operations: u64,
    compute_time_ms: u64,
    coordination_time_ms: u64,
    total_latency_ms: u64,
    max_latency_ms: u64,
    responses: u64,
    errors: u64,
}

fn default_width() -> usize {
    DEFAULT_WIDTH
}

fn default_regions() -> usize {
    DEFAULT_REGIONS
}

fn default_iterations() -> usize {
    DEFAULT_ITERATIONS
}

fn default_tolerance() -> f64 {
    DEFAULT_TOLERANCE
}

fn state_cell() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let mut guard = state_cell()
        .lock()
        .expect("heat_diffusion state lock poisoned");
    f(&mut guard)
}

fn parse_payload(payload: &[u8]) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice(payload).map_err(|e| format!("invalid payload: {}", e))
}

fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
    let payload = parse_payload(payload)?;
    if let Some(op) = payload
        .get("op")
        .and_then(|value| value.as_str())
        .map(str::to_string)
    {
        Ok(op)
    } else if msg_type == "call" || msg_type == "cast" {
        Err("missing op".to_string())
    } else {
        Ok(msg_type.to_string())
    }
}

fn json_bytes(value: Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn json_error(err: impl Into<String>) -> Vec<u8> {
    json_bytes(serde_json::json!({ "error": err.into() }))
}

fn host_info(message: impl AsRef<str>) {
    log("info", message.as_ref());
}

fn actor_node_id(actor_id: &str) -> String {
    actor_id
        .rsplit_once('@')
        .map(|(_, node_id)| node_id.to_string())
        .unwrap_or_else(|| "local".to_string())
}

fn actor_application_id(actor_id: &str) -> String {
    if let Some(namespace) = actor_id
        .split_once("//")
        .and_then(|(_, suffix)| suffix.split_once('@').map(|(qualified, _)| qualified))
        .and_then(|qualified| qualified.rsplit_once("::").map(|(_, namespace)| namespace))
    {
        return namespace.to_string();
    }

    actor_id
        .split_once(':')
        .and_then(|(_, suffix)| suffix.split_once('@').map(|(namespace, _)| namespace))
        .map(str::to_string)
        .unwrap_or_default()
}

fn actor_name_from_actor_id(actor_id: &str) -> Option<String> {
    actor_id
        .split_once("//")
        .map(|(name, _)| name.to_string())
        .or_else(|| actor_id.split_once(':').map(|(name, _)| name.to_string()))
        .filter(|name| !name.is_empty())
}

fn resolve_role(config: &InitConfig, actor_id: &str) -> Option<String> {
    config
        .args
        .as_ref()
        .and_then(|args| args.get("role"))
        .cloned()
        .or_else(|| config.role.clone())
        .or_else(|| actor_name_from_actor_id(actor_id))
        .filter(|role| role == "leader" || role == "worker")
}

fn current_application_id() -> String {
    with_state(|state| state.application_id.clone())
}

fn json_object_to_metric_map(value: Option<&serde_json::Value>) -> HashMap<String, u64> {
    value.and_then(|value| value.as_object()).map(|entries| {
        entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()
    }).unwrap_or_default()
}

fn application_metrics_from_json(metrics: serde_json::Value) -> ApplicationMetrics {
    ApplicationMetrics {
        actor_counts: json_object_to_metric_map(metrics.get("actor_counts")),
        supervisor_count: metrics.get("supervisor_count").and_then(|value| value.as_u64()).unwrap_or(0) as u32,
        uptime_seconds: metrics.get("uptime_seconds").and_then(|value| value.as_u64()).unwrap_or(0),
        message_count: metrics.get("message_count").and_then(|value| value.as_u64()).unwrap_or(0),
        error_count: metrics.get("error_count").and_then(|value| value.as_u64()).unwrap_or(0),
        counter_metrics: json_object_to_metric_map(metrics.get("counter_metrics")),
        latency_totals_ms: json_object_to_metric_map(metrics.get("latency_totals_ms")),
        latency_max_ms: json_object_to_metric_map(metrics.get("latency_max_ms")),
        latency_samples: json_object_to_metric_map(metrics.get("latency_samples")),
    }
}

fn application_status_name(status: i32) -> String {
    ApplicationStatus::try_from(status).map(|status| status.as_str_name().to_ascii_lowercase()).unwrap_or_else(|_| "application_status_unspecified".to_string())
}

fn application_metrics_to_json(metrics: &ApplicationMetrics) -> serde_json::Value {
    serde_json::json!({
        "actor_counts": metrics.actor_counts,
        "supervisor_count": metrics.supervisor_count,
        "uptime_seconds": metrics.uptime_seconds,
        "message_count": metrics.message_count,
        "error_count": metrics.error_count,
        "counter_metrics": metrics.counter_metrics,
        "latency_totals_ms": metrics.latency_totals_ms,
        "latency_max_ms": metrics.latency_max_ms,
        "latency_samples": metrics.latency_samples,
    })
}

fn application_info_to_json(application: &ApplicationInfo) -> serde_json::Value {
    serde_json::json!({
        "application_id": application.application_id,
        "name": application.name,
        "version": application.version,
        "status": application_status_name(application.status),
        "metrics": application.metrics.as_ref().map(application_metrics_to_json).unwrap_or(serde_json::json!({})),
    })
}

fn application_status_response_to_json(response: &GetApplicationStatusResponse) -> serde_json::Value {
    serde_json::json!({
        "application": response.application.as_ref().map(application_info_to_json).unwrap_or(serde_json::Value::Null),
        "state": serde_json::Value::Null,
        "error": response.error,
        "node_id": response.node_id,
        "node_address": response.node_address,
    })
}

fn merge_application_metrics(metrics: serde_json::Value) -> Result<serde_json::Value, String> {
    let metrics_bytes = application_metrics_from_json(metrics).encode_to_vec();
    let response = application_metrics_add(&current_application_id(), &metrics_bytes)?;
    let response = ApplicationMetrics::decode(response.as_slice())
        .map_err(|err| format!("invalid ApplicationMetrics protobuf: {}", err))?;
    Ok(application_metrics_to_json(&response))
}

fn require_application_metrics_merge(
    metrics: serde_json::Value,
    context: &str,
) -> Result<serde_json::Value, String> {
    merge_application_metrics(metrics).map_err(|err| format!("{}: {}", context, err))
}

fn application_metrics(node_id: &str) -> Result<serde_json::Value, String> {
    let response = application_get_metrics(&current_application_id(), node_id)?;
    let response = ApplicationMetrics::decode(response.as_slice())
        .map_err(|err| format!("invalid ApplicationMetrics protobuf: {}", err))?;
    Ok(application_metrics_to_json(&response))
}

fn application_status(node_id: &str) -> Result<serde_json::Value, String> {
    let response = application_get_status(&current_application_id(), node_id)?;
    let response = GetApplicationStatusResponse::decode(response.as_slice())
        .map_err(|err| format!("invalid GetApplicationStatusResponse protobuf: {}", err))?;
    Ok(application_status_response_to_json(&response))
}

fn shard_group_create_request_bytes(group_id: &str, actor_type: &str, shard_count: usize) -> Vec<u8> {
    CreateShardGroupRequest {
        request_id: String::new(),
        config: Some(DataParallelConfig {
            group_id: group_id.to_string(),
            shard_count: shard_count as u32,
            partition_strategy: PartitionStrategy::PartitionStrategyHash as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement: Some(NodePlacement {
                strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
                ..Default::default()
            }),
        }),
        actor_type: actor_type.to_string(),
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    }
    .encode_to_vec()
}

fn decode_shard_group_create_response(bytes: &[u8]) -> Result<Vec<String>, String> {
    let response = CreateShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid CreateShardGroupResponse protobuf: {}", err))?;
    Ok(response.group.map(|group| group.shard_actor_ids).unwrap_or_default())
}

fn scatter_gather_request_bytes(
    group_id: &str,
    aggregation: ShardGroupAggregationStrategy,
    payload: serde_json::Value,
    min_responses: usize,
    timeout_ms: u64,
) -> Vec<u8> {
    ScatterGatherRequest {
        request_id: String::new(),
        group_id: group_id.to_string(),
        query: Some(ProtoMessage {
            id: format!("req-{}", now_ms()),
            message_type: "call".to_string(),
            payload: json_bytes(payload),
            ..Default::default()
        }),
        timeout: Some(prost_types::Duration {
            seconds: (timeout_ms / 1000) as i64,
            nanos: ((timeout_ms % 1000) * 1_000_000) as i32,
        }),
        aggregation: aggregation as i32,
        min_responses: min_responses as u32,
    }
    .encode_to_vec()
}

fn decode_proto_json_payload(payload: &[u8], context: &str) -> Result<serde_json::Value, String> {
    if payload.is_empty() {
        return Ok(serde_json::Value::Null);
    }
    serde_json::from_slice(payload).map_err(|err| format!("invalid {} payload JSON: {}", context, err))
}

fn decode_scatter_gather_response(bytes: &[u8]) -> Result<Vec<serde_json::Value>, String> {
    let response = ScatterGatherResponse::decode(bytes)
        .map_err(|err| format!("invalid ScatterGatherResponse protobuf: {}", err))?;
    response.shard_responses.into_iter().map(|shard| {
        let payload = shard.response.as_ref().map(|message| decode_proto_json_payload(&message.payload, "scatter_gather")).transpose()?.unwrap_or(serde_json::Value::Null);
        let actor_id = shard.response.as_ref().map(|message| message.sender_id.clone()).unwrap_or_default();
        let latency_ms = shard.latency.map(|latency| {
            let millis_from_seconds = latency.seconds.saturating_mul(1000);
            let millis_from_nanos = i64::from(latency.nanos) / 1_000_000;
            millis_from_seconds.saturating_add(millis_from_nanos).max(0) as u64
        }).unwrap_or(0);
        Ok(serde_json::json!({
            "shard_id": shard.shard_id,
            "shard_actor_id": shard.shard_actor_id,
            "actor_id": actor_id,
            "payload": payload,
            "success": shard.success,
            "error": shard.error,
            "latency_ms": latency_ms,
        }))
    }).collect()
}

fn json_object_to_u64_map(value: Option<&serde_json::Value>) -> HashMap<String, u64> {
    value
        .and_then(|value| value.as_object())
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed)))
                .collect()
        })
        .unwrap_or_default()
}

fn saturating_map_delta(
    end: &HashMap<String, u64>,
    start: &HashMap<String, u64>,
) -> HashMap<String, u64> {
    end.iter()
        .map(|(key, end_value)| {
            let start_value = start.get(key).copied().unwrap_or(0);
            (key.clone(), end_value.saturating_sub(start_value))
        })
        .collect()
}

fn metrics_value<'a>(
    metrics: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    metrics.get(field)
}

fn metrics_map(metrics: &serde_json::Value, field: &str) -> HashMap<String, u64> {
    json_object_to_u64_map(metrics_value(metrics, field))
}

fn metrics_scalar(metrics: &serde_json::Value, field: &str) -> u64 {
    metrics_value(metrics, field)
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

fn accumulate_metrics_delta(
    node_metrics: &mut NodeMetrics,
    role_metrics: &mut HashMap<String, RoleMetrics>,
    start_metrics: &serde_json::Value,
    end_metrics: &serde_json::Value,
) {
    let actor_counts = metrics_map(end_metrics, "actor_counts");
    let counter_delta = saturating_map_delta(
        &metrics_map(end_metrics, "counter_metrics"),
        &metrics_map(start_metrics, "counter_metrics"),
    );
    let latency_total_delta = saturating_map_delta(
        &metrics_map(end_metrics, "latency_totals_ms"),
        &metrics_map(start_metrics, "latency_totals_ms"),
    );
    let latency_max_end = metrics_map(end_metrics, "latency_max_ms");
    let latency_sample_delta = saturating_map_delta(
        &metrics_map(end_metrics, "latency_samples"),
        &metrics_map(start_metrics, "latency_samples"),
    );
    let message_delta = metrics_scalar(end_metrics, "message_count")
        .saturating_sub(metrics_scalar(start_metrics, "message_count"));
    let error_delta = metrics_scalar(end_metrics, "error_count")
        .saturating_sub(metrics_scalar(start_metrics, "error_count"));

    node_metrics.actors = actor_counts
        .get("total")
        .copied()
        .unwrap_or(node_metrics.actors);
    node_metrics.leader_actors = actor_counts
        .get("leader")
        .copied()
        .unwrap_or(node_metrics.leader_actors);
    node_metrics.worker_actors = actor_counts
        .get("worker")
        .copied()
        .unwrap_or(node_metrics.worker_actors);
    let leader_message_delta = counter_delta.get("leader_messages").copied().unwrap_or(0);
    let worker_message_delta = counter_delta.get("worker_messages").copied().unwrap_or(0);
    let leader_compute_delta = latency_total_delta
        .get("leader.compute")
        .copied()
        .unwrap_or(0);
    let worker_compute_delta = latency_total_delta
        .get("worker.compute")
        .copied()
        .unwrap_or_else(|| latency_total_delta.get("compute").copied().unwrap_or(0));
    let leader_coordination_delta = latency_total_delta
        .get("leader.coordination")
        .copied()
        .unwrap_or(0);
    let worker_coordination_delta = latency_total_delta
        .get("worker.coordination")
        .copied()
        .unwrap_or_else(|| {
            latency_total_delta
                .get("coordination")
                .copied()
                .unwrap_or(0)
        });

    node_metrics.leader_messages = leader_message_delta;
    node_metrics.worker_messages = worker_message_delta;
    node_metrics.messages = if message_delta > 0 {
        message_delta
    } else {
        node_metrics.leader_messages + node_metrics.worker_messages
    };
    node_metrics.tuple_operations = counter_delta
        .get("worker_tuple_operations")
        .copied()
        .unwrap_or_else(|| counter_delta.get("tuple_operations").copied().unwrap_or(0));
    node_metrics.compute_time_ms = worker_compute_delta + leader_compute_delta;
    node_metrics.coordination_time_ms = worker_coordination_delta + leader_coordination_delta;
    node_metrics.total_latency_ms = latency_total_delta.get("worker").copied().unwrap_or(0)
        + latency_total_delta.get("leader").copied().unwrap_or(0);
    node_metrics.max_latency_ms = latency_max_end
        .get("worker")
        .copied()
        .unwrap_or(0)
        .max(latency_max_end.get("leader").copied().unwrap_or(0));
    node_metrics.responses = latency_sample_delta.get("worker").copied().unwrap_or(0)
        + latency_sample_delta.get("leader").copied().unwrap_or(0);
    node_metrics.errors = error_delta;

    let leader_metrics = role_metrics.entry("leader".to_string()).or_default();
    leader_metrics.actors += actor_counts.get("leader").copied().unwrap_or(0);
    leader_metrics.messages += leader_message_delta;
    leader_metrics.compute_time_ms += leader_compute_delta;
    leader_metrics.coordination_time_ms += leader_coordination_delta;
    leader_metrics.total_latency_ms += latency_total_delta.get("leader").copied().unwrap_or(0);
    leader_metrics.max_latency_ms = leader_metrics
        .max_latency_ms
        .max(latency_max_end.get("leader").copied().unwrap_or(0));
    leader_metrics.responses += latency_sample_delta.get("leader").copied().unwrap_or(0);
    leader_metrics.errors += error_delta;

    let worker_metrics = role_metrics.entry("worker".to_string()).or_default();
    worker_metrics.actors += actor_counts.get("worker").copied().unwrap_or(0);
    worker_metrics.messages += worker_message_delta;
    worker_metrics.tuple_operations += counter_delta
        .get("worker_tuple_operations")
        .copied()
        .unwrap_or_else(|| counter_delta.get("tuple_operations").copied().unwrap_or(0));
    worker_metrics.compute_time_ms += worker_compute_delta;
    worker_metrics.coordination_time_ms += worker_coordination_delta;
    worker_metrics.total_latency_ms += latency_total_delta.get("worker").copied().unwrap_or(0);
    worker_metrics.max_latency_ms = worker_metrics
        .max_latency_ms
        .max(latency_max_end.get("worker").copied().unwrap_or(0));
    worker_metrics.responses += latency_sample_delta.get("worker").copied().unwrap_or(0);
    worker_metrics.errors += error_delta;
}

fn apply_topology_actor_counts(
    per_node_metrics: &mut HashMap<String, NodeMetrics>,
    per_role_metrics: &mut HashMap<String, RoleMetrics>,
    leader_node_id: &str,
    shard_actor_ids: &[String],
) {
    for metrics in per_node_metrics.values_mut() {
        metrics.leader_actors = 0;
        metrics.worker_actors = 0;
    }

    let leader_metrics = per_node_metrics
        .entry(leader_node_id.to_string())
        .or_default();
    leader_metrics.leader_actors = 1;
    if leader_metrics.actors == 0 {
        leader_metrics.actors = 1;
    }

    for shard_actor_id in shard_actor_ids {
        let node_id = actor_node_id(shard_actor_id);
        let metrics = per_node_metrics.entry(node_id).or_default();
        metrics.worker_actors += 1;
        metrics.actors = metrics.leader_actors + metrics.worker_actors;
    }

    let leader_role = per_role_metrics.entry("leader".to_string()).or_default();
    leader_role.actors = 1;
    let worker_role = per_role_metrics.entry("worker".to_string()).or_default();
    worker_role.actors = shard_actor_ids.len() as u64;
}

fn proto_string_field(value: impl Into<String>) -> ProtoTupleField {
    ProtoTupleField {
        value: Some(ProtoTupleValue::String(value.into())),
    }
}

fn proto_integer_field(value: i64) -> ProtoTupleField {
    ProtoTupleField {
        value: Some(ProtoTupleValue::Integer(value)),
    }
}

fn proto_wildcard_field() -> ProtoTupleField {
    ProtoTupleField {
        value: Some(ProtoTupleValue::Wildcard(true)),
    }
}

fn build_tuple(fields: Vec<ProtoTupleField>) -> Tuple {
    Tuple {
        id: String::new(),
        fields,
        timestamp: None,
        lease: None,
        metadata: Default::default(),
        location: None,
    }
}

fn parse_tuple_line(bytes: &[u8]) -> Option<Vec<f64>> {
    let response = ReadResponse::decode(bytes).ok()?;
    let tuple = response.tuples.first()?;
    let encoded = match tuple.fields.get(4)?.value.as_ref()? {
        ProtoTupleValue::String(value) => value,
        _ => return None,
    };
    serde_json::from_str(encoded).ok()
}

fn build_boundary_tuple(label: &str, iteration: usize, region_id: usize, data: &[f64]) -> Vec<u8> {
    WriteRequest {
        request_id: String::new(),
        tuples: vec![build_tuple(vec![
            proto_string_field("boundary"),
            proto_integer_field(iteration as i64),
            proto_integer_field(region_id as i64),
            proto_string_field(label),
            proto_string_field(serde_json::to_string(data).unwrap_or_else(|_| "[]".to_string())),
        ])],
        transaction_id: String::new(),
    }
    .encode_to_vec()
}

fn boundary_pattern(label: &str, iteration: usize, region_id: usize) -> Vec<u8> {
    ReadRequest {
        request_id: String::new(),
        template: Some(build_tuple(vec![
            proto_string_field("boundary"),
            proto_integer_field(iteration as i64),
            proto_integer_field(region_id as i64),
            proto_string_field(label),
            proto_wildcard_field(),
        ])),
        timeout: None,
        blocking: false,
        take: false,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    }
    .encode_to_vec()
}

fn initialize_worker(region_id: usize, width: usize, num_regions: usize) -> WorkerRegion {
    let initial_temp = if num_regions <= 1 {
        50.0
    } else {
        region_id as f64 * (100.0 / (num_regions - 1) as f64)
    };
    let hotspot_center = width / 2;
    let hotspot_radius = (width / 10).max(8);
    let mut data = Vec::with_capacity(width);
    for idx in 0..width {
        let horizontal_ratio = if width <= 1 {
            0.0
        } else {
            idx as f64 / (width - 1) as f64
        };
        let vertical_ratio = if num_regions <= 1 {
            0.0
        } else {
            region_id as f64 / (num_regions - 1) as f64
        };
        let base_gradient = 20.0 + (horizontal_ratio * 50.0) + (vertical_ratio * 30.0);
        let wave = (((idx + 1) * (region_id + 3)) as f64 / 37.0).sin() * 4.0;
        let hotspot = if idx.abs_diff(hotspot_center) <= hotspot_radius {
            18.0 - ((idx.abs_diff(hotspot_center) as f64 / hotspot_radius as f64) * 18.0)
        } else {
            0.0
        };
        data.push((base_gradient + wave + hotspot + initial_temp * 0.1).clamp(0.0, 100.0));
    }
    let fixed_boundary = if region_id == 0 {
        vec![0.0; width]
    } else if region_id + 1 == num_regions {
        vec![100.0; width]
    } else {
        vec![initial_temp; width]
    };
    if width > 0 {
        data[0] = fixed_boundary[0];
        data[width - 1] = fixed_boundary[width - 1];
    }
    WorkerRegion {
        region_id,
        width,
        num_regions,
        data,
        fixed_boundary,
    }
}

fn compute_region(region: &WorkerRegion, north: &[f64], south: &[f64]) -> (Vec<f64>, f64) {
    let mut next = region.data.clone();
    let mut max_diff = 0.0_f64;
    if region.width < 3 {
        return (next, max_diff);
    }
    for idx in 1..(region.width - 1) {
        let new_value =
            (region.data[idx - 1] + region.data[idx + 1] + north[idx] + south[idx]) / 4.0;
        let diff = (new_value - region.data[idx]).abs();
        if diff > max_diff {
            max_diff = diff;
        }
        next[idx] = new_value;
    }
    (next, max_diff)
}

fn node_metrics_json(metrics: &NodeMetrics) -> serde_json::Value {
    let avg_latency_ms = if metrics.responses > 0 {
        metrics.total_latency_ms as f64 / metrics.responses as f64
    } else {
        0.0
    };

    serde_json::json!({
        "actors": metrics.actors,
        "leader_actors": metrics.leader_actors,
        "worker_actors": metrics.worker_actors,
        "messages": metrics.messages,
        "leader_messages": metrics.leader_messages,
        "worker_messages": metrics.worker_messages,
        "tuple_operations": metrics.tuple_operations,
        "compute_time_ms": metrics.compute_time_ms,
        "coordination_time_ms": metrics.coordination_time_ms,
        "avg_latency_ms": avg_latency_ms,
        "max_latency_ms": metrics.max_latency_ms,
        "responses": metrics.responses,
        "errors": metrics.errors,
    })
}

fn role_metrics_json(metrics: &RoleMetrics) -> serde_json::Value {
    let avg_latency_ms = if metrics.responses > 0 {
        metrics.total_latency_ms as f64 / metrics.responses as f64
    } else {
        0.0
    };

    serde_json::json!({
        "actors": metrics.actors,
        "messages": metrics.messages,
        "tuple_operations": metrics.tuple_operations,
        "compute_time_ms": metrics.compute_time_ms,
        "coordination_time_ms": metrics.coordination_time_ms,
        "avg_latency_ms": avg_latency_ms,
        "max_latency_ms": metrics.max_latency_ms,
        "responses": metrics.responses,
        "errors": metrics.errors,
    })
}

fn decode_nested_json_value(mut value: serde_json::Value) -> serde_json::Value {
    for _ in 0..3 {
        let serde_json::Value::String(encoded) = value else {
            break;
        };
        match serde_json::from_str::<serde_json::Value>(&encoded) {
            Ok(decoded) => value = decoded,
            Err(_) => return serde_json::Value::String(encoded),
        }
    }
    value
}

fn normalize_worker_payload(value: serde_json::Value) -> serde_json::Value {
    let mut current = decode_nested_json_value(value);

    for _ in 0..6 {
        let Some(object) = current.as_object() else {
            return current;
        };

        if object.contains_key("compute_time_ms")
            || object.contains_key("coordination_time_ms")
            || object.contains_key("tuple_operations")
            || object.contains_key("messages_processed")
        {
            return current;
        }

        let nested = object
            .get("payload")
            .or_else(|| object.get("result"))
            .or_else(|| object.get("response"))
            .or_else(|| object.get("data"))
            .cloned();

        match nested {
            Some(next) => current = decode_nested_json_value(next),
            None => return current,
        }
    }

    current
}

fn json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value
        .get(key)
        .and_then(|field| match field {
            serde_json::Value::Number(number) => number.as_u64(),
            serde_json::Value::String(text) => text.parse::<u64>().ok(),
            _ => None,
        })
        .unwrap_or(0)
}

fn json_f64(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|field| match field {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    })
}

struct HeatDiffusionActor;

impl Guest for HeatDiffusionActor {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let config: InitConfig = match serde_json::from_slice(&config) {
            Ok(config) => config,
            Err(err) => return Err(format!("invalid init config: {}", err)),
        };
        let actor_id = config.actor_id.clone().unwrap_or_else(|| self_id());
        let role = resolve_role(&config, &actor_id).ok_or_else(|| {
            format!(
                "invalid init config: missing required role for actor {}",
                actor_id
            )
        })?;
        with_state(|state| {
            state.actor_id = actor_id.clone();
            state.application_id = actor_application_id(&actor_id);
            state.role = role.clone();
            state.last_result = None;
            state.worker = None;
        });
        host_info(format!(
            "heat_diffusion initialized role={} actor_id={}",
            role, actor_id
        ));
        Ok(())
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let op = match parse_op(&msg_type, &payload) {
            Ok(op) => op,
            Err(err) => return Ok(json_error(err)),
        };
        let role = with_state(|state| state.role.clone());
        match (role.as_str(), op.as_str()) {
            ("leader", "run") => leader::LeaderActor::default()
                .handle_operation(&from_actor, "run", &payload),
            ("leader", "metrics") => Ok(with_state(|state| {
                json_bytes(
                    state
                        .last_result
                        .clone()
                        .unwrap_or_else(|| serde_json::json!({ "error": "no metrics yet" })),
                )
            })),
            ("worker", "init") => worker::WorkerActor::default()
                .handle_operation(&from_actor, "init", &payload),
            ("worker", "compute") => worker::WorkerActor::default()
                .handle_operation(&from_actor, "compute", &payload),
            _ => Ok(json_error(format!(
                "unsupported op '{}' for role '{}'",
                op, role
            ))),
        }
    }

    fn get_state() -> Result<Vec<u8>, String> {
        Ok(with_state(|state| {
            serde_json::to_vec(state).unwrap_or_else(|_| b"{}".to_vec())
        }))
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        match serde_json::from_slice::<AppState>(&state) {
            Ok(new_state) => {
                with_state(|state| {
                    *state = new_state;
                });
                Ok(())
            }
            Err(err) => Err(format!("invalid state: {}", err)),
        }
    }
}

export!(HeatDiffusionActor);

#[cfg(test)]
mod tests {
    use super::{
        accumulate_metrics_delta, actor_application_id, actor_name_from_actor_id,
        normalize_worker_payload, resolve_role, InitConfig, NodeMetrics, RoleMetrics,
    };
    use std::collections::HashMap;

    #[test]
    fn normalize_worker_payload_unwraps_nested_remote_envelope() {
        let payload = serde_json::json!({
            "payload": {
                "result": serde_json::to_string(&serde_json::json!({
                    "role": "worker",
                    "node_id": "node-b",
                    "compute_time_ms": 11,
                    "coordination_time_ms": 7,
                    "tuple_operations": 4,
                    "messages_processed": 1
                }))
                .expect("serialize nested payload")
            }
        });

        let normalized = normalize_worker_payload(payload);
        assert_eq!(
            normalized.get("role").and_then(|value| value.as_str()),
            Some("worker")
        );
        assert_eq!(
            normalized.get("node_id").and_then(|value| value.as_str()),
            Some("node-b")
        );
        assert_eq!(
            normalized
                .get("compute_time_ms")
                .and_then(|value| value.as_u64()),
            Some(11)
        );
        assert_eq!(
            normalized
                .get("coordination_time_ms")
                .and_then(|value| value.as_u64()),
            Some(7)
        );
        assert_eq!(
            normalized
                .get("tuple_operations")
                .and_then(|value| value.as_u64()),
            Some(4)
        );
    }

    #[test]
    fn actor_application_id_extracts_namespace() {
        let actor_id = "01ABC//worker::heat-diffusion-rust@test-node-8093";
        assert_eq!(actor_application_id(actor_id), "heat-diffusion-rust");
    }

    #[test]
    fn actor_application_id_extracts_namespace_from_legacy_runtime_id() {
        let actor_id = "worker:heat-diffusion-rust@test-node-8093";
        assert_eq!(actor_application_id(actor_id), "heat-diffusion-rust");
    }

    #[test]
    fn accumulate_metrics_delta_uses_application_metrics_as_source_of_truth() {
        let start = serde_json::json!({
            "message_count": 10,
            "error_count": 1,
            "actor_counts": { "total": 9, "leader": 1, "worker": 8 },
            "counter_metrics": { "leader_messages": 1, "worker_messages": 10, "worker_tuple_operations": 20 },
            "latency_totals_ms": {
                "leader": 12,
                "leader.compute": 2,
                "leader.coordination": 10,
                "worker": 100,
                "worker.compute": 40,
                "worker.coordination": 60
            },
            "latency_max_ms": { "leader": 12, "worker": 14 },
            "latency_samples": { "leader": 1, "worker": 10 }
        });
        let end = serde_json::json!({
            "message_count": 18,
            "error_count": 2,
            "actor_counts": { "total": 9, "leader": 1, "worker": 8 },
            "counter_metrics": { "leader_messages": 4, "worker_messages": 18, "worker_tuple_operations": 44 },
            "latency_totals_ms": {
                "leader": 33,
                "leader.compute": 8,
                "leader.coordination": 25,
                "worker": 188,
                "worker.compute": 77,
                "worker.coordination": 111
            },
            "latency_max_ms": { "leader": 33, "worker": 21 },
            "latency_samples": { "leader": 1, "worker": 18 }
        });

        let mut node_metrics = NodeMetrics::default();
        let mut role_metrics = HashMap::<String, RoleMetrics>::new();
        accumulate_metrics_delta(&mut node_metrics, &mut role_metrics, &start, &end);

        assert_eq!(node_metrics.actors, 9);
        assert_eq!(node_metrics.leader_actors, 1);
        assert_eq!(node_metrics.worker_actors, 8);
        assert_eq!(node_metrics.messages, 8);
        assert_eq!(node_metrics.leader_messages, 3);
        assert_eq!(node_metrics.worker_messages, 8);
        assert_eq!(node_metrics.tuple_operations, 24);
        assert_eq!(node_metrics.compute_time_ms, 43);
        assert_eq!(node_metrics.coordination_time_ms, 66);
        assert_eq!(node_metrics.responses, 8);
        assert_eq!(node_metrics.max_latency_ms, 33);
        assert_eq!(node_metrics.errors, 1);

        let leader = role_metrics.get("leader").expect("leader metrics");
        assert_eq!(leader.actors, 1);
        assert_eq!(leader.messages, 3);
        assert_eq!(leader.compute_time_ms, 6);
        assert_eq!(leader.coordination_time_ms, 15);
        assert_eq!(leader.responses, 0);

        let worker = role_metrics.get("worker").expect("worker metrics");
        assert_eq!(worker.actors, 8);
        assert_eq!(worker.messages, 8);
        assert_eq!(worker.tuple_operations, 24);
        assert_eq!(worker.compute_time_ms, 37);
        assert_eq!(worker.coordination_time_ms, 51);
        assert_eq!(worker.responses, 8);
    }

    #[test]
    fn actor_name_from_actor_id_handles_canonical_ids() {
        assert_eq!(
            actor_name_from_actor_id(
                "worker//heat_diffusion_wasm::heat-diffusion-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }

    #[test]
    fn resolve_role_prefers_declared_fields() {
        let config = InitConfig {
            actor_id: Some(
                "heat-diffusion-123//heat_diffusion_wasm::heat-diffusion-rust@test-node-8093"
                    .to_string(),
            ),
            role: Some("worker".to_string()),
            args: None,
        };
        assert_eq!(
            resolve_role(
                &config,
                "heat-diffusion-123//heat_diffusion_wasm::heat-diffusion-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }
}
