// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Parallel AI Inference - Rust WASM app
//
// Multi-role deployable WASM application using the actor-world WIT host:
// - inference_worker: simulated AI inference
// - benchmark: shard / collective / pool benchmarks
// - orchestrator: workflow-like coordinator
// - metrics_event: fire-and-forget event sink via process groups
// - circuit_breaker: simple closed/open/half-open worker-health FSM

use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, AllReduceShardGroupResponse, BarrierShardGroupRequest,
    BarrierShardGroupResponse, BroadcastShardGroupRequest, BroadcastShardGroupResponse,
    CollectiveReduction, CollectiveTargetField, CreateShardGroupRequest,
    CreateShardGroupResponse, DataParallelConfig, NodePlacement, NodePlacementStrategy,
    PartitionStrategy, ReduceShardGroupRequest, ReduceShardGroupResponse, RebalancePolicy,
    ScatterGatherRequest, ScatterGatherResponse, ShardGroupAggregationStrategy,
};
use plexspaces_proto::application::v1::ApplicationMetrics;
use plexspaces_proto::common::v1::Message as ProtoMessage;
use plexspaces_proto::prost_types;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;
use plexspaces_sdk::simple_actor::ActorWorldHandlers;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

mod leader;
mod worker;

const METRICS_GROUP: &str = "metrics-events";
const POOL_NAME: &str = "inference-pool";
const DEFAULT_MODEL_TYPE: &str = "small";
const DEFAULT_WORKER_TIMEOUT_MS: u64 = 30_000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AppState {
    actor_id: String,
    application_id: String,
    role: String,
    model_type: String,
    requests_processed: u64,
    total_latency_ms: u64,
    error_count: u64,
    results: Vec<Value>,
    benchmark_running: bool,
    mode: String,
    shard_group_id: String,
    total_processed: u64,
    orchestrator_metrics: Value,
    new_shards_requested: u64,
    events_received: u64,
    last_event: Value,
    failure_count: u64,
    success_count: u64,
    threshold: u64,
    fsm_state: String,
}

#[derive(Debug, Deserialize)]
struct InitConfig {
    actor_id: Option<String>,
    role: Option<String>,
    declaration_name: Option<String>,
    args: Option<HashMap<String, String>>,
}

