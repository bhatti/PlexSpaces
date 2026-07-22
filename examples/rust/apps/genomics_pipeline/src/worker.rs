use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};
use plexspaces::actor::host_actor::self_id;
use plexspaces::actor::host_logging::now_ms;

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct WorkerActor;

#[plexspaces_handlers(wasm)]
impl WorkerActor {
    #[handler("init")]
    fn init_shard(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_init(payload))
    }

    #[handler("process_stage")]
    fn process_stage_op(
        &mut self,
        _from_actor: &str,
        payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(handle_worker_stage(payload))
    }
}

pub(super) fn distribute_units(total: u64, index: usize, count: usize) -> u64 {
    if count == 0 {
        return 0;
    }
    let count_u64 = count as u64;
    let base = total / count_u64;
    let remainder = total % count_u64;
    base + if (index as u64) < remainder { 1 } else { 0 }
}

fn cpu_spin(iterations: u64, seed: u64) -> u64 {
    let rounds = iterations.min(250_000).max(25_000);
    let mut value = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    for step in 0..rounds {
        value = value
            .wrapping_mul(6364136223846793005)
            .wrapping_add(step ^ 0xA5A5_A5A5_A5A5_A5A5);
        value ^= value.rotate_left((step % 31) as u32);
    }
    value
}

pub(super) fn assigned_chromosomes(
    chromosomes: &[String],
    shard_index: usize,
    shard_count: usize,
) -> Vec<String> {
    chromosomes
        .iter()
        .enumerate()
        .filter(|(idx, _)| shard_count == 0 || idx % shard_count == shard_index)
        .map(|(_, chromosome)| chromosome.clone())
        .collect()
}

fn qc_stage(request: &WorkerRequest, worker: &WorkerShard) -> serde_json::Value {
    let reads = distribute_units(request.total_reads, worker.shard_index, worker.shard_count);
    let compute_start = now_ms();
    let checksum = cpu_spin((reads / 32).max(40_000), reads + worker.shard_index as u64);
    let compute_time_ms = now_ms().saturating_sub(compute_start);
    let coordination_time_ms = 6 + ((worker.shard_index as u64 + reads) % 7);
    let passed_reads = ((reads as f64) * 0.945) as u64;
    let failed_reads = reads.saturating_sub(passed_reads);
    let avg_quality =
        32.0 + (request.min_quality_score as f64 * 0.18) + ((checksum % 100) as f64 / 500.0);

    serde_json::json!({
        "sample_id": request.sample_id,
        "reads_processed": reads,
        "passed_reads": passed_reads,
        "failed_reads": failed_reads,
        "avg_quality": avg_quality,
        "stage_operations": reads / 50_000 + 1,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
    })
}

fn alignment_stage(request: &WorkerRequest, worker: &WorkerShard) -> serde_json::Value {
    let reads = distribute_units(request.total_reads, worker.shard_index, worker.shard_count);
    let compute_start = now_ms();
    let checksum = cpu_spin(
        (reads / 24).max(50_000),
        reads ^ 0x55AA55AA ^ worker.shard_index as u64,
    );
    let compute_time_ms = now_ms().saturating_sub(compute_start);
    let coordination_time_ms = 8 + ((worker.shard_index as u64 + reads / 10_000) % 9);
    let aligned_reads = ((reads as f64) * 0.973) as u64;
    let unaligned_reads = reads.saturating_sub(aligned_reads);
    let avg_mapping_quality = 41.0 + ((checksum % 250) as f64 / 100.0);

    serde_json::json!({
        "sample_id": request.sample_id,
        "reads_processed": reads,
        "aligned_reads": aligned_reads,
        "unaligned_reads": unaligned_reads,
        "avg_mapping_quality": avg_mapping_quality,
        "reference_genome": request.reference_genome,
        "stage_operations": reads / 60_000 + 1,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
    })
}

fn variant_stage(request: &WorkerRequest, worker: &WorkerShard) -> serde_json::Value {
    let assigned =
        assigned_chromosomes(&request.chromosomes, worker.shard_index, worker.shard_count);
    let mut total_snps = 0_u64;
    let mut total_indels = 0_u64;
    let mut chromosome_results = Vec::new();
    let compute_start = now_ms();

    for chromosome in &assigned {
        let seed = chromosome
            .bytes()
            .fold(worker.shard_index as u64, |acc, byte| acc + byte as u64)
            + request.total_reads;
        let checksum = cpu_spin((request.total_reads / 64).max(35_000), seed);
        let chromosome_snps = ((request.total_reads / 900_000) + (checksum % 240)) as u64;
        let chromosome_indels = ((request.total_reads / 1_400_000) + (checksum % 90)) as u64;
        total_snps += chromosome_snps;
        total_indels += chromosome_indels;
        chromosome_results.push(serde_json::json!({
            "chromosome": chromosome,
            "snps": chromosome_snps,
            "indels": chromosome_indels,
        }));
    }

    let compute_time_ms = now_ms().saturating_sub(compute_start);
    let coordination_time_ms = 10 + assigned.len() as u64 * 3;

    serde_json::json!({
        "sample_id": request.sample_id,
        "reads_processed": distribute_units(request.total_reads, worker.shard_index, worker.shard_count),
        "chromosomes_processed": assigned,
        "chromosome_results": chromosome_results,
        "variant_snps": total_snps,
        "variant_indels": total_indels,
        "stage_operations": chromosome_results.len() as u64,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
    })
}

