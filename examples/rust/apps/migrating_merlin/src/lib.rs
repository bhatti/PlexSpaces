// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Merlin → PlexSpaces: Parameter sweep (Rust WASM).
// SDK + WIT: `host::pool_*`, `host::ts_*`, `host::pg_*`, `host::send`, `host::application_metrics_add`.
// Coordinator: scatter tasks → pool checkout + `send(work_available)` or `pg_broadcast` → local drain → gather.
// Workers: `work_available` takes tasks from tuple space and writes results.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-simple-actor",
    world: "actor-world",
});

use exports::plexspaces::simple_actor::actor::Guest;
use plexspaces::simple_actor::host;
use plexspaces_sdk::simple_actor::SimpleActorHandlers;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

const SIMULATION_MS: u64 = 22;
const TUPLE_PREFIX: &str = "merlin";
const WORKER_GROUP: &str = "merlin-workers";
const POOL_NAME: &str = "merlin-workers";
const MAX_CHECKOUT_WORKERS: usize = 10;
const GATHER_POLL_MAX: usize = 200;
const CHECKOUT_TIMEOUT_MS: u64 = 5000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SweepState {
    application_id: String,
    sweep_id: String,
    num_params: u32,
    num_completed: u32,
    status: String,
    total_compute_ms: f64,
    total_coord_ms: f64,
    created_at_ms: u64,
    updated_at_ms: u64,
    cancel_requested: bool,
    worker_joined: bool,
}

impl SweepState {
    fn new() -> Self {
        Self {
            status: "idle".to_string(),
            ..Default::default()
        }
    }
}

fn state_cell() -> &'static Mutex<SweepState> {
    static STATE: OnceLock<Mutex<SweepState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SweepState::new()))
}