#[derive(Debug, Deserialize)]
struct InferenceRequest {
    request_id: Option<String>,
    input: Option<String>,
    model_type: Option<String>,
    work_multiplier: Option<usize>,
    batch_size: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ShardBenchmarkRequest {
    shard_counts: Option<Vec<usize>>,
    requests_per_shard: Option<usize>,
    warmup_requests: Option<usize>,
    logical_actor_count: Option<usize>,
    payload_size_bytes: Option<usize>,
    model_type: Option<String>,
    work_multiplier: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ScalingBenchmarkRequest {
    shard_counts: Option<Vec<usize>>,
    requests_per_shard: Option<usize>,
    warmup_requests: Option<usize>,
    logical_actor_count: Option<usize>,
    payload_size_bytes: Option<usize>,
    model_type: Option<String>,
    work_multiplier: Option<usize>,
    /// When true, logical_actor_count is per-shard (problem grows with shards = weak scaling).
    /// When false (default), logical_actor_count is fixed total (strong scaling).
    weak_scaling: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct PoolBenchmarkRequest {
    pool_name: Option<String>,
    total_requests: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct CollectiveBenchmarkRequest {
    num_shards: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct WorkflowRunRequest {
    mode: Option<String>,
    num_shards: Option<usize>,
    num_requests: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ScaleRequest {
    new_shards: Option<usize>,
}

fn state_cell() -> &'static Mutex<AppState> {
    static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AppState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let mut guard = state_cell()
        .lock()
        .expect("parallel_ai_inference state lock poisoned");
    f(&mut guard)
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

fn actor_node_id(actor_id: &str) -> String {
    actor_id
        .rsplit_once('@')
        .map(|(_, node_id)| node_id.to_string())
        .unwrap_or_else(|| "local".to_string())
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
        .filter(|role| {
            matches!(
                role.as_str(),
                "inference_worker" | "benchmark" | "orchestrator" | "metrics_event" | "circuit_breaker"
            )
        })
}

fn current_application_id() -> String {
    with_state(|state| state.application_id.clone())
}

fn json_bytes(value: Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn json_error(err: impl Into<String>) -> Vec<u8> {
    json_bytes(json!({ "error": err.into() }))
}

fn parse_payload(payload: &[u8]) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(json!({}));
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

fn duration_from_ms(timeout_ms: u64) -> prost_types::Duration {
    prost_types::Duration {
        seconds: (timeout_ms / 1000) as i64,
        nanos: ((timeout_ms % 1000) * 1_000_000) as i32,
    }
}

fn decode_proto_json_payload(payload: &[u8], context: &str) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_slice(payload).map_err(|err| format!("invalid {context} payload JSON: {err}"))
}

fn json_object_to_u64_map(value: Option<&Value>) -> HashMap<String, u64> {
    value
        .and_then(|value| value.as_object())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed)))
                .collect()
        })
        .unwrap_or_default()
}

fn merge_application_metrics_for(
    application_id: &str,
    metrics: Value,
    context: &str,
) -> Result<(), String> {
    let metrics_bytes = ApplicationMetrics {
        actor_counts: json_object_to_u64_map(metrics.get("actor_counts")),
        supervisor_count: metrics
            .get("supervisor_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0) as u32,
        uptime_seconds: metrics
            .get("uptime_seconds")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        message_count: metrics
            .get("message_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        error_count: metrics
            .get("error_count")
            .and_then(|value| value.as_u64())
            .unwrap_or(0),
        counter_metrics: json_object_to_u64_map(metrics.get("counter_metrics")),
        latency_totals_ms: json_object_to_u64_map(metrics.get("latency_totals_ms")),
        latency_max_ms: json_object_to_u64_map(metrics.get("latency_max_ms")),
        latency_samples: json_object_to_u64_map(metrics.get("latency_samples")),
    }
    .encode_to_vec();
    host::application_metrics_add(application_id, &metrics_bytes)
        .map(|_| ())
        .map_err(|err| format!("{context}: {err}"))
}

fn require_application_metrics_merge(metrics: Value, context: &str) -> Result<(), String> {
    let application_id = current_application_id();
    if application_id.is_empty() {
        return Ok(());
    }
    merge_application_metrics_for(&application_id, metrics, context)
}

fn make_proto_message(payload: Value) -> ProtoMessage {
    ProtoMessage {
        id: format!("req-{}", host::now_ms()),
        message_type: "call".to_string(),
        payload: json_bytes(payload),
        ..Default::default()
    }
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
        .map_err(|err| format!("invalid CreateShardGroupResponse protobuf: {err}"))?;
    Ok(response
        .group
        .map(|group| group.shard_actor_ids)
        .unwrap_or_default())
}

fn scatter_gather_request_bytes(
    group_id: &str,
    aggregation: ShardGroupAggregationStrategy,
    payload: Value,
    min_responses: usize,
    timeout_ms: u64,
) -> Vec<u8> {
    ScatterGatherRequest {
        group_id: group_id.to_string(),
        query: Some(make_proto_message(payload)),
        timeout: Some(duration_from_ms(timeout_ms)),
        aggregation: aggregation as i32,
        min_responses: min_responses as u32,
    }
    .encode_to_vec()
}

fn decode_scatter_gather_response(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let response = ScatterGatherResponse::decode(bytes)
        .map_err(|err| format!("invalid ScatterGatherResponse protobuf: {err}"))?;
    response
        .shard_responses
        .into_iter()
        .map(|shard| {
            let payload = shard
                .response
                .as_ref()
                .map(|message| decode_proto_json_payload(&message.payload, "scatter_gather"))
                .transpose()?
                .unwrap_or(Value::Null);
            let latency_ms = shard
                .latency
                .map(|latency| {
                    let millis_from_seconds = latency.seconds.saturating_mul(1000);
                    let millis_from_nanos = i64::from(latency.nanos) / 1_000_000;
                    millis_from_seconds.saturating_add(millis_from_nanos).max(0) as u64
                })
                .unwrap_or(0);
            Ok(json!({
                "shard_id": shard.shard_id,
                "shard_actor_id": shard.shard_actor_id,
                "payload": payload,
                "success": shard.success,
                "error": shard.error,
                "latency_ms": latency_ms,
            }))
        })
        .collect()
}

fn broadcast_request_bytes(group_id: &str, payload: Value, min_acks: usize, timeout_ms: u64) -> Vec<u8> {
    BroadcastShardGroupRequest {
        group_id: group_id.to_string(),
        message: Some(ProtoMessage {
            id: format!("req-{}", host::now_ms()),
            message_type: "cast".to_string(),
            payload: json_bytes(payload),
            ..Default::default()
        }),
        timeout: Some(duration_from_ms(timeout_ms)),
        min_acks: min_acks as u32,
    }
    .encode_to_vec()
}

fn barrier_request_bytes(group_id: &str, round: u64, min_acks: usize, timeout_ms: u64) -> Vec<u8> {
    BarrierShardGroupRequest {
        group_id: group_id.to_string(),
        barrier_id: format!("barrier-{group_id}-{round}"),
        round,
        timeout: Some(duration_from_ms(timeout_ms)),
        min_acks: min_acks as u32,
    }
    .encode_to_vec()
}

fn reduce_request_bytes(group_id: &str, target: &str, min_responses: usize, timeout_ms: u64) -> Vec<u8> {
    ReduceShardGroupRequest {
        group_id: group_id.to_string(),
        map_function: Some(make_proto_message(json!({ "op": "get_numeric_stats" }))),
        timeout: Some(duration_from_ms(timeout_ms)),
        min_responses: min_responses as u32,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: Some(CollectiveTargetField {
            value_path: target.to_string(),
        }),
    }
    .encode_to_vec()
}

fn all_reduce_request_bytes(group_id: &str, target: &str, min_responses: usize, timeout_ms: u64) -> Vec<u8> {
    AllReduceShardGroupRequest {
        group_id: group_id.to_string(),
        map_function: Some(make_proto_message(json!({ "op": "get_numeric_stats" }))),
        timeout: Some(duration_from_ms(timeout_ms)),
        min_responses: min_responses as u32,
        reduction: CollectiveReduction::CollectiveReductionSum as i32,
        target: Some(CollectiveTargetField {
            value_path: target.to_string(),
        }),
    }
    .encode_to_vec()
}

fn decode_broadcast_response(bytes: &[u8]) -> Result<BroadcastShardGroupResponse, String> {
    BroadcastShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid BroadcastShardGroupResponse protobuf: {err}"))
}

fn decode_barrier_response(bytes: &[u8]) -> Result<BarrierShardGroupResponse, String> {
    BarrierShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid BarrierShardGroupResponse protobuf: {err}"))
}

fn decode_reduce_response(bytes: &[u8]) -> Result<ReduceShardGroupResponse, String> {
    ReduceShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid ReduceShardGroupResponse protobuf: {err}"))
}

fn decode_all_reduce_response(bytes: &[u8]) -> Result<AllReduceShardGroupResponse, String> {
    AllReduceShardGroupResponse::decode(bytes)
        .map_err(|err| format!("invalid AllReduceShardGroupResponse protobuf: {err}"))
}

fn model_iterations(model_type: &str) -> usize {
    match model_type {
        "medium" => 20_000,
        "large" => 50_000,
        _ => 5_000,
    }
}

fn synthetic_worker_coordination_ms(actor_id: &str, model_type: &str) -> u64 {
    let base = match model_type {
        "large" => 4,
        "medium" => 3,
        _ => 2,
    };
    let hash = actor_id
        .bytes()
        .fold(0_u64, |acc, byte| acc.wrapping_add(byte as u64));
    base + (hash % 3)
}

fn round_two(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}

fn percentile_from_sorted(values: &[u64], percentile: f64) -> u64 {
    if values.is_empty() {
        return 0;
    }
    let idx = ((values.len() as f64 * percentile).ceil() as usize)
        .saturating_sub(1)
        .min(values.len().saturating_sub(1));
    values[idx]
}

fn default_scaling_shards() -> Vec<usize> {
    vec![2, 4, 6, 8, 16, 32, 64, 128]
}

fn benchmark_input(size_bytes: usize) -> String {
    "x".repeat(size_bytes.max(1))
}

fn unwrap_payload(payload: &Value) -> Value {
    let mut current = payload.clone();
    for _ in 0..4 {
        let Some(object) = current.as_object() else {
            return current;
        };
        if object.contains_key("status")
            || object.contains_key("result")
            || object.contains_key("requests_processed")
        {
            return current;
        }
        let Some(next) = object
            .get("payload")
            .or_else(|| object.get("result"))
            .or_else(|| object.get("response"))
            .or_else(|| object.get("data"))
            .cloned()
        else {
            return current;
        };
        current = next;
    }
    current
}

fn notify_metrics_event(worker_id: &str, latency_ms: u64, model_type: &str) {
    if let Ok(members) = host::pg_members(METRICS_GROUP) {
        if let Some(target) = members.first() {
            let payload = json!({
                "op": "inference_completed",
                "worker_id": worker_id,
                "latency_ms": latency_ms,
                "model_type": model_type,
            });
            let _ = host::send(target, "cast", &json_bytes(payload));
        }
    }
}

fn handle_worker_infer(payload: &[u8]) -> Vec<u8> {
    let request: InferenceRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid infer payload: {err}")),
    };
    let request_id = request.request_id.unwrap_or_else(|| "request".to_string());
    let request_model_type = request.model_type.clone();
    let work_multiplier = request.work_multiplier.unwrap_or(1).max(1);
    let batch_size = request.batch_size.unwrap_or(1).max(1);
    let (worker_id, application_id, default_model_type) = with_state(|state| {
        (
            state.actor_id.clone(),
            state.application_id.clone(),
            if state.model_type.is_empty() {
                DEFAULT_MODEL_TYPE.to_string()
            } else {
                state.model_type.clone()
            },
        )
    });
    let model_type = request_model_type.unwrap_or(default_model_type);

    // Run one model pass regardless of batch_size — batched inference fuses N items into
    // a single forward pass, so compute grows sub-linearly. Throughput accounting uses
    // batch_size as a multiplier; coordination cost is paid once for the whole batch.
    let iterations = model_iterations(&model_type).saturating_mul(work_multiplier);

    let batch_start_ms = host::now_ms();
    let mut acc = 0_u64;
    for i in 0..iterations {
        acc = acc.wrapping_add(i as u64);
    }
    let compute_time_ms = host::now_ms().saturating_sub(batch_start_ms);
    // One coordination RTT for the whole batch — this is the key batching benefit
    let coordination_time_ms = synthetic_worker_coordination_ms(&worker_id, &model_type);
    let latency_ms = compute_time_ms.saturating_add(coordination_time_ms);

    with_state(|state| {
        state.requests_processed += batch_size as u64;
        state.total_latency_ms += latency_ms;
    });

    let _ = merge_application_metrics_for(
        &application_id,
        json!({
            "message_count": 1,
            "counter_metrics": {
                "requests_processed": batch_size as u64,
                "inference_count": batch_size as u64,
            },
            "latency_totals_ms": {
                "inference": latency_ms,
                "compute": compute_time_ms,
                "coordination": coordination_time_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
            },
            "latency_max_ms": {
                "inference": latency_ms,
                "compute": compute_time_ms,
                "coordination": coordination_time_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
            },
            "latency_samples": {
                "inference": 1,
                "compute": 1,
                "coordination": 1,
                "worker.compute": 1,
                "worker.coordination": 1,
            },
        }),
        "worker inference metrics update",
    );

    notify_metrics_event(&worker_id, latency_ms, &model_type);

    json_bytes(json!({
        "status": "ok",
        "result": format!("inference-result-{request_id}"),
        "model": model_type,
        "batch_size": batch_size,
        "latency_ms": latency_ms,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
        "worker_id": worker_id,
        "node_id": actor_node_id(&host::self_id()),
        "acc": acc % 1000,
    }))
}

fn handle_worker_get_metrics() -> Vec<u8> {
    with_state(|state| {
        let avg = if state.requests_processed > 0 {
            state.total_latency_ms as f64 / state.requests_processed as f64
        } else {
            0.0
        };
        json_bytes(json!({
            "status": "ok",
            "requests_processed": state.requests_processed,
            "avg_latency_ms": avg,
            "error_count": state.error_count,
            "worker_id": state.actor_id,
            "model_type": state.model_type,
        }))
    })
}

fn handle_worker_get_numeric_stats() -> Vec<u8> {
    with_state(|state| {
        let avg = if state.requests_processed > 0 {
            state.total_latency_ms as f64 / state.requests_processed as f64
        } else {
            0.0
        };
        json_bytes(json!({
            "requests_processed": state.requests_processed,
            "avg_latency_ms": avg,
            "error_count": state.error_count,
            "total_latency_ms": state.total_latency_ms,
        }))
    })
}

fn handle_worker_reset() -> Vec<u8> {
    with_state(|state| {
        state.requests_processed = 0;
        state.total_latency_ms = 0;
        state.error_count = 0;
    });
    json_bytes(json!({ "status": "ok" }))
}

fn run_shard_benchmark_impl(
    shard_counts: Vec<usize>,
    requests_per_shard: usize,
    warmup_requests: usize,
    logical_actor_count: Option<usize>,
    payload_size_bytes: usize,
    model_type: &str,
    work_multiplier: usize,
    benchmark_name: &str,
    weak_scaling: bool,
) -> Value {
    let mut benchmark_results = Vec::new();
    let leader_node_id = actor_node_id(&host::self_id());
    let mut baseline: Option<(usize, f64)> = None;

    for shard_count in shard_counts {
        let group_id = format!("bench-shard-{shard_count}-{}", host::now_ms());
        let create_request_bytes =
            shard_group_create_request_bytes(&group_id, "inference_worker", shard_count);
        let create_response = match host::create_shard_group(&create_request_bytes) {
            Ok(response) => response,
            Err(err) => {
                benchmark_results.push(json!({
                    "shards": shard_count,
                    "error": err,
                }));
                continue;
            }
        };
        let shard_actor_ids = match decode_shard_group_create_response(&create_response) {
            Ok(actor_ids) => actor_ids,
            Err(err) => {
                benchmark_results.push(json!({
                    "shards": shard_count,
                    "error": err,
                }));
                continue;
            }
        };
        if shard_actor_ids.is_empty() {
            benchmark_results.push(json!({
                "shards": shard_count,
                "error": "failed to create shard group",
            }));
            continue;
        }

        // Strong scaling: fixed total work divided among shards (batch shrinks as shards grow).
        // Weak scaling: fixed work per shard (batch constant, total grows with shards).
        let actors_per_shard = logical_actor_count.unwrap_or(requests_per_shard).max(1);
        let batch_size = if weak_scaling {
            actors_per_shard
        } else {
            (actors_per_shard / shard_count.max(1)).max(1)
        };
        let scatter_gather_rounds = requests_per_shard.max(1);
        let total_requests = shard_count * batch_size * scatter_gather_rounds;
        let benchmark_input = benchmark_input(payload_size_bytes);
        let mut latencies: Vec<u64> = Vec::new();
        let mut total_compute_ms = 0_u64;
        let mut total_coordination_ms = 0_u64;
        let mut total_errors = 0_u64;
        let mut worker_nodes = std::collections::BTreeSet::new();
        let mut remote_nodes = std::collections::BTreeSet::new();
        let bench_start = host::now_ms();

        for warmup_idx in 0..warmup_requests {
            let request_bytes = scatter_gather_request_bytes(
                &group_id,
                ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
                json!({
                    "op": "infer",
                    "request_id": format!("warmup-{shard_count}-{warmup_idx}"),
                    "input": benchmark_input.clone(),
                    "model_type": model_type,
                    "work_multiplier": work_multiplier,
                    "batch_size": batch_size,
                }),
                shard_count,
                DEFAULT_WORKER_TIMEOUT_MS,
            );
            let _ = host::scatter_gather(&request_bytes);
        }

        for i in 0..scatter_gather_rounds {
            let request_bytes = scatter_gather_request_bytes(
                &group_id,
                ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
                json!({
                    "op": "infer",
                    "request_id": format!("bench-{shard_count}-{i}"),
                    "input": benchmark_input.clone(),
                    "model_type": model_type,
                    "work_multiplier": work_multiplier,
                    "batch_size": batch_size,
                }),
                shard_count,
                DEFAULT_WORKER_TIMEOUT_MS,
            );
            let request_start = host::now_ms();
            let response = match host::scatter_gather(&request_bytes) {
                Ok(response) => response,
                Err(err) => {
                    benchmark_results.push(json!({
                        "shards": shard_count,
                        "error": err,
                    }));
                    latencies.clear();
                    break;
                }
            };
            // Wall-clock coordination time includes the full scatter_gather RTT
            let rtt_ms = host::now_ms().saturating_sub(request_start);
            total_coordination_ms = total_coordination_ms.saturating_add(rtt_ms);
            if let Ok(shards) = decode_scatter_gather_response(&response) {
                for shard in shards {
                    let payload = unwrap_payload(&shard["payload"]);
                    if payload.get("status").and_then(|value| value.as_str()) == Some("ok") {
                        latencies.push(
                            payload
                                .get("latency_ms")
                                .and_then(|value| value.as_u64())
                                .unwrap_or_else(|| shard["latency_ms"].as_u64().unwrap_or(0)),
                        );
                        total_compute_ms = total_compute_ms.saturating_add(
                            payload
                                .get("compute_time_ms")
                                .and_then(|value| value.as_u64())
                                .unwrap_or(0),
                        );
                        if let Some(node_id) = payload.get("node_id").and_then(|value| value.as_str()) {
                            let node_id = node_id.to_string();
                            worker_nodes.insert(node_id.clone());
                            if node_id != leader_node_id {
                                remote_nodes.insert(node_id);
                            }
                        }
                    } else {
                        total_errors = total_errors.saturating_add(1);
                    }
                }
            }
        }

        if latencies.is_empty() {
            continue;
        }

        let elapsed_ms = host::now_ms().saturating_sub(bench_start);
        let elapsed_s = if elapsed_ms > 0 {
            elapsed_ms as f64 / 1000.0
        } else {
            0.001
        };
        let throughput_rps = total_requests as f64 / elapsed_s;
        let avg_latency_ms = latencies.iter().sum::<u64>() as f64 / latencies.len() as f64;
        latencies.sort_unstable();
        let p50_latency_ms = percentile_from_sorted(&latencies, 0.50);
        let p95_latency_ms = percentile_from_sorted(&latencies, 0.95);
        let p99_latency_ms = percentile_from_sorted(&latencies, 0.99);
        let total_measured_ms = total_compute_ms.saturating_add(total_coordination_ms);
        let compute_pct = if total_measured_ms > 0 {
            (total_compute_ms as f64 * 100.0) / total_measured_ms as f64
        } else {
            0.0
        };
        let coordination_pct = if total_measured_ms > 0 {
            (total_coordination_ms as f64 * 100.0) / total_measured_ms as f64
        } else {
            0.0
        };
        let granularity_ratio = if total_coordination_ms > 0 {
            total_compute_ms as f64 / total_coordination_ms as f64
        } else {
            total_compute_ms as f64
        };
        let parallel_efficiency_pct = if let Some((baseline_shards, baseline_throughput)) = baseline {
            let ideal = baseline_throughput * (shard_count as f64 / baseline_shards as f64);
            if ideal > 0.0 {
                (throughput_rps / ideal) * 100.0
            } else {
                0.0
            }
        } else {
            baseline = Some((shard_count, throughput_rps));
            100.0
        };

        benchmark_results.push(json!({
            "shards": shard_count,
            "batch_size": batch_size,
            "scatter_gather_rounds": scatter_gather_rounds,
            "total_requests": total_requests,
            "weak_scaling": weak_scaling,
            "throughput_rps": round_two(throughput_rps),
            "avg_latency_ms": round_two(avg_latency_ms),
            "p50_latency_ms": p50_latency_ms,
            "p95_latency_ms": p95_latency_ms,
            "p99_latency_ms": p99_latency_ms,
            "wall_time_ms": elapsed_ms,
            "compute_time_ms": total_compute_ms,
            "coordination_time_ms": total_coordination_ms,
            "compute_pct": round_two(compute_pct),
            "coordination_pct": round_two(coordination_pct),
            "granularity_ratio": round_two(granularity_ratio),
            "parallel_efficiency_pct": round_two(parallel_efficiency_pct),
            "error_count": total_errors,
            "logical_actor_count": logical_actor_count.unwrap_or(total_requests),
            "payload_size_bytes": payload_size_bytes,
            "model_type": model_type,
            "work_multiplier": work_multiplier,
            "worker_node_count": worker_nodes.len(),
            "remote_nodes_with_work": remote_nodes.into_iter().collect::<Vec<_>>(),
            "shard_actor_ids": shard_actor_ids,
        }));
    }

    json!({
        "status": "ok",
        "benchmark": benchmark_name,
        "results": benchmark_results,
    })
}

fn handle_benchmark_run_shard(payload: &[u8]) -> Vec<u8> {
    let request: ShardBenchmarkRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid shard benchmark payload: {err}")),
    };
    with_state(|state| state.benchmark_running = true);
    let result = run_shard_benchmark_impl(
        request.shard_counts.unwrap_or_else(|| vec![1, 2, 4, 8]),
        request.requests_per_shard.unwrap_or(10),
        request.warmup_requests.unwrap_or(0),
        request.logical_actor_count,
        request.payload_size_bytes.unwrap_or(8_192),
        request.model_type.as_deref().unwrap_or(DEFAULT_MODEL_TYPE),
        request.work_multiplier.unwrap_or(1).max(1),
        "shard",
        false,
    );
    with_state(|state| {
        state.benchmark_running = false;
        state.results.push(result.clone());
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": { "shard_benchmarks_run": 1 },
        }),
        "benchmark shard metrics update",
    );
    json_bytes(result)
}

