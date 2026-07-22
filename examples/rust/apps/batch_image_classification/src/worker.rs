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

    #[handler("classify")]
    fn classify_batch(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(handle_worker_classify(payload))
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
    let total_images = payload
        .get("total_images")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_TOTAL_IMAGES as u64) as usize;
    let batches = payload
        .get("batches")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_BATCHES as u64) as usize;
    let image_vector_size = payload
        .get("image_vector_size")
        .and_then(|value| value.as_u64())
        .unwrap_or(DEFAULT_IMAGE_VECTOR_SIZE as u64) as usize;
    let class_labels = payload
        .get("class_labels")
        .and_then(|value| value.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect::<Vec<_>>()
        })
        .filter(|labels| !labels.is_empty())
        .unwrap_or_else(default_class_labels);

    let shard = WorkerShard {
        shard_id,
        worker_count,
        total_images,
        batches,
        image_vector_size,
        images_for_shard: distribute_images(total_images, worker_count, shard_id),
        class_labels,
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

pub(super) fn handle_worker_classify(payload: &[u8]) -> Vec<u8> {
    let request: ClassifyRequest = match serde_json::from_slice(payload) {
        Ok(request) => request,
        Err(err) => {
            return super::json_bytes(
                serde_json::json!({ "error": format!("invalid classify payload: {}", err) }),
            )
        }
    };
    let shard = match with_state(|state| state.worker.clone()) {
        Some(shard) => shard,
        None => {
            return super::json_bytes(serde_json::json!({ "error": "worker not initialized" }));
        }
    };

    let images_for_round = shard.images_for_shard / shard.batches
        + usize::from(request.round < (shard.images_for_shard % shard.batches));

    let compute_start = now_ms();
    let mut predictions: HashMap<String, u64> = shard
        .class_labels
        .iter()
        .map(|label| (label.clone(), 0))
        .collect();
    let class_count = shard.class_labels.len().max(1);
    let mut score_accumulator = 0_u64;

    for image_index in 0..images_for_round {
        let mut score = ((request.round as u64 + 1)
            * (shard.shard_id as u64 + 3)
            * (image_index as u64 + 5)
            * 17)
            + (request.model_id.len() as u64 * 31);
        for feature in 0..shard.image_vector_size {
            score = score
                .wrapping_mul(1_103_515_245)
                .wrapping_add((feature as u64 + 1) * 12_345)
                .rotate_left((feature % 13) as u32);
            score_accumulator = score_accumulator.wrapping_add(score & 0xFF);
        }
        let class_index = (score as usize) % class_count;
        let label = shard.class_labels[class_index].clone();
        *predictions.entry(label).or_insert(0) += 1;
    }

    let compute_time_ms = now_ms().saturating_sub(compute_start);
    let coordination_time_ms = 0_u64;
    let latency_ms = compute_time_ms + coordination_time_ms;
    let classification_operations =
        (images_for_round as u64).saturating_mul(shard.image_vector_size as u64);

    if let Err(err) = require_application_metrics_merge(
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "worker_messages": 1,
                "worker_classify_calls": 1,
                "classification_operations": classification_operations,
                "images_processed": images_for_round as u64,
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
        "worker classify metrics update",
    ) {
        return super::json_bytes(serde_json::json!({ "error": err }));
    }

    super::json_bytes(serde_json::json!({
        "status": "ok",
        "actor_id": self_id(),
        "role": "worker",
        "node_id": actor_node_id(&self_id()),
        "predictions": predictions,
        "images_processed": images_for_round,
        "classification_operations": classification_operations,
        "compute_time_ms": compute_time_ms,
        "coordination_time_ms": coordination_time_ms,
        "latency_ms": latency_ms,
        "messages_processed": 1,
        "score_checksum": score_accumulator,
        "errors": 0,
    }))
}
