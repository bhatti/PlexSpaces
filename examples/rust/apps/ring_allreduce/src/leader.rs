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
        let next_actor_id = shard_actor_ids[(shard_id + 1) % shard_actor_ids.len()].clone();
        let previous_actor_id =
            shard_actor_ids[(shard_id + shard_actor_ids.len() - 1) % shard_actor_ids.len()].clone();
        let request_bytes = serde_json::json!({
            "op": "init",
            "shard_id": shard_id,
            "worker_count": request.worker_count,
            "rounds": request.rounds,
            "vector_size": request.vector_size,
            "values_per_shard": request.values_per_shard,
            "next_actor_id": next_actor_id,
            "previous_actor_id": previous_actor_id,
        })
        .to_string()
        .into_bytes();
        let response = host::ask(shard_actor_id, "init", &request_bytes, 10_000).map_err(|err| {
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

pub(super) fn handle_leader_run(payload: &[u8]) -> Vec<u8> {
    let request: RunRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid run payload: {}", err) }),
            )
        }
    };
    if request.worker_count == 0 || request.rounds == 0 {
        return super::json_bytes(
            serde_json::json!({ "error": "worker_count and rounds must be greater than zero" }),
        );
    }

    let group_id = format!("ring-allreduce-{}", host::now_ms());
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
    let mut start_metrics = HashMap::new();
    for node_id in &participant_node_ids {
        match application_metrics(node_id) {
            Ok(metrics) => {
                start_metrics.insert(node_id.clone(), metrics);
            }
            Err(err) => {
                return super::json_bytes(serde_json::json!({
                    "error": format!("failed to capture application metrics for {}: {}", node_id, err),
                }));
            }
        }
    }

    let wall_start = host::now_ms();
    let ring_steps_per_round = request.worker_count.saturating_sub(1);
    let mut total_errors = 0_u64;
    let mut total_values_reduced = 0_u64;
    let mut reduced_checksum = 0_u64;
    let mut round_metrics = Vec::new();
    let mut remote_nodes_with_work: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();

    for round in 0..request.rounds {
        let round_start = host::now_ms();
        let mut round_errors = 0_u64;
        let mut round_successes = 0_u64;
        let mut round_worker_latency_ms = 0_u64;
        let mut round_max_latency_ms = 0_u64;
        let mut round_checksum = 0_u64;
        let mut round_values_reduced = 0_u64;

        for phase in 0..ring_steps_per_round {
            leader_message_count += 1;
            let scatter_request = serde_json::json!({
                "op": "ring_step",
                "round": round,
                "phase": phase,
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
            let phase_compute_start = host::now_ms();

            for shard in shard_responses {
                let success = shard
                    .get("success")
                    .and_then(|value| value.as_bool())
                    .unwrap_or(false);
                if !success {
                    round_errors += 1;
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
                    round_errors += 1;
                    total_errors += 1;
                    host::log(
                        "info",
                        &format!(
                            "ring_allreduce shard error actor_id={} error={}",
                            actor_id, error_message
                        ),
                    );
                    continue;
                }

                let latency_ms = json_u64(&payload, "latency_ms");
                round_worker_latency_ms += latency_ms;
                round_max_latency_ms = round_max_latency_ms.max(latency_ms);
                round_values_reduced += json_u64(&payload, "values_reduced");
                round_checksum =
                    round_checksum.wrapping_add(json_u64(&payload, "partial_checksum"));
                round_successes += 1;
            }

            leader_compute_ms += host::now_ms().saturating_sub(phase_compute_start);
        }

        leader_message_count += 1;
        let finalize_request = serde_json::json!({
            "op": "finalize_round",
            "round": round,
        });
        let finalize_start = host::now_ms();
        let finalize_request_bytes = scatter_gather_request_bytes(
            &group_id,
            plexspaces_proto::actor::v1::ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
            finalize_request,
            request.worker_count,
            30_000,
        );
        let finalize_response = match host::scatter_gather(&finalize_request_bytes) {
            Ok(response) => response,
            Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
        };
        leader_coordination_ms += host::now_ms().saturating_sub(finalize_start);
        let finalize_responses = match decode_scatter_gather_response(&finalize_response) {
            Ok(shard_responses) => shard_responses,
            Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
        };
        let finalize_compute_start = host::now_ms();
        for shard in finalize_responses {
            let raw_payload = shard
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let payload = normalize_worker_payload(raw_payload);
            if let Some(error_message) = payload.get("error").and_then(|value| value.as_str()) {
                round_errors += 1;
                total_errors += 1;
                host::log(
                    "info",
                    &format!("ring_allreduce finalize error error={}", error_message),
                );
                continue;
            }
            reduced_checksum =
                reduced_checksum.wrapping_add(json_u64(&payload, "reduced_checksum"));
        }
        leader_compute_ms += host::now_ms().saturating_sub(finalize_compute_start);

        total_values_reduced += round_values_reduced;
        round_metrics.push(serde_json::json!({
            "round": round + 1,
            "ring_steps": ring_steps_per_round,
            "values_reduced": round_values_reduced,
            "checksum": round_checksum,
            "responses": round_successes,
            "errors": round_errors,
            "avg_latency_ms": if round_successes > 0 {
                round_worker_latency_ms as f64 / round_successes as f64
            } else {
                0.0
            },
            "max_latency_ms": round_max_latency_ms,
            "round_time_ms": host::now_ms().saturating_sub(round_start),
        }));
    }

    let wall_time_ms = host::now_ms().saturating_sub(wall_start);
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "leader_messages": leader_message_count,
                "leader_runs": 1,
                "ring_rounds": request.rounds as u64,
                "ring_steps": (request.rounds * ring_steps_per_round) as u64,
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
    let mut end_metrics = HashMap::new();
    for node_id in &participant_node_ids {
        match application_metrics(node_id) {
            Ok(metrics) => {
                end_metrics.insert(node_id.clone(), metrics);
            }
            Err(err) => {
                return super::json_bytes(serde_json::json!({
                    "error": format!("failed to collect final application metrics for {}: {}", node_id, err),
                }));
            }
        }

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
            }
            Err(err) => {
                return super::json_bytes(serde_json::json!({
                    "error": format!("failed to collect final application status for {}: {}", node_id, err),
                }));
            }
        }
    }

    for node_id in &participant_node_ids {
        let Some(start_metrics) = start_metrics.get(node_id) else {
            continue;
        };
        let Some(end_metrics) = end_metrics.get(node_id) else {
            continue;
        };
        let node_metrics = per_node_metrics.entry(node_id.clone()).or_default();
        accumulate_metrics_delta(
            node_metrics,
            &mut per_role_metrics,
            start_metrics,
            end_metrics,
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
    let total_reduce_operations: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.reduce_operations)
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
        "group_id": group_id,
        "vector_size": request.vector_size,
        "worker_count": request.worker_count,
        "rounds": request.rounds,
        "values_per_shard": request.values_per_shard,
        "node_count": per_node_metrics.len(),
        "leader_node_id": leader_node_id,
        "node_addresses": node_addresses,
        "shard_actor_ids": shard_actor_ids,
        "ring_steps": ring_steps_per_round * request.rounds,
        "actor_count": request.worker_count + 1,
        "message_count": total_messages,
        "reduce_operation_count": total_reduce_operations,
        "values_reduced": total_values_reduced,
        "reduced_checksum": reduced_checksum,
        "compute_time_ms": total_compute_ms,
        "coordination_time_ms": total_coordination_ms,
        "total_time_ms": total_time_ms,
        "wall_time_ms": wall_time_ms,
        "granularity_ratio": granularity_ratio,
        "efficiency": efficiency,
        "avg_round_latency_ms": if request.rounds > 0 {
            wall_time_ms as f64 / request.rounds as f64
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
        "rounds_detail": round_metrics,
        "nodes": node_metrics,
        "roles": role_metrics,
    });
    with_state(|state| {
        state.last_result = Some(result.clone());
    });
    super::json_bytes(result)
}