fn handle_benchmark_run_scaling(payload: &[u8]) -> Vec<u8> {
    let request: ScalingBenchmarkRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid scaling benchmark payload: {err}")),
    };
    with_state(|state| state.benchmark_running = true);
    let weak = request.weak_scaling.unwrap_or(false);
    let result = run_shard_benchmark_impl(
        request.shard_counts.unwrap_or_else(default_scaling_shards),
        request.requests_per_shard.unwrap_or(4),
        request.warmup_requests.unwrap_or(2),
        request.logical_actor_count.or(Some(200)),
        request.payload_size_bytes.unwrap_or(262_144),
        request.model_type.as_deref().unwrap_or("large"),
        request.work_multiplier.unwrap_or(10).max(1),
        if weak { "weak_scaling" } else { "scaling" },
        weak,
    );
    with_state(|state| {
        state.benchmark_running = false;
        state.results.push(result.clone());
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": { "scaling_benchmarks_run": 1 },
        }),
        "benchmark scaling metrics update",
    );
    json_bytes(result)
}

fn handle_benchmark_run_weak_scaling(payload: &[u8]) -> Vec<u8> {
    let request: ScalingBenchmarkRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid weak scaling benchmark payload: {err}")),
    };
    with_state(|state| state.benchmark_running = true);
    let result = run_shard_benchmark_impl(
        request.shard_counts.unwrap_or_else(default_scaling_shards),
        request.requests_per_shard.unwrap_or(4),
        request.warmup_requests.unwrap_or(2),
        request.logical_actor_count.or(Some(50)),
        request.payload_size_bytes.unwrap_or(262_144),
        request.model_type.as_deref().unwrap_or("large"),
        request.work_multiplier.unwrap_or(10).max(1),
        "weak_scaling",
        true,
    );
    with_state(|state| {
        state.benchmark_running = false;
        state.results.push(result.clone());
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": { "weak_scaling_benchmarks_run": 1 },
        }),
        "benchmark weak_scaling metrics update",
    );
    json_bytes(result)
}

