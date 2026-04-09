use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct LeaderActor;

#[plexspaces_handlers(wasm)]
impl LeaderActor {
    #[handler("run")]
    fn run(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_leader_run(payload))
    }
}

fn ensure_worker_initialization(
    shard_actor_ids: &[String],
    request: &RunRequest,
) -> Result<(), String> {
    for (shard_id, shard_actor_id) in shard_actor_ids.iter().enumerate() {
        let request_bytes = serde_json::json!({
            "op": "init",
            "shard_id": shard_id,
            "worker_count": request.worker_count,
            "param_count": request.param_count,
            "iterations": request.iterations,
            "batch_size": request.batch_size,
        })
        .to_string()
        .into_bytes();
        let response =
            host::ask(shard_actor_id, "init", &request_bytes, 10_000).map_err(|err| {
                format!(
                    "worker init failed for shard {} ({}): {}",
                    shard_id, shard_actor_id, err
                )
            })?;

        let payload: serde_json::Value = serde_json::from_slice(&response).map_err(|err| {
            format!(
                "invalid worker init response for shard {} ({}): {}",
                shard_id, shard_actor_id, err
            )
        })?;
        if let Some(error) = payload.get("error").and_then(|value| value.as_str()) {
            return Err(format!(
                "worker init failed for shard {} ({}): {}",
                shard_id, shard_actor_id, error
            ));
        }
        if payload.get("status").and_then(|value| value.as_str()) != Some("ok") {
            return Err(format!(
                "worker init returned unexpected payload for shard {} ({}): {}",
                shard_id, shard_actor_id, payload
            ));
        }
    }

    Ok(())
}

fn update_weight_checksum(
    current_checksum: u64,
    aggregated_gradient_checksum: u64,
    gradient_scale: u64,
    iteration: usize,
) -> u64 {
    current_checksum
        .wrapping_mul(1_315_423_911)
        .wrapping_add(aggregated_gradient_checksum.rotate_left((iteration % 31) as u32))
        .wrapping_add(gradient_scale.wrapping_mul(17))
}

