use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct WorkerActor;

#[plexspaces_handlers(wasm)]
impl WorkerActor {
    #[handler("init")]
    fn init_shard(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_init(payload))
    }

    #[handler("process_shard_work")]
    fn process_shard_work(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_process_shard_work(payload))
    }
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
    let shard_id = payload
        .get("shard_id")
        .and_then(|value| value.as_u64())
        .unwrap_or(0) as usize;
    let worker_count = payload
        .get("worker_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_WORKER_COUNT as u64) as usize;
    let param_count = payload
        .get("param_count")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_PARAM_COUNT as u64) as usize;
    let iterations = payload
        .get("iterations")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_ITERATIONS as u64) as usize;
    let batch_size = payload
        .get("batch_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_BATCH_SIZE as u64) as usize;

    let shard = WorkerShard {
        shard_id,
        worker_count,
        param_count,
        iterations,
        batch_size,
        samples_for_shard: distribute_samples(
            batch_size * iterations * worker_count,
            worker_count,
            shard_id,
        ),
    };
    with_state(|state| {
        state.worker = Some(shard);
    });
    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "counter_metrics": { "worker_init_messages": 1 },
        }),
        "worker init metrics update",
    ) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }
    super::json_bytes(serde_json::json!({ "status": "ok", "shard_id": shard_id }))
}

pub(super) fn handle_worker_process_shard_work(payload: &[u8]) -> Vec<u8> {
    let request: GradientRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid process_shard_work payload: {}", err) }),
            )
        }
    };
    let shard = match with_state(|state| state.worker.clone()) {
        Some(shard) => shard,
        None => return super::json_bytes(serde_json::json!({ "error": "worker not initialized" })),
    };

    let compute_start = host::now_ms();
    let mut gradient_checksum = 0_u64;
    let mut gradient_scale = 0_u64;
    let per_iteration_samples = shard.samples_for_shard / shard.iterations.max(1);
    let batch_samples = per_iteration_samples.max(shard.batch_size / shard.worker_count.max(1));

    for sample_index in 0..batch_samples {
        let mut accumulator = request
            .weight_checksum
            .wrapping_add((request.iteration as u64 + 1) * 37)
            .wrapping_add((shard.shard_id as u64 + 1) * 101)
            .wrapping_add((sample_index as u64 + 1) * 13);
        for param_block in 0..(shard.param_count / 64).max(8) {
            accumulator = accumulator
                .wrapping_mul(1_664_525)
                .wrapping_add((param_block as u64 + 1) * 1_013_904_223)
                .rotate_left(((sample_index + param_block) % 19) as u32);
            gradient_checksum = gradient_checksum.wrapping_add(accumulator & 0xFFFF);
            gradient_scale = gradient_scale.wrapping_add((accumulator >> 8) & 0x3FF);
        }
    }

    let compute_time_ms = host::now_ms().saturating_sub(compute_start);
    let coordination_time_ms =
        ((request.iteration as u64 + 1) * 2) + ((shard.shard_id as u64 % 5) + 1);
    let latency_ms = compute_time_ms + coordination_time_ms;
    let gradient_operations =
        (batch_samples as u64).saturating_mul((shard.param_count / 64).max(8) as u64);

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "worker_messages": 1,
                "worker_gradient_calls": 1,
                "gradient_operations": gradient_operations,
                "samples_processed": batch_samples as u64,
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
        "worker shard work metrics update",
    ) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }

    super::json_bytes(serde_json::json!({
        "status": "ok",
        "actor_id": host::self_id(),
        "role": "worker",
        "node_id": actor_node_id(&host::self_id()),
        "samples_processed": batch_samples,
        "gradient_operations": gradient_operations,
        "gradient_checksum": gradient_checksum,
        "gradient_scale": gradient_scale,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
        "latency_ms": latency_ms,
        "messages_processed": 1,
        "errors": 0,
    }))
}
