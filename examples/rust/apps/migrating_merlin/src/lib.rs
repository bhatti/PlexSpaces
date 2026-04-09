// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Merlin → PlexSpaces: Parameter sweep (Rust WASM).
// SDK + WIT: `host::pool_*`, `host::ts_*`, `host::pg_*`, `host::send`, `host::application_metrics_add`.
// Coordinator: scatter tasks → pool checkout + `send(work_available)` or `pg_broadcast` → local drain → gather.
// Workers: `work_available` takes tasks from tuple space and writes results.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use prost::Message as ProstMessage;
use plexspaces_proto::application::v1::ApplicationMetrics;
use plexspaces_proto::tuplespace::v1::{
    tuple_field::Value as ProtoTupleValue, ReadRequest, ReadResponse, Tuple,
    TupleField as ProtoTupleField, WriteRequest,
};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;
use plexspaces_sdk::simple_actor::ActorWorldHandlers;
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
    let metrics_bytes = ApplicationMetrics {
        actor_counts: metrics.get("actor_counts").and_then(|value| value.as_object()).map(|entries| entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()).unwrap_or_default(),
        supervisor_count: metrics.get("supervisor_count").and_then(|value| value.as_u64()).unwrap_or(0) as u32,
        uptime_seconds: metrics.get("uptime_seconds").and_then(|value| value.as_u64()).unwrap_or(0),
        message_count: metrics.get("message_count").and_then(|value| value.as_u64()).unwrap_or(0),
        error_count: metrics.get("error_count").and_then(|value| value.as_u64()).unwrap_or(0),
        counter_metrics: metrics.get("counter_metrics").and_then(|value| value.as_object()).map(|entries| entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()).unwrap_or_default(),
        latency_totals_ms: metrics.get("latency_totals_ms").and_then(|value| value.as_object()).map(|entries| entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()).unwrap_or_default(),
        latency_max_ms: metrics.get("latency_max_ms").and_then(|value| value.as_object()).map(|entries| entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()).unwrap_or_default(),
        latency_samples: metrics.get("latency_samples").and_then(|value| value.as_object()).map(|entries| entries.iter().filter_map(|(key, value)| value.as_u64().map(|parsed| (key.clone(), parsed))).collect()).unwrap_or_default(),
    }.encode_to_vec();
    host::application_metrics_add(application_id, &metrics_bytes)
        .map(|_| ())
        .map_err(|err| format!("{context}: {err}"))
}

fn parse_payload(payload: &[u8]) -> Result<Value, String> {
    if payload.is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_slice(payload).map_err(|e| format!("invalid payload: {}", e))
}

fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
    let payload = parse_payload(payload)?;
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

fn json_bytes(value: Value) -> Vec<u8> {
    value.to_string().into_bytes()
}

fn json_error(err: impl Into<String>) -> Vec<u8> {
    json_bytes(serde_json::json!({ "error": err.into() }))
}

fn json_value_to_proto_tuple_field(
    value: &Value,
    allow_wildcard_string: bool,
) -> Result<ProtoTupleField, String> {
    let field = match value {
        Value::Null => ProtoTupleField {
            value: Some(ProtoTupleValue::Wildcard(true)),
        },
        Value::String(text) if allow_wildcard_string && text == "*" => ProtoTupleField {
            value: Some(ProtoTupleValue::Wildcard(true)),
        },
        Value::Bool(boolean) => ProtoTupleField {
            value: Some(ProtoTupleValue::Boolean(*boolean)),
        },
        Value::Number(number) => {
            if let Some(integer) = number.as_i64() {
                ProtoTupleField {
                    value: Some(ProtoTupleValue::Integer(integer)),
                }
            } else if let Some(float) = number.as_f64() {
                ProtoTupleField {
                    value: Some(ProtoTupleValue::Float(float)),
                }
            } else {
                return Err("unsupported numeric tuple field".to_string());
            }
        }
        Value::String(text) => ProtoTupleField {
            value: Some(ProtoTupleValue::String(text.clone())),
        },
        Value::Array(_) | Value::Object(_) => {
            return Err("tuplespace tuple fields must be scalar JSON values".to_string());
        }
    };
    Ok(field)
}

fn json_array_to_proto_tuple(values: &[Value], allow_wildcard_string: bool) -> Result<Tuple, String> {
    let mut fields = Vec::with_capacity(values.len());
    for value in values {
        fields.push(json_value_to_proto_tuple_field(value, allow_wildcard_string)?);
    }
    Ok(Tuple {
        id: String::new(),
        fields,
        timestamp: None,
        lease: None,
        metadata: Default::default(),
        location: None,
    })
}

fn proto_tuple_field_to_json(field: &ProtoTupleField) -> Value {
    match field.value.as_ref() {
        Some(ProtoTupleValue::Integer(value)) => serde_json::json!(value),
        Some(ProtoTupleValue::Float(value)) => serde_json::json!(value),
        Some(ProtoTupleValue::String(value)) => serde_json::json!(value),
        Some(ProtoTupleValue::Boolean(value)) => serde_json::json!(value),
        Some(ProtoTupleValue::Binary(value)) => serde_json::json!(String::from_utf8_lossy(value)),
        Some(ProtoTupleValue::Null(_)) | Some(ProtoTupleValue::Wildcard(_)) | None => Value::Null,
    }
}

