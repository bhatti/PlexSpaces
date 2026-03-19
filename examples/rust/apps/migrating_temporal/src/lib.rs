// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Order fulfillment workflow — Rust WASM (Temporal-style run / signal / query).
// SDK macros + WIT: same pattern as data_parallel_worker (gen_server_actor(wasm), plexspaces_handlers(wasm)).
//
// Important (wasm32): `std::sync::Mutex` is non-reentrant. Never call `host::application_metrics_add`
// (or anything that resolves `application_id` via the same mutex) while holding the state lock.

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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrderStep {
    name: String,
    #[serde(rename = "completed_at_ms")]
    completed_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct OrderState {
    application_id: String,
    order_id: String,
    customer_id: String,
    status: String,
    steps: Vec<OrderStep>,
    cancel_requested: bool,
    total_compute_ms: f64,
    total_coord_ms: f64,
    created_at_ms: u64,
    updated_at_ms: u64,
}

impl OrderState {
    fn new() -> Self {
        Self {
            status: "pending".to_string(),
            ..Default::default()
        }
    }
}

fn state_cell() -> &'static Mutex<OrderState> {
    static STATE: OnceLock<Mutex<OrderState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(OrderState::new()))
}

fn with_state<T>(f: impl FnOnce(&mut OrderState) -> T) -> T {
    let mut guard = state_cell()
        .lock()
        .expect("order state lock poisoned");
    f(&mut *guard)
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

/// Resolve application id for metrics. Do **not** call while holding `state_cell()` — it locks the same mutex.
fn resolve_application_id() -> String {
    let id = state_cell()
        .lock()
        .expect("order state lock poisoned")
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

fn has_step(steps: &[OrderStep], name: &str) -> bool {
    steps.iter().any(|s| s.name == name)
}

/// Build cancelled response JSON and metrics delta — no host calls; caller merges metrics **after** dropping state lock.
fn compensate_body(state: &mut OrderState, reason: &str) -> (String, serde_json::Value) {
    state.status = "cancelled".to_string();
    state.updated_at_ms = host::now_ms();
    let body = serde_json::json!({
        "status": "cancelled",
        "order_id": state.order_id,
        "reason": reason,
        "steps_rolled_back": state.steps.len()
    })
    .to_string();
    let metrics = serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "workflow_compensations": 1 },
    });
    (body, metrics)
}

enum RunWorkflowPhase {
    Early(String),
    WithHostMetrics {
        response: String,
        metrics: serde_json::Value,
        context: &'static str,
    },
}

