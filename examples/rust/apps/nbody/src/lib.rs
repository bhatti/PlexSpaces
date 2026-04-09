// SPDX-License-Identifier: LGPL-2.1-or-later
//
// N-body gravitational simulation (Rust WASM).
// SDK: `#[gen_server_actor(wasm)]` + `host::application_metrics_add` (metrics merged after state updates).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{Mutex, OnceLock};
use prost::Message as ProstMessage;
use plexspaces_proto::application::v1::ApplicationMetrics;

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;
use plexspaces_sdk::simple_actor::ActorWorldHandlers;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

const G: f64 = 1.0;
const SOFTENING_SQ: f64 = 1e-8;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Body {
    id: String,
    mass: f64,
    position: [f64; 3],
    velocity: [f64; 3],
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SimState {
    application_id: String,
    bodies: Vec<Body>,
    step_count: u64,
    total_compute_ms: f64,
    total_coord_ms: f64,
}

impl SimState {
    fn default_bodies() -> Vec<Body> {
        vec![
            Body {
                id: "0".into(),
                mass: 1.0,
                position: [-0.5, 0.0, 0.0],
                velocity: [0.0, -0.05, 0.0],
            },
            Body {
                id: "1".into(),
                mass: 1.0,
                position: [0.5, 0.0, 0.0],
                velocity: [0.0, 0.05, 0.0],
            },
        ]
    }

    fn kinetic_energy(&self) -> f64 {
        self.bodies
            .iter()
            .map(|b| {
                let v2 = b.velocity[0] * b.velocity[0]
                    + b.velocity[1] * b.velocity[1]
                    + b.velocity[2] * b.velocity[2];
                0.5 * b.mass * v2
            })
            .sum()
    }

    fn potential_energy(&self) -> f64 {
        let n = self.bodies.len();
        let mut pe = 0.0;
        for i in 0..n {
            for j in (i + 1)..n {
                let mut r = [0.0f64; 3];
                for k in 0..3 {
                    r[k] = self.bodies[j].position[k] - self.bodies[i].position[k];
                }
                let dist = (r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + SOFTENING_SQ).sqrt();
                pe -= G * self.bodies[i].mass * self.bodies[j].mass / dist;
            }
        }
        pe
    }

    /// One leapfrog-style substep: kick-drift (explicit Euler on acceleration from positions).
    fn step(&mut self, dt: f64) {
        let n = self.bodies.len();
        if n == 0 {
            return;
        }

        let mut acc = vec![[0.0f64; 3]; n];
        for i in 0..n {
            for j in 0..n {
                if i == j {
                    continue;
                }
                let mut r = [0.0f64; 3];
                for k in 0..3 {
                    r[k] = self.bodies[j].position[k] - self.bodies[i].position[k];
                }
                let dist_sq = r[0] * r[0] + r[1] * r[1] + r[2] * r[2] + SOFTENING_SQ;
                let inv = 1.0 / dist_sq.sqrt();
                let inv3 = inv * inv * inv;
                let gm = G * self.bodies[j].mass;
                for k in 0..3 {
                    acc[i][k] += gm * r[k] * inv3;
                }
            }
        }

        for i in 0..n {
            for k in 0..3 {
                self.bodies[i].velocity[k] += acc[i][k] * dt;
            }
        }
        for i in 0..n {
            for k in 0..3 {
                self.bodies[i].position[k] += self.bodies[i].velocity[k] * dt;
            }
        }
        self.step_count += 1;
    }
}

fn state_cell() -> &'static Mutex<SimState> {
    static STATE: OnceLock<Mutex<SimState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(SimState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut SimState) -> T) -> T {
    let mut g = state_cell().lock().expect("nbody state lock poisoned");
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
        .expect("nbody state lock")
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

fn pair_count(n: usize) -> u64 {
    if n < 2 {
        0
    } else {
        (n * (n - 1)) as u64
    }
}

fn metrics_step(compute_ms: u64, coord_ms: u64, pairs: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": {
            "nbody_steps": 1,
            "nbody_pair_interactions": pairs,
        },
        "latency_totals_ms": {
            "nbody.compute": compute_ms,
            "nbody.coordination": coord_ms,
        },
        "latency_max_ms": {
            "nbody.compute": compute_ms,
            "nbody.coordination": coord_ms,
        },
        "latency_samples": {
            "nbody.compute": 1,
            "nbody.coordination": 1,
        },
    })
}