fn annotation_stage(request: &WorkerRequest, worker: &WorkerShard) -> serde_json::Value {
    let batches = distribute_units(
        request.annotation_batches,
        worker.shard_index,
        worker.shard_count,
    );
    let compute_start = now_ms();
    let checksum = cpu_spin(
        (batches * 18_000).max(30_000),
        request.total_reads + batches,
    );
    let compute_time_ms = now_ms().saturating_sub(compute_start);
    let coordination_time_ms = 5 + batches;
    let annotated_variants = batches * 28 + (checksum % 13);
    let pathogenic = annotated_variants / 12;
    let benign = annotated_variants / 2;
    let vus = annotated_variants.saturating_sub(pathogenic + benign);

    serde_json::json!({
        "sample_id": request.sample_id,
        "annotation_batches": batches,
        "annotated_variants": annotated_variants,
        "pathogenic_variants": pathogenic,
        "benign_variants": benign,
        "vus_variants": vus,
        "stage_operations": batches.max(1),
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
    })
}

fn process_stage(
    request: &WorkerRequest,
    worker: &WorkerShard,
) -> Result<serde_json::Value, String> {
    let payload = match request.stage.as_str() {
        "quality_control" => qc_stage(request, worker),
        "alignment" => alignment_stage(request, worker),
        "variant_calling" => variant_stage(request, worker),
        "annotation" => annotation_stage(request, worker),
        other => return Err(format!("unsupported stage: {}", other)),
    };
    Ok(payload)
}

pub(super) fn handle_worker_init(payload: &[u8]) -> Vec<u8> {
    let payload: serde_json::Value = match serde_json::from_slice(payload) {
        Ok(payload) => payload,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid init payload: {}", err) }),
            )
        }
    };
    let shard_index = payload
        .get("shard_index")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let shard_count = payload
        .get("shard_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_WORKERS as u64) as usize;

    with_state(|state| {
        state.worker = Some(WorkerShard {
            shard_index,
            shard_count,
        });
    });

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "counter_metrics": { "worker_init_messages": 1 },
        }),
        "worker init metrics update",
    ) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }

    super::json_bytes(serde_json::json!({ "status": "ok", "shard_index": shard_index }))
}

pub(super) fn handle_worker_stage(payload: &[u8]) -> Vec<u8> {
    let request: WorkerRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid stage payload: {}", err) }),
            )
        }
    };

    let worker = match with_state(|state| state.worker.clone()) {
        Some(worker) => worker,
        None => {
            return super::json_bytes(serde_json::json!({ "error": "worker not initialized" }));
        }
    };

    let coordination_start = now_ms();
    let mut payload = match process_stage(&request, &worker) {
        Ok(payload) => payload,
        Err(err) => return super::json_bytes(serde_json::json!({ "error": err })),
    };
    let coordination_time_ms = json_u64(&payload, "coordination_time_ms")
        + now_ms().saturating_sub(coordination_start);
    let compute_time_ms = json_u64(&payload, "compute_time_ms");
    let stage_operations = json_u64(&payload, "stage_operations");
    let latency_ms = compute_time_ms + coordination_time_ms;
    let actor_id = self_id();
    let node_id = actor_node_id(&actor_id);

    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "actor_id".to_string(),
            serde_json::Value::String(actor_id.clone()),
        );
        object.insert(
            "node_id".to_string(),
            serde_json::Value::String(node_id.clone()),
        );
        object.insert(
            "role".to_string(),
            serde_json::Value::String("worker".to_string()),
        );
        object.insert(
            "coordination_time_ms".to_string(),
            serde_json::Value::Number(coordination_time_ms.into()),
        );
        object.insert(
            "latency_ms".to_string(),
            serde_json::Value::Number(latency_ms.into()),
        );
        object.insert(
            "messages_processed".to_string(),
            serde_json::Value::Number(1_u64.into()),
        );
        object.insert(
            "errors".to_string(),
            serde_json::Value::Number(0_u64.into()),
        );
    }

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "worker_messages": 1,
                "worker_stage_operations": stage_operations,
                format!("stage.{}.messages", request.stage): 1,
                format!("stage.{}.operations", request.stage): stage_operations,
            },
            "latency_totals_ms": {
                "worker": latency_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
                "compute": compute_time_ms,
                "coordination": coordination_time_ms,
                format!("stage.{}", request.stage): latency_ms,
            },
            "latency_max_ms": {
                "worker": latency_ms,
                "worker.compute": compute_time_ms,
                "worker.coordination": coordination_time_ms,
                format!("stage.{}", request.stage): latency_ms,
            },
            "latency_samples": {
                "worker": 1,
                "worker.compute": 1,
                "worker.coordination": 1,
                format!("stage.{}", request.stage): 1,
            },
        }),
        "worker stage metrics update",
    ) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }

    super::json_bytes(payload)
}
