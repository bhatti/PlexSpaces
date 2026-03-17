// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Heat Diffusion - Rust WASM app
//
// Leader actor:
// - Creates a shard group of worker actors using the simple-actor WIT host.
// - Initializes workers and runs iterative scatter-gather.
// - Returns benchmark metrics with compute vs coordination split.
//
// Worker actor:
// - Maintains one region of the grid.
// - Exchanges ghost boundaries via TupleSpace.
// - Returns per-iteration compute and coordination timing.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-simple-actor",
    world: "actor-world",
});

use exports::plexspaces::simple_actor::actor::Guest;
use plexspaces::simple_actor::host;

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

fn parse_op(msg_type: &str, payload_json: &str) -> Result<String, String> {
    let payload: serde_json::Value =
        serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
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

fn host_info(message: impl AsRef<str>) {
    host::log("info", message.as_ref());
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

fn current_application_id() -> String {
    with_state(|state| state.application_id.clone())
}

fn merge_application_metrics(metrics: serde_json::Value) -> Result<serde_json::Value, String> {
    let response = host::application_metrics_add(&current_application_id(), &metrics.to_string());
    if response.starts_with("ERROR:") {
        Err(response)
    } else {
        serde_json::from_str(&response)
            .map_err(|err| format!("invalid application metrics response: {}", err))
    }
}

fn require_application_metrics_merge(
    metrics: serde_json::Value,
    context: &str,
) -> Result<serde_json::Value, String> {
    merge_application_metrics(metrics).map_err(|err| format!("{}: {}", context, err))
}

fn application_status(node_id: &str) -> Result<serde_json::Value, String> {
    let response = host::application_get_status(&current_application_id(), node_id);
    if response.starts_with("ERROR:") {
        Err(response)
    } else {
        serde_json::from_str(&response)
            .map_err(|err| format!("invalid application status response: {}", err))
    }
}

fn ensure_worker_initialization(
    shard_actor_ids: &[String],
    width: usize,
    num_regions: usize,
) -> Result<(), String> {
    for (region_id, shard_actor_id) in shard_actor_ids.iter().enumerate() {
        let response = host::ask(
            shard_actor_id,
            "init",
            &serde_json::json!({
                "op": "init",
                "region_id": region_id,
                "width": width,
                "num_regions": num_regions,
            })
            .to_string(),
            10_000,
        );
        if response.starts_with("ERROR:") {
            return Err(format!(
                "worker init failed for shard {} ({}): {}",
                region_id, shard_actor_id, response
            ));
        }

        let payload: serde_json::Value = serde_json::from_str(&response).map_err(|err| {
            format!(
                "invalid worker init response for shard {} ({}): {}",
                region_id, shard_actor_id, err
            )
        })?;
        if let Some(error) = payload.get("error").and_then(|value| value.as_str()) {
            return Err(format!(
                "worker init failed for shard {} ({}): {}",
                region_id, shard_actor_id, error
            ));
        }
        if payload.get("status").and_then(|value| value.as_str()) != Some("ok") {
            return Err(format!(
                "worker init returned unexpected payload for shard {} ({}): {}",
                region_id, shard_actor_id, payload
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

fn status_metrics_value<'a>(
    status: &'a serde_json::Value,
    field: &str,
) -> Option<&'a serde_json::Value> {
    status
        .get("application")
        .and_then(|application| application.get("metrics"))
        .and_then(|metrics| metrics.get(field))
}

fn status_metrics_map(status: &serde_json::Value, field: &str) -> HashMap<String, u64> {
    json_object_to_u64_map(status_metrics_value(status, field))
}

fn status_metrics_scalar(status: &serde_json::Value, field: &str) -> u64 {
    status_metrics_value(status, field)
        .and_then(|value| value.as_u64())
        .unwrap_or(0)
}

fn accumulate_status_delta(
    node_metrics: &mut NodeMetrics,
    role_metrics: &mut HashMap<String, RoleMetrics>,
    start_status: &serde_json::Value,
    end_status: &serde_json::Value,
) {
    let actor_counts = status_metrics_map(end_status, "actor_counts");
    let counter_delta = saturating_map_delta(
        &status_metrics_map(end_status, "counter_metrics"),
        &status_metrics_map(start_status, "counter_metrics"),
    );
    let latency_total_delta = saturating_map_delta(
        &status_metrics_map(end_status, "latency_totals_ms"),
        &status_metrics_map(start_status, "latency_totals_ms"),
    );
    let latency_max_end = status_metrics_map(end_status, "latency_max_ms");
    let latency_sample_delta = saturating_map_delta(
        &status_metrics_map(end_status, "latency_samples"),
        &status_metrics_map(start_status, "latency_samples"),
    );
    let message_delta = status_metrics_scalar(end_status, "message_count")
        .saturating_sub(status_metrics_scalar(start_status, "message_count"));
    let error_delta = status_metrics_scalar(end_status, "error_count")
        .saturating_sub(status_metrics_scalar(start_status, "error_count"));

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

fn parse_tuple_line(tuple_json: &str) -> Option<Vec<f64>> {
    if tuple_json.is_empty() {
        return None;
    }
    let tuple: Vec<serde_json::Value> = serde_json::from_str(tuple_json).ok()?;
    let encoded = tuple.get(4)?.as_str()?;
    serde_json::from_str(encoded).ok()
}

fn build_boundary_tuple(label: &str, iteration: usize, region_id: usize, data: &[f64]) -> String {
    serde_json::json!([
        "boundary",
        iteration,
        region_id,
        label,
        serde_json::to_string(data).unwrap_or_else(|_| "[]".to_string()),
    ])
    .to_string()
}

fn boundary_pattern(label: &str, iteration: usize, region_id: usize) -> String {
    serde_json::json!(["boundary", iteration, region_id, label, "*"]).to_string()
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

fn handle_leader_run(payload_json: &str) -> String {
    let request: RunRequest = match serde_json::from_str(payload_json) {
        Ok(request) => request,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid run payload: {}", err) })
                .to_string()
        }
    };
    let group_id = format!("heat-diffusion-{}", host::now_ms());
    let create_request = serde_json::json!({
        "group_id": group_id,
        "actor_type": "worker",
        "shard_count": request.num_regions,
        "partition_strategy": "hash",
        "rebalance_policy": "manual",
        "placement": {
            "strategy": "from_registry"
        },
        "initial_state": {},
    });
    let create_response = host::create_shard_group(&create_request.to_string());
    if create_response.starts_with("ERROR:") {
        return serde_json::json!({ "error": create_response }).to_string();
    }
    let create_payload: serde_json::Value = match serde_json::from_str(&create_response) {
        Ok(value) => value,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid create_shard_group response: {}", err) })
                .to_string()
        }
    };
    let shard_actor_ids: Vec<String> = create_payload
        .get("shard_actor_ids")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
        .filter_map(|value| value.as_str().map(str::to_string))
        .collect();
    let leader_node_id = actor_node_id(&host::self_id());
    let mut per_node_metrics: HashMap<String, NodeMetrics> = HashMap::new();
    let mut per_role_metrics: HashMap<String, RoleMetrics> = HashMap::new();

    let mut leader_message_count = 1_u64;
    let mut leader_compute_ms = 0_u64;
    let mut leader_coordination_ms = 0_u64;

    let init_start = host::now_ms();
    if let Err(err) = ensure_worker_initialization(&shard_actor_ids, request.width, request.num_regions) {
        return serde_json::json!({ "error": err }).to_string();
    }
    leader_coordination_ms += host::now_ms().saturating_sub(init_start);
    leader_message_count += shard_actor_ids.len() as u64;

    let participant_node_ids: std::collections::BTreeSet<String> =
        std::iter::once(leader_node_id.clone())
            .chain(
                shard_actor_ids
                    .iter()
                    .map(|actor_id| actor_node_id(actor_id)),
            )
            .collect();
    let mut start_statuses = HashMap::new();
    for node_id in &participant_node_ids {
        match application_status(node_id) {
            Ok(status) => {
                start_statuses.insert(node_id.clone(), status);
            }
            Err(err) => {
                return serde_json::json!({
                    "error": format!("failed to capture application status for {}: {}", node_id, err),
                })
                .to_string();
            }
        }
    }

    let wall_start = host::now_ms();
    let mut final_iteration = 0_usize;
    let mut max_diff = 0.0_f64;
    let mut converged = false;
    let mut total_errors = 0_u64;
    let mut iteration_metrics = Vec::new();
    let mut remote_nodes_with_work: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();

    for iteration in 1..=request.max_iterations {
        leader_message_count += 1;
        let scatter_request = serde_json::json!({
            "group_id": group_id,
            "query": {
                "op": "compute",
                "iteration": iteration,
                "tolerance": request.tolerance,
            },
            "aggregation": "concat",
            "min_responses": request.num_regions,
            "timeout_ms": 30000,
        });
        let scatter_start = host::now_ms();
        let response = host::scatter_gather(&scatter_request.to_string());
        leader_coordination_ms += host::now_ms().saturating_sub(scatter_start);
        if response.starts_with("ERROR:") {
            return serde_json::json!({ "error": response }).to_string();
        }
        let parsed: serde_json::Value = match serde_json::from_str(&response) {
            Ok(value) => value,
            Err(err) => {
                return serde_json::json!({ "error": format!("invalid scatter_gather response: {}", err) })
                    .to_string()
            }
        };
        let shard_responses = parsed
            .get("shard_responses")
            .and_then(|value| value.as_array())
            .cloned()
            .unwrap_or_default();

        let mut iteration_errors = 0_u64;
        let mut iteration_worker_latency_ms = 0_u64;
        let mut iteration_max_latency_ms = 0_u64;
        let mut iteration_successes = 0_u64;
        max_diff = 0.0;
        let iteration_compute_start = host::now_ms();

        for shard in shard_responses {
            let success = shard
                .get("success")
                .and_then(|value| value.as_bool())
                .unwrap_or(false);
            if !success {
                iteration_errors += 1;
                total_errors += 1;
                continue;
            }
            let raw_payload = shard
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let payload = normalize_worker_payload(raw_payload);
            let actor_id = payload
                .get("actor_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .or_else(|| {
                    shard
                        .get("shard_actor_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .or_else(|| {
                    shard
                        .get("actor_id")
                        .and_then(|value| value.as_str())
                        .map(str::to_string)
                })
                .unwrap_or_default();
            let node_id = payload
                .get("node_id")
                .and_then(|value| value.as_str())
                .map(str::to_string)
                .filter(|value| !value.is_empty())
                .or_else(|| {
                    if actor_id.is_empty() {
                        None
                    } else {
                        Some(actor_node_id(&actor_id))
                    }
                })
                .unwrap_or_else(|| leader_node_id.clone());
            let role = payload
                .get("role")
                .and_then(|value| value.as_str())
                .unwrap_or("worker")
                .to_string();
            if let Some(error_message) = payload.get("error").and_then(|value| value.as_str()) {
                iteration_errors += 1;
                total_errors += 1;
                let node_metrics = per_node_metrics.entry(node_id.clone()).or_default();
                node_metrics.errors += 1;
                let role_metrics = per_role_metrics.entry(role).or_default();
                role_metrics.errors += 1;
                host_info(format!(
                    "heat_diffusion shard error actor_id={} node_id={} error={}",
                    actor_id, node_id, error_message
                ));
                continue;
            }
            let latency_ms = payload
                .get("latency_ms")
                .and_then(|value| match value {
                    serde_json::Value::Number(number) => number.as_u64(),
                    serde_json::Value::String(text) => text.parse::<u64>().ok(),
                    _ => None,
                })
                .unwrap_or_else(|| {
                    json_u64(&payload, "compute_time_ms")
                        + json_u64(&payload, "coordination_time_ms")
                });
            let worker_errors = json_u64(&payload, "errors");
            if let Some(value) = json_f64(&payload, "max_diff") {
                if value > max_diff {
                    max_diff = value;
                }
            }
            iteration_errors += worker_errors;
            iteration_worker_latency_ms += latency_ms;
            iteration_max_latency_ms = iteration_max_latency_ms.max(latency_ms);
            iteration_successes += 1;
            if role == "worker" && node_id != leader_node_id {
                remote_nodes_with_work.insert(node_id.clone());
            }
        }
        leader_compute_ms += host::now_ms().saturating_sub(iteration_compute_start);

        final_iteration = iteration;
        converged = max_diff < request.tolerance;
        iteration_metrics.push(serde_json::json!({
            "iteration": iteration,
            "max_diff": max_diff,
            "errors": iteration_errors,
            "responses": iteration_successes,
            "avg_latency_ms": if iteration_successes > 0 {
                iteration_worker_latency_ms as f64 / iteration_successes as f64
            } else {
                0.0
            },
            "max_latency_ms": iteration_max_latency_ms,
        }));
        if converged {
            break;
        }
    }

    let wall_time_ms = host::now_ms().saturating_sub(wall_start);
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "leader_messages": leader_message_count,
                "leader_runs": 1,
                "scatter_gather_rounds": final_iteration as u64,
            },
            "latency_totals_ms": {
                "leader": leader_compute_ms + leader_coordination_ms,
                "leader.compute": leader_compute_ms,
                "leader.coordination": leader_coordination_ms,
            },
            "latency_max_ms": {
                "leader": leader_compute_ms + leader_coordination_ms,
                "leader.compute": leader_compute_ms,
                "leader.coordination": leader_coordination_ms,
            },
            "latency_samples": {
                "leader": 1,
                "leader.compute": 1,
                "leader.coordination": 1,
            },
        }),
        "leader metrics update",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }

    let mut node_addresses = serde_json::Map::new();
    let mut end_statuses = HashMap::new();
    for node_id in &participant_node_ids {
        match application_status(node_id) {
            Ok(status) => {
                if let Some(node_address) =
                    status.get("node_address").and_then(|value| value.as_str())
                {
                    node_addresses.insert(
                        node_id.clone(),
                        serde_json::Value::String(node_address.to_string()),
                    );
                }
                end_statuses.insert(node_id.clone(), status);
            }
            Err(err) => {
                return serde_json::json!({
                    "error": format!("failed to collect final application status for {}: {}", node_id, err),
                })
                .to_string();
            }
        }
    }

    for node_id in &participant_node_ids {
        let Some(start_status) = start_statuses.get(node_id) else {
            continue;
        };
        let Some(end_status) = end_statuses.get(node_id) else {
            continue;
        };
        let node_metrics = per_node_metrics.entry(node_id.clone()).or_default();
        accumulate_status_delta(
            node_metrics,
            &mut per_role_metrics,
            start_status,
            end_status,
        );
    }
    apply_topology_actor_counts(
        &mut per_node_metrics,
        &mut per_role_metrics,
        &leader_node_id,
        &shard_actor_ids,
    );

    let total_messages: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.messages)
        .sum();
    let total_tuple_operations: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.tuple_operations)
        .sum();
    let total_compute_ms: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.compute_time_ms)
        .sum();
    let total_coordination_ms: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.coordination_time_ms)
        .sum();
    let total_worker_latency_ms: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.total_latency_ms)
        .sum();
    let max_worker_latency_ms = per_node_metrics
        .values()
        .map(|metrics| metrics.max_latency_ms)
        .max()
        .unwrap_or(0);
    let total_worker_responses: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.responses)
        .sum();

    let total_points = request.width * request.num_regions * final_iteration.max(1);
    let node_metrics = per_node_metrics
        .iter()
        .map(|(node_id, metrics)| (node_id.clone(), node_metrics_json(metrics)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let role_metrics = per_role_metrics
        .iter()
        .map(|(role, metrics)| (role.clone(), role_metrics_json(metrics)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let total_time_ms = total_compute_ms + total_coordination_ms;
    let granularity_ratio = if total_coordination_ms > 0 {
        total_compute_ms as f64 / total_coordination_ms as f64
    } else {
        0.0
    };
    let efficiency = if total_time_ms > 0 {
        total_compute_ms as f64 / total_time_ms as f64
    } else {
        0.0
    };
    let worker_nodes = per_node_metrics
        .iter()
        .filter(|(node_id, metrics)| *node_id != &leader_node_id && metrics.responses > 0)
        .count();
    let max_actors_on_node = per_node_metrics
        .values()
        .map(|metrics| metrics.actors)
        .max()
        .unwrap_or(0);
    let min_actors_on_node = per_node_metrics
        .values()
        .map(|metrics| metrics.actors)
        .min()
        .unwrap_or(0);
    let actor_distribution_skew = max_actors_on_node.saturating_sub(min_actors_on_node);
    let result = serde_json::json!({
        "status": "ok",
        "group_id": group_id,
        "width": request.width,
        "num_regions": request.num_regions,
        "node_count": per_node_metrics.len(),
        "leader_node_id": leader_node_id,
        "node_addresses": node_addresses,
        "shard_actor_ids": shard_actor_ids,
        "scatter_gather_rounds": final_iteration,
        "actor_count": request.num_regions + 1,
        "message_count": total_messages,
        "tuple_operation_count": total_tuple_operations,
        "total_points": total_points,
        "final_iteration": final_iteration,
        "max_diff": max_diff,
        "converged": converged,
        "compute_time_ms": total_compute_ms,
        "coordination_time_ms": total_coordination_ms,
        "total_time_ms": total_time_ms,
        "wall_time_ms": wall_time_ms,
        "granularity_ratio": granularity_ratio,
        "efficiency": efficiency,
        "avg_iteration_latency_ms": if final_iteration > 0 {
            wall_time_ms as f64 / final_iteration as f64
        } else {
            0.0
        },
        "avg_worker_latency_ms": if total_worker_responses > 0 {
            total_worker_latency_ms as f64 / total_worker_responses as f64
        } else {
            0.0
        },
        "max_worker_latency_ms": max_worker_latency_ms,
        "error_count": total_errors,
        "worker_node_count": worker_nodes,
        "remote_nodes_with_work": remote_nodes_with_work.into_iter().collect::<Vec<_>>(),
        "actor_distribution_skew": actor_distribution_skew,
        "nodes": node_metrics,
        "roles": role_metrics,
        "iterations": iteration_metrics,
    });
    with_state(|state| {
        state.last_result = Some(result.clone());
    });
    result.to_string()
}

fn handle_worker_init(payload_json: &str) -> String {
    let payload: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(payload) => payload,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid init payload: {}", err) })
                .to_string()
        }
    };
    let region_id = payload
        .get("region_id")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let width = payload
        .get("width")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_WIDTH as u64) as usize;
    let num_regions = payload
        .get("num_regions")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_REGIONS as u64) as usize;
    let region = initialize_worker(region_id, width, num_regions);
    with_state(|state| {
        state.worker = Some(region);
    });
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "counter_metrics": { "worker_init_messages": 1 },
        }),
        "worker init metrics update",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({ "status": "ok", "region_id": region_id }).to_string()
}