fn metrics_status_query() -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "nbody_status_queries": 1 },
    })
}

const COMPUTE_MS_PER_PAIR: f64 = 0.0004;
const COORD_MS_PER_STEP: f64 = 0.02;

fn handle_reset(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let phase = with_state(|state| {
        let bodies_val = payload.get("bodies").cloned();
        state.bodies = match bodies_val {
            Some(serde_json::Value::Array(arr)) if !arr.is_empty() => {
                match serde_json::from_value(serde_json::Value::Array(arr)) {
                    Ok(v) => v,
                    Err(e) => {
                        return (
                            serde_json::json!({ "error": format!("invalid bodies: {}", e) })
                                .to_string(),
                            None,
                        );
                    }
                }
            }
            _ => SimState::default_bodies(),
        };
        state.step_count = 0;
        state.total_compute_ms = 0.0;
        state.total_coord_ms = 0.0;
        let n = state.bodies.len();
        let pairs = pair_count(n);
        let compute_u = ((pairs as f64) * COMPUTE_MS_PER_PAIR).max(0.001) as u64;
        let coord_u = COORD_MS_PER_STEP as u64;
        let m = serde_json::json!({
            "message_count": 1,
            "counter_metrics": { "nbody_resets": 1 },
            "latency_totals_ms": {
                "nbody.compute": compute_u,
                "nbody.coordination": coord_u,
            },
            "latency_max_ms": {
                "nbody.compute": compute_u,
                "nbody.coordination": coord_u,
            },
            "latency_samples": {
                "nbody.compute": 1,
                "nbody.coordination": 1,
            },
        });
        (
            serde_json::json!({ "ok": true, "n": n }).to_string(),
            Some(m),
        )
    });
    let (body, maybe_metrics) = phase;
    if let Some(metrics) = maybe_metrics {
        if let Err(err) = merge_application_metrics_for(&application_id, metrics, "reset metrics") {
            return serde_json::json!({ "error": err }).to_string();
        }
    }
    body
}

fn handle_step(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let dt = payload
        .get("dt")
        .and_then(|v| v.as_f64())
        .filter(|d| *d > 0.0)
        .unwrap_or(0.01);

    let phase = with_state(|state| {
        if state.bodies.is_empty() {
            state.bodies = SimState::default_bodies();
        }
        let n = state.bodies.len();
        let pairs = pair_count(n);
        let compute_ms = (pairs as f64) * COMPUTE_MS_PER_PAIR;
        let coord_ms = COORD_MS_PER_STEP;
        state.total_compute_ms += compute_ms;
        state.total_coord_ms += coord_ms;
        state.step(dt);
        let compute_u = compute_ms.max(0.001) as u64;
        let coord_u = coord_ms as u64;
        let ke = state.kinetic_energy();
        let pe = state.potential_energy();
        let m = metrics_step(compute_u, coord_u, pairs);
        (
            serde_json::json!({
                "step_count": state.step_count,
                "kinetic_energy": ke,
                "potential_energy": pe,
                "dt": dt,
            })
            .to_string(),
            Some(m),
        )
    });
    let (body, maybe_metrics) = phase;
    if let Some(metrics) = maybe_metrics {
        if let Err(err) = merge_application_metrics_for(&application_id, metrics, "step metrics") {
            return serde_json::json!({ "error": err }).to_string();
        }
    }
    body
}

