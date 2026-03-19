// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Session manager (Rust WASM): idle timeout + periodic heartbeat using `host::send_after`.
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionState {
    application_id: String,
    user_id: String,
    activity_count: u64,
    heartbeat_ticks: u64,
    idle_fired: bool,
    last_event_ms: u64,
    idle_timeout_ms: u64,
    heartbeat_ms: u64,
    timers_started: u64,
    total_compute_ms: f64,
    total_coord_ms: f64,
}

fn state_cell() -> &'static Mutex<SessionState> {
    static STATE: OnceLock<Mutex<SessionState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SessionState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut SessionState) -> T) -> T {
    let mut g = state_cell().lock().expect("session_manager state lock poisoned");
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
        .expect("session_manager state lock")
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

const COMPUTE_MS_TOUCH: f64 = 0.15;
const COORD_MS_TOUCH: f64 = 0.03;
const COMPUTE_MS_TIMER: f64 = 0.1;
const COORD_MS_TIMER: f64 = 0.02;

fn metrics_touch(compute_ms: u64, coord_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "session_touch_events": 1 },
        "latency_totals_ms": {
            "session.compute": compute_ms,
            "session.coordination": coord_ms,
        },
        "latency_max_ms": {
            "session.compute": compute_ms,
            "session.coordination": coord_ms,
        },
        "latency_samples": {
            "session.compute": 1,
            "session.coordination": 1,
        },
    })
}

fn metrics_timer(kind: &str, compute_ms: u64, coord_ms: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { format!("session_timer_{}", kind): 1 },
        "latency_totals_ms": {
            "session.compute": compute_ms,
            "session.coordination": coord_ms,
        },
        "latency_max_ms": {
            "session.compute": compute_ms,
            "session.coordination": coord_ms,
        },
        "latency_samples": {
            "session.compute": 1,
            "session.coordination": 1,
        },
    })
}

fn metrics_status_query() -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "session_status_queries": 1 },
    })
}

fn schedule_idle() {
    let idle_ms = with_state(|s| s.idle_timeout_ms);
    if idle_ms == 0 {
        return;
    }
    let _ = host::send_after(idle_ms, "session_idle", "{}");
}

fn schedule_heartbeat() {
    let hb_ms = with_state(|s| s.heartbeat_ms);
    if hb_ms == 0 {
        return;
    }
    let _ = host::send_after(hb_ms, "session_heartbeat", "{}");
}

fn handle_start_session(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let user_id = payload
        .get("user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("user")
        .to_string();
    let idle_timeout_ms = payload
        .get("idle_timeout_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(5_000)
        .min(600_000);
    let heartbeat_ms = payload
        .get("heartbeat_ms")
        .and_then(|v| v.as_u64())
        .unwrap_or(2_000)
        .min(120_000);

    let idle_tid = host::send_after(idle_timeout_ms, "session_idle", "{}");
    let hb_tid = host::send_after(heartbeat_ms, "session_heartbeat", "{}");

    if idle_tid.starts_with("ERROR:") {
        return serde_json::json!({ "error": idle_tid }).to_string();
    }
    if hb_tid.starts_with("ERROR:") {
        return serde_json::json!({ "error": hb_tid }).to_string();
    }

    with_state(|s| {
        s.user_id = user_id;
        s.idle_timeout_ms = idle_timeout_ms;
        s.heartbeat_ms = heartbeat_ms;
        s.activity_count = 0;
        s.heartbeat_ticks = 0;
        s.idle_fired = false;
        s.last_event_ms = host::now_ms();
        s.timers_started += 2;
        s.total_compute_ms += 0.5;
        s.total_coord_ms += 0.1;
    });

    let m = serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "session_start_sessions": 1, "session_timers_scheduled_delta": 2 },
        "latency_totals_ms": {
            "session.compute": 1u64,
            "session.coordination": 1u64,
        },
        "latency_max_ms": {
            "session.compute": 1u64,
            "session.coordination": 1u64,
        },
        "latency_samples": {
            "session.compute": 1,
            "session.coordination": 1,
        },
    });
    if let Err(err) = merge_application_metrics_for(&application_id, m, "start_session metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }

    serde_json::json!({
        "ok": true,
        "idle_timer": idle_tid,
        "heartbeat_timer": hb_tid,
        "idle_timeout_ms": idle_timeout_ms,
        "heartbeat_ms": heartbeat_ms,
    })
    .to_string()
}

