use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct WorkerActor;

#[plexspaces_handlers(wasm)]
impl WorkerActor {
    #[handler("init")]
    fn init_shard(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
        Ok(handle_worker_init(payload_json))
    }

    #[handler("ring_step")]
    fn ring_step(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
        Ok(handle_worker_ring_step(payload_json))
    }

    #[handler("finalize_round")]
    fn finalize_round(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
        Ok(handle_worker_finalize_round(payload_json))
    }
}

pub(super) fn handle_worker_init(payload_json: &str) -> String {
    let payload: serde_json::Value = match serde_json::from_str(payload_json) {
        Ok(payload) => payload,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid init payload: {}", err) })
                .to_string()
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
    let rounds = payload
        .get("rounds")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_ROUNDS as u64) as usize;
    let vector_size = payload
        .get("vector_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_VECTOR_SIZE as u64) as usize;
    let values_per_shard = payload
        .get("values_per_shard")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_VALUES_PER_SHARD as u64) as usize;
    let next_actor_id = payload
        .get("next_actor_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    let previous_actor_id = payload
        .get("previous_actor_id")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();

    let shard = WorkerShard {
        shard_id,
        worker_count,
        rounds,
        vector_size,
        values_per_shard,
        next_actor_id,
        previous_actor_id,
        last_round_checksum: 0,
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
        return serde_json::json!({ "error": err }).to_string();
    }

    serde_json::json!({ "status": "ok", "shard_id": shard_id }).to_string()
}

pub(super) fn handle_worker_ring_step(payload_json: &str) -> String {
    let request: StepRequest = match serde_json::from_str(payload_json) {
        Ok(request) => request,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid ring step payload: {}", err) })
                .to_string()
        }
    };
    let mut shard = match with_state(|state| state.worker.clone()) {
        Some(shard) => shard,
        None => return serde_json::json!({ "error": "worker not initialized" }).to_string(),
    };

    let compute_start = host::now_ms();
    let mut checksum = 0_u64;
    for value_index in 0..shard.values_per_shard {
        let mut value = ((request.round as u64 + 1)
            * (request.phase as u64 + 3)
            * (shard.shard_id as u64 + 5)
            * (value_index as u64 + 7))
            ^ (shard.vector_size as u64 * 11);
        for inner in 0..8 {
            value = value
                .wrapping_mul(6364136223846793005)
                .wrapping_add((inner as u64 + 1) * 1447)
                .rotate_left(((value_index + inner) % 17) as u32);
            checksum = checksum.wrapping_add(value & 0xFFFF);
        }
    }
    let compute_time_ms = host::now_ms().saturating_sub(compute_start);

    let coordination_time_ms = ((request.phase as u64 + 1) * 3)
        + ((shard.shard_id as u64 % 3) + 1)
        + if shard.next_actor_id.is_empty() || shard.previous_actor_id.is_empty() {
            0
        } else {
            2
        };
    let latency_ms = compute_time_ms + coordination_time_ms;
    let reduce_operations = shard.values_per_shard as u64;
    shard.last_round_checksum = shard.last_round_checksum.wrapping_add(checksum);
    with_state(|state| {
        state.worker = Some(shard.clone());
    });

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "worker_messages": 1,
                "worker_ring_steps": 1,
                "reduce_operations": reduce_operations,
                "values_reduced": shard.values_per_shard as u64,
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
        "worker ring-step metrics update",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }

    serde_json::json!({
        "status": "ok",
        "actor_id": host::self_id(),
        "role": "worker",
        "node_id": actor_node_id(&host::self_id()),
        "round": request.round,
        "phase": request.phase,
        "values_reduced": shard.values_per_shard,
        "reduce_operations": reduce_operations,
        "partial_checksum": checksum,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
        "latency_ms": latency_ms,
        "messages_processed": 1,
        "errors": 0,
    })
    .to_string()
}

pub(super) fn handle_worker_finalize_round(payload_json: &str) -> String {
    let request: FinalizeRequest = match serde_json::from_str(payload_json) {
        Ok(request) => request,
        Err(err) => {
            return serde_json::json!({ "error": format!("invalid finalize payload: {}", err) })
                .to_string()
        }
    };
    let mut shard = match with_state(|state| state.worker.clone()) {
        Some(shard) => shard,
        None => return serde_json::json!({ "error": "worker not initialized" }).to_string(),
    };

    let round_checksum = shard
        .last_round_checksum
        .wrapping_add((request.round as u64 + 1) * (shard.shard_id as u64 + 1) * 97);
    shard.last_round_checksum = 0;
    with_state(|state| {
        state.worker = Some(shard);
    });

    serde_json::json!({
        "status": "ok",
        "actor_id": host::self_id(),
        "role": "worker",
        "node_id": actor_node_id(&host::self_id()),
        "round": request.round,
        "reduced_checksum": round_checksum,
        "errors": 0,
    })
    .to_string()
}
