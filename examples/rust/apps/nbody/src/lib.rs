// SPDX-License-Identifier: LGPL-2.1-or-later
//
// N-body gravitational simulation (Rust WASM).
// SDK: `#[gen_server_actor(wasm)]` + `host::application_metrics_add` (metrics merged after state updates).

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

    #[handler("reset")]
    fn reset_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_reset(&payload))
    }

    #[handler("step")]
    fn step_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_step(&payload))
    }

    #[handler("run_steps")]
    fn run_steps_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_run_steps(&payload))
    }

    #[handler("get_state")]
    fn get_state_op(
        &mut self,
        _from_actor: &str,
        _payload_json: &str,
    ) -> Result<String, String> {
        Ok(handle_get_state())
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

struct NBodyBridge;

impl Guest for NBodyBridge {
    fn init(config_json: String) -> String {
        let mut actor = NBodyActor::default();
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
        let mut actor = NBodyActor::default();
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
        match serde_json::from_str::<SimState>(&state_json) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                String::new()
            }
            Err(_) => "ERROR: invalid state JSON".to_string(),
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
            parse_op("call", r#"{"op":"step","dt":0.01}"#).expect("op"),
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
