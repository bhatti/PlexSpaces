// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Calculator (Rust WASM): add / subtract / multiply / divide on two operands.
// SDK: `#[gen_server_actor(wasm)]` + `host::application_metrics_add`.

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

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Operation {
    Add,
    Subtract,
    Multiply,
    Divide,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct CalcState {
    application_id: String,
    calculation_count: u64,
    last_result: Option<f64>,
    total_compute_ms: f64,
    total_coord_ms: f64,
}

fn state_cell() -> &'static Mutex<CalcState> {
    static STATE: OnceLock<Mutex<CalcState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(CalcState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut CalcState) -> T) -> T {
    let mut g = state_cell().lock().expect("calculator state lock poisoned");
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
        .expect("calculator state lock")
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

const COMPUTE_MS_BASE: f64 = 0.35;
const COORD_MS_PER_OP: f64 = 0.04;

fn metrics_op(compute_ms: u64, coord_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "calculator_ops": 1 },
        "latency_totals_ms": {
            "calculator.compute": compute_ms,
            "calculator.coordination": coord_ms,
        },
        "latency_max_ms": {
            "calculator.compute": compute_ms,
            "calculator.coordination": coord_ms,
        },
        "latency_samples": {
            "calculator.compute": 1,
            "calculator.coordination": 1,
        },
    })
}

fn metrics_status_query() -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "calculator_status_queries": 1 },
    })
}

fn execute(op: Operation, operands: &[f64]) -> Result<f64, String> {
    if operands.len() != 2 {
        return Err("exactly two operands required".into());
    }
    let a = operands[0];
    let b = operands[1];
    match op {
        Operation::Add => Ok(a + b),
        Operation::Subtract => Ok(a - b),
        Operation::Multiply => Ok(a * b),
        Operation::Divide => {
            if b == 0.0 {
                Err("division by zero".into())
            } else {
                Ok(a / b)
            }
        }
    }
}

fn parse_operation(s: &str) -> Result<Operation, String> {
    match s.to_lowercase().as_str() {
        "add" => Ok(Operation::Add),
        "subtract" => Ok(Operation::Subtract),
        "multiply" => Ok(Operation::Multiply),
        "divide" => Ok(Operation::Divide),
        _ => Err("operation must be add, subtract, multiply, or divide".into()),
    }
}

fn handle_calculate(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let req: Result<(Operation, Vec<f64>), String> = (|| {
        let op_str = payload
            .get("operation")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "missing operation".to_string())?;
        let op = parse_operation(op_str)?;
        let ops = payload
            .get("operands")
            .and_then(|v| v.as_array())
            .ok_or_else(|| "missing operands array".to_string())?;
        let nums: Vec<f64> = ops
            .iter()
            .filter_map(|v| v.as_f64())
            .collect();
        Ok((op, nums))
    })();

    let (op, nums) = match req {
        Ok(x) => x,
        Err(e) => return serde_json::json!({ "error": e }).to_string(),
    };

    match execute(op, &nums) {
        Ok(result) => {
            let compute_u = COMPUTE_MS_BASE.max(0.001) as u64;
            let coord_u = COORD_MS_PER_OP as u64;
            with_state(|s| {
                s.calculation_count += 1;
                s.last_result = Some(result);
                s.total_compute_ms += COMPUTE_MS_BASE;
                s.total_coord_ms += COORD_MS_PER_OP;
            });
            let m = metrics_op(compute_u, coord_u);
            if let Err(err) =
                merge_application_metrics_for(&application_id, m, "calculate metrics")
            {
                return serde_json::json!({ "error": err }).to_string();
            }
            serde_json::json!({ "result": result, "calculation_count": with_state(|s| s.calculation_count) }).to_string()
        }
        Err(e) => serde_json::json!({ "error": e }).to_string(),
    }
}

fn handle_reset() -> String {
    let application_id = resolve_application_id();
    with_state(|s| {
        s.calculation_count = 0;
        s.last_result = None;
        s.total_compute_ms = 0.0;
        s.total_coord_ms = 0.0;
    });
    let m = serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "calculator_resets": 1 },
        "latency_totals_ms": {
            "calculator.compute": 1u64,
            "calculator.coordination": 1u64,
        },
        "latency_max_ms": {
            "calculator.compute": 1u64,
            "calculator.coordination": 1u64,
        },
        "latency_samples": {
            "calculator.compute": 1,
            "calculator.coordination": 1,
        },
    });
    if let Err(err) = merge_application_metrics_for(&application_id, m, "reset metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({ "ok": true }).to_string()
}

