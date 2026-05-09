// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Genomics Pipeline - Rust WASM app
//
// Leader actor:
// - Creates a shard group of worker actors using the actor-world WIT host.
// - Orchestrates QC, alignment, variant calling, and annotation with scatter/gather.
// - Returns benchmark metrics with compute versus coordination split and per-node totals.
//
// Worker actor:
// - Owns a shard index inside the worker pool.
// - Processes its portion of a genomics stage.
// - Reports stage-level compute and coordination timing.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
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

const DEFAULT_TOTAL_READS: u64 = 12_000_000;
const DEFAULT_WORKERS: usize = 16;
const DEFAULT_MIN_QUALITY_SCORE: u8 = 20;
const DEFAULT_ANNOTATION_BATCHES: u64 = 32;
const DEFAULT_CHROMOSOMES: [&str; 24] = [
    "chr1", "chr2", "chr3", "chr4", "chr5", "chr6", "chr7", "chr8", "chr9", "chr10", "chr11",
    "chr12", "chr13", "chr14", "chr15", "chr16", "chr17", "chr18", "chr19", "chr20", "chr21",
    "chr22", "chrX", "chrY",
];

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
struct WorkerShard {
    shard_index: usize,
    shard_count: usize,
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
    args: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct RunRequest {
    #[serde(default = "default_sample_id")]
    sample_id: String,
    #[serde(default = "default_total_reads")]
    total_reads: u64,
    #[serde(default = "default_worker_count")]
    worker_count: usize,
    #[serde(default = "default_min_quality_score")]
    min_quality_score: u8,
    #[serde(default = "default_annotation_batches")]
    annotation_batches: u64,
    #[serde(default = "default_reference_genome")]
    reference_genome: String,
    #[serde(default = "default_chromosomes")]
    chromosomes: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct WorkerRequest {
    stage: String,
    sample_id: String,
    total_reads: u64,
    min_quality_score: u8,
    annotation_batches: u64,
    reference_genome: String,
    chromosomes: Vec<String>,
}

#[derive(Default)]
struct NodeMetrics {
    actors: u64,
    leader_actors: u64,
    worker_actors: u64,
    messages: u64,
    leader_messages: u64,
    worker_messages: u64,
    stage_operations: u64,
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
    stage_operations: u64,
    compute_time_ms: u64,
    coordination_time_ms: u64,
    total_latency_ms: u64,
    max_latency_ms: u64,
    responses: u64,
    errors: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct StageRecord {
    name: String,
    responses: u64,
    reads_processed: u64,
    stage_operations: u64,
    avg_latency_ms: f64,
    max_latency_ms: u64,
    errors: u64,
}

#[derive(Default)]
struct StageAccumulator {
    reads_processed: u64,
    passed_reads: u64,
    failed_reads: u64,
    aligned_reads: u64,
    unaligned_reads: u64,
    variant_snps: u64,
    variant_indels: u64,
    annotated_variants: u64,
    pathogenic_variants: u64,
    benign_variants: u64,
    vus_variants: u64,
    stage_operations: u64,
    total_latency_ms: u64,
    max_latency_ms: u64,
    responses: u64,
    errors: u64,
}

fn default_sample_id() -> String {
    "SAMPLE-GENOMICS-001".to_string()
}

fn default_total_reads() -> u64 {
    DEFAULT_TOTAL_READS
}

fn default_worker_count() -> usize {
    DEFAULT_WORKERS
}

fn default_min_quality_score() -> u8 {
    DEFAULT_MIN_QUALITY_SCORE
}

fn default_annotation_batches() -> u64 {
    DEFAULT_ANNOTATION_BATCHES
}

fn default_reference_genome() -> String {
    "hg38".to_string()
}

fn default_chromosomes() -> Vec<String> {
    DEFAULT_CHROMOSOMES
        .iter()
        .map(|entry| (*entry).to_string())
        .collect()
}

fn state_cell() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let mut guard = state_cell()
        .lock()
        .expect("genomics_pipeline state lock poisoned");
    f(&mut guard)
}

fn host_info(message: impl AsRef<str>) {
    host::log("info", message.as_ref());
}

fn parse_payload(payload: &[u8]) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice(payload).map_err(|err| format!("invalid payload: {}", err))
}

fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
    let payload = parse_payload(payload)?;
    Ok(payload
        .get("op")
        .and_then(|value| value.as_str())
        .unwrap_or(msg_type)
        .to_string())
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

fn application_metrics(node_id_or_address: &str) -> Result<serde_json::Value, String> {
    let response = host::application_get_metrics(&current_application_id(), node_id_or_address)?;
    let response = ApplicationMetrics::decode(response.as_slice())
        .map_err(|err| format!("invalid ApplicationMetrics protobuf: {}", err))?;
    Ok(application_metrics_to_json(&response))
}

fn application_status(node_id_or_address: &str) -> Result<serde_json::Value, String> {
    let response = host::application_get_status(&current_application_id(), node_id_or_address)?;
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
    }.encode_to_vec()
}