fn handle_worker_compute(payload_json: &str) -> String {
    let request: ComputeRequest = match serde_json::from_str(payload_json) {
        Ok(request) => request,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid compute payload: {}", err) })
                .to_string()
        }
    };
    let mut region = match with_state(|state| state.worker.clone()) {
        Some(region) => region,
        None => {
            return serde_json::json!({ "error": "worker not initialized" }).to_string();
        }
    };

    let coord_start = host::now_ms();
    let north_tuple =
        build_boundary_tuple("north", request.iteration, region.region_id, &region.data);
    let south_tuple =
        build_boundary_tuple("south", request.iteration, region.region_id, &region.data);
    let north_write = host::ts_write(&north_tuple);
    if north_write.starts_with("ERROR:") {
        return serde_json::json!({ "error": north_write }).to_string();
    }
    let south_write = host::ts_write(&south_tuple);
    if south_write.starts_with("ERROR:") {
        return serde_json::json!({ "error": south_write }).to_string();
    }

    let north = if region.region_id > 0 {
        let response = host::ts_read(&boundary_pattern(
            "south",
            request.iteration,
            region.region_id - 1,
        ));
        parse_tuple_line(&response).unwrap_or_else(|| region.fixed_boundary.clone())
    } else {
        region.fixed_boundary.clone()
    };
    let south = if region.region_id + 1 < region.num_regions {
        let response = host::ts_read(&boundary_pattern(
            "north",
            request.iteration,
            region.region_id + 1,
        ));
        parse_tuple_line(&response).unwrap_or_else(|| region.fixed_boundary.clone())
    } else {
        region.fixed_boundary.clone()
    };

    let has_north_neighbor = region.region_id > 0;
    let has_south_neighbor = region.region_id + 1 < region.num_regions;

    let compute_start = host::now_ms();
    let (next, max_diff) = compute_region(&region, &north, &south);
    let compute_time_ms = host::now_ms().saturating_sub(compute_start);
    region.data = next;
    with_state(|state| {
        state.worker = Some(region);
    });

    let coordination_time_ms = host::now_ms()
        .saturating_sub(coord_start)
        .saturating_sub(compute_time_ms);
    let tuple_operations =
        2_u64 + if has_north_neighbor { 1 } else { 0 } + if has_south_neighbor { 1 } else { 0 };
    let latency_ms = compute_time_ms + coordination_time_ms;
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "worker_messages": 1,
                "worker_compute_calls": 1,
                "tuple_operations": tuple_operations,
                "worker_tuple_operations": tuple_operations,
            },
            "latency_totals_ms": {
                "compute": compute_time_ms,
                "coordination": coordination_time_ms,
                "worker": latency_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
            },
            "latency_max_ms": {
                "compute": compute_time_ms,
                "coordination": coordination_time_ms,
                "worker": latency_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
            },
            "latency_samples": {
                "compute": 1,
                "coordination": 1,
                "worker": 1,
                "worker.compute": 1,
                "worker.coordination": 1,
            },
        }),
        "worker compute metrics update",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({
        "status": "ok",
        "actor_id": host::self_id(),
        "role": "worker",
        "node_id": actor_node_id(&host::self_id()),
        "max_diff": max_diff,
        "converged": max_diff < request.tolerance,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
        "latency_ms": latency_ms,
        "messages_processed": 1,
        "tuple_operations": tuple_operations,
        "errors": 0,
    })
    .to_string()
}

