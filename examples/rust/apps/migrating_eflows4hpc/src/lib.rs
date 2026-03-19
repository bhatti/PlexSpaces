// SPDX-License-Identifier: LGPL-2.1-or-later
//
// EFlows4HPC → PlexSpaces: HPC ensemble coordinator + workers (Rust WASM).
// SDK + WIT: `host::ts_*`, `host::pg_*`, `host::now_ms`, `host::log`, `host::application_metrics_add`.
// Metrics merges use `application_id` resolved **before** the state mutex (wasm32-safe).

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

const TASK_MS: u64 = 18;
const RESULT_PREFIX: &str = "ensemble";
const WORKER_GROUP: &str = "ensemble-workers";
const GATHER_POLL_MAX: usize = 200;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct EnsembleState {
    application_id: String,
    ensemble_id: String,
    num_tasks: u32,
    num_completed: u32,
    status: String,
    total_compute_ms: f64,
    total_coord_ms: f64,
    created_at_ms: u64,
    updated_at_ms: u64,
    cancel_requested: bool,
    worker_joined: bool,
}

impl EnsembleState {
    fn new() -> Self {
        Self {
            status: "idle".to_string(),
            ..Default::default()
        }
    }
}

fn state_cell() -> &'static Mutex<EnsembleState> {
    static STATE: OnceLock<Mutex<EnsembleState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(EnsembleState::new()))
}

