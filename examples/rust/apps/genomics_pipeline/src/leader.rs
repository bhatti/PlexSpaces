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

fn stage_request(request: &RunRequest, stage: &str) -> serde_json::Value {
    serde_json::json!({
        "op": "process_stage",
        "stage": stage,
        "sample_id": request.sample_id,
        "total_reads": request.total_reads,
        "min_quality_score": request.min_quality_score,
        "annotation_batches": request.annotation_batches,
        "reference_genome": request.reference_genome,
        "chromosomes": request.chromosomes,
    })
}

fn collect_stage(
    stage_name: &str,
    response: serde_json::Value,
    leader_node_id: &str,
    per_node_metrics: &mut HashMap<String, NodeMetrics>,
    per_role_metrics: &mut HashMap<String, RoleMetrics>,
    remote_nodes_with_work: &mut BTreeSet<String>,
    total_errors: &mut u64,
) -> Result<(StageAccumulator, StageRecord), String> {
    let shard_responses = response
        .get("shard_responses")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();

    let mut accumulator = StageAccumulator::default();

    for shard in shard_responses {
        let success = shard
            .get("success")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        if !success {
            accumulator.errors += 1;
            *total_errors += 1;
            continue;
        }

        let payload = normalize_worker_payload(
            shard
                .get("payload")
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );

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
            .unwrap_or_else(|| leader_node_id.to_string());
        let role = payload
            .get("role")
            .and_then(|value| value.as_str())
            .unwrap_or("worker")
            .to_string();

        if let Some(error) = payload.get("error").and_then(|value| value.as_str()) {
            accumulator.errors += 1;
            *total_errors += 1;
            let node_metrics = per_node_metrics.entry(node_id.clone()).or_default();
            node_metrics.errors += 1;
            let role_metrics = per_role_metrics.entry(role).or_default();
            role_metrics.errors += 1;
            host_info(format!(
                "genomics_pipeline stage error stage={} actor_id={} node_id={} error={}",
                stage_name, actor_id, node_id, error
            ));
            continue;
        }

        let latency_ms = json_u64(&payload, "latency_ms").max(
            json_u64(&payload, "compute_time_ms") + json_u64(&payload, "coordination_time_ms"),
        );
        let stage_operations = json_u64(&payload, "stage_operations");
        accumulator.reads_processed += json_u64(&payload, "reads_processed");
        accumulator.passed_reads += json_u64(&payload, "passed_reads");
        accumulator.failed_reads += json_u64(&payload, "failed_reads");
        accumulator.aligned_reads += json_u64(&payload, "aligned_reads");
        accumulator.unaligned_reads += json_u64(&payload, "unaligned_reads");
        accumulator.variant_snps += json_u64(&payload, "variant_snps");
        accumulator.variant_indels += json_u64(&payload, "variant_indels");
        accumulator.annotated_variants += json_u64(&payload, "annotated_variants");
        accumulator.pathogenic_variants += json_u64(&payload, "pathogenic_variants");
        accumulator.benign_variants += json_u64(&payload, "benign_variants");
        accumulator.vus_variants += json_u64(&payload, "vus_variants");
        accumulator.stage_operations += stage_operations;
        accumulator.total_latency_ms += latency_ms;
        accumulator.max_latency_ms = accumulator.max_latency_ms.max(latency_ms);
        accumulator.responses += 1;
        if role == "worker" && node_id != leader_node_id {
            remote_nodes_with_work.insert(node_id.clone());
        }
    }

    let stage_record = StageRecord {
        name: stage_name.to_string(),
        responses: accumulator.responses,
        reads_processed: accumulator.reads_processed,
        stage_operations: accumulator.stage_operations,
        avg_latency_ms: if accumulator.responses > 0 {
            accumulator.total_latency_ms as f64 / accumulator.responses as f64
        } else {
            0.0
        },
        max_latency_ms: accumulator.max_latency_ms,
        errors: accumulator.errors,
    };

    Ok((accumulator, stage_record))
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

    let group_id = format!("genomics-pipeline-{}", host::now_ms());
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
    if let Err(err) = ensure_worker_initialization(&shard_actor_ids) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }
    leader_coordination_ms += host::now_ms().saturating_sub(init_start);
    leader_message_count += shard_actor_ids.len() as u64;

    let participant_node_ids: BTreeSet<String> = std::iter::once(leader_node_id.clone())
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

    let stages = [
        "quality_control",
        "alignment",
        "variant_calling",
        "annotation",
    ];
    let wall_start = host::now_ms();
    let mut stage_records = Vec::new();
    let mut remote_nodes_with_work = BTreeSet::new();
    let mut total_errors = 0_u64;
    let mut qc_result = serde_json::Value::Null;
    let mut alignment_result = serde_json::Value::Null;
    let mut variant_result = serde_json::Value::Null;
    let mut annotation_result = serde_json::Value::Null;

    for stage_name in stages {
        leader_message_count += 1;
        let scatter_request = stage_request(&request, stage_name);
        let stage_wait_start = host::now_ms();
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
        leader_coordination_ms += host::now_ms().saturating_sub(stage_wait_start);

        let parsed = match decode_scatter_gather_response(&response) {
            Ok(shard_responses) => serde_json::json!({ "shard_responses": shard_responses }),
            Err(err) => return super::json_bytes(serde_json::json!({
                "error": format!("invalid scatter_gather response for {}: {}", stage_name, err)
            })),
        };

        let stage_compute_start = host::now_ms();
        let (accumulator, record) = match collect_stage(
            stage_name,
            parsed,
            &leader_node_id,
            &mut per_node_metrics,
            &mut per_role_metrics,
            &mut remote_nodes_with_work,
            &mut total_errors,
        ) {
            Ok(result) => result,
            Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
        };
        leader_compute_ms += host::now_ms().saturating_sub(stage_compute_start);
        stage_records.push(record);

        match stage_name {
            "quality_control" => {
                qc_result = serde_json::json!({
                    "reads_processed": accumulator.reads_processed,
                    "passed_reads": accumulator.passed_reads,
                    "failed_reads": accumulator.failed_reads,
                    "pass_rate": if accumulator.reads_processed > 0 {
                        accumulator.passed_reads as f64 / accumulator.reads_processed as f64
                    } else {
                        0.0
                    },
                });
            }
            "alignment" => {
                alignment_result = serde_json::json!({
                    "reads_processed": accumulator.reads_processed,
                    "aligned_reads": accumulator.aligned_reads,
                    "unaligned_reads": accumulator.unaligned_reads,
                    "alignment_rate": if accumulator.reads_processed > 0 {
                        accumulator.aligned_reads as f64 / accumulator.reads_processed as f64
                    } else {
                        0.0
                    },
                    "reference_genome": request.reference_genome,
                });
            }
            "variant_calling" => {
                variant_result = serde_json::json!({
                    "chromosome_count": request.chromosomes.len(),
                    "total_snps": accumulator.variant_snps,
                    "total_indels": accumulator.variant_indels,
                });
            }
            "annotation" => {
                annotation_result = serde_json::json!({
                    "annotated_variants": accumulator.annotated_variants,
                    "pathogenic_variants": accumulator.pathogenic_variants,
                    "benign_variants": accumulator.benign_variants,
                    "vus_variants": accumulator.vus_variants,
                });
            }
            _ => {}
        }
    }

    let report_compute_start = host::now_ms();
    let report_summary = serde_json::json!({
        "sample_id": request.sample_id,
        "reference_genome": request.reference_genome,
        "status": if total_errors == 0 { "completed" } else { "completed_with_errors" },
        "quality_control": qc_result,
        "alignment": alignment_result,
        "variant_calling": variant_result,
        "annotation": annotation_result,
    });
    leader_compute_ms += host::now_ms().saturating_sub(report_compute_start);
    let wall_time_ms = host::now_ms().saturating_sub(wall_start);

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "leader_messages": leader_message_count,
                "leader_runs": 1,
                "scatter_gather_rounds": stages.len() as u64,
                "pipeline_stage_operations": stages.len() as u64,
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
    let total_stage_operations: u64 = per_node_metrics
        .values()
        .map(|metrics| metrics.stage_operations)
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
        "sample_id": request.sample_id,
        "reference_genome": request.reference_genome,
        "total_reads": request.total_reads,
        "annotation_batches": request.annotation_batches,
        "chromosome_count": request.chromosomes.len(),
        "node_count": per_node_metrics.len(),
        "worker_node_count": worker_nodes,
        "leader_node_id": leader_node_id,
        "node_addresses": node_addresses,
        "shard_actor_ids": shard_actor_ids,
        "scatter_gather_rounds": stages.len(),
        "actor_count": request.worker_count + 1,
        "message_count": total_messages,
        "stage_operation_count": total_stage_operations,
        "compute_time_ms": total_compute_ms,
        "coordination_time_ms": total_coordination_ms,
        "total_time_ms": total_time_ms,
        "wall_time_ms": wall_time_ms,
        "granularity_ratio": granularity_ratio,
        "efficiency": efficiency,
        "avg_worker_latency_ms": if total_worker_responses > 0 {
            total_worker_latency_ms as f64 / total_worker_responses as f64
        } else {
            0.0
        },
        "max_worker_latency_ms": max_worker_latency_ms,
        "error_count": total_errors,
        "remote_nodes_with_work": remote_nodes_with_work.into_iter().collect::<Vec<_>>(),
        "actor_distribution_skew": actor_distribution_skew,
        "report": report_summary,
        "stages": stage_records,
        "nodes": node_metrics,
        "roles": role_metrics,
    });

    with_state(|state| {
        state.last_result = Some(result.clone());
    });
    super::json_bytes(result)
}