fn handle_run_steps(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let dt = payload
        .get("dt")
        .and_then(|v| v.as_f64())
        .filter(|d| *d > 0.0)
        .unwrap_or(0.01);
    let count = payload
        .get("count")
        .and_then(|v| v.as_u64())
        .unwrap_or(10)
        .min(500);

    for _ in 0..count {
        let step_out = handle_step(&serde_json::json!({ "dt": dt }));
        if step_out.contains("\"error\"") {
            return step_out;
        }
    }

    if let Err(e) = merge_application_metrics_for(
        &application_id,
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "nbody_burst_runs": 1,
                "nbody_burst_steps_delta": count,
            },
        }),
        "run_steps burst rollup",
    ) {
        return serde_json::json!({ "error": e }).to_string();
    }

    with_state(|state| {
        serde_json::json!({
            "burst_steps": count,
            "step_count": state.step_count,
            "kinetic_energy": state.kinetic_energy(),
            "potential_energy": state.potential_energy(),
        })
        .to_string()
    })
}

fn handle_get_state() -> String {
    with_state(|state| {
        serde_json::json!({
            "bodies": state.bodies,
            "step_count": state.step_count,
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
    with_state(|state| {
        let total_ms = state.total_compute_ms + state.total_coord_ms;
        let compute_pct = if total_ms > 0.0 {
            100.0 * state.total_compute_ms / total_ms
        } else {
            0.0
        };
        let coord_pct = if total_ms > 0.0 {
            100.0 * state.total_coord_ms / total_ms
        } else {
            0.0
        };
        let gran = if state.total_coord_ms > 0.0 {
            state.total_compute_ms / state.total_coord_ms
        } else {
            0.0
        };
        serde_json::json!({
            "n": state.bodies.len(),
            "step_count": state.step_count,
            "total_compute_ms": state.total_compute_ms,
            "total_coord_ms": state.total_coord_ms,
            "compute_pct": compute_pct,
            "coord_pct": coord_pct,
            "granularity_ratio": gran,
            "kinetic_energy": state.kinetic_energy(),
            "potential_energy": state.potential_energy(),
            "use_case": "nbody",
            "orchestration": "single_actor"
        })
        .to_string()
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct NBodyActor;

#[plexspaces_handlers(wasm)]
impl NBodyActor {
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

    #[handler("reset")]
    fn reset_op(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload = parse_payload(payload)?;
        Ok(json_bytes(
            serde_json::from_str(&handle_reset(&payload))
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid reset response" })),
        ))
    }

    #[handler("step")]
    fn step_op(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload = parse_payload(payload)?;
        Ok(json_bytes(
            serde_json::from_str(&handle_step(&payload))
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid step response" })),
        ))
    }

    #[handler("run_steps")]
    fn run_steps_op(&mut self, _from_actor: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let payload = parse_payload(payload)?;
        Ok(json_bytes(
            serde_json::from_str(&handle_run_steps(&payload))
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid run_steps response" })),
        ))
    }

    #[handler("get_state")]
    fn get_state_op(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(json_bytes(
            serde_json::from_str(&handle_get_state())
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid get_state response" })),
        ))
    }

    #[handler("get_status")]
    fn get_status_op(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(json_bytes(
            serde_json::from_str(&handle_get_status())
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid get_status response" })),
        ))
    }

    #[handler("status")]
    fn status_alias(&mut self, _from_actor: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        Ok(json_bytes(
            serde_json::from_str(&handle_get_status())
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid get_status response" })),
        ))
    }
}

struct NBodyBridge;

impl Guest for NBodyBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let mut actor = NBodyActor::default();
        ActorWorldHandlers::init(&mut actor, &config)
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let op = match parse_op(&msg_type, &payload) {
            Ok(op) => op,
            Err(err) => return Ok(json_error(err)),
        };
        let mut actor = NBodyActor::default();
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
        match serde_json::from_slice::<SimState>(&state) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                Ok(())
            }
            Err(_) => Err("invalid state JSON".to_string()),
        }
    }
}

export!(NBodyBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_embedded_op() {
        assert_eq!(
            parse_op("call", br#"{"op":"step","dt":0.01}"#).expect("op"),
            "step"
        );
    }

    #[test]
    fn two_body_step_is_finite() {
        let mut s = SimState::default();
        s.bodies = SimState::default_bodies();
        s.step(0.01);
        assert!(s.bodies[0].position[0].is_finite());
        assert!(s.kinetic_energy().is_finite());
    }

    #[test]
    fn pair_count_matches_loop() {
        assert_eq!(pair_count(4), 12);
    }
}
