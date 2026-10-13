// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Job Queue Outbox (Rust WASM)
//
// Demonstrates high-scale outbox dispatch with admission control:
//   - Leader creates 1000 virtual workers via create_shard_group WIT call
//   - Leader dispatches jobs to all workers via outbox_dispatch WIT call
//   - Upgrade simulation: Ask workers for upgrade status (ask WIT call)
//   - Heartbeat OVERWRITE: LWW idempotency with conflict_resolution="overwrite"
//   - Scaling benchmark: measures throughput at 100/500/1000 worker scales
//
// Actor routing: both types live in one WASM binary, dispatched by actor_type
// in the init config (set by the PlexSpaces supervisor framework).

use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use plexspaces_proto::actor::v1::{
    CreateShardGroupRequest, CreateShardGroupResponse, DataParallelConfig, NodePlacement,
    NodePlacementStrategy, PartitionStrategy, RebalancePolicy,
};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::{ask, self_id};
use plexspaces::actor::host_logging::{log, now_ms};
use plexspaces::actor::host_shard::create_shard_group;
use plexspaces::actor::outbox::outbox_dispatch;

const DISPATCHER_ID: &str = "job-outbox-dispatcher";
const HEARTBEAT_IDEM_KEY: &str = "hb-leader";
const DEFAULT_WORKERS: usize = 1000;

// ============================================================================
// Leader state
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct LeaderState {
    actor_id: String,
    total_dispatched: u64,
    total_accepted: u64,
    total_rejected: u64,
    dispatch_count: u64,
    wall_time_ms: u64,
    heartbeat_count: u64,
    ask_rtt_min_ms: u64,
    ask_rtt_max_ms: u64,
    ask_rtt_sum_ms: u64,
    ask_rtt_count: u64,
    shard_group_id: String,
    worker_ids: Vec<String>,
}

// ============================================================================
// Worker state
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct WorkerState {
    actor_id: String,
    jobs_received: u64,
    last_heartbeat_seq: i64,
}

// ============================================================================
// Shared actor kind enum stored in the global cell
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(tag = "kind")]
enum ActorKind {
    #[default]
    Uninitialized,
    Leader(LeaderState),
    Worker(WorkerState),
}

fn state_cell() -> &'static Mutex<ActorKind> {
    static STATE: OnceLock<Mutex<ActorKind>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(ActorKind::default()))
}

fn with_state<T>(f: impl FnOnce(&mut ActorKind) -> T) -> T {
    let mut g = state_cell().lock().expect("state lock poisoned");
    f(&mut g)
}

// ============================================================================
// Helpers
// ============================================================================

fn json_bytes(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap_or_else(|_| b"{\"error\":\"serialize failed\"}".to_vec())
}

fn json_err(msg: &str) -> Vec<u8> {
    json_bytes(json!({"error": msg}))
}

fn parse_payload(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return json!({});
    }
    serde_json::from_slice(bytes).unwrap_or(json!({}))
}

fn str_field<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(|x| x.as_str()).unwrap_or("")
}

fn _u64_field(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}

/// Parse (accepted, rejected) counts from outbox-dispatch result JSON.
fn parse_dispatch_result(result_str: &str) -> (u64, u64) {
    serde_json::from_str::<Value>(result_str)
        .ok()
        .map(|v| {
            let accepted = v.get("accepted").and_then(|x| x.as_u64()).unwrap_or(0);
            let rejected = v.get("rejected").and_then(|x| x.as_u64()).unwrap_or(0);
            (accepted, rejected)
        })
        .unwrap_or((0, 0))
}

