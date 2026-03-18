use super::*;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

#[gen_server_actor(wasm)]
#[derive(Default)]
pub(super) struct WorkerActor;

#[plexspaces_handlers(wasm)]
impl WorkerActor {
    #[handler("init")]
    fn init_region(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
        Ok(handle_worker_init(payload_json))
    }

    #[handler("compute")]
    fn compute_region(&mut self, _from_actor: &str, payload_json: &str) -> Result<String, String> {
        Ok(handle_worker_compute(payload_json))
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

pub(super) fn handle_worker_compute(payload_json: &str) -> String {
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