fn handle_benchmark_run_pool(payload: &[u8]) -> Vec<u8> {
    let request: PoolBenchmarkRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid pool benchmark payload: {err}")),
    };
    let pool_name = request.pool_name.unwrap_or_else(|| POOL_NAME.to_string());
    let total_requests = request.total_requests.unwrap_or(20);
    with_state(|state| state.benchmark_running = true);

    let mut wait_times = Vec::new();
    let mut exec_times = Vec::new();
    let mut successful = 0_u64;
    let mut failed = 0_u64;
    let bench_start = host::now_ms();

    for i in 0..total_requests {
        let checkout_start = host::now_ms();
        let checkout = match host::pool_checkout(&pool_name, 5_000) {
            Ok(bytes) => bytes,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let wait_ms = host::now_ms().saturating_sub(checkout_start);
        if checkout.is_empty() {
            failed += 1;
            continue;
        }

        let handle: Value = match serde_json::from_slice(&checkout) {
            Ok(handle) => handle,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let actor_id = handle
            .get("actor_id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let checkout_id = handle
            .get("checkout_id")
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        if actor_id.is_empty() || checkout_id.is_empty() {
            failed += 1;
            continue;
        }

        let exec_start = host::now_ms();
        let healthy = match host::ask(
            &actor_id,
            "infer",
            &json_bytes(json!({
                "op": "infer",
                "request_id": format!("pool-{i}"),
                "input": "pool-sample",
            })),
            10_000,
        ) {
            Ok(_) => {
                exec_times.push(host::now_ms().saturating_sub(exec_start));
                successful += 1;
                true
            }
            Err(_) => {
                failed += 1;
                false
            }
        };
        let _ = host::pool_checkin(&pool_name, &actor_id, &checkout_id, healthy);
        wait_times.push(wait_ms);
    }

    let elapsed_ms = host::now_ms().saturating_sub(bench_start);
    let avg_wait_ms = if wait_times.is_empty() {
        0.0
    } else {
        wait_times.iter().sum::<u64>() as f64 / wait_times.len() as f64
    };
    let avg_exec_ms = if exec_times.is_empty() {
        0.0
    } else {
        exec_times.iter().sum::<u64>() as f64 / exec_times.len() as f64
    };
    let pool_utilization = if total_requests == 0 {
        0.0
    } else {
        successful as f64 / total_requests as f64
    };

    let result = json!({
        "status": "ok",
        "benchmark": "pool",
        "pool_name": pool_name,
        "total_requests": total_requests,
        "successful": successful,
        "failed": failed,
        "avg_wait_ms": (avg_wait_ms * 100.0).round() / 100.0,
        "avg_exec_ms": (avg_exec_ms * 100.0).round() / 100.0,
        "pool_utilization": (pool_utilization * 1000.0).round() / 1000.0,
        "elapsed_ms": elapsed_ms,
    });
    with_state(|state| {
        state.benchmark_running = false;
        state.results.push(result.clone());
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": { "pool_benchmarks_run": 1 },
        }),
        "benchmark pool metrics update",
    );
    json_bytes(result)
}

fn handle_benchmark_run_collective(payload: &[u8]) -> Vec<u8> {
    let request: CollectiveBenchmarkRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid collective benchmark payload: {err}")),
    };
    let num_shards = request.num_shards.unwrap_or(4);
    with_state(|state| state.benchmark_running = true);

    let group_id = format!("bench-collective-{}", host::now_ms());
    let create_request_bytes =
        shard_group_create_request_bytes(&group_id, "inference_worker", num_shards);
    let create_response = match host::create_shard_group(&create_request_bytes) {
        Ok(response) => response,
        Err(err) => return json_error(err),
    };
    let shard_actor_ids = match decode_shard_group_create_response(&create_response) {
        Ok(actor_ids) => actor_ids,
        Err(err) => return json_error(err),
    };
    if shard_actor_ids.is_empty() {
        return json_error("failed to create shard group for collective benchmark");
    }

    let mut timings = serde_json::Map::new();

    let t0 = host::now_ms();
    let broadcast_result = host::broadcast_shard_group(&broadcast_request_bytes(
        &group_id,
        json!({ "op": "reset" }),
        num_shards,
        10_000,
    ));
    let broadcast_ms = host::now_ms().saturating_sub(t0);
    timings.insert("broadcast_ms".to_string(), json!(broadcast_ms));

    let t0 = host::now_ms();
    let barrier_result = host::barrier_shard_group(&barrier_request_bytes(
        &group_id,
        1,
        num_shards,
        10_000,
    ));
    let barrier_ms = host::now_ms().saturating_sub(t0);
    timings.insert("barrier_ms".to_string(), json!(barrier_ms));

    let t0 = host::now_ms();
    let scatter_result = host::scatter_gather(&scatter_gather_request_bytes(
        &group_id,
        ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
        json!({
            "op": "infer",
            "request_id": "collective-infer-0",
            "input": "collective-input",
        }),
        num_shards,
        DEFAULT_WORKER_TIMEOUT_MS,
    ));
    let scatter_gather_ms = host::now_ms().saturating_sub(t0);
    timings.insert("scatter_gather_ms".to_string(), json!(scatter_gather_ms));
    let total_ok = scatter_result
        .ok()
        .and_then(|bytes| decode_scatter_gather_response(&bytes).ok())
        .map(|responses| {
            responses
                .iter()
                .filter(|response| {
                    unwrap_payload(&response["payload"])
                        .get("status")
                        .and_then(|value| value.as_str())
                        == Some("ok")
                })
                .count() as u64
        })
        .unwrap_or(0);

    let t0 = host::now_ms();
    let reduce_result = host::reduce_shard_group(&reduce_request_bytes(
        &group_id,
        "requests_processed",
        num_shards,
        10_000,
    ));
    let reduce_ms = host::now_ms().saturating_sub(t0);
    timings.insert("reduce_ms".to_string(), json!(reduce_ms));

    let t0 = host::now_ms();
    let allreduce_result = host::all_reduce_shard_group(&all_reduce_request_bytes(
        &group_id,
        "requests_processed",
        num_shards,
        10_000,
    ));
    let allreduce_ms = host::now_ms().saturating_sub(t0);
    timings.insert("allreduce_ms".to_string(), json!(allreduce_ms));

    let broadcast_acks = broadcast_result
        .ok()
        .and_then(|bytes| decode_broadcast_response(&bytes).ok())
        .map(|response| response.shard_responses.len() as u64)
        .unwrap_or(0);
    let barrier_reached = barrier_result
        .ok()
        .and_then(|bytes| decode_barrier_response(&bytes).ok())
        .map(|response| response.shard_responses.len() >= num_shards)
        .unwrap_or(false);
    let reduce_responses = reduce_result
        .ok()
        .and_then(|bytes| decode_reduce_response(&bytes).ok())
        .map(|response| response.shard_responses.len() as u64)
        .unwrap_or(0);
    let allreduce_responses = allreduce_result
        .ok()
        .and_then(|bytes| decode_all_reduce_response(&bytes).ok())
        .map(|response| response.shard_responses.len() as u64)
        .unwrap_or(0);

    let result = json!({
        "status": "ok",
        "benchmark": "collective",
        "num_shards": num_shards,
        "shard_actor_ids": shard_actor_ids,
        "timings": timings,
        "broadcast_acks": broadcast_acks,
        "barrier_reached": barrier_reached,
        "reduce_responses": reduce_responses,
        "allreduce_responses": allreduce_responses,
        "total_ok": total_ok,
    });
    with_state(|state| {
        state.benchmark_running = false;
        state.results.push(result.clone());
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": { "collective_benchmarks_run": 1 },
            "latency_totals_ms": {
                "broadcast": broadcast_ms,
                "barrier": barrier_ms,
                "reduce": reduce_ms,
                "allreduce": allreduce_ms,
            },
            "latency_max_ms": {
                "broadcast": broadcast_ms,
                "barrier": barrier_ms,
                "reduce": reduce_ms,
                "allreduce": allreduce_ms,
            },
            "latency_samples": {
                "broadcast": 1,
                "barrier": 1,
                "reduce": 1,
                "allreduce": 1,
            },
        }),
        "benchmark collective metrics update",
    );
    json_bytes(result)
}