fn handle_get_stats() -> String {
    with_state(|s| {
        serde_json::json!({
            "calculation_count": s.calculation_count,
            "last_result": s.last_result,
        })
        .to_string()
    })
}

fn handle_get_status() -> String {
    let application_id = resolve_application_id();
    if let Err(err) = merge_application_metrics_for(
        &application_id,
        metrics_status_query(),
        "get_status metrics",
    ) {
        return serde_json::json!({ "error": err }).to_string();
    }
    with_state(|s| {
        let total_ms = s.total_compute_ms + s.total_coord_ms;
        let compute_pct = if total_ms > 0.0 {
            100.0 * s.total_compute_ms / total_ms
        } else {
            0.0
        };
        let coord_pct = if total_ms > 0.0 {
            100.0 * s.total_coord_ms / total_ms
        } else {
            0.0
        };
        let gran = if s.total_coord_ms > 0.0 {
            s.total_compute_ms / s.total_coord_ms
        } else {
            0.0
        };
        serde_json::json!({
            "calculation_count": s.calculation_count,
            "last_result": s.last_result,
            "total_compute_ms": s.total_compute_ms,
            "total_coord_ms": s.total_coord_ms,
            "compute_pct": compute_pct,
            "coord_pct": coord_pct,
            "granularity_ratio": gran,
            "use_case": "calculator",
            "orchestration": "single_actor"
        })
        .to_string()
    })
}

/// Batch calculate for benchmarks: `{ "operations": [ { "operation", "operands" }, ... ] }` (max 200).
fn handle_batch(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let arr = match payload.get("operations").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => {
            return serde_json::json!({ "error": "missing non-empty operations array" }).to_string();
        }
    };
    let n = arr.len().min(200);
    let mut errors = 0u64;
    for item in arr.iter().take(n) {
        let out = handle_calculate(item);
        if out.contains("\"error\"") && !out.contains("\"result\"") {
            errors += 1;
        }
    }
    if let Err(e) = merge_application_metrics_for(
        &application_id,
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "calculator_batch_runs": 1,
                "calculator_batch_ops_delta": n as u64,
                "calculator_batch_errors_delta": errors,
            },
        }),
        "batch rollup",
    ) {
        return serde_json::json!({ "error": e }).to_string();
    }
    handle_get_stats()
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct CalculatorActor;

#[plexspaces_handlers(wasm)]
impl CalculatorActor {
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

    #[handler("calculate")]
    fn calculate_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_calculate(&payload))
    }

    #[handler("reset")]
    fn reset_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_reset())
    }

    #[handler("get_stats")]
    fn get_stats_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_stats())
    }

    #[handler("get_status")]
    fn get_status_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_status())
    }

    #[handler("status")]
    fn status_alias(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_status())
    }

    #[handler("batch")]
    fn batch_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_batch(&payload))
    }
}

struct CalculatorBridge;

impl Guest for CalculatorBridge {
    fn init(config_json: String) -> String {
        let mut actor = CalculatorActor::default();
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
        let mut actor = CalculatorActor::default();
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
        match serde_json::from_str::<CalcState>(&state_json) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                String::new()
            }
            Err(_) => "ERROR: invalid state JSON".to_string(),
        }
    }
}

export!(CalculatorBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_embedded_op() {
        assert_eq!(
            parse_op("call", r#"{"op":"calculate","operation":"add","operands":[1,2]}"#)
                .expect("op"),
            "calculate"
        );
    }

    #[test]
    fn add_and_divide() {
        assert_eq!(execute(Operation::Add, &[1.0, 2.0]).unwrap(), 3.0);
        assert_eq!(execute(Operation::Divide, &[10.0, 2.0]).unwrap(), 5.0);
    }

    #[test]
    fn divide_by_zero() {
        assert!(execute(Operation::Divide, &[1.0, 0.0]).is_err());
    }
}
