// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Parameter Server - Rust WASM app
//
// Leader actor:
// - Creates a shard group of worker actors using the actor-world WIT host.
// - Fans out current weights, gathers gradients, and updates centralized weights.
// - Returns benchmark metrics with compute vs coordination split.
//
// Worker actor:
// - Owns one synthetic training shard.
// - Computes gradients for its shard and reports per-iteration metrics.

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
use plexspaces_proto::common::v1::Message as ProtoMessage;
use plexspaces_proto::prost_types;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;
use plexspaces_sdk::simple_actor::ActorWorldHandlers;

mod leader;
mod worker;

const DEFAULT_PARAM_COUNT: usize = 8_192;
const DEFAULT_WORKER_COUNT: usize = 16;
const DEFAULT_ITERATIONS: usize = 20;
const DEFAULT_BATCH_SIZE: usize = 512;

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct WorkerShard {
    shard_id: usize,
    worker_count: usize,
    param_count: usize,
    iterations: usize,
    batch_size: usize,
    samples_for_shard: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct AppState {
    actor_id: String,
    application_id: String,
    role: String,
    last_result: Option<serde_json::Value>,
    worker: Option<WorkerShard>,
}

#[derive(Debug, Deserialize)]
struct InitConfig {
    actor_id: Option<String>,
    role: Option<String>,
    declaration_name: Option<String>,
    args: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    #[serde(default = "default_param_count")]
    param_count: usize,
    #[serde(default = "default_worker_count")]
    worker_count: usize,
    #[serde(default = "default_iterations")]
    iterations: usize,
    #[serde(default = "default_batch_size")]
    batch_size: usize,
}

#[derive(Debug, Deserialize)]
struct GradientRequest {
    iteration: usize,
    weight_checksum: u64,
}

#[derive(Default)]
struct NodeMetrics {
    actors: u64,
    leader_actors: u64,
    worker_actors: u64,
    messages: u64,
    leader_messages: u64,
    worker_messages: u64,
    gradient_operations: u64,
    samples_processed: u64,
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
    gradient_operations: u64,
    samples_processed: u64,
    compute_time_ms: u64,
    coordination_time_ms: u64,
    total_latency_ms: u64,
    max_latency_ms: u64,
    responses: u64,
    errors: u64,
}

fn default_param_count() -> usize {
    DEFAULT_PARAM_COUNT
}

fn default_worker_count() -> usize {
    DEFAULT_WORKER_COUNT
}

fn default_iterations() -> usize {
    DEFAULT_ITERATIONS
}

fn default_batch_size() -> usize {
    DEFAULT_BATCH_SIZE
}

fn state_cell() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let mut guard = state_cell()
        .lock()
        .expect("parameter_server state lock poisoned");
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
        .or_else(|| config.declaration_name.clone())
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
    let response = host::application_metrics_add(&current_application_id(), &metrics_bytes)?;
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
    let response = host::application_get_metrics(&current_application_id(), node_id)?;
    let response = ApplicationMetrics::decode(response.as_slice())
        .map_err(|err| format!("invalid ApplicationMetrics protobuf: {}", err))?;
    Ok(application_metrics_to_json(&response))
}

fn application_status(node_id: &str) -> Result<serde_json::Value, String> {
    let response = host::application_get_status(&current_application_id(), node_id)?;
    let response = GetApplicationStatusResponse::decode(response.as_slice())
        .map_err(|err| format!("invalid GetApplicationStatusResponse protobuf: {}", err))?;
    Ok(application_status_response_to_json(&response))
}

