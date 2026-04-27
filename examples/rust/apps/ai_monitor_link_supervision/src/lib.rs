// SPDX-License-Identifier: AGPL-3.0-or-later
//
// AI Monitor/Link Supervision (Rust WASM)
//
// Demonstrates monitor and link primitives for fault-tolerant AI pipelines.
// Actors use host::monitor() / host::demonitor() / host::link() / host::unlink()
// to implement FLP/Byzantine-inspired fault detection.

use prost::Message;
use serde_json::{json, Value};

// ─────────────────────────────────────────────────────────────────────────────
// Protobuf state types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone, PartialEq, Message)]
pub struct InferenceWorkerState {
    #[prost(string, tag = "1")]
    pub actor_id: String,
    #[prost(string, tag = "2")]
    pub worker_id: String,
    #[prost(string, tag = "3")]
    pub mode: String, // "normal" | "byzantine"
    #[prost(uint64, tag = "4")]
    pub total_requests: u64,
    #[prost(uint64, tag = "5")]
    pub error_count: u64,
    #[prost(string, repeated, tag = "6")]
    pub linked_peers: Vec<String>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ValidatorState {
    #[prost(string, tag = "1")]
    pub actor_id: String,
    #[prost(uint64, tag = "2")]
    pub total_validations: u64,
    #[prost(uint64, tag = "3")]
    pub pass_count: u64,
    #[prost(uint64, tag = "4")]
    pub fail_count: u64,
    #[prost(uint64, tag = "5")]
    pub byzantine_count: u64,
    #[prost(message, repeated, tag = "6")]
    pub monitor_refs: Vec<MonitorEntry>,
    #[prost(message, repeated, tag = "7")]
    pub down_events: Vec<DownEvent>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MonitorEntry {
    #[prost(string, tag = "1")]
    pub worker_id: String,
    #[prost(string, tag = "2")]
    pub monitor_ref: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct DownEvent {
    #[prost(string, tag = "1")]
    pub monitor_ref: String,
    #[prost(string, tag = "2")]
    pub actor_id: String,
    #[prost(string, tag = "3")]
    pub reason: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct SupervisorState {
    #[prost(string, tag = "1")]
    pub actor_id: String,
    #[prost(string, repeated, tag = "2")]
    pub worker_pool: Vec<String>,
    #[prost(message, repeated, tag = "3")]
    pub monitor_refs: Vec<MonitorEntry>,
    #[prost(uint64, tag = "4")]
    pub down_events_received: u64,
    #[prost(uint64, tag = "5")]
    pub total_dispatched: u64,
    #[prost(uint64, tag = "6")]
    pub next_worker_idx: u64,
}

#[derive(Clone, PartialEq, Message)]
pub struct AuditLogState {
    #[prost(uint64, tag = "1")]
    pub events_received: u64,
    #[prost(string, tag = "2")]
    pub last_event_type: String,
    #[prost(string, tag = "3")]
    pub last_actor_id: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

fn encode<M: Message>(msg: &M) -> Vec<u8> {
    msg.encode_to_vec()
}

fn decode<M: Message + Default>(bytes: &[u8]) -> M {
    M::decode(bytes).unwrap_or_default()
}

fn parse_json(bytes: &[u8]) -> Value {
    if bytes.is_empty() {
        return json!({});
    }
    serde_json::from_slice(bytes).unwrap_or_else(|_| json!({}))
}

fn get_op(msg_type: &str, payload: &Value) -> String {
    payload
        .get("op")
        .and_then(|v| v.as_str())
        .unwrap_or(msg_type)
        .to_string()
}

fn get_str<'a>(v: &'a Value, key: &str, default: &'a str) -> &'a str {
    v.get(key).and_then(|s| s.as_str()).unwrap_or(default)
}

const FLP_THRESHOLD_NUM: u64 = 1;
const FLP_THRESHOLD_DEN: u64 = 3;

fn is_byzantine_response(result: &str) -> bool {
    let lower = result.to_ascii_lowercase();
    if lower.contains("42 is the answer")
        || lower.contains("sky is green")
        || lower == "null"
        || lower.contains("checkpoint corrupted")
        || lower.contains("error: ")
    {
        return true;
    }
    result.trim().len() < 10
}

const BYZANTINE_RESPONSES: &[&str] = &[
    "42 is the answer to everything",
    "The sky is green on Tuesdays",
    "null",
    "ERROR: model checkpoint corrupted",
];

fn normal_inference(prompt: &str) -> String {
    let lower = prompt.to_ascii_lowercase();
    if lower.contains("actor") {
        "The actor model is a mathematical model of concurrent computation where each actor processes messages asynchronously.".to_string()
    } else if lower.contains("fault") || lower.contains("tolerance") {
        "Fault tolerance is achieved through redundancy, isolation, and supervision trees that restart failed components.".to_string()
    } else if lower.contains("flp") || lower.contains("impossibility") {
        "The FLP theorem proves no deterministic async protocol guarantees consensus with even one crash-faulty process.".to_string()
    } else if lower.contains("byzantine") {
        "Byzantine faults are arbitrary failures where a node may send inconsistent messages. Requires 3f+1 replicas.".to_string()
    } else {
        format!("Processed: {}", &prompt[..prompt.len().min(60)])
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Actor logic — pure functions operating on state, testable without WASM
// ─────────────────────────────────────────────────────────────────────────────

fn handle_inference_worker(
    state: &mut InferenceWorkerState,
    msg_type: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let v = parse_json(payload);
    let op = get_op(msg_type, &v);
    match op.as_str() {
        "__EXIT__" => {
            let exit_from = get_str(&v, "exit_from", "").to_string();
            let _exit_reason = get_str(&v, "exit_reason", "").to_string();
            state.linked_peers.retain(|p| p != &exit_from);
            Ok(json!({}).to_string().into_bytes())
        }
        "infer" => {
            state.total_requests += 1;
            let prompt = get_str(&v, "prompt", "");
            let request_id = get_str(&v, "request_id", "").to_string();

            if state.mode == "byzantine" {
                let idx = (state.total_requests as usize) % BYZANTINE_RESPONSES.len();
                state.error_count += 1;
                return Ok(json!({
                    "status": "ok",
                    "request_id": request_id,
                    "result": BYZANTINE_RESPONSES[idx],
                    "worker_id": state.worker_id,
                    "mode": "byzantine",
                })
                .to_string()
                .into_bytes());
            }

            let result = normal_inference(prompt);
            Ok(json!({
                "status": "ok",
                "request_id": request_id,
                "result": result,
                "worker_id": state.worker_id,
                "mode": "normal",
            })
            .to_string()
            .into_bytes())
        }
        "set_mode" => {
            state.mode = get_str(&v, "mode", "normal").to_string();
            Ok(json!({ "status": "ok", "mode": state.mode }).to_string().into_bytes())
        }
        "status" => Ok(json!({
            "status": "ok",
            "worker_id": state.worker_id,
            "mode": state.mode,
            "total_requests": state.total_requests,
            "error_count": state.error_count,
            "linked_peers": state.linked_peers,
        })
        .to_string()
        .into_bytes()),
        other => Ok(json!({ "error": format!("unknown op: {other}") })
            .to_string()
            .into_bytes()),
    }
}

fn handle_validator(
    state: &mut ValidatorState,
    msg_type: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let v = parse_json(payload);
    let op = get_op(msg_type, &v);
    match op.as_str() {
        "__DOWN__" => {
            let monitor_ref = get_str(&v, "monitor_ref", "").to_string();
            let down_from = get_str(&v, "down_from", "").to_string();
            let down_reason = get_str(&v, "down_reason", "").to_string();
            state.down_events.push(DownEvent {
                monitor_ref: monitor_ref.clone(),
                actor_id: down_from,
                reason: down_reason,
            });
            state.monitor_refs.retain(|m| m.monitor_ref != monitor_ref);
            Ok(json!({}).to_string().into_bytes())
        }
        "validate" => {
            state.total_validations += 1;
            let result = get_str(&v, "result", "");
            let worker_id = get_str(&v, "worker_id", "").to_string();

            let byzantine = is_byzantine_response(result);
            if byzantine {
                state.byzantine_count += 1;
                state.fail_count += 1;
            } else {
                state.pass_count += 1;
            }

            let flp_exceeded = state.total_validations >= FLP_THRESHOLD_DEN
                && state.byzantine_count * FLP_THRESHOLD_DEN
                    >= state.total_validations * FLP_THRESHOLD_NUM;

            let flp_ratio = if state.total_validations > 0 {
                state.byzantine_count as f64 / state.total_validations as f64
            } else {
                0.0
            };

            Ok(json!({
                "status": "ok",
                "valid": !byzantine,
                "worker_id": worker_id,
                "byzantine_suspected": byzantine,
                "flp_threshold_exceeded": flp_exceeded,
                "flp_ratio": (flp_ratio * 1000.0).round() / 1000.0,
            })
            .to_string()
            .into_bytes())
        }
        "status" => {
            let flp_ratio = if state.total_validations > 0 {
                state.byzantine_count as f64 / state.total_validations as f64
            } else {
                0.0
            };
            Ok(json!({
                "status": "ok",
                "total_validations": state.total_validations,
                "pass_count": state.pass_count,
                "fail_count": state.fail_count,
                "byzantine_count": state.byzantine_count,
                "flp_threshold": 0.333,
                "flp_ratio": (flp_ratio * 1000.0).round() / 1000.0,
                "monitor_count": state.monitor_refs.len(),
                "down_events_received": state.down_events.len(),
            })
            .to_string()
            .into_bytes())
        }
        other => Ok(json!({ "error": format!("unknown op: {other}") })
            .to_string()
            .into_bytes()),
    }
}

fn handle_supervisor(
    state: &mut SupervisorState,
    msg_type: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let v = parse_json(payload);
    let op = get_op(msg_type, &v);
    match op.as_str() {
        "__DOWN__" => {
            let monitor_ref = get_str(&v, "monitor_ref", "").to_string();
            let down_from = get_str(&v, "down_from", "").to_string();
            let _down_reason = get_str(&v, "down_reason", "").to_string();
            state.down_events_received += 1;
            state.worker_pool.retain(|w| w != &down_from);
            state.monitor_refs.retain(|m| m.monitor_ref != monitor_ref);
            Ok(json!({}).to_string().into_bytes())
        }
        "status" => Ok(json!({
            "status": "ok",
            "worker_pool": state.worker_pool,
            "monitor_count": state.monitor_refs.len(),
            "down_events_received": state.down_events_received,
            "total_dispatched": state.total_dispatched,
        })
        .to_string()
        .into_bytes()),
        other => Ok(json!({ "error": format!("unknown op: {other}") })
            .to_string()
            .into_bytes()),
    }
}

fn handle_audit_log(
    state: &mut AuditLogState,
    msg_type: &str,
    payload: &[u8],
) -> Result<Vec<u8>, String> {
    let v = parse_json(payload);
    let op = get_op(msg_type, &v);
    match op.as_str() {
        "log_event" => {
            state.events_received += 1;
            state.last_event_type = get_str(&v, "event_type", "").to_string();
            state.last_actor_id = get_str(&v, "actor_id", "").to_string();
            Ok(json!({}).to_string().into_bytes())
        }
        "get_stats" => Ok(json!({
            "status": "ok",
            "events_received": state.events_received,
            "last_event_type": state.last_event_type,
            "last_actor_id": state.last_actor_id,
        })
        .to_string()
        .into_bytes()),
        other => Ok(json!({ "error": format!("unknown op: {other}") })
            .to_string()
            .into_bytes()),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WASM bindings — single Guest impl that routes by actor_type from app-config
//
// PlexSpaces loads this WASM for each actor declared in app-config.toml.
// Each instance receives a unique `config` in `init()` containing
// `actor_type = "inference_worker" | "validator_agent" | ...` (from ChildSpec).
// All state is stored in a single enum-keyed OnceLock cell per instance.
// ─────────────────────────────────────────────────────────────────────────────

/// Build a canonical sibling ID from a bare child ID and this actor's own ID.
/// Supervised siblings have deterministic IDs:
///   {child_id}//{actor_type}::{namespace}@{node}
/// where child_id == actor_type (from ChildSpec.id == ChildSpec.actor_type).
/// If bare_id already contains "//" it is canonical and returned unchanged.
fn sibling_id(bare_id: &str, self_actor_id: &str) -> String {
    if bare_id.is_empty() {
        return bare_id.to_string();
    }
    if bare_id.contains("//") {
        return bare_id.to_string();
    }
    // Parse canonical form: {name}//{actor_type}::{namespace}@{node_id}
    let sep = match self_actor_id.find("//") {
        Some(i) => i,
        None => return bare_id.to_string(),
    };
    let rest = &self_actor_id[sep + 2..]; // "{actor_type}::{namespace}@{node_id}"
    let at_idx = rest.find('@');
    let node_id = at_idx.map(|i| &rest[i + 1..]).unwrap_or("");
    let type_ns = at_idx.map(|i| &rest[..i]).unwrap_or(rest);
    let colon_idx = type_ns.find("::");
    let namespace = colon_idx.map(|i| &type_ns[i + 2..]).unwrap_or("");
    // Sibling: name == bare_id, actor_type == bare_id (deterministic supervised children)
    format!("{bare_id}//{bare_id}::{namespace}@{node_id}")
}

#[cfg(target_arch = "wasm32")]
mod wasm_app {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    wit_bindgen::generate!({
        path: "../../../../wit/plexspaces-actor",
        world: "actor-world",
    });

    use exports::plexspaces::actor::actor::Guest;
    use plexspaces::actor::host;

    // ── Unified actor state enum ─────────────────────────────────────────────

    #[allow(clippy::large_enum_variant)]
    enum ActorState {
        Uninitialized,
        InferenceWorker(InferenceWorkerState),
        Validator(ValidatorState),
        Supervisor(SupervisorState),
        AuditLog(AuditLogState),
    }

    fn actor_cell() -> &'static Mutex<ActorState> {
        static STATE: OnceLock<Mutex<ActorState>> = OnceLock::new();
        STATE.get_or_init(|| Mutex::new(ActorState::Uninitialized))
    }

    struct ActorBridge;

    impl Guest for ActorBridge {
        fn init(config: Vec<u8>) -> Result<(), String> {
            let v = parse_json(&config);
            // actor_type is passed directly by the framework from ChildSpec.actor_type.
            // Fall back to normalizing from actor_id for backwards compatibility.
            let actor_type = if let Some(t) = v.get("actor_type").and_then(|t| t.as_str()) {
                if !t.is_empty() { t.to_string() } else { String::new() }
            } else {
                String::new()
            };
            let actor_type = if actor_type.is_empty() {
                // fallback: normalize actor_id (e.g. "inference_worker_a//inference_worker::ns@node" -> "inference_worker")
                let actor_id = v.get("actor_id").and_then(|s| s.as_str()).unwrap_or("");
                if let Some(sep) = actor_id.find("//") {
                    let rest = &actor_id[sep + 2..];
                    rest.find("::").map(|i| rest[..i].to_string())
                        .unwrap_or_else(|| rest.find('@').map(|i| rest[..i].to_string()).unwrap_or_default())
                } else {
                    String::new()
                }
            } else {
                actor_type
            };

            let args = v.get("args").cloned().unwrap_or_else(|| json!({}));

            let state = match actor_type.as_str() {
                "inference_worker" => {
                    let mut s = InferenceWorkerState::default();
                    s.actor_id = host::self_id();
                    s.worker_id = get_str(&args, "worker_id", "default-worker").to_string();
                    s.mode = "normal".to_string();
                    ActorState::InferenceWorker(s)
                }
                "validator_agent" => {
                    let mut s = ValidatorState::default();
                    s.actor_id = host::self_id();
                    ActorState::Validator(s)
                }
                "pipeline_supervisor" => {
                    let mut s = SupervisorState::default();
                    s.actor_id = host::self_id();
                    ActorState::Supervisor(s)
                }
                "audit_log" => ActorState::AuditLog(AuditLogState::default()),
                _ => return Err(format!("unknown actor_type: {actor_type}")),
            };

            *actor_cell().lock().expect("lock") = state;
            Ok(())
        }

        fn handle(
            _from_actor: String,
            msg_type: String,
            payload: Vec<u8>,
        ) -> Result<Vec<u8>, String> {
            let v = parse_json(&payload);
            let op = get_op(&msg_type, &v);
            let mut guard = actor_cell().lock().expect("lock");

            match &mut *guard {
                ActorState::InferenceWorker(state) => match op.as_str() {
                    "link_with" => {
                        let bare = get_str(&v, "peer_id", "").to_string();
                        let peer_id = sibling_id(&bare, &state.actor_id);
                        if peer_id.is_empty() {
                            return Ok(json!({ "error": "peer_id required" }).to_string().into_bytes());
                        }
                        host::link(&peer_id).ok();
                        if !state.linked_peers.contains(&peer_id) {
                            state.linked_peers.push(peer_id.clone());
                        }
                        Ok(json!({ "status": "ok", "peer_id": peer_id }).to_string().into_bytes())
                    }
                    "unlink_from" => {
                        let bare = get_str(&v, "peer_id",
                            state.linked_peers.first().map(|s| s.as_str()).unwrap_or("")).to_string();
                        let peer_id = sibling_id(&bare, &state.actor_id);
                        host::unlink(&peer_id).ok();
                        state.linked_peers.retain(|p| p != &peer_id);
                        Ok(json!({ "status": "ok", "peer_id": peer_id }).to_string().into_bytes())
                    }
                    _ => handle_inference_worker(state, &msg_type, &payload),
                },
                ActorState::Validator(state) => match op.as_str() {
                    "monitor_worker" => {
                        let bare = get_str(&v, "worker_id", "").to_string();
                        let canonical = sibling_id(&bare, &state.actor_id);
                        if canonical.is_empty() {
                            return Ok(json!({ "error": "worker_id required" }).to_string().into_bytes());
                        }
                        let monitor_ref = host::monitor(&canonical).unwrap_or_default();
                        state.monitor_refs.push(MonitorEntry {
                            worker_id: canonical.clone(),
                            monitor_ref: monitor_ref.clone(),
                        });
                        Ok(json!({ "status": "ok", "monitor_ref": monitor_ref, "worker_id": canonical })
                            .to_string()
                            .into_bytes())
                    }
                    "demonitor_worker" => {
                        let bare = get_str(&v, "worker_id", "").to_string();
                        let canonical = sibling_id(&bare, &state.actor_id);
                        if let Some(idx) = state.monitor_refs.iter().position(|m| m.worker_id == canonical) {
                            let entry = state.monitor_refs.remove(idx);
                            host::demonitor(&entry.monitor_ref).ok();
                            Ok(json!({ "status": "ok", "worker_id": canonical }).to_string().into_bytes())
                        } else {
                            Ok(json!({ "status": "not_found", "worker_id": canonical }).to_string().into_bytes())
                        }
                    }
                    _ => handle_validator(state, &msg_type, &payload),
                },
                ActorState::Supervisor(state) => match op.as_str() {
                    "monitor_worker" => {
                        let bare = get_str(&v, "worker_id", "").to_string();
                        let canonical = sibling_id(&bare, &state.actor_id);
                        if canonical.is_empty() {
                            return Ok(json!({ "error": "worker_id required" }).to_string().into_bytes());
                        }
                        let monitor_ref = host::monitor(&canonical).unwrap_or_default();
                        state.monitor_refs.push(MonitorEntry {
                            worker_id: canonical.clone(),
                            monitor_ref: monitor_ref.clone(),
                        });
                        if !state.worker_pool.contains(&canonical) {
                            state.worker_pool.push(canonical.clone());
                        }
                        Ok(json!({ "status": "ok", "monitor_ref": monitor_ref, "worker_id": canonical })
                            .to_string()
                            .into_bytes())
                    }
                    "demonitor_worker" => {
                        let bare = get_str(&v, "worker_id", "").to_string();
                        let canonical = sibling_id(&bare, &state.actor_id);
                        if let Some(idx) = state.monitor_refs.iter().position(|m| m.worker_id == canonical) {
                            let entry = state.monitor_refs.remove(idx);
                            host::demonitor(&entry.monitor_ref).ok();
                        }
                        state.worker_pool.retain(|w| w != &canonical);
                        Ok(json!({ "status": "ok", "worker_id": canonical }).to_string().into_bytes())
                    }
                    "dispatch" => {
                        if state.worker_pool.is_empty() {
                            return Ok(json!({ "status": "error", "reason": "no_workers_available" })
                                .to_string()
                                .into_bytes());
                        }
                        let idx = (state.next_worker_idx as usize) % state.worker_pool.len();
                        state.next_worker_idx += 1;
                        let worker_id = state.worker_pool[idx].clone();
                        state.total_dispatched += 1;
                        let prompt = get_str(&v, "prompt", "").to_string();
                        let request_id = get_str(&v, "request_id", "").to_string();
                        drop(guard); // release lock before ask
                        let ask_payload = json!({ "op": "infer", "prompt": prompt, "request_id": request_id });
                        let result_bytes = host::ask(&worker_id, "infer", &ask_payload.to_string().into_bytes(), 30_000)
                            .unwrap_or_else(|e| json!({ "error": e }).to_string().into_bytes());
                        let result = parse_json(&result_bytes);
                        let byzantine = result.get("mode").and_then(|m| m.as_str()) == Some("byzantine");
                        return Ok(json!({
                            "status": "ok",
                            "worker_used": worker_id,
                            "request_id": request_id,
                            "result": result,
                            "byzantine_detected": byzantine,
                        })
                        .to_string()
                        .into_bytes());
                    }
                    _ => handle_supervisor(state, &msg_type, &payload),
                },
                ActorState::AuditLog(state) => handle_audit_log(state, &msg_type, &payload),
                ActorState::Uninitialized => Err("actor not initialized".to_string()),
            }
        }

        fn get_state() -> Result<Vec<u8>, String> {
            let guard = actor_cell().lock().expect("lock");
            match &*guard {
                ActorState::InferenceWorker(s) => Ok(encode(s)),
                ActorState::Validator(s) => Ok(encode(s)),
                ActorState::Supervisor(s) => Ok(encode(s)),
                ActorState::AuditLog(s) => Ok(encode(s)),
                ActorState::Uninitialized => Ok(vec![]),
            }
        }

        fn set_state(_bytes: Vec<u8>) -> Result<(), String> {
            // State restoration handled by init() for this example
            Ok(())
        }
    }

    export!(ActorBridge);
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests (host-native, no WASM needed)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_payload(v: Value) -> Vec<u8> {
        v.to_string().into_bytes()
    }

    fn decode_json(bytes: &[u8]) -> Value {
        serde_json::from_slice(bytes).expect("should be valid JSON")
    }

    // ── InferenceWorker tests ────────────────────────────────────────────────

    #[test]
    fn test_inference_normal_mode() {
        let mut state = InferenceWorkerState {
            worker_id: "worker-a".to_string(),
            mode: "normal".to_string(),
            ..Default::default()
        };
        let reply = decode_json(
            &handle_inference_worker(
                &mut state,
                "call",
                &mk_payload(json!({ "op": "infer", "prompt": "Explain the actor model", "request_id": "r1" })),
            )
            .unwrap(),
        );
        assert_eq!(reply["status"], "ok");
        assert_eq!(reply["mode"], "normal");
        assert!(reply["result"].as_str().unwrap().contains("actor model"));
        assert_eq!(state.total_requests, 1);
        assert_eq!(state.error_count, 0);
    }

    #[test]
    fn test_inference_byzantine_mode() {
        let mut state = InferenceWorkerState {
            worker_id: "worker-b".to_string(),
            mode: "byzantine".to_string(),
            ..Default::default()
        };
        let reply = decode_json(
            &handle_inference_worker(
                &mut state,
                "call",
                &mk_payload(json!({ "op": "infer", "prompt": "anything", "request_id": "r2" })),
            )
            .unwrap(),
        );
        assert_eq!(reply["status"], "ok");
        assert_eq!(reply["mode"], "byzantine");
        assert_eq!(state.error_count, 1);
    }

    #[test]
    fn test_inference_set_mode() {
        let mut state = InferenceWorkerState {
            mode: "normal".to_string(),
            ..Default::default()
        };
        let reply = decode_json(
            &handle_inference_worker(
                &mut state,
                "call",
                &mk_payload(json!({ "op": "set_mode", "mode": "byzantine" })),
            )
            .unwrap(),
        );
        assert_eq!(reply["status"], "ok");
        assert_eq!(state.mode, "byzantine");
    }

    #[test]
    fn test_inference_exit_removes_peer() {
        let mut state = InferenceWorkerState {
            linked_peers: vec!["worker-b".to_string()],
            ..Default::default()
        };
        let reply = decode_json(
            &handle_inference_worker(
                &mut state,
                "__EXIT__",
                &mk_payload(json!({ "exit_from": "worker-b", "exit_reason": "error" })),
            )
            .unwrap(),
        );
        assert!(state.linked_peers.is_empty());
    }

    // ── ValidatorAgent tests ─────────────────────────────────────────────────

    #[test]
    fn test_validator_accepts_good_output() {
        let mut state = ValidatorState::default();
        let reply = decode_json(
            &handle_validator(
                &mut state,
                "call",
                &mk_payload(json!({
                    "op": "validate",
                    "worker_id": "w",
                    "result": "The actor model is a mathematical model of concurrent computation.",
                })),
            )
            .unwrap(),
        );
        assert_eq!(reply["status"], "ok");
        assert_eq!(reply["valid"], true);
        assert_eq!(reply["byzantine_suspected"], false);
        assert_eq!(state.pass_count, 1);
    }

    #[test]
    fn test_validator_rejects_byzantine_output() {
        let mut state = ValidatorState::default();
        let reply = decode_json(
            &handle_validator(
                &mut state,
                "call",
                &mk_payload(json!({
                    "op": "validate",
                    "worker_id": "w",
                    "result": "42 is the answer to everything",
                })),
            )
            .unwrap(),
        );
        assert_eq!(reply["valid"], false);
        assert_eq!(reply["byzantine_suspected"], true);
        assert_eq!(state.byzantine_count, 1);
    }

    #[test]
    fn test_validator_flp_threshold() {
        let mut state = ValidatorState::default();
        // 2 byzantine out of 3 = 66% > 33% threshold
        for result in &["42 is the answer to everything", "null", "good long result here"] {
            handle_validator(
                &mut state,
                "call",
                &mk_payload(json!({ "op": "validate", "worker_id": "w", "result": result })),
            )
            .unwrap();
        }
        let status_reply = decode_json(
            &handle_validator(&mut state, "call", &mk_payload(json!({ "op": "status" }))).unwrap(),
        );
        assert_eq!(state.byzantine_count, 2);
        assert_eq!(state.total_validations, 3);
    }

    #[test]
    fn test_validator_down_clears_monitor_ref() {
        let mut state = ValidatorState {
            monitor_refs: vec![MonitorEntry {
                worker_id: "w".to_string(),
                monitor_ref: "ref-123".to_string(),
            }],
            ..Default::default()
        };
        handle_validator(
            &mut state,
            "__DOWN__",
            &mk_payload(json!({
                "monitor_ref": "ref-123",
                "down_from": "w",
                "down_reason": "normal",
            })),
        )
        .unwrap();
        assert!(state.monitor_refs.is_empty());
        assert_eq!(state.down_events.len(), 1);
    }

    // ── PipelineSupervisor tests ─────────────────────────────────────────────

    #[test]
    fn test_supervisor_down_removes_worker() {
        let mut state = SupervisorState {
            worker_pool: vec!["worker-a".to_string()],
            monitor_refs: vec![MonitorEntry {
                worker_id: "worker-a".to_string(),
                monitor_ref: "ref-999".to_string(),
            }],
            ..Default::default()
        };
        handle_supervisor(
            &mut state,
            "__DOWN__",
            &mk_payload(json!({
                "monitor_ref": "ref-999",
                "down_from": "worker-a",
                "down_reason": "crashed",
            })),
        )
        .unwrap();
        assert!(state.worker_pool.is_empty());
        assert!(state.monitor_refs.is_empty());
        assert_eq!(state.down_events_received, 1);
    }

    // ── Byzantine detection helper tests ────────────────────────────────────

    #[test]
    fn test_is_byzantine_response_patterns() {
        assert!(is_byzantine_response("42 is the answer to everything"));
        assert!(is_byzantine_response("null"));
        assert!(is_byzantine_response("short"));
        assert!(!is_byzantine_response(
            "The actor model is a mathematical model of concurrent computation."
        ));
    }
}