fn proto_tuple_to_json(tuple: &Tuple) -> Value {
    Value::Array(tuple.fields.iter().map(proto_tuple_field_to_json).collect())
}

fn encode_write_request(tuple_value: &Value) -> Result<Vec<u8>, String> {
    let values = tuple_value
        .as_array()
        .ok_or_else(|| "tuple must be a JSON array".to_string())?;
    let request = WriteRequest {
        tuples: vec![json_array_to_proto_tuple(values, false)?],
        transaction_id: String::new(),
    };
    Ok(request.encode_to_vec())
}

fn encode_read_request(pattern_value: &Value, take: bool, max_results: i32) -> Result<Vec<u8>, String> {
    let values = pattern_value
        .as_array()
        .ok_or_else(|| "pattern must be a JSON array".to_string())?;
    let request = ReadRequest {
        template: Some(json_array_to_proto_tuple(values, true)?),
        timeout: None,
        blocking: false,
        take,
        max_results,
        transaction_id: String::new(),
        spatial_filter: None,
    };
    Ok(request.encode_to_vec())
}

fn decode_read_response(bytes: &[u8]) -> Result<Vec<Value>, String> {
    let response = ReadResponse::decode(bytes)
        .map_err(|err| format!("failed to decode tuplespace response: {}", err))?;
    Ok(response.tuples.iter().map(proto_tuple_to_json).collect())
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

        let num_params = payload
            .get("num_params")
            .and_then(|v| v.as_u64())
            .unwrap_or(10) as u32;
        if num_params == 0 {
            return finish_run(t0, 0.0, "no_params", state);
        }

        let param_base = payload
            .get("param_base")
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i64;

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
                param_val
            ]);
            let tuple_bytes = match encode_write_request(&tuple) {
                Ok(bytes) => bytes,
                Err(err_msg) => {
                    state.status = "error".to_string();
                    let err = serde_json::json!({
                        "status": "error",
                        "sweep_id": state.sweep_id,
                        "message": err_msg
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
            };
            let w = host::ts_write(&tuple_bytes);
            if let Err(err_msg) = w {
                state.status = "error".to_string();
                let err = serde_json::json!({
                    "status": "error",
                    "sweep_id": state.sweep_id,
                    "message": err_msg
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
            if let Ok(out) = host::pool_checkout(POOL_NAME, CHECKOUT_TIMEOUT_MS) {
                if out.is_empty() {
                    break;
                }
                if let Ok(handle) = serde_json::from_slice::<serde_json::Value>(&out) {
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
                        let payload_bytes = payload_send.to_string().into_bytes();
                        let s = host::send(&actor_id, "work_available", &payload_bytes);
                        if let Err(err) = s {
                            host::log("warn", &format!("send work_available: {}", err));
                        }
                    }
                }
            } else {
                break;
            }
        }

        if checkout_handles.is_empty() {
            let payload_broadcast = serde_json::json!({
                "sweep_id": state.sweep_id,
                "num_params": num_params
            });
            let payload_bytes = payload_broadcast.to_string().into_bytes();
            let b = host::pg_broadcast(WORKER_GROUP, "work_available", &payload_bytes);
            if let Err(err) = b {
                host::log("warn", &format!("pg_broadcast: {}", err));
            }
        }

        let task_pattern = serde_json::json!([
            TUPLE_PREFIX,
            state.sweep_id,
            "task",
            serde_json::Value::Null,
            serde_json::Value::Null
        ]);
        loop {
            let pattern_bytes = match encode_read_request(&task_pattern, true, 1) {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            let raw = match host::ts_take(&pattern_bytes) {
                Ok(raw) if !raw.is_empty() => raw,
                _ => break,
            };
            let tuple: Vec<serde_json::Value> = match decode_read_response(&raw) {
                Ok(mut tuples) => match tuples.pop() {
                    Some(Value::Array(fields)) => fields,
                    _ => break,
                },
                Err(_) => break,
            };
            let param_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += SIMULATION_MS as f64;
            let result = serde_json::json!([
                TUPLE_PREFIX,
                state.sweep_id,
                "result",
                param_id,
                SIMULATION_MS
            ]);
            let result_bytes = match encode_write_request(&result) {
                Ok(bytes) => bytes,
                Err(err) => {
                    host::log("warn", &format!("coordinator encode result tuple: {}", err));
                    break;
                }
            };
            let w = host::ts_write(&result_bytes);
            if let Err(err) = w {
                host::log("warn", &format!("coordinator ts_write result: {}", err));
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
        for _ in 0..GATHER_POLL_MAX {
            if state.cancel_requested {
                state.status = "cancelled".to_string();
                return finish_run(t0, compute_ms, "cancelled", state);
            }
            let pattern_bytes = match encode_read_request(&pattern, false, i32::MAX) {
                Ok(bytes) => bytes,
                Err(_) => Vec::new(),
            };
            let raw = host::ts_read_all(&pattern_bytes).unwrap_or_default();
            let results = decode_read_response(&raw).unwrap_or_default();
            state.num_completed = results.len() as u32;
            if state.num_completed >= num_params {
                break;
            }
            state.updated_at_ms = host::now_ms();
        }

        for (actor_id, checkout_id) in checkout_handles {
            let c = host::pool_checkin(POOL_NAME, &actor_id, &checkout_id, true);
            if let Err(err) = c {
                host::log("warn", &format!("pool_checkin: {}", err));
            }
        }

        state.status = "completed".to_string();
        state.updated_at_ms = host::now_ms();
        finish_run(t0, compute_ms, "completed", state)
    });

    if let Err(err) =
        merge_application_metrics_for(&application_id, metrics, "sweep workflow_run metrics")
    {
        return serde_json::json!({ "error": err }).to_string();
    }
    body
}

fn run_worker(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let (body, metrics) = with_state(|state| {
        if !state.worker_joined {
            let j = host::pg_join(WORKER_GROUP);
            if let Err(err) = j {
                host::log("warn", &format!("pg_join: {}", err));
            }
            state.worker_joined = true;
            host::log("info", &format!("Joined process group {}", WORKER_GROUP));
        }
        let sweep_id = payload
            .get("sweep_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let num_params = payload
            .get("num_params")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
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
        let mut processed = 0u32;
        let mut compute_ms = 0.0f64;
        loop {
            let pattern_bytes = match encode_read_request(&pattern, true, 1) {
                Ok(bytes) => bytes,
                Err(_) => break,
            };
            let raw = match host::ts_take(&pattern_bytes) {
                Ok(raw) if !raw.is_empty() => raw,
                _ => break,
            };
            let tuple: Vec<serde_json::Value> = match decode_read_response(&raw) {
                Ok(mut tuples) => match tuples.pop() {
                    Some(Value::Array(fields)) => fields,
                    _ => break,
                },
                Err(_) => break,
            };
            let param_id = tuple.get(3).cloned().unwrap_or(serde_json::Value::Null);
            compute_ms += SIMULATION_MS as f64;
            let result = serde_json::json!([
                TUPLE_PREFIX,
                sweep_id,
                "result",
                param_id,
                SIMULATION_MS
            ]);
            let result_bytes = match encode_write_request(&result) {
                Ok(bytes) => bytes,
                Err(err) => {
                    host::log("warn", &format!("worker encode result tuple: {}", err));
                    break;
                }
            };
            let w = host::ts_write(&result_bytes);
            if let Err(err) = w {
                host::log("warn", &format!("worker ts_write: {}", err));
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

    if let Err(err) =
        merge_application_metrics_for(&application_id, metrics, "sweep work_available metrics")
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
    fn configure(&mut self, config: &[u8]) -> Result<(), String> {
        let v = parse_payload(config)?;
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
    fn workflow_run(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload = parse_payload(payload)?;
        Ok(json_bytes(
            serde_json::from_str(&run_coordinator(&payload)).unwrap_or_else(
                |_| serde_json::json!({ "error": "invalid workflow_run response" }),
            ),
        ))
    }

    #[handler("workflow_signal:cancel")]
    fn workflow_signal_cancel(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
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
        Ok(json_bytes(serde_json::json!({})))
    }

    #[handler("workflow_query:status")]
    fn workflow_query_status(
        &mut self,
        _from_actor: &str,
        _payload: &[u8],
    ) -> Result<Vec<u8>, String> {
        Ok(json_bytes(
            serde_json::from_str(&query_status_json()).unwrap_or_else(
                |_| serde_json::json!({ "error": "invalid workflow_query response" }),
            ),
        ))
    }

    #[handler("work_available")]
    fn work_available(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload = parse_payload(payload).unwrap_or_else(|_| serde_json::json!({}));
        Ok(json_bytes(
            serde_json::from_str(&run_worker(&payload)).unwrap_or_else(
                |_| serde_json::json!({ "error": "invalid work_available response" }),
            ),
        ))
    }
}

struct SweepBridge;

impl Guest for SweepBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let mut actor = SweepActor::default();
        ActorWorldHandlers::init(&mut actor, &config)
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let op = match parse_op(&msg_type, &payload) {
            Ok(op) => op,
            Err(err) => return Ok(json_error(err)),
        };
        let mut actor = SweepActor::default();
        Ok(actor
            .handle_operation(&from_actor, &op, &payload)
            .unwrap_or_else(json_error))
    }

    fn get_state() -> Result<Vec<u8>, String> {
        with_state(|state| {
            serde_json::to_vec(state).map_err(|err| format!("state encode failed: {err}"))
        })
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        if state.is_empty() {
            return Ok(());
        }
        match serde_json::from_slice::<SweepState>(&state) {
            Ok(next) => {
                with_state(|state| {
                    *state = next;
                });
                Ok(())
            }
            Err(err) => Err(format!("invalid state JSON: {}", err)),
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
            parse_op(
                "call",
                br#"{"op":"workflow_run","sweep_id":"s1","num_params":3}"#
            )
            .expect("op"),
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