pub(super) fn handle_leader_run(payload: &[u8]) -> Vec<u8> {
    let request: RunRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid run payload: {}", err) }),
            )
        }
    };
    if request.worker_count == 0 || request.iterations == 0 {
        return super::json_bytes(
            serde_json::json!({ "error": "worker_count and iterations must be greater than zero" }),
        );
    }

    let group_id = format!("data-parallel-worker-{}", host::now_ms());
    let create_request_bytes =
        shard_group_create_request_bytes(&group_id, "worker", request.worker_count);
    let create_response = match host::create_shard_group(&create_request_bytes) {
        Ok(response) => response,
        Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
    };
    let shard_actor_ids = match decode_shard_group_create_response(&create_response) {
        Ok(actor_ids) => actor_ids,
        Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
    };

    let leader_node_id = actor_node_id(&host::self_id());
    let mut per_node_metrics: HashMap<String, NodeMetrics> = HashMap::new();
    let mut per_role_metrics: HashMap<String, RoleMetrics> = HashMap::new();

    let mut leader_message_count = 1_u64;
    let mut leader_compute_ms = 0_u64;
    let mut leader_coordination_ms = 0_u64;

    let init_start = host::now_ms();
    if let Err(err) = ensure_worker_initialization(&shard_actor_ids, &request) {
        return super::json_bytes(serde_json::json!({ "error": err }));
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
                return super::json_bytes(serde_json::json!({
                    "error": format!("failed to capture application status for {}: {}", node_id, err),
                }));
            }
        }
    }

    let wall_start = host::now_ms();
    let mut total_errors = 0_u64;
    let mut total_samples_processed = 0_u64;
    let mut weight_checksum = request.param_count as u64 * 97;
    let mut iteration_metrics = Vec::new();
    let mut remote_nodes_with_work: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();

    for iteration in 0..request.iterations {
        leader_message_count += 1;
        let scatter_request = serde_json::json!({
            "op": "process_shard_work",
            "iteration": iteration,
            "weight_checksum": weight_checksum,
        });
        let scatter_start = host::now_ms();
        let scatter_request_bytes = scatter_gather_request_bytes(
            &group_id,
            plexspaces_proto::actor::v1::ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
            scatter_request,
            request.worker_count,
            30_000,
        );
        let response = match host::scatter_gather(&scatter_request_bytes) {
            Ok(response) => response,
            Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
        };
        leader_coordination_ms += host::now_ms().saturating_sub(scatter_start);
        let shard_responses = match decode_scatter_gather_response(&response) {
            Ok(shard_responses) => shard_responses,
            Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
        };

        let mut iteration_errors = 0_u64;
        let mut iteration_successes = 0_u64;
        let mut iteration_worker_latency_ms = 0_u64;
        let mut iteration_max_latency_ms = 0_u64;
        let mut iteration_samples_processed = 0_u64;
        let mut aggregated_gradient_checksum = 0_u64;
        let mut aggregated_gradient_scale = 0_u64;
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
            if node_id != leader_node_id {
                remote_nodes_with_work.insert(node_id);
            }
            if let Some(error_message) = payload.get("error").and_then(|value| value.as_str()) {
                iteration_errors += 1;
                total_errors += 1;
                host::log(
                    "info",
                    &format!(
                        "data_parallel_worker shard error actor_id={} error={}",
                        actor_id, error_message
                    ),
                );
                continue;
            }

            let latency_ms = json_u64(&payload, "latency_ms");
            iteration_worker_latency_ms += latency_ms;
            iteration_max_latency_ms = iteration_max_latency_ms.max(latency_ms);
            let samples_processed = json_u64(&payload, "samples_processed");
            iteration_samples_processed += samples_processed;
            total_samples_processed += samples_processed;
            aggregated_gradient_checksum =
                aggregated_gradient_checksum.wrapping_add(json_u64(&payload, "gradient_checksum"));
            aggregated_gradient_scale =
                aggregated_gradient_scale.wrapping_add(json_u64(&payload, "gradient_scale"));
            iteration_successes += 1;
        }

        weight_checksum = update_weight_checksum(
            weight_checksum,
            aggregated_gradient_checksum,
            aggregated_gradient_scale.max(1),
            iteration,
        );
        leader_compute_ms += host::now_ms().saturating_sub(iteration_compute_start);

        iteration_metrics.push(serde_json::json!({
            "iteration": iteration + 1,
            "samples_processed": iteration_samples_processed,
            "gradient_checksum": aggregated_gradient_checksum,
            "responses": iteration_successes,
            "errors": iteration_errors,
            "avg_latency_ms": if iteration_successes > 0 {
                iteration_worker_latency_ms as f64 / iteration_successes as f64
            } else {
                0.0
            },
            "max_latency_ms": iteration_max_latency_ms,
            "weight_checksum": weight_checksum,
        }));
    }

    let wall_time_ms = host::now_ms().saturating_sub(wall_start);
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "leader_messages": leader_message_count,
                "leader_runs": 1,
                "weight_update_count": request.iterations as u64,
                "training_rounds": request.iterations as u64,
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
        return super::json_bytes(serde_json::json!({ "error": err }));
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
                return super::json_bytes(serde_json::json!({
                    "error": format!("failed to collect final application status for {}: {}", node_id, err),
                }));
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
    let total_gradient_operations: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.gradient_operations)
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

    let node_metrics = per_node_metrics
        .iter()
        .map(|(node_id, metrics)| (node_id.clone(), node_metrics_json(metrics)))
        .collect::<serde_json::Map<String, serde_json::Value>>();
    let role_metrics = per_role_metrics
        .iter()
        .map(|(role, metrics)| (role.clone(), role_metrics_json(metrics)))
        .collect::<serde_json::Map<String, serde_json::Value>>();

    let result = serde_json::json!({
        "status": "ok",
        "orchestration": "scatter_gather",
        "use_case": "data_parallel",
        "group_id": group_id,
        "param_count": request.param_count,
        "worker_count": request.worker_count,
        "iterations": request.iterations,
        "batch_size": request.batch_size,
        "node_count": per_node_metrics.len(),
        "leader_node_id": leader_node_id,
        "node_addresses": node_addresses,
        "shard_actor_ids": shard_actor_ids,
        "training_rounds": request.iterations,
        "actor_count": request.worker_count + 1,
        "message_count": total_messages,
        "gradient_operation_count": total_gradient_operations,
        "samples_processed": total_samples_processed,
        "weight_update_count": request.iterations,
        "weight_checksum": weight_checksum,
        "compute_time_ms": total_compute_ms,
        "coordination_time_ms": total_coordination_ms,
        "total_time_ms": total_time_ms,
        "wall_time_ms": wall_time_ms,
        "granularity_ratio": granularity_ratio,
        "efficiency": efficiency,
        "avg_iteration_latency_ms": if request.iterations > 0 {
            wall_time_ms as f64 / request.iterations as f64
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
        "iterations_detail": iteration_metrics,
        "nodes": node_metrics,
        "roles": role_metrics,
    });
    with_state(|state| {
        state.last_result = Some(result.clone());
    });
    super::json_bytes(result)
}