fn decode_shard_group_create_response(bytes: &[u8]) -> Result<Vec<String>, String> {
    let response = CreateShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid CreateShardGroupResponse protobuf: {}", err))?;
    Ok(response.group.map(|group| group.shard_actor_ids).unwrap_or_default())
}

fn scatter_gather_request_bytes(group_id: &str, aggregation: ShardGroupAggregationStrategy, payload: serde_json::Value, min_responses: usize, timeout_ms: u64) -> Vec<u8> {
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
    }.encode_to_vec()
}

fn decode_proto_json_payload(payload: &[u8], context: &str) -> Result<serde_json::Value, String> {
    if payload.is_empty() { return Ok(serde_json::Value::Null); }
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

fn ensure_worker_initialization(shard_actor_ids: &[String]) -> Result<(), String> {
    let shard_count = shard_actor_ids.len();
    for (shard_index, shard_actor_id) in shard_actor_ids.iter().enumerate() {
        let request_bytes = serde_json::json!({
            "op": "init",
            "shard_index": shard_index,
            "shard_count": shard_count,
        })
        .to_string()
        .into_bytes();
        let response = host::ask(shard_actor_id, "init", &request_bytes, 10_000).map_err(|err| {
            format!(
                "worker init failed for shard {} ({}): {}",
                shard_index, shard_actor_id, err
            )
        })?;

        let payload: serde_json::Value = serde_json::from_slice(&response).map_err(|err| {
            format!(
                "invalid worker init response for shard {} ({}): {}",
                shard_index, shard_actor_id, err
            )
        })?;
        if let Some(error) = payload.get("error").and_then(|value| value.as_str()) {
            return Err(format!(
                "worker init failed for shard {} ({}): {}",
                shard_index, shard_actor_id, error
            ));
        }
        if payload.get("status").and_then(|value| value.as_str()) != Some("ok") {
            return Err(format!(
                "worker init returned unexpected payload for shard {} ({}): {}",
                shard_index, shard_actor_id, payload
            ));
        }
    }

    Ok(())
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
    let worker_stage_operations = counter_delta
        .get("worker_stage_operations")
        .copied()
        .unwrap_or_else(|| counter_delta.get("stage_operations").copied().unwrap_or(0));

    node_metrics.leader_messages = leader_message_delta;
    node_metrics.worker_messages = worker_message_delta;
    node_metrics.messages = if message_delta > 0 {
        message_delta
    } else {
        node_metrics.leader_messages + node_metrics.worker_messages
    };
    node_metrics.stage_operations = worker_stage_operations;
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
    worker_metrics.stage_operations += worker_stage_operations;
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
        "stage_operations": metrics.stage_operations,
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
        "stage_operations": metrics.stage_operations,
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
            || object.contains_key("stage_operations")
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

fn handle_query() -> Vec<u8> {
    with_state(|state| {
        json_bytes(
            state
                .last_result
                .clone()
                .unwrap_or_else(|| serde_json::json!({ "error": "no results yet" })),
        )
    })
}

fn handle_message(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
    let role = with_state(|state| state.role.clone());
    let op = match parse_op(&msg_type, &payload) {
        Ok(op) => op,
        Err(err) => return Ok(json_error(err)),
    };

    match (role.as_str(), op.as_str()) {
        ("leader", "run") => leader::LeaderActor::default()
            .handle_operation(&from_actor, "run", &payload),
        ("leader", "query") | ("worker", "query") => Ok(handle_query()),
        ("worker", "init") => worker::WorkerActor::default()
            .handle_operation(&from_actor, "init", &payload),
        ("worker", "process_stage") => worker::WorkerActor::default()
            .handle_operation(&from_actor, "process_stage", &payload),
        (_, "init") => Ok(json_bytes(serde_json::json!({ "status": "ok" }))),
        _ => Ok(json_error(format!(
            "unsupported op '{}' for role '{}'",
            op, role
        ))),
    }
}

struct Component;

impl Guest for Component {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let config: InitConfig = match serde_json::from_slice(&config) {
            Ok(config) => config,
            Err(err) => {
                return Err(format!("invalid init config: {}", err));
            }
        };
        let actor_id = config
            .actor_id
            .clone()
            .unwrap_or_else(|| host::self_id());
        let role = resolve_role(&config, &actor_id).ok_or_else(|| {
            format!(
                "invalid init config: missing required role for actor {}",
                actor_id
            )
        })?;
        if role != "leader" && role != "worker" {
            return Err(format!(
                "invalid init config: missing required role for actor {}",
                actor_id
            ));
        }

        with_state(|state| {
            state.actor_id = actor_id.clone();
            state.application_id = actor_application_id(&state.actor_id);
            state.role = role.clone();
            state.last_result = None;
            state.worker = None;
        });

        host_info(format!(
            "genomics_pipeline init actor_id={} role={}",
            host::self_id(),
            role
        ));
        Ok(())
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        handle_message(from_actor, msg_type, payload)
    }

    fn get_state() -> Result<Vec<u8>, String> {
        Ok(with_state(|state| {
            serde_json::to_vec(state).unwrap_or_else(|_| b"{}".to_vec())
        }))
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        let next: AppState = match serde_json::from_slice(&state) {
            Ok(state) => state,
            Err(err) => return Err(format!("invalid state: {}", err)),
        };
        with_state(|state| {
            *state = next;
        });
        Ok(())
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_application_id_handles_canonical_actor_ids() {
        assert_eq!(
            actor_application_id("sample-1//worker::genomics-pipeline-rust@test-node-8093"),
            "genomics-pipeline-rust"
        );
    }

    #[test]
    fn actor_application_id_handles_legacy_short_ids() {
        assert_eq!(
            actor_application_id("worker:genomics-pipeline-rust@test-node-8093"),
            "genomics-pipeline-rust"
        );
    }

    #[test]
    fn distribute_units_balances_remainder() {
        assert_eq!(worker::distribute_units(10, 0, 3), 4);
        assert_eq!(worker::distribute_units(10, 1, 3), 3);
        assert_eq!(worker::distribute_units(10, 2, 3), 3);
    }

    #[test]
    fn assigned_chromosomes_spreads_evenly() {
        let chromosomes = vec![
            "chr1".to_string(),
            "chr2".to_string(),
            "chr3".to_string(),
            "chr4".to_string(),
        ];
        assert_eq!(
            worker::assigned_chromosomes(&chromosomes, 0, 2),
            vec!["chr1".to_string(), "chr3".to_string()]
        );
        assert_eq!(
            worker::assigned_chromosomes(&chromosomes, 1, 2),
            vec!["chr2".to_string(), "chr4".to_string()]
        );
    }

    #[test]
    fn normalize_worker_payload_unwraps_nested_remote_envelope() {
        let nested = serde_json::json!({
            "payload": serde_json::to_string(&serde_json::json!({
                "payload": serde_json::to_string(&serde_json::json!({
                    "compute_time_ms": 12,
                    "coordination_time_ms": 4,
                    "stage_operations": 2,
                })).unwrap()
            })).unwrap()
        });
        let normalized = normalize_worker_payload(nested);
        assert_eq!(json_u64(&normalized, "compute_time_ms"), 12);
        assert_eq!(json_u64(&normalized, "coordination_time_ms"), 4);
        assert_eq!(json_u64(&normalized, "stage_operations"), 2);
    }

    #[test]
    fn actor_name_from_actor_id_handles_canonical_ids() {
        assert_eq!(
            actor_name_from_actor_id(
                "worker//genomics_pipeline_wasm::genomics-pipeline-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }

    #[test]
    fn resolve_role_prefers_declared_fields() {
        let config = InitConfig {
            actor_id: Some(
                "genomics-pipeline-123//genomics_pipeline_wasm::genomics-pipeline-rust@test-node-8093"
                    .to_string(),
            ),
            role: Some("worker".to_string()),
            args: None,
        };
        assert_eq!(
            resolve_role(
                &config,
                "genomics-pipeline-123//genomics_pipeline_wasm::genomics-pipeline-rust@test-node-8093"
            ),
            Some("worker".to_string())
        );
    }
}