fn run_workflow(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let phase = with_state(|state| {
        let t0 = host::now_ms();
        if state.created_at_ms == 0 {
            state.created_at_ms = t0;
        }
        state.order_id = payload
            .get("order_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&state.order_id)
            .to_string();
        state.customer_id = payload
            .get("customer_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&state.customer_id)
            .to_string();
        state.updated_at_ms = host::now_ms();

        if state.status != "pending" && state.status != "validated" {
            return RunWorkflowPhase::Early(
                serde_json::json!({
                    "status": state.status,
                    "order_id": state.order_id,
                    "message": "Workflow already completed or cancelled"
                })
                .to_string(),
            );
        }

        let mut compute_ms = 0.0f64;

        if !has_step(&state.steps, "validate") {
            compute_ms += 8.0;
            state.steps.push(OrderStep {
                name: "validate".to_string(),
                completed_at_ms: host::now_ms(),
            });
            state.status = "validated".to_string();
            state.updated_at_ms = host::now_ms();
        }
        if state.cancel_requested {
            let (body, metrics) = compensate_body(state, "cancel_requested");
            return RunWorkflowPhase::WithHostMetrics {
                response: body,
                metrics,
                context: "workflow compensate metrics",
            };
        }

        if !has_step(&state.steps, "reserve_inventory") {
            compute_ms += 12.0;
            state.steps.push(OrderStep {
                name: "reserve_inventory".to_string(),
                completed_at_ms: host::now_ms(),
            });
            state.status = "inventory_reserved".to_string();
            state.updated_at_ms = host::now_ms();
        }
        if state.cancel_requested {
            let (body, metrics) = compensate_body(state, "cancel_requested");
            return RunWorkflowPhase::WithHostMetrics {
                response: body,
                metrics,
                context: "workflow compensate metrics",
            };
        }

        if !has_step(&state.steps, "charge_payment") {
            compute_ms += 10.0;
            state.steps.push(OrderStep {
                name: "charge_payment".to_string(),
                completed_at_ms: host::now_ms(),
            });
            state.status = "payment_charged".to_string();
            state.updated_at_ms = host::now_ms();
        }
        if state.cancel_requested {
            let (body, metrics) = compensate_body(state, "cancel_requested");
            return RunWorkflowPhase::WithHostMetrics {
                response: body,
                metrics,
                context: "workflow compensate metrics",
            };
        }

        if !has_step(&state.steps, "ship") {
            compute_ms += 15.0;
            state.steps.push(OrderStep {
                name: "ship".to_string(),
                completed_at_ms: host::now_ms(),
            });
            state.status = "shipped".to_string();
            state.updated_at_ms = host::now_ms();
        }

        let total_elapsed = (host::now_ms().saturating_sub(t0)) as f64;
        let compute_reported = compute_ms.min(total_elapsed);
        let coord_reported = (total_elapsed - compute_reported).max(0.0);
        state.total_compute_ms += compute_reported;
        state.total_coord_ms += coord_reported;

        let metrics = serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "workflow_runs": 1,
                "workflow_steps_completed": state.steps.len() as u64,
            },
            "latency_totals_ms": {
                "workflow.compute": compute_reported as u64,
                "workflow.coordination": coord_reported as u64,
            },
            "latency_max_ms": {
                "workflow.compute": compute_reported as u64,
                "workflow.coordination": coord_reported as u64,
            },
            "latency_samples": {
                "workflow.compute": 1,
                "workflow.coordination": 1,
            },
        });

        let response = serde_json::json!({
            "status": state.status,
            "order_id": state.order_id,
            "steps_completed": state.steps.len(),
            "total_compute_ms": state.total_compute_ms,
            "total_coord_ms": state.total_coord_ms
        })
        .to_string();

        RunWorkflowPhase::WithHostMetrics {
            response,
            metrics,
            context: "workflow_run metrics",
        }
    });

    match phase {
        RunWorkflowPhase::Early(s) => s,
        RunWorkflowPhase::WithHostMetrics {
            response,
            metrics,
            context,
        } => {
            if let Err(err) = merge_application_metrics_for(&application_id, metrics, context) {
                return serde_json::json!({ "error": err }).to_string();
            }
            response
        }
    }
}

fn query_status() -> String {
    let application_id = resolve_application_id();
    if let Err(err) = merge_application_metrics_for(
        &application_id,
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": { "workflow_queries": 1 },
        }),
        "workflow_query metrics",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    with_state(|state| {
        serde_json::json!({
            "order_id": state.order_id,
            "customer_id": state.customer_id,
            "status": state.status,
            "steps_count": state.steps.len(),
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
struct OrderFulfillmentActor;

#[plexspaces_handlers(wasm)]
impl OrderFulfillmentActor {
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
            if let Some(id) = v.get("order_id").and_then(|x| x.as_str()) {
                state.order_id = id.to_string();
            }
            if let Some(id) = v.get("customer_id").and_then(|x| x.as_str()) {
                state.customer_id = id.to_string();
            }
            let now = host::now_ms();
            state.created_at_ms = now;
            state.updated_at_ms = now;
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
        Ok(run_workflow(&payload))
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
                "counter_metrics": { "workflow_signals_cancel": 1 },
            }),
            "workflow_signal cancel metrics",
        )?;
        Ok("{}".to_string())
    }

    #[handler("workflow_query:status")]
    fn workflow_query_status(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(query_status())
    }
}

struct OrderFulfillmentBridge;

impl Guest for OrderFulfillmentBridge {
    fn init(config_json: String) -> String {
        let mut actor = OrderFulfillmentActor::default();
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
        let mut actor = OrderFulfillmentActor::default();
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
        match serde_json::from_str::<OrderState>(&state_json) {
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

export!(OrderFulfillmentBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_embedded_op() {
        assert_eq!(
            parse_op("call", r#"{"op":"workflow_run","order_id":"a"}"#).expect("op"),
            "workflow_run"
        );
    }

    #[test]
    fn actor_application_id_parses_qualified_actor() {
        assert_eq!(
            actor_application_id("01ABC//order-fulfillment::temporal-order-fulfillment-rust@test-node"),
            "temporal-order-fulfillment-rust"
        );
    }
}