fn handle_benchmark_get_results() -> Vec<u8> {
    with_state(|state| {
        json_bytes(json!({
            "status": "ok",
            "results": state.results.clone(),
            "total_benchmarks": state.results.len(),
        }))
    })
}

fn orchestrator_run_shard_mode(group_id: &str, num_shards: usize, num_requests: usize) -> Value {
    let create_request_bytes =
        shard_group_create_request_bytes(group_id, "inference_worker", num_shards);
    let create_response = match host::create_shard_group(&create_request_bytes) {
        Ok(response) => response,
        Err(err) => return json!({ "status": "error", "error": err }),
    };
    let shard_actor_ids = match decode_shard_group_create_response(&create_response) {
        Ok(actor_ids) => actor_ids,
        Err(err) => return json!({ "status": "error", "error": err }),
    };
    if shard_actor_ids.is_empty() {
        return json!({ "status": "error", "error": "failed to create shard group" });
    }

    let mut total_latency = 0_u64;
    let mut total_ok = 0_u64;
    let mut results = Vec::new();

    for i in 0..num_requests {
        let request_bytes = scatter_gather_request_bytes(
            group_id,
            ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
            json!({
                "op": "infer",
                "request_id": format!("orch-{i}"),
                "input": "orchestrated-input",
            }),
            num_shards,
            DEFAULT_WORKER_TIMEOUT_MS,
        );
        let response = match host::scatter_gather(&request_bytes) {
            Ok(response) => response,
            Err(err) => return json!({ "status": "error", "error": err }),
        };
        let shards = match decode_scatter_gather_response(&response) {
            Ok(shards) => shards,
            Err(err) => return json!({ "status": "error", "error": err }),
        };
        for shard in shards {
            let payload = unwrap_payload(&shard["payload"]);
            if payload.get("status").and_then(|value| value.as_str()) == Some("ok") {
                total_ok += 1;
                total_latency += payload
                    .get("latency_ms")
                    .and_then(|value| value.as_u64())
                    .unwrap_or_else(|| shard["latency_ms"].as_u64().unwrap_or(0));
                results.push(payload.get("result").cloned().unwrap_or(Value::Null));
            }
        }
    }

    let avg_latency_ms = if total_ok > 0 {
        total_latency as f64 / total_ok as f64
    } else {
        0.0
    };
    json!({
        "status": "ok",
        "mode": "shard",
        "total_processed": total_ok,
        "num_shards": num_shards,
        "shard_actor_ids": shard_actor_ids,
        "metrics": {
            "total_ok": total_ok,
            "avg_latency_ms": (avg_latency_ms * 100.0).round() / 100.0,
            "num_shards": num_shards,
        },
        "sample_results": results.into_iter().take(5).collect::<Vec<_>>(),
    })
}

