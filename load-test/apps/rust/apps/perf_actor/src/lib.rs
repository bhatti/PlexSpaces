// SPDX-License-Identifier: AGPL-3.0-or-later
// PerfActor — Rust WASM actor for PlexSpaces load testing.
//
// Operations: echo, compute (Mersenne prime), kv_put/kv_get, pg_broadcast, shard_task, get_stats.
// Identical semantics to the Python / Go / TypeScript WASM variants.

use serde_json::Value;
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::{pg_broadcast, pg_join};
use plexspaces::actor::host_kv::{kv_get, kv_put};
use plexspaces_sdk::simple_actor::ActorWorldHandlers;
use plexspaces_sdk::{gen_server_actor, plexspaces_handlers};

// ─── State ────────────────────────────────────────────────────────────────────

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct PerfState {
    echo_count: u64,
    compute_count: u64,
    kv_count: u64,
    pg_count: u64,
    shard_count: u64,
}

fn state_cell() -> &'static Mutex<PerfState> {
    static STATE: OnceLock<Mutex<PerfState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PerfState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut PerfState) -> T) -> T {
    f(&mut state_cell().lock().expect("perf state lock"))
}

// ─── Lucas-Lehmer test ────────────────────────────────────────────────────────

fn is_mersenne_prime(p: u32) -> bool {
    if p == 2 {
        return true;
    }
    if p < 2 {
        return false;
    }
    // Use u128 for small p (p <= 126 to avoid overflow); for larger p this would need BigInt.
    // For load testing purposes p=7 is the default — fast, deterministic ~1µs work.
    if p > 62 {
        // Fallback: just return false for very large p (BigInt not available in no_std WASM)
        return false;
    }
    let mp: u128 = (1u128 << p) - 1;
    let mut s: u128 = 4;
    for _ in 0..(p - 2) {
        s = (s.wrapping_mul(s).wrapping_sub(2)) % mp;
    }
    s == 0
}

// ─── Gradient descent step ────────────────────────────────────────────────────