fn handle_touch(_payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let compute_u = COMPUTE_MS_TOUCH.max(0.001) as u64;
    let coord_u = COORD_MS_TOUCH as u64;
    with_state(|s| {
        s.activity_count += 1;
        s.last_event_ms = host::now_ms();
        s.total_compute_ms += COMPUTE_MS_TOUCH;
        s.total_coord_ms += COORD_MS_TOUCH;
    });
    schedule_idle();
    let m = metrics_touch(compute_u, coord_u);
    if let Err(err) = merge_application_metrics_for(&application_id, m, "touch metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({
        "activity_count": with_state(|s| s.activity_count),
        "last_event_ms": with_state(|s| s.last_event_ms),
    })
    .to_string()
}

fn handle_session_idle() -> String {
    let application_id = resolve_application_id();
    let compute_u = COMPUTE_MS_TIMER.max(0.001) as u64;
    let coord_u = COORD_MS_TIMER as u64;
    with_state(|s| {
        s.idle_fired = true;
        s.total_compute_ms += COMPUTE_MS_TIMER;
        s.total_coord_ms += COORD_MS_TIMER;
    });
    let m = metrics_timer("idle", compute_u, coord_u);
    if let Err(err) = merge_application_metrics_for(&application_id, m, "session_idle metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({ "idle": "fired" }).to_string()
}

fn handle_session_heartbeat() -> String {
    let application_id = resolve_application_id();
    let compute_u = COMPUTE_MS_TIMER.max(0.001) as u64;
    let coord_u = COORD_MS_TIMER as u64;
    with_state(|s| {
        s.heartbeat_ticks += 1;
        s.total_compute_ms += COMPUTE_MS_TIMER;
        s.total_coord_ms += COORD_MS_TIMER;
    });
    schedule_heartbeat();
    let m = metrics_timer("heartbeat", compute_u, coord_u);
    if let Err(err) = merge_application_metrics_for(&application_id, m, "session_heartbeat metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }
    serde_json::json!({
        "heartbeat_ticks": with_state(|s| s.heartbeat_ticks),
    })
    .to_string()
}

fn handle_reset() -> String {
    let application_id = resolve_application_id();
    with_state(|s| {
        s.user_id.clear();
        s.activity_count = 0;
        s.heartbeat_ticks = 0;
        s.idle_fired = false;
        s.last_event_ms = 0;
        s.idle_timeout_ms = 0;
        s.heartbeat_ms = 0;
        s.timers_started = 0;
        s.total_compute_ms = 0.0;
        s.total_coord_ms = 0.0;
    });
    let m = serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "session_resets": 1 },
        "latency_totals_ms": {
            "session.compute": 1u64,
            "session.coordination": 1u64,
        },
        "latency_max_ms": {
            "session.compute": 1u64,
            "session.coordination": 1u64,
        },
        "latency_samples": {
            "session.compute": 1,
            "session.coordination": 1,
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
            "user_id": s.user_id,
            "activity_count": s.activity_count,
            "heartbeat_ticks": s.heartbeat_ticks,
            "idle_fired": s.idle_fired,
            "last_event_ms": s.last_event_ms,
            "timers_started": s.timers_started,
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
            "user_id": s.user_id,
            "activity_count": s.activity_count,
            "heartbeat_ticks": s.heartbeat_ticks,
            "idle_fired": s.idle_fired,
            "timers_started": s.timers_started,
            "total_compute_ms": s.total_compute_ms,
            "total_coord_ms": s.total_coord_ms,
            "compute_pct": compute_pct,
            "coord_pct": coord_pct,
            "granularity_ratio": gran,
            "use_case": "session_manager",
            "orchestration": "single_actor"
        })
        .to_string()
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct SessionManagerActor;

#[plexspaces_handlers(wasm)]
impl SessionManagerActor {
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

    #[handler("start_session")]
    fn start_session_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_start_session(&payload))
    }

    #[handler("touch")]
    fn touch_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value = serde_json::from_str(payload_json).unwrap_or_default();
        Ok(handle_touch(&payload))
    }

    #[handler("session_idle")]
    fn session_idle_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_session_idle())
    }

    #[handler("session_heartbeat")]
    fn session_heartbeat_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_session_heartbeat())
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
}

struct SessionManagerBridge;

impl Guest for SessionManagerBridge {
    fn init(config_json: String) -> String {
        let mut actor = SessionManagerActor::default();
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
        let mut actor = SessionManagerActor::default();
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
        match serde_json::from_str::<SessionState>(&state_json) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                String::new()
            }
            Err(_) => "ERROR: invalid state JSON".to_string(),
        }
    }
}

export!(SessionManagerBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_embedded() {
        assert_eq!(
            parse_op("call", r#"{"op":"touch"}"#).expect("op"),
            "touch"
        );
    }

    #[test]
    fn parse_op_timer_style() {
        assert_eq!(parse_op("session_idle", "{}").expect("op"), "session_idle");
    }
}