fn orchestrator_run_collective_mode(group_id: &str, num_shards: usize) -> Value {
    let create_request_bytes =
        shard_group_create_request_bytes(group_id, "inference_worker", num_shards);
    let create_response = match host::create_shard_group(&create_request_bytes) {
        Ok(response) => response,
        Err(err) => return json!({ "status": "error", "error": err }),
    };
    let shard_actor_ids = match decode_shard_group_create_response(&create_response) {
        Ok(actor_ids) => actor_ids,
        Err(err) => return json!({ "status": "error", "error": err }),
    };
    if shard_actor_ids.is_empty() {
        return json!({ "status": "error", "error": "failed to create shard group for collective mode" });
    }

    let mut timings = serde_json::Map::new();

    let t0 = host::now_ms();
    let _ = host::broadcast_shard_group(&broadcast_request_bytes(
        group_id,
        json!({ "op": "reset" }),
        num_shards,
        10_000,
    ));
    timings.insert("broadcast_ms".to_string(), json!(host::now_ms().saturating_sub(t0)));

    let t0 = host::now_ms();
    let _ = host::barrier_shard_group(&barrier_request_bytes(group_id, 1, num_shards, 10_000));
    timings.insert("barrier_ms".to_string(), json!(host::now_ms().saturating_sub(t0)));

    let t0 = host::now_ms();
    let response = host::scatter_gather(&scatter_gather_request_bytes(
        group_id,
        ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
        json!({
            "op": "infer",
            "request_id": "collective-infer-0",
            "input": "collective-input",
        }),
        num_shards,
        DEFAULT_WORKER_TIMEOUT_MS,
    ));
    timings.insert("scatter_gather_ms".to_string(), json!(host::now_ms().saturating_sub(t0)));
    let total_ok = response
        .ok()
        .and_then(|bytes| decode_scatter_gather_response(&bytes).ok())
        .map(|responses| {
            responses
                .iter()
                .filter(|response| {
                    unwrap_payload(&response["payload"])
                        .get("status")
                        .and_then(|value| value.as_str())
                        == Some("ok")
                })
                .count() as u64
        })
        .unwrap_or(0);

    let t0 = host::now_ms();
    let _ = host::reduce_shard_group(&reduce_request_bytes(
        group_id,
        "requests_processed",
        num_shards,
        10_000,
    ));
    timings.insert("reduce_ms".to_string(), json!(host::now_ms().saturating_sub(t0)));

    json!({
        "status": "ok",
        "mode": "collective",
        "total_processed": total_ok,
        "num_shards": num_shards,
        "shard_actor_ids": shard_actor_ids,
        "timings": timings,
        "metrics": {
            "timings": timings,
            "total_ok": total_ok,
            "num_shards": num_shards,
        },
    })
}