fn gradient_step(values: &[f64], lr: f64) -> Value {
    if values.is_empty() {
        return serde_json::json!({ "gradient": 0.0, "count": 0 });
    }
    let n = values.len() as f64;
    let mean: f64 = values.iter().sum::<f64>() / n;
    let gradient: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let sample: Vec<f64> = values.iter().take(3).map(|v| v - lr * (v - mean)).collect();
    serde_json::json!({ "gradient": gradient, "count": values.len(), "mean": mean, "sample": sample })
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn parse_payload(payload: &[u8]) -> Value {
    if payload.is_empty() {
        return serde_json::json!({});
    }
    serde_json::from_slice(payload).unwrap_or_else(|_| serde_json::json!({}))
}

fn ok_bytes(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

fn err_bytes(msg: impl Into<String>) -> Vec<u8> {
    serde_json::json!({ "error": msg.into() }).to_string().into_bytes()
}

// ─── Actor ────────────────────────────────────────────────────────────────────

#[gen_server_actor(wasm)]
#[derive(Default)]
struct PerfActor;

#[plexspaces_handlers(wasm)]
impl PerfActor {
    #[init_handler]
    fn configure(&mut self, _config: &[u8]) -> Result<(), String> {
        Ok(())
    }

    #[handler("echo")]
    fn handle_echo(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let count = with_state(|s| { s.echo_count += 1; s.echo_count });
        Ok(ok_bytes(serde_json::json!({ "ok": true, "echo": p, "count": count })))
    }

    #[handler("compute")]
    fn handle_compute(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let exponent = p.get("p").and_then(|v| v.as_u64()).unwrap_or(7) as u32;
        let result = is_mersenne_prime(exponent);
        let count = with_state(|s| { s.compute_count += 1; s.compute_count });
        Ok(ok_bytes(serde_json::json!({ "ok": true, "p": exponent, "is_mersenne_prime": result, "count": count })))
    }

    #[handler("kv_put")]
    fn handle_kv_put(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("perf_key");
        let value = p.get("value").and_then(|v| v.as_str()).unwrap_or("perf_val");
        kv_put(key, value.as_bytes()).map_err(|e| format!("kv_put error: {:?}", e))?;
        let count = with_state(|s| { s.kv_count += 1; s.kv_count });
        Ok(ok_bytes(serde_json::json!({ "ok": true, "key": key, "count": count })))
    }

    #[handler("kv_get")]
    fn handle_kv_get(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let key = p.get("key").and_then(|v| v.as_str()).unwrap_or("perf_key");
        match kv_get(key) {
            Ok(bytes) => {
                let val = String::from_utf8_lossy(&bytes);
                Ok(ok_bytes(serde_json::json!({ "ok": true, "key": key, "value": val })))
            }
            Err(e) => Ok(err_bytes(format!("kv_get error: {:?}", e))),
        }
    }

    #[handler("pg_broadcast")]
    fn handle_pg_broadcast(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let group = p.get("group").and_then(|v| v.as_str()).unwrap_or("perf-group");
        let msg = p.get("message").cloned().unwrap_or_else(|| serde_json::json!({ "event": "ping" }));
        pg_join(group).map_err(|e| format!("pg_join error: {:?}", e))?;
        pg_broadcast(group, "perf_event", msg.to_string().as_bytes())
            .map_err(|e| format!("pg_broadcast error: {:?}", e))?;
        let count = with_state(|s| { s.pg_count += 1; s.pg_count });
        Ok(ok_bytes(serde_json::json!({ "ok": true, "group": group, "count": count })))
    }

    #[handler("shard_task")]
    fn handle_shard_task(&mut self, _from: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
        let p = parse_payload(payload);
        let shard_index = p.get("shard_index").and_then(|v| v.as_u64()).unwrap_or(0);
        let lr = p.get("lr").and_then(|v| v.as_f64()).unwrap_or(0.01);
        let values: Vec<f64> = if let Some(arr) = p.get("values").and_then(|v| v.as_array()) {
            arr.iter().filter_map(|v| v.as_f64()).collect()
        } else {
            (0..100).map(|i| i as f64).collect()
        };
        let mut stats = gradient_step(&values, lr);
        let count = with_state(|s| { s.shard_count += 1; s.shard_count });
        stats["ok"] = serde_json::json!(true);
        stats["shard_index"] = serde_json::json!(shard_index);
        stats["count"] = serde_json::json!(count);
        Ok(ok_bytes(stats))
    }

    #[handler("get_stats")]
    fn handle_get_stats(&mut self, _from: &str, _payload: &[u8]) -> Result<Vec<u8>, String> {
        let (echo, compute, kv, pg, shard) = with_state(|s| {
            (s.echo_count, s.compute_count, s.kv_count, s.pg_count, s.shard_count)
        });
        Ok(ok_bytes(serde_json::json!({
            "ok": true,
            "echo_count": echo,
            "compute_count": compute,
            "kv_count": kv,
            "pg_count": pg,
            "shard_count": shard,
        })))
    }
}

// ─── WIT bridge ───────────────────────────────────────────────────────────────

struct PerfBridge;

impl Guest for PerfBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let mut actor = PerfActor::default();
        ActorWorldHandlers::init(&mut actor, &config)
    }

    fn handle(from_actor: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        let op = if msg_type == "call" || msg_type == "cast" {
            serde_json::from_slice::<Value>(&payload)
                .ok()
                .and_then(|v| v.get("op").and_then(|o| o.as_str()).map(str::to_string))
                .unwrap_or(msg_type.clone())
        } else {
            msg_type.clone()
        };
        let mut actor = PerfActor::default();
        Ok(actor.handle_operation(&from_actor, &op, &payload).unwrap_or_else(err_bytes))
    }

    fn get_state() -> Result<Vec<u8>, String> {
        with_state(|s| serde_json::to_vec(s).map_err(|e| format!("state encode: {e}")))
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        if state.is_empty() {
            return Ok(());
        }
        match serde_json::from_slice::<PerfState>(&state) {
            Ok(s) => {
                *state_cell().lock().expect("set_state lock") = s;
                Ok(())
            }
            Err(_) => Err("invalid state JSON".to_string()),
        }
    }
}

export!(PerfBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mersenne_p7() {
        assert!(is_mersenne_prime(7)); // 127 is prime
    }

    #[test]
    fn mersenne_p4_not_prime() {
        assert!(!is_mersenne_prime(4)); // 2^4-1=15, not prime
    }

    #[test]
    fn gradient_step_basic() {
        let v: Vec<f64> = (0..10).map(|i| i as f64).collect();
        let result = gradient_step(&v, 0.01);
        assert!(result.get("gradient").is_some());
    }
}