fn with_state<T>(f: impl FnOnce(&mut SweepState) -> T) -> T {
    let mut g = state_cell().lock().expect("sweep state lock poisoned");
    f(&mut *g)
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

fn resolve_application_id() -> String {
    let id = state_cell()
        .lock()
        .expect("sweep state lock poisoned")
        .application_id
        .clone();
    if !id.is_empty() {
        return id;
    }
    actor_application_id(&host::self_id())
}

fn merge_application_metrics_for(
    application_id: &str,
    metrics: serde_json::Value,
    context: &str,
) -> Result<(), String> {
    let response = host::application_metrics_add(application_id, &metrics.to_string());
    if response.starts_with("ERROR:") {
        Err(format!("{}: {}", context, response))
    } else {
        Ok(())
    }
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

fn finish_run(
    t0: u64,
    compute_ms: f64,
    status: &str,
    state: &mut SweepState,
) -> (String, serde_json::Value) {
    let elapsed = (host::now_ms().saturating_sub(t0)) as f64;
    let elapsed = if elapsed < compute_ms {
        compute_ms
    } else {
        elapsed
    };
    let coord_ms = (elapsed - compute_ms).max(0.0);
    state.total_compute_ms += compute_ms;
    state.total_coord_ms += coord_ms;
    let approx_bytes = (state.num_params as i64 * 100) + (state.num_completed as i64 * 80);
    let body = serde_json::json!({
        "status": status,
        "sweep_id": state.sweep_id,
        "num_params": state.num_params,
        "num_completed": state.num_completed,
        "data_size_bytes": approx_bytes,
        "total_compute_ms": state.total_compute_ms,
        "total_coord_ms": state.total_coord_ms
    })
    .to_string();
    let metrics = serde_json::json!({
        "message_count": 1,
        "counter_metrics": {
            "sweep_workflow_finishes": 1,
        },
        "latency_totals_ms": {
            "sweep.compute": compute_ms as u64,
            "sweep.coordination": coord_ms as u64,
        },
        "latency_max_ms": {
            "sweep.compute": compute_ms as u64,
            "sweep.coordination": coord_ms as u64,
        },
        "latency_samples": {
            "sweep.compute": 1,
            "sweep.coordination": 1,
        },
    });
    (body, metrics)
}

fn run_coordinator(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let (body, metrics) = with_state(|state| {
        let t0 = host::now_ms();
        if state.created_at_ms == 0 {
            state.created_at_ms = t0;
        }
        state.sweep_id = payload
            .get("sweep_id")
            .or_else(|| payload.get("job_id"))
            .and_then(|v| v.as_str())
            .unwrap_or(&state.sweep_id)
            .to_string();
        state.updated_at_ms = host::now_ms();

        if state.cancel_requested {
            state.status = "cancelled".to_string();
            return finish_run(t0, 0.0, "cancelled", state);
        }
        if state.status == "completed" {
            return finish_run(t0, 0.0, "completed", state);
        }

        let num_params = payload.get("num_params").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
        if num_params == 0 {
            return finish_run(t0, 0.0, "no_params", state);
        }

        let param_base = payload.get("param_base").and_then(|v| v.as_i64()).unwrap_or(0) as i64;

        state.num_params = num_params;
        state.num_completed = 0;
        state.status = "scattering".to_string();
        let mut compute_ms = 0.0f64;

        for i in 0..num_params {
            let param_val = param_base + i as i64;
            let tuple = serde_json::json!([
                TUPLE_PREFIX,
                state.sweep_id,
                "task",
                format!("p{}", i),
                { "param": param_val }
            ]);
            let w = host::ts_write(&tuple.to_string());
            if w.starts_with("ERROR") {
                state.status = "error".to_string();
                let err = serde_json::json!({
                    "status": "error",
                    "sweep_id": state.sweep_id,
                    "message": w
                })
                .to_string();
                return (
                    err,
                    serde_json::json!({
                        "message_count": 1,
                        "counter_metrics": { "sweep_ts_write_errors": 1 },
                    }),
                );
            }
            compute_ms += 2.0;
        }
        state.updated_at_ms = host::now_ms();
        state.status = "running".to_string();

        let mut checkout_handles: Vec<(String, String)> = Vec::new();
        let n_checkout = MAX_CHECKOUT_WORKERS.min(num_params as usize);
        for _ in 0..n_checkout {
            let out = host::pool_checkout(POOL_NAME, CHECKOUT_TIMEOUT_MS);
            if out.is_empty() || out.starts_with("ERROR") {
                break;
            }
            if let Ok(handle) = serde_json::from_str::<serde_json::Value>(&out) {
                let actor_id = handle
                    .get("actor_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let checkout_id = handle
                    .get("checkout_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !actor_id.is_empty() && !checkout_id.is_empty() {
                    checkout_handles.push((actor_id.clone(), checkout_id));
                    let payload_send = serde_json::json!({
                        "sweep_id": state.sweep_id,
                        "num_params": num_params
                    });
                    let s = host::send(
                        &actor_id,
                        "work_available",
                        &payload_send.to_string(),
                    );
                    if s.starts_with("ERROR") {
                        host::log("warn", &format!("send work_available: {}", s));
                    }
                }
            }
        }

        if checkout_handles.is_empty() {
            let payload_broadcast = serde_json::json!({
                "sweep_id": state.sweep_id,
                "num_params": num_params
            });
            let b = host::pg_broadcast(
                WORKER_GROUP,
                "work_available",
                &payload_broadcast.to_string(),
            );
            if b.starts_with("ERROR") {
                host::log("warn", &format!("pg_broadcast: {}", b));
            }
        }

        let task_pattern = serde_json::json!([
            TUPLE_PREFIX,
            state.sweep_id,
            "task",
            serde_json::Value::Null,
            serde_json::Value::Null
        ]);
        let task_pattern_str = task_pattern.to_string();
        loop {
            let raw = host::ts_take(&task_pattern_str);
            if raw.is_empty() || raw.starts_with("ERROR") {
                break;
            }
            let tuple: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
                Ok(t) => t,
                Err(_) => break,
            };
            let param_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += SIMULATION_MS as f64;
            let result = serde_json::json!([
                TUPLE_PREFIX,
                state.sweep_id,
                "result",
                param_id,
                { "ok": true, "ms": SIMULATION_MS }
            ]);
            let w = host::ts_write(&result.to_string());
            if w.starts_with("ERROR") {
                host::log("warn", &format!("coordinator ts_write result: {}", w));
                break;
            }
        }

        let pattern = serde_json::json!([
            TUPLE_PREFIX,
            state.sweep_id,
            "result",
            serde_json::Value::Null,
            serde_json::Value::Null
        ]);
        let pattern_str = pattern.to_string();
        for _ in 0..GATHER_POLL_MAX {
            if state.cancel_requested {
                state.status = "cancelled".to_string();
                return finish_run(t0, compute_ms, "cancelled", state);
            }
            let raw = host::ts_read_all(&pattern_str);
            let results: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
            state.num_completed = results.len() as u32;
            if state.num_completed >= num_params {
                break;
            }
            state.updated_at_ms = host::now_ms();
        }

        for (actor_id, checkout_id) in checkout_handles {
            let c = host::pool_checkin(POOL_NAME, &actor_id, &checkout_id, true);
            if c.starts_with("ERROR") {
                host::log("warn", &format!("pool_checkin: {}", c));
            }
        }

        state.status = "completed".to_string();
        state.updated_at_ms = host::now_ms();
        finish_run(t0, compute_ms, "completed", state)
    });

    if let Err(err) = merge_application_metrics_for(&application_id, metrics, "sweep workflow_run metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }
    body
}

fn run_worker(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let (body, metrics) = with_state(|state| {
        if !state.worker_joined {
            let j = host::pg_join(WORKER_GROUP);
            if j.starts_with("ERROR") {
                host::log("warn", &format!("pg_join: {}", j));
            }
            state.worker_joined = true;
            host::log(
                "info",
                &format!("Joined process group {}", WORKER_GROUP),
            );
        }
        let sweep_id = payload
            .get("sweep_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let num_params = payload.get("num_params").and_then(|v| v.as_u64()).unwrap_or(0);
        if sweep_id.is_empty() || num_params == 0 {
            return (
                serde_json::json!({ "ok": true, "message": "warmup" }).to_string(),
                serde_json::json!({
                    "message_count": 1,
                    "counter_metrics": { "sweep_worker_warmups": 1 },
                }),
            );
        }

        let t0 = host::now_ms();
        let pattern = serde_json::json!([
            TUPLE_PREFIX,
            sweep_id,
            "task",
            serde_json::Value::Null,
            serde_json::Value::Null
        ]);
        let pattern_str = pattern.to_string();
        let mut processed = 0u32;
        let mut compute_ms = 0.0f64;
        loop {
            let raw = host::ts_take(&pattern_str);
            if raw.is_empty() || raw.starts_with("ERROR") {
                break;
            }
            let tuple: Vec<serde_json::Value> = match serde_json::from_str(&raw) {
                Ok(t) => t,
                Err(_) => break,
            };
            let param_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += SIMULATION_MS as f64;
            let result = serde_json::json!([
                TUPLE_PREFIX,
                sweep_id,
                "result",
                param_id,
                { "ok": true, "ms": SIMULATION_MS }
            ]);
            let w = host::ts_write(&result.to_string());
            if w.starts_with("ERROR") {
                host::log("warn", &format!("worker ts_write: {}", w));
                break;
            }
            processed += 1;
        }
        let elapsed = (host::now_ms().saturating_sub(t0)) as f64;
        state.total_compute_ms += compute_ms;
        state.total_coord_ms += (elapsed - compute_ms).max(0.0);
        let body = serde_json::json!({
            "ok": true,
            "processed": processed,
            "sweep_id": sweep_id
        })
        .to_string();
        let coord_part = (elapsed - compute_ms).max(0.0);
        let metrics = serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "sweep_worker_batches": 1,
                "sweep_worker_tasks_processed": processed,
            },
            "latency_totals_ms": {
                "sweep.worker.compute": compute_ms as u64,
                "sweep.worker.coordination": coord_part as u64,
            },
            "latency_max_ms": {
                "sweep.worker.compute": compute_ms as u64,
                "sweep.worker.coordination": coord_part as u64,
            },
            "latency_samples": {
                "sweep.worker.compute": 1,
                "sweep.worker.coordination": 1,
            },
        });
        (body, metrics)
    });

    if let Err(err) = merge_application_metrics_for(&application_id, metrics, "sweep work_available metrics")
    {
        return serde_json::json!({ "error": err }).to_string();
    }
    body
}

fn query_status_json() -> String {
    let application_id = resolve_application_id();
    if let Err(err) = merge_application_metrics_for(
        &application_id,
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": { "sweep_status_queries": 1 },
        }),
        "sweep workflow_query metrics",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    with_state(|state| {
        serde_json::json!({
            "sweep_id": state.sweep_id,
            "status": state.status,
            "num_params": state.num_params,
            "num_completed": state.num_completed,
            "cancel_requested": state.cancel_requested,
            "total_compute_ms": state.total_compute_ms,
            "total_coord_ms": state.total_coord_ms,
            "created_at_ms": state.created_at_ms,
            "updated_at_ms": state.updated_at_ms
        })
        .to_string()
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct SweepActor;

#[plexspaces_handlers(wasm)]
impl SweepActor {
    #[init_handler]
    fn configure(&mut self, config_json: &str) -> Result<(), String> {
        let v: serde_json::Value =
            serde_json::from_str(config_json).map_err(|e| format!("invalid init JSON: {}", e))?;
        with_state(|state| {
            let actor_id = v.get("actor_id").and_then(|x| x.as_str()).unwrap_or("");
            state.application_id = if actor_id.is_empty() {
                actor_application_id(&host::self_id())
            } else {
                actor_application_id(actor_id)
            };
        });
        Ok(())
    }

    #[handler("workflow_run")]
    fn workflow_run(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(run_coordinator(&payload))
    }

    #[handler("workflow_signal:cancel")]
    fn workflow_signal_cancel(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        let application_id = resolve_application_id();
        with_state(|state| {
            state.cancel_requested = true;
            state.updated_at_ms = host::now_ms();
        });
        merge_application_metrics_for(
            &application_id,
            serde_json::json!({
                "message_count": 1,
                "counter_metrics": { "sweep_cancels": 1 },
            }),
            "sweep cancel signal metrics",
        )?;
        Ok("{}".to_string())
    }

    #[handler("workflow_query:status")]
    fn workflow_query_status(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(query_status_json())
    }

    #[handler("work_available")]
    fn work_available(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).unwrap_or_else(|_| serde_json::json!({}));
        Ok(run_worker(&payload))
    }
}

struct SweepBridge;

impl Guest for SweepBridge {
    fn init(config_json: String) -> String {
        let mut actor = SweepActor::default();
        match SimpleActorHandlers::init(&mut actor, &config_json) {
            Ok(()) => String::new(),
            Err(err) => err,
        }
    }

    fn handle(from_actor: String, msg_type: String, payload_json: String) -> String {
        let op = match parse_op(&msg_type, &payload_json) {
            Ok(op) => op,
            Err(err) => return serde_json::json!({ "error": err }).to_string(),
        };
        let mut actor = SweepActor::default();
        actor
            .handle_operation(&from_actor, &op, &payload_json)
            .unwrap_or_else(|err| serde_json::json!({ "error": err }).to_string())
    }

    fn get_state() -> String {
        with_state(|state| serde_json::to_string(state).unwrap_or_else(|_| "{}".to_string()))
    }

    fn set_state(state_json: String) -> String {
        if state_json.is_empty() {
            return String::new();
        }
        match serde_json::from_str::<SweepState>(&state_json) {
            Ok(next) => {
                with_state(|state| {
                    *state = next;
                });
                String::new()
            }
            Err(err) => format!("ERROR: invalid state JSON: {}", err),
        }
    }
}

export!(SweepBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_workflow_run() {
        assert_eq!(
            parse_op("call", r#"{"op":"workflow_run","sweep_id":"s1","num_params":3}"#).expect("op"),
            "workflow_run"
        );
    }

    #[test]
    fn actor_application_id_extracts_namespace() {
        assert_eq!(
            actor_application_id("x//sweep::migrating-merlin-sweep-rust@node"),
            "migrating-merlin-sweep-rust"
        );
    }
}