fn with_state<T>(f: impl FnOnce(&mut EnsembleState) -> T) -> T {
    let mut g = state_cell().lock().expect("ensemble state lock poisoned");
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
        .expect("ensemble state lock poisoned")
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
    state: &mut EnsembleState,
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
    let body = serde_json::json!({
        "status": status,
        "ensemble_id": state.ensemble_id,
        "num_tasks": state.num_tasks,
        "num_completed": state.num_completed,
        "total_compute_ms": state.total_compute_ms,
        "total_coord_ms": state.total_coord_ms
    })
    .to_string();
    let metrics = serde_json::json!({
        "message_count": 1,
        "counter_metrics": {
            "ensemble_workflow_finishes": 1,
        },
        "latency_totals_ms": {
            "ensemble.compute": compute_ms as u64,
            "ensemble.coordination": coord_ms as u64,
        },
        "latency_max_ms": {
            "ensemble.compute": compute_ms as u64,
            "ensemble.coordination": coord_ms as u64,
        },
        "latency_samples": {
            "ensemble.compute": 1,
            "ensemble.coordination": 1,
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
        state.ensemble_id = payload
            .get("ensemble_id")
            .or_else(|| payload.get("job_id"))
            .and_then(|v| v.as_str())
            .unwrap_or(&state.ensemble_id)
            .to_string();
        state.updated_at_ms = host::now_ms();

        if state.cancel_requested {
            state.status = "cancelled".to_string();
            return finish_run(t0, 0.0, "cancelled", state);
        }
        if state.status == "completed" {
            return finish_run(t0, 0.0, "completed", state);
        }

        let num_tasks = payload.get("num_tasks").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
        if num_tasks == 0 {
            return finish_run(t0, 0.0, "no_tasks", state);
        }

        state.num_tasks = num_tasks;
        state.num_completed = 0;
        state.status = "scattering".to_string();
        let mut compute_ms = 0.0f64;

        for i in 0..num_tasks {
            let task_id = format!("t{}", i);
            let tuple = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "task", task_id, i]);
            let w = host::ts_write(&tuple.to_string());
            if w.starts_with("ERROR") {
                state.status = "error".to_string();
                let err = serde_json::json!({
                    "status": "error",
                    "ensemble_id": state.ensemble_id,
                    "message": w
                })
                .to_string();
                return (
                    err,
                    serde_json::json!({
                        "message_count": 1,
                        "counter_metrics": { "ensemble_ts_write_errors": 1 },
                    }),
                );
            }
            compute_ms += 2.0;
        }
        state.updated_at_ms = host::now_ms();
        state.status = "running".to_string();

        let payload_broadcast = serde_json::json!({
            "ensemble_id": state.ensemble_id,
            "num_tasks": num_tasks
        });
        let b = host::pg_broadcast(
            WORKER_GROUP,
            "tasks_ready",
            &payload_broadcast.to_string(),
        );
        if b.starts_with("ERROR") {
            host::log("warn", &format!("pg_broadcast: {}", b));
        }

        let task_pattern = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "task", serde_json::Value::Null, serde_json::Value::Null]);
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
            let task_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += TASK_MS as f64;
            let result = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "result", task_id, { "ok": true, "ms": TASK_MS }]);
            let w = host::ts_write(&result.to_string());
            if w.starts_with("ERROR") {
                host::log("warn", &format!("ts_write result: {}", w));
                break;
            }
        }

        let pattern = serde_json::json!([RESULT_PREFIX, state.ensemble_id, "result", serde_json::Value::Null, serde_json::Value::Null]);
        let pattern_str = pattern.to_string();
        for _ in 0..GATHER_POLL_MAX {
            if state.cancel_requested {
                state.status = "cancelled".to_string();
                return finish_run(t0, compute_ms, "cancelled", state);
            }
            let raw = host::ts_read_all(&pattern_str);
            let results: Vec<serde_json::Value> = serde_json::from_str(&raw).unwrap_or_default();
            state.num_completed = results.len() as u32;
            if state.num_completed >= num_tasks {
                break;
            }
            state.updated_at_ms = host::now_ms();
        }

        state.status = "completed".to_string();
        state.updated_at_ms = host::now_ms();
        finish_run(t0, compute_ms, "completed", state)
    });

    if let Err(err) = merge_application_metrics_for(&application_id, metrics, "ensemble workflow_run metrics") {
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
        let ensemble_id = payload
            .get("ensemble_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let num_tasks = payload.get("num_tasks").and_then(|v| v.as_u64()).unwrap_or(0);
        if ensemble_id.is_empty() || num_tasks == 0 {
            return (
                serde_json::json!({ "ok": true, "message": "warmup" }).to_string(),
                serde_json::json!({
                    "message_count": 1,
                    "counter_metrics": { "ensemble_worker_warmups": 1 },
                }),
            );
        }

        let t0 = host::now_ms();
        let pattern = serde_json::json!([RESULT_PREFIX, ensemble_id, "task", serde_json::Value::Null, serde_json::Value::Null]);
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
            let task_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += TASK_MS as f64;
            let result = serde_json::json!([RESULT_PREFIX, ensemble_id, "result", task_id, { "ok": true, "ms": TASK_MS }]);
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
            "ensemble_id": ensemble_id
        })
        .to_string();
        let coord_part = (elapsed - compute_ms).max(0.0);
        let metrics = serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "ensemble_worker_batches": 1,
                "ensemble_worker_tasks_processed": processed,
            },
            "latency_totals_ms": {
                "ensemble.worker.compute": compute_ms as u64,
                "ensemble.worker.coordination": coord_part as u64,
            },
            "latency_max_ms": {
                "ensemble.worker.compute": compute_ms as u64,
                "ensemble.worker.coordination": coord_part as u64,
            },
            "latency_samples": {
                "ensemble.worker.compute": 1,
                "ensemble.worker.coordination": 1,
            },
        });
        (body, metrics)
    });

    if let Err(err) = merge_application_metrics_for(&application_id, metrics, "ensemble tasks_ready metrics") {
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
            "counter_metrics": { "ensemble_status_queries": 1 },
        }),
        "ensemble workflow_query metrics",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    with_state(|state| {
        serde_json::json!({
            "ensemble_id": state.ensemble_id,
            "status": state.status,
            "num_tasks": state.num_tasks,
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
struct EnsembleActor;

#[plexspaces_handlers(wasm)]
impl EnsembleActor {
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
                "counter_metrics": { "ensemble_cancels": 1 },
            }),
            "ensemble cancel signal metrics",
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

    #[handler("tasks_ready")]
    fn tasks_ready(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).unwrap_or_else(|_| serde_json::json!({}));
        Ok(run_worker(&payload))
    }
}

struct EnsembleBridge;

impl Guest for EnsembleBridge {
    fn init(config_json: String) -> String {
        let mut actor = EnsembleActor::default();
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
        let mut actor = EnsembleActor::default();
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
        match serde_json::from_str::<EnsembleState>(&state_json) {
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

export!(EnsembleBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_workflow_run() {
        assert_eq!(
            parse_op("call", r#"{"op":"workflow_run","ensemble_id":"e1","num_tasks":3}"#)
                .expect("op"),
            "workflow_run"
        );
    }

    #[test]
    fn actor_application_id_extracts_namespace() {
        assert_eq!(
            actor_application_id("x//ensemble::migrating-eflows4hpc-ensemble@node"),
            "migrating-eflows4hpc-ensemble"
        );
    }
}