struct HeatDiffusionActor;

impl Guest for HeatDiffusionActor {
    fn init(config_json: String) -> String {
        let config: InitConfig = match serde_json::from_str(&config_json) {
            Ok(config) => config,
            Err(err) => return format!("invalid init config: {}", err),
        };
        let actor_id = config.actor_id.unwrap_or_else(|| host::self_id());
        let role = match config.args.as_ref().and_then(|args| args.get("role")) {
            Some(role) if role == "leader" || role == "worker" => role.clone(),
            Some(role) => {
                return format!(
                    "invalid init config: unsupported role '{}' for actor {}",
                    role, actor_id
                )
            }
            None => {
                return format!(
                    "invalid init config: missing required role for actor {}",
                    actor_id
                )
            }
        };
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
        String::new()
    }

    fn handle(_from_actor: String, msg_type: String, payload_json: String) -> String {
        let op = match parse_op(&msg_type, &payload_json) {
            Ok(op) => op,
            Err(err) => return serde_json::json!({ "error": err }).to_string(),
        };
        let role = with_state(|state| state.role.clone());
        match (role.as_str(), op.as_str()) {
            ("leader", "run") => handle_leader_run(&payload_json),
            ("leader", "metrics") => with_state(|state| {
                state
                    .last_result
                    .clone()
                    .unwrap_or_else(|| serde_json::json!({ "error": "no metrics yet" }))
                    .to_string()
            }),
            ("worker", "init") => handle_worker_init(&payload_json),
            ("worker", "compute") => handle_worker_compute(&payload_json),
            _ => serde_json::json!({
                "error": format!("unsupported op '{}' for role '{}'", op, role)
            })
            .to_string(),
        }
    }

    fn get_state() -> String {
        with_state(|state| serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()))
    }

    fn set_state(state_json: String) -> String {
        match serde_json::from_str::<AppState>(&state_json) {
            Ok(new_state) => {
                with_state(|state| {
                    *state = new_state;
                });
                String::new()
            }
            Err(err) => format!("invalid state: {}", err),
        }
    }
}

export!(HeatDiffusionActor);

#[cfg(test)]
mod tests {
    use super::{
        accumulate_status_delta, actor_application_id, normalize_worker_payload, NodeMetrics,
        RoleMetrics,
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
    fn accumulate_status_delta_uses_application_metrics_as_source_of_truth() {
        let start = serde_json::json!({
            "application": {
                "metrics": {
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
                }
            }
        });
        let end = serde_json::json!({
            "application": {
                "metrics": {
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
                }
            }
        });

        let mut node_metrics = NodeMetrics::default();
        let mut role_metrics = HashMap::<String, RoleMetrics>::new();
        accumulate_status_delta(&mut node_metrics, &mut role_metrics, &start, &end);

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
}