fn orchestrator_run_pool_mode(num_requests: usize) -> Value {
    let mut successful = 0_u64;
    let mut failed = 0_u64;
    let mut total_latency = 0_u64;
    for i in 0..num_requests {
        let checkout = match host::pool_checkout(POOL_NAME, 5_000) {
            Ok(bytes) => bytes,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        if checkout.is_empty() {
            failed += 1;
            continue;
        }
        let handle: Value = match serde_json::from_slice(&checkout) {
            Ok(handle) => handle,
            Err(_) => {
                failed += 1;
                continue;
            }
        };
        let actor_id = handle.get("actor_id").and_then(|value| value.as_str()).unwrap_or("");
        let checkout_id = handle.get("checkout_id").and_then(|value| value.as_str()).unwrap_or("");
        if actor_id.is_empty() || checkout_id.is_empty() {
            failed += 1;
            continue;
        }
        let t0 = host::now_ms();
        let healthy = match host::ask(
            actor_id,
            "infer",
            &json_bytes(json!({
                "op": "infer",
                "request_id": format!("pool-orch-{i}"),
                "input": "pool-input",
            })),
            10_000,
        ) {
            Ok(_) => {
                successful += 1;
                total_latency += host::now_ms().saturating_sub(t0);
                true
            }
            Err(_) => {
                failed += 1;
                false
            }
        };
        let _ = host::pool_checkin(POOL_NAME, actor_id, checkout_id, healthy);
    }

    let avg_latency_ms = if successful > 0 {
        total_latency as f64 / successful as f64
    } else {
        0.0
    };
    json!({
        "status": "ok",
        "mode": "pool",
        "total_processed": successful,
        "metrics": {
            "successful": successful,
            "failed": failed,
            "avg_latency_ms": (avg_latency_ms * 100.0).round() / 100.0,
        },
    })
}

fn handle_orchestrator_workflow_run(payload: &[u8]) -> Vec<u8> {
    let request: WorkflowRunRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid workflow_run payload: {err}")),
    };
    let mode = request.mode.unwrap_or_else(|| "shard".to_string());
    let num_shards = request.num_shards.unwrap_or(4);
    let num_requests = request.num_requests.unwrap_or(20);
    let group_id = format!("orch-{mode}-{}", host::now_ms());

    let result = match mode.as_str() {
        "shard" => orchestrator_run_shard_mode(&group_id, num_shards, num_requests),
        "pool" => orchestrator_run_pool_mode(num_requests),
        "collective" => orchestrator_run_collective_mode(&group_id, num_shards),
        _ => json!({ "status": "error", "error": format!("unknown mode: {mode}") }),
    };

    with_state(|state| {
        state.mode = mode.clone();
        state.shard_group_id = group_id.clone();
        state.total_processed = result
            .get("total_processed")
            .and_then(|value| value.as_u64())
            .unwrap_or(0);
        state.orchestrator_metrics = result.get("metrics").cloned().unwrap_or_else(|| json!({}));
    });

    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": {
                "workflow_runs": 1,
                format!("workflow_mode_{mode}"): 1,
            },
        }),
        "orchestrator workflow metrics update",
    );

    json_bytes(result)
}

fn handle_orchestrator_workflow_signal_scale(payload: &[u8]) -> Vec<u8> {
    let request: ScaleRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => return json_error(format!("invalid workflow scale payload: {err}")),
    };
    let new_shards = request.new_shards.unwrap_or(4) as u64;
    with_state(|state| state.new_shards_requested = new_shards);
    json_bytes(json!({ "status": "ok", "new_shards_requested": new_shards }))
}

fn handle_orchestrator_status() -> Vec<u8> {
    with_state(|state| {
        json_bytes(json!({
            "mode": state.mode,
            "total_processed": state.total_processed,
            "metrics": state.orchestrator_metrics.clone(),
            "shard_group_id": state.shard_group_id,
            "new_shards_requested": state.new_shards_requested,
        }))
    })
}