fn shard_group_create_request_bytes(group_id: &str, actor_type: &str, shard_count: usize) -> Vec<u8> {
    CreateShardGroupRequest {
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
        group_id: group_id.to_string(),
        query: Some(ProtoMessage {
            id: format!("req-{}", host::now_ms()),
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
    node_metrics.gradient_operations = counter_delta
        .get("gradient_operations")
        .copied()
        .unwrap_or(0);
    node_metrics.samples_processed = counter_delta.get("samples_processed").copied().unwrap_or(0);
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
    worker_metrics.gradient_operations += counter_delta
        .get("gradient_operations")
        .copied()
        .unwrap_or(0);
    worker_metrics.samples_processed +=
        counter_delta.get("samples_processed").copied().unwrap_or(0);
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
        "gradient_operations": metrics.gradient_operations,
        "samples_processed": metrics.samples_processed,
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
        "gradient_operations": metrics.gradient_operations,
        "samples_processed": metrics.samples_processed,
        "compute_time_ms": metrics.compute_time_ms,
        "coordination_time_ms": metrics.coordination_time_ms,
        "avg_latency_ms": avg_latency_ms,
        "max_latency_ms": metrics.max_latency_ms,
        "responses": metrics.responses,
        "errors": metrics.errors,
    })
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
            || object.contains_key("gradient_operations")
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

fn distribute_samples(total_samples: usize, worker_count: usize, shard_id: usize) -> usize {
    let base = total_samples / worker_count;
    let remainder = total_samples % worker_count;
    base + usize::from(shard_id < remainder)
}

struct ParameterServerActor;

impl ParameterServerActor {
    fn initialize(config_json: String) -> Result<String, String> {
        let config: InitConfig = serde_json::from_str(&config_json)
            .map_err(|err| format!("invalid init config: {}", err))?;
        let actor_id = config
            .actor_id
            .clone()
            .unwrap_or_else(|| "parameter-server".to_string());
        let role = resolve_role(&config, &actor_id)
            .ok_or_else(|| format!("missing required role for actor {}", actor_id))?;
        let application_id = actor_application_id(&actor_id);
        with_state(|state| {
            state.actor_id = actor_id;
            state.application_id = application_id;
            state.role = role;
            state.last_result = None;
            state.worker = None;
        });
        Ok(String::new())
    }
}

impl Guest for ParameterServerActor {
    fn init(config: Vec<u8>) -> Result<(), String> {
        Self::initialize(
            String::from_utf8(config).map_err(|err| format!("invalid init config bytes: {}", err))?,
        )
        .map(|_| ())
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
            ("worker", "compute_gradient") => worker::WorkerActor::default()
                .handle_operation(&from_actor, "compute_gradient", &payload),
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

export!(ParameterServerActor);

#[cfg(test)]
mod tests {
    use super::{
        accumulate_metrics_delta, actor_application_id, actor_name_from_actor_id,
        distribute_samples, normalize_worker_payload, resolve_role, InitConfig, NodeMetrics,
        RoleMetrics,
    };
    use std::collections::HashMap;

    #[test]
    fn actor_application_id_extracts_namespace() {
        assert_eq!(
            actor_application_id("01ABC//leader::parameter-server-rust@test-node"),
            "parameter-server-rust"
        );
    }

    #[test]
    fn actor_application_id_extracts_namespace_from_legacy_runtime_id() {
        assert_eq!(
            actor_application_id("leader:parameter-server-rust@test-node"),
            "parameter-server-rust"
        );
    }

    #[test]
    fn distribute_samples_balances_remainder() {
        assert_eq!(distribute_samples(10, 3, 0), 4);
        assert_eq!(distribute_samples(10, 3, 1), 3);
        assert_eq!(distribute_samples(10, 3, 2), 3);
    }

    #[test]
    fn normalize_worker_payload_unwraps_nested_remote_envelope() {
        let payload = serde_json::json!({
            "payload": {
                "result": serde_json::to_string(&serde_json::json!({
                    "role": "worker",
                    "node_id": "node-b",
                    "compute_time_ms": 13,
                    "coordination_time_ms": 5,
                    "gradient_operations": 64,
                    "samples_processed": 8,
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
                .get("gradient_operations")
                .and_then(|value| value.as_u64()),
            Some(64)
        );
    }

    #[test]
    fn accumulate_metrics_delta_collects_worker_and_leader_metrics() {
        let start_status = serde_json::json!({
            "actor_counts": {
                "total": 2,
                "leader": 1,
                "worker": 1
            },
            "counter_metrics": {
                "leader_messages": 0,
                "worker_messages": 0,
                "gradient_operations": 0,
                "samples_processed": 0
            },
            "latency_totals_ms": {
                "leader": 0,
                "leader.compute": 0,
                "leader.coordination": 0,
                "worker": 0,
                "worker.compute": 0,
                "worker.coordination": 0
            },
            "latency_max_ms": {
                "leader": 0,
                "worker": 0
            },
            "latency_samples": {
                "leader": 0,
                "worker": 0
            },
            "message_count": 0,
            "error_count": 0
        });
        let end_status = serde_json::json!({
            "actor_counts": {
                "total": 9,
                "leader": 1,
                "worker": 8
            },
            "counter_metrics": {
                "leader_messages": 3,
                "worker_messages": 8,
                "gradient_operations": 1024,
                "samples_processed": 2048
            },
            "latency_totals_ms": {
                "leader": 120,
                "leader.compute": 40,
                "leader.coordination": 80,
                "worker": 240,
                "worker.compute": 180,
                "worker.coordination": 60
            },
            "latency_max_ms": {
                "leader": 120,
                "worker": 75
            },
            "latency_samples": {
                "leader": 1,
                "worker": 8
            },
            "message_count": 11,
            "error_count": 0
        });

        let mut node_metrics = NodeMetrics::default();
        let mut role_metrics = HashMap::<String, RoleMetrics>::new();
        accumulate_metrics_delta(
            &mut node_metrics,
            &mut role_metrics,
            &start_status,
            &end_status,
        );

        assert_eq!(node_metrics.actors, 9);
        assert_eq!(node_metrics.messages, 11);
        assert_eq!(node_metrics.gradient_operations, 1024);
        assert_eq!(node_metrics.samples_processed, 2048);
        assert_eq!(node_metrics.compute_time_ms, 220);
        assert_eq!(node_metrics.coordination_time_ms, 140);

        let worker_metrics = role_metrics
            .get("worker")
            .expect("worker role metrics should exist");
        assert_eq!(worker_metrics.actors, 8);
        assert_eq!(worker_metrics.messages, 8);
        assert_eq!(worker_metrics.gradient_operations, 1024);
        assert_eq!(worker_metrics.samples_processed, 2048);

        let leader_metrics = role_metrics
            .get("leader")
            .expect("leader role metrics should exist");
        assert_eq!(leader_metrics.actors, 1);
        assert_eq!(leader_metrics.messages, 3);
        assert_eq!(leader_metrics.compute_time_ms, 40);
        assert_eq!(leader_metrics.coordination_time_ms, 80);
    }

    #[test]
    fn actor_name_from_actor_id_handles_canonical_ids() {
        assert_eq!(
            actor_name_from_actor_id(
                "worker//parameter_server_wasm::parameter-server-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }

    #[test]
    fn resolve_role_prefers_declared_fields() {
        let config = InitConfig {
            actor_id: Some(
                "parameter-server-123//parameter_server_wasm::parameter-server-rust@test-node-8093"
                    .to_string(),
            ),
            role: Some("worker".to_string()),
            declaration_name: Some("worker".to_string()),
            args: None,
        };
        assert_eq!(
            resolve_role(
                &config,
                "parameter-server-123//parameter_server_wasm::parameter-server-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }
}