/// Build a create_shard_group request bytes for the given actor_type and count.
fn shard_group_create_request(group_id: &str, actor_type: &str, count: usize) -> Vec<u8> {
    CreateShardGroupRequest {
        request_id: String::new(),
        config: Some(DataParallelConfig {
            group_id: group_id.to_string(),
            shard_count: count as u32,
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

// ============================================================================
// Leader handlers
// ============================================================================

fn leader_init_workers(state: &mut LeaderState, v: &Value) -> Vec<u8> {
    let count = v.get("count").and_then(|x| x.as_u64()).unwrap_or(DEFAULT_WORKERS as u64) as usize;
    let group_id = format!("job-queue-{}", now_ms());

    let req_bytes = shard_group_create_request(&group_id, "JobQueueWorker", count);

    let worker_ids = match create_shard_group(&req_bytes) {
        Ok(resp_bytes) => {
            match CreateShardGroupResponse::decode(resp_bytes.as_slice()) {
                Ok(resp) => resp.group.map(|g| g.shard_actor_ids).unwrap_or_default(),
                Err(e) => {
                    let _ = log("warn", &format!("create_shard_group decode error: {}", e));
                    Vec::new()
                }
            }
        }
        Err(e) => {
            let _ = log("warn", &format!("create_shard_group error: {:?}", e));
            Vec::new()
        }
    };

    state.shard_group_id = group_id.clone();
    state.worker_ids = worker_ids.clone();

    let _ = log(
        "info",
        &format!(
            "Leader {}: created shard group {} with {} workers",
            state.actor_id,
            group_id,
            worker_ids.len()
        ),
    );

    json_bytes(json!({
        "success": true,
        "group_id": group_id,
        "worker_count": worker_ids.len(),
    }))
}

fn leader_dispatch_jobs(state: &mut LeaderState, v: &Value) -> Vec<u8> {
    if state.worker_ids.is_empty() {
        return json_err("no workers initialized — call init_workers first");
    }

    let inner_payload = v.get("payload").cloned().unwrap_or(json!({"task": "process_item"}));
    let payload_json = inner_payload.to_string();
    let payload_bytes: Vec<u8> = payload_json.as_bytes().to_vec();
    let ctx = r#"{"tenant":"default","namespace":"default"}"#;

    let t0 = now_ms();
    let (accepted, rejected) = match outbox_dispatch(
        ctx,
        DISPATCHER_ID,
        &state.worker_ids,
        &payload_bytes,
        "cast",
        "",
        "",
    ) {
        Ok(result_str) => parse_dispatch_result(&result_str),
        Err(e) => {
            let _ = log("warn", &format!("outbox_dispatch error: {:?}", e));
            (0, state.worker_ids.len() as u64)
        }
    };
    let wall_ms = now_ms().saturating_sub(t0);

    state.total_dispatched += state.worker_ids.len() as u64;
    state.total_accepted += accepted;
    state.total_rejected += rejected;
    state.dispatch_count += 1;
    state.wall_time_ms = state.wall_time_ms.saturating_add(wall_ms);

    let throughput = if wall_ms > 0 { accepted * 1000 / wall_ms } else { 0 };

    json_bytes(json!({
        "success": true,
        "accepted": accepted,
        "rejected": rejected,
        "wall_time_ms": wall_ms,
        "throughput_per_s": throughput,
        "total_dispatched": state.total_dispatched,
    }))
}

fn leader_upgrade_workers(state: &mut LeaderState, v: &Value) -> Vec<u8> {
    let count = v.get("count").and_then(|x| x.as_u64()).unwrap_or(10) as usize;
    let n = count.min(state.worker_ids.len());
    if n == 0 {
        return json_err("no workers initialized");
    }

    let upgrade_payload = json!({"action": "upgrade", "version": "2.0.0"}).to_string();
    let upgrade_bytes: Vec<u8> = upgrade_payload.as_bytes().to_vec();

    let mut workers_asked: u64 = 0;
    let mut rtt_min = u64::MAX;
    let mut rtt_max = 0u64;
    let mut rtt_sum = 0u64;

    for worker_id in state.worker_ids.iter().take(n) {
        let t0 = now_ms();
        match ask(worker_id, "upgrade", &upgrade_bytes, 5000) {
            Ok(_) => {
                let rtt = now_ms().saturating_sub(t0);
                workers_asked += 1;
                rtt_min = rtt_min.min(rtt);
                rtt_max = rtt_max.max(rtt);
                rtt_sum += rtt;
            }
            Err(e) => {
                let _ = log("warn", &format!("ask {} upgrade error: {:?}", worker_id, e));
            }
        }
    }

    if workers_asked == 0 {
        rtt_min = 0;
    }
    let rtt_avg = if workers_asked > 0 { rtt_sum / workers_asked } else { 0 };

    // Update leader RTT tracking
    if workers_asked > 0 {
        if state.ask_rtt_count == 0 {
            state.ask_rtt_min_ms = rtt_min;
            state.ask_rtt_max_ms = rtt_max;
        } else {
            state.ask_rtt_min_ms = state.ask_rtt_min_ms.min(rtt_min);
            state.ask_rtt_max_ms = state.ask_rtt_max_ms.max(rtt_max);
        }
        state.ask_rtt_sum_ms += rtt_sum;
        state.ask_rtt_count += workers_asked;
    }

    json_bytes(json!({
        "success": true,
        "workers_asked": workers_asked,
        "ask_rtt_min_ms": rtt_min,
        "ask_rtt_max_ms": rtt_max,
        "ask_rtt_avg_ms": rtt_avg,
    }))
}

fn leader_send_heartbeat(state: &mut LeaderState, v: &Value) -> Vec<u8> {
    let seq = v.get("seq").and_then(|x| x.as_i64()).unwrap_or(0);
    let my_id = self_id();
    let target = if my_id.is_empty() {
        "leader:JobQueueOutboxCoordinator".to_string()
    } else {
        my_id
    };

    let ctx = r#"{"tenant":"default","namespace":"default"}"#;
    let hb_payload = json!({"event": "heartbeat", "seq": seq}).to_string();
    let hb_bytes: Vec<u8> = hb_payload.as_bytes().to_vec();

    let _ = outbox_dispatch(
        ctx,
        DISPATCHER_ID,
        &[target],
        &hb_bytes,
        "cast",
        HEARTBEAT_IDEM_KEY,
        "overwrite",
    );

    state.heartbeat_count += 1;

    json_bytes(json!({
        "success": true,
        "heartbeat_count": state.heartbeat_count,
        "seq": seq,
    }))
}

fn leader_scaling_benchmark(state: &mut LeaderState) -> Vec<u8> {
    if state.worker_ids.is_empty() {
        return json_bytes(json!({"error": "no workers — call init_workers first"}));
    }
    let ctx = r#"{"tenant":"default","namespace":"default"}"#;
    let payload_bytes = br#"{"task":"benchmark_job"}"#.to_vec();
    let scales: &[usize] = &[100, 500, 1000];
    let available = &state.worker_ids;
    let mut rows: Vec<Value> = Vec::new();
    let mut base_throughput = 0u64;

    for &n in scales {
        // Reuse existing worker pool (sliced) — no new shard groups to avoid actor accumulation
        let count = n.min(available.len());
        let targets = &available[..count];

        let t0 = now_ms();
        let (accepted, _rejected) = match outbox_dispatch(
            ctx,
            DISPATCHER_ID,
            targets,
            &payload_bytes,
            "cast",
            "",
            "",
        ) {
            Ok(s) => parse_dispatch_result(&s),
            Err(_) => (0, count as u64),
        };
        let wall_ms = now_ms().saturating_sub(t0).max(1);
        let throughput = accepted * 1000 / wall_ms;

        if rows.is_empty() {
            base_throughput = throughput.max(1);
        }
        let speedup_pct = throughput * 100 / base_throughput;

        rows.push(json!({
            "workers": count,
            "accepted": accepted,
            "wall_time_ms": wall_ms,
            "throughput": throughput,
            "speedup_pct": speedup_pct,
        }));
    }

    json_bytes(json!({
        "success": true,
        "scales": rows,
    }))
}

fn leader_get_metrics(state: &LeaderState) -> Vec<u8> {
    let throughput = if state.wall_time_ms > 0 {
        state.total_accepted * 1000 / state.wall_time_ms
    } else {
        0
    };
    let ask_rtt_avg = if state.ask_rtt_count > 0 {
        state.ask_rtt_sum_ms / state.ask_rtt_count
    } else {
        0
    };

    json_bytes(json!({
        "success": true,
        "total_dispatched": state.total_dispatched,
        "total_accepted": state.total_accepted,
        "total_rejected": state.total_rejected,
        "dispatch_count": state.dispatch_count,
        "wall_time_ms": state.wall_time_ms,
        "throughput_per_s": throughput,
        "heartbeat_count": state.heartbeat_count,
        "ask_rtt_min_ms": state.ask_rtt_min_ms,
        "ask_rtt_max_ms": state.ask_rtt_max_ms,
        "ask_rtt_avg_ms": ask_rtt_avg,
        "worker_count": state.worker_ids.len(),
        "shard_group_id": state.shard_group_id,
    }))
}

// ============================================================================
// Worker handlers
// ============================================================================

fn worker_process(state: &mut WorkerState) -> Vec<u8> {
    state.jobs_received += 1;
    json_bytes(json!({"status": "ok", "jobs_received": state.jobs_received}))
}

fn worker_upgrade(state: &WorkerState) -> Vec<u8> {
    json_bytes(json!({
        "success": true,
        "status": "need_package",
        "version": "2.0.0",
        "worker_id": state.actor_id,
    }))
}

fn worker_stats(state: &WorkerState) -> Vec<u8> {
    json_bytes(json!({
        "success": true,
        "jobs_received": state.jobs_received,
        "last_heartbeat_seq": state.last_heartbeat_seq,
    }))
}

fn worker_heartbeat(state: &mut WorkerState, v: &Value) -> Vec<u8> {
    let seq = v.get("seq").and_then(|x| x.as_i64()).unwrap_or(0);
    if seq > state.last_heartbeat_seq {
        state.last_heartbeat_seq = seq;
    }
    json_bytes(json!({"status": "ok"}))
}

// ============================================================================
// WIT Guest implementation
// ============================================================================

struct JobQueueOutboxActor;

impl Guest for JobQueueOutboxActor {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let v = parse_payload(&config);
        let actor_id = str_field(&v, "actor_id").to_string();
        let actor_type = str_field(&v, "actor_type").to_string();
        let role = str_field(&v, "role").to_string();

        let kind_str = if !actor_type.is_empty() { &actor_type } else { &role };

        with_state(|s| {
            *s = match kind_str.as_str() {
                "JobQueueOutboxCoordinator" | "JobQueueLeader" | "coordinator" | "leader" => {
                    ActorKind::Leader(LeaderState {
                        actor_id: actor_id.clone(),
                        ..Default::default()
                    })
                }
                "JobQueueWorker" | "OutboxWorker" | "worker" => {
                    ActorKind::Worker(WorkerState {
                        actor_id: actor_id.clone(),
                        ..Default::default()
                    })
                }
                _ => {
                    if actor_id.contains("coordinator") || actor_id.contains("leader") {
                        ActorKind::Leader(LeaderState {
                            actor_id: actor_id.clone(),
                            ..Default::default()
                        })
                    } else {
                        ActorKind::Worker(WorkerState {
                            actor_id: actor_id.clone(),
                            ..Default::default()
                        })
                    }
                }
            };
        });

        let _ = log("info", &format!("JobQueueOutbox initialized: type={kind_str} id={actor_id}"));
        Ok(())
    }

    fn handle(
        _from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let v = parse_payload(&payload);

        let result = with_state(|s| match s {
            ActorKind::Leader(state) => match msg_type.as_str() {
                "init_workers" => leader_init_workers(state, &v),
                "dispatch_jobs" => leader_dispatch_jobs(state, &v),
                "upgrade_workers" => leader_upgrade_workers(state, &v),
                "send_heartbeat" => leader_send_heartbeat(state, &v),
                "scaling_benchmark" => leader_scaling_benchmark(state),
                "get_metrics" => leader_get_metrics(state),
                // Legacy ops kept for backward compat
                "dispatch" => {
                    let targets: Vec<String> = v.get("targets")
                        .and_then(|a| a.as_array())
                        .map(|arr| arr.iter().filter_map(|i| i.as_str().map(str::to_string)).collect())
                        .unwrap_or_default();
                    if targets.is_empty() {
                        return json_err("targets required");
                    }
                    let inner = v.get("payload").cloned().unwrap_or(json!({"task": "process"}));
                    let ctx = r#"{"tenant":"default","namespace":"default"}"#;
                    let (accepted, rejected) = match outbox_dispatch(ctx, DISPATCHER_ID, &targets, &inner.to_string().as_bytes().to_vec(), "cast", "", "") {
                        Ok(r) => parse_dispatch_result(&r),
                        Err(_) => (0, targets.len() as u64),
                    };
                    state.total_dispatched += targets.len() as u64;
                    state.total_accepted += accepted;
                    state.total_rejected += rejected;
                    state.dispatch_count += 1;
                    json_bytes(json!({"success": true, "accepted": accepted, "rejected": rejected}))
                }
                "status" => {
                    json_bytes(json!({
                        "success": true,
                        "dispatcher_id": DISPATCHER_ID,
                        "total_dispatched": state.total_dispatched,
                    }))
                }
                "summary" => {
                    json_bytes(json!({
                        "success": true,
                        "total_dispatched": state.total_dispatched,
                        "total_accepted": state.total_accepted,
                        "total_rejected": state.total_rejected,
                        "dispatch_count": state.dispatch_count,
                    }))
                }
                _ => json_err(&format!("unknown leader op: {}", msg_type)),
            },
            ActorKind::Worker(state) => match msg_type.as_str() {
                "upgrade" => worker_upgrade(state),
                "stats" => worker_stats(state),
                "heartbeat" => worker_heartbeat(state, &v),
                _ => worker_process(state), // "process", "cast", "" or any cast → job
            },
            ActorKind::Uninitialized => json_err("actor not initialized"),
        });

        Ok(result)
    }

    fn get_state() -> Result<Vec<u8>, String> {
        Ok(with_state(|s| serde_json::to_vec(s).unwrap_or_default()))
    }

    fn set_state(state_bytes: Vec<u8>) -> Result<(), String> {
        if let Ok(loaded) = serde_json::from_slice::<ActorKind>(&state_bytes) {
            with_state(|s| *s = loaded);
        }
        Ok(())
    }
}

export!(JobQueueOutboxActor);