fn handle_metrics_event_inference_completed(payload: &[u8]) -> Vec<u8> {
    let payload: Value = match serde_json::from_slice(payload) {
        Ok(payload) => payload,
        Err(err) => return json_error(format!("invalid metrics event payload: {err}")),
    };
    let worker_id = payload
        .get("worker_id")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let latency_ms = payload
        .get("latency_ms")
        .and_then(|value| value.as_u64())
        .unwrap_or(0);
    let model_type = payload
        .get("model_type")
        .and_then(|value| value.as_str())
        .unwrap_or(DEFAULT_MODEL_TYPE)
        .to_string();

    with_state(|state| {
        state.events_received += 1;
        state.last_event = json!({
            "worker_id": worker_id,
            "latency_ms": latency_ms,
            "model_type": model_type,
        });
    });
    let _ = require_application_metrics_merge(
        json!({
            "message_count": 1,
            "counter_metrics": {
                "events_received": 1,
                format!("model_{model_type}_events"): 1,
            },
            "latency_totals_ms": { "event_latency": latency_ms },
            "latency_max_ms": { "event_latency": latency_ms },
            "latency_samples": { "event_latency": 1 },
        }),
        "metrics event update",
    );
    json_bytes(json!({ "status": "ok" }))
}

fn handle_metrics_event_get_stats() -> Vec<u8> {
    with_state(|state| {
        json_bytes(json!({
            "events_received": state.events_received,
            "last_event": state.last_event,
        }))
    })
}

fn handle_circuit_record_success() -> Vec<u8> {
    with_state(|state| {
        if state.fsm_state == "half_open" {
            state.success_count += 1;
            if state.success_count >= 2 {
                state.fsm_state = "closed".to_string();
                state.failure_count = 0;
                state.success_count = 0;
            }
        } else if state.fsm_state == "closed" {
            state.failure_count = state.failure_count.saturating_sub(1);
        }
        json_bytes(json!({
            "state": state.fsm_state,
            "failures": state.failure_count,
        }))
    })
}

fn handle_circuit_record_failure() -> Vec<u8> {
    let mut should_schedule_reset = false;
    let response = with_state(|state| {
        if state.fsm_state == "closed" {
            state.failure_count += 1;
            if state.failure_count >= state.threshold {
                state.fsm_state = "open".to_string();
                should_schedule_reset = true;
            }
        } else if state.fsm_state == "half_open" {
            state.fsm_state = "open".to_string();
            should_schedule_reset = true;
        }
        json_bytes(json!({
            "state": state.fsm_state,
            "failures": state.failure_count,
        }))
    });
    if should_schedule_reset {
        let _ = host::send_after(5_000, "try_reset", b"{}");
    }
    response
}

fn handle_circuit_try_reset() -> Vec<u8> {
    with_state(|state| {
        if state.fsm_state == "open" {
            state.fsm_state = "half_open".to_string();
            state.success_count = 0;
        }
        json_bytes(json!({ "state": state.fsm_state }))
    })
}

fn handle_circuit_is_allowed() -> Vec<u8> {
    with_state(|state| {
        json_bytes(json!({
            "allowed": state.fsm_state != "open",
            "state": state.fsm_state,
        }))
    })
}

fn handle_circuit_get_state() -> Vec<u8> {
    with_state(|state| {
        json_bytes(json!({
            "fsm_state": state.fsm_state,
            "failures": state.failure_count,
            "successes": state.success_count,
        }))
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct MetricsEventActor;

#[plexspaces_handlers(wasm)]
impl MetricsEventActor {
    #[handler("inference_completed")]
    fn inference_completed(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_metrics_event_inference_completed(payload))
    }

    #[handler("get_stats")]
    fn get_stats(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_metrics_event_get_stats())
    }
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct CircuitBreakerActor;

#[plexspaces_handlers(wasm)]
impl CircuitBreakerActor {
    #[handler("record_success")]
    fn record_success(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_circuit_record_success())
    }

    #[handler("record_failure")]
    fn record_failure(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_circuit_record_failure())
    }

    #[handler("try_reset")]
    fn try_reset(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_circuit_try_reset())
    }

    #[handler("is_allowed")]
    fn is_allowed(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_circuit_is_allowed())
    }

    #[handler("get_state")]
    fn get_state(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_circuit_get_state())
    }
}

struct ParallelAiInferenceBridge;

impl ParallelAiInferenceBridge {
    fn initialize(config_json: String) -> Result<String, String> {
        let config: InitConfig = serde_json::from_str(&config_json)
            .map_err(|err| format!("invalid init config: {err}"))?;
        let actor_id = config
            .actor_id
            .clone()
            .unwrap_or_else(|| host::self_id());
        let role = resolve_role(&config, &actor_id)
            .ok_or_else(|| format!("missing required role for actor {actor_id}"))?;
        let application_id = actor_application_id(&actor_id);
        let model_type = config
            .args
            .as_ref()
            .and_then(|args| args.get("model_type"))
            .cloned()
            .unwrap_or_else(|| DEFAULT_MODEL_TYPE.to_string());
        let threshold = config
            .args
            .as_ref()
            .and_then(|args| args.get("threshold"))
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(3);

        with_state(|state| {
            *state = AppState::default();
            state.actor_id = actor_id.clone();
            state.application_id = application_id;
            state.role = role.clone();
            state.model_type = model_type;
            state.mode = "shard".to_string();
            state.threshold = threshold;
            state.fsm_state = "closed".to_string();
        });

        if role == "metrics_event" {
            let _ = host::pg_join(METRICS_GROUP);
        }

        Ok(String::new())
    }
}

impl Guest for ParallelAiInferenceBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let config_json =
            String::from_utf8(config).map_err(|err| format!("invalid init config bytes: {err}"))?;
        Self::initialize(config_json).map(|_| ())
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let op = match parse_op(&msg_type, &payload) {
            Ok(op) => op,
            Err(err) => return Ok(json_error(err)),
        };
        let role = with_state(|state| state.role.clone());

        match role.as_str() {
            "inference_worker" => worker::InferenceWorkerActor::default().handle_operation(&from_actor, &op, &payload),
            "benchmark" => leader::BenchmarkActor::default().handle_operation(&from_actor, &op, &payload),
            "orchestrator" => leader::OrchestratorActor::default().handle_operation(&from_actor, &op, &payload),
            "metrics_event" => MetricsEventActor::default().handle_operation(&from_actor, &op, &payload),
            "circuit_breaker" => CircuitBreakerActor::default().handle_operation(&from_actor, &op, &payload),
            _ => Ok(json_error(format!(
                "unsupported op '{}' for role '{}'",
                op, role
            ))),
        }
    }

    fn get_state() -> Result<Vec<u8>, String> {
        with_state(|state| {
            serde_json::to_vec(state).map_err(|err| format!("state encode failed: {err}"))
        })
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        if state.is_empty() {
            return Ok(());
        }
        match serde_json::from_slice::<AppState>(&state) {
            Ok(next_state) => {
                let mut guard = state_cell()
                    .lock()
                    .expect("parallel_ai_inference set_state lock poisoned");
                *guard = next_state;
                Ok(())
            }
            Err(err) => Err(format!("invalid state JSON: {err}")),
        }
    }
}

export!(ParallelAiInferenceBridge);
