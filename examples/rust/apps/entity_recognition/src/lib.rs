// SPDX-License-Identifier: LGPL-2.1-or-later
//
// Entity recognition (Rust WASM): simplified document → entity extraction.
// SDK: `#[gen_server_actor(wasm)]` + `host::application_metrics_add`.
//
// Full native multi-actor FSM / GenServer / GenEvent demo: `examples/rust/embedded/entity_recognition/`.

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedEntity {
    entity_type: String,
    value: String,
    confidence: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PipelineState {
    application_id: String,
    documents_processed: u64,
    entities_total: u64,
    last_doc_id: String,
    total_compute_ms: f64,
    total_coord_ms: f64,
}

fn state_cell() -> &'static Mutex<PipelineState> {
    static STATE: OnceLock<Mutex<PipelineState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PipelineState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut PipelineState) -> T) -> T {
    let mut g = state_cell().lock().expect("entity_recognition state lock poisoned");
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
        .expect("entity_recognition state lock")
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

const COMPUTE_MS_BASE: f64 = 0.5;
const COORD_MS_PER_DOC: f64 = 0.06;
const COMPUTE_MS_PER_ENTITY: f64 = 0.02;

fn metrics_for_doc(compute_ms: u64, coord_ms: u64, entities_found: u64) -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": {
            "entity_recognition_documents": 1,
            "entity_recognition_entities_delta": entities_found,
        },
        "latency_totals_ms": {
            "entity_recognition.compute": compute_ms,
            "entity_recognition.coordination": coord_ms,
        },
        "latency_max_ms": {
            "entity_recognition.compute": compute_ms,
            "entity_recognition.coordination": coord_ms,
        },
        "latency_samples": {
            "entity_recognition.compute": 1,
            "entity_recognition.coordination": 1,
        },
    })
}

fn metrics_status_query() -> serde_json::Value {
    serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "entity_recognition_status_queries": 1 },
    })
}

/// Deterministic, dependency-free extraction (demo rules only).
pub fn extract_entities_from_text(text: &str) -> Vec<ExtractedEntity> {
    let mut out: Vec<ExtractedEntity> = Vec::new();
    for raw in text.split_whitespace() {
        let w = raw
            .trim_matches(|c| c == '.' || c == ',' || c == ';' || c == ':' || c == '(' || c == ')');
        if w.is_empty() {
            continue;
        }
        if w.contains('@') && w.contains('.') {
            out.push(ExtractedEntity {
                entity_type: "EMAIL".into(),
                value: w.to_string(),
                confidence: 0.85,
            });
        } else if w.starts_with("http://") || w.starts_with("https://") {
            out.push(ExtractedEntity {
                entity_type: "URL".into(),
                value: w.to_string(),
                confidence: 0.9,
            });
        } else if w.len() >= 2
            && w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
            && w.chars().all(|c| c.is_alphanumeric())
        {
            out.push(ExtractedEntity {
                entity_type: "TOKEN".into(),
                value: w.to_string(),
                confidence: 0.35,
            });
        }
        if out.len() >= 64 {
            break;
        }
    }
    out
}

fn handle_process_document(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let doc_id = payload
        .get("doc_id")
        .and_then(|v| v.as_str())
        .unwrap_or("doc")
        .to_string();
    let content = payload
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let entities = extract_entities_from_text(&content);
    let n = entities.len() as u64;
    let compute_ms = COMPUTE_MS_BASE + COMPUTE_MS_PER_ENTITY * (n as f64);
    let coord_ms = COORD_MS_PER_DOC;
    let compute_u = compute_ms.max(0.001) as u64;
    let coord_u = coord_ms as u64;

    with_state(|s| {
        s.documents_processed += 1;
        s.entities_total += n;
        s.last_doc_id = doc_id.clone();
        s.total_compute_ms += compute_ms;
        s.total_coord_ms += coord_ms;
    });

    let m = metrics_for_doc(compute_u, coord_u, n);
    if let Err(err) = merge_application_metrics_for(&application_id, m, "process_document metrics") {
        return serde_json::json!({ "error": err }).to_string();
    }

    serde_json::json!({
        "doc_id": doc_id,
        "stage": "complete",
        "entity_count": n,
        "entities": entities,
    })
    .to_string()
}

fn handle_reset() -> String {
    let application_id = resolve_application_id();
    with_state(|s| {
        s.documents_processed = 0;
        s.entities_total = 0;
        s.last_doc_id.clear();
        s.total_compute_ms = 0.0;
        s.total_coord_ms = 0.0;
    });
    let m = serde_json::json!({
        "message_count": 1,
        "counter_metrics": { "entity_recognition_resets": 1 },
        "latency_totals_ms": {
            "entity_recognition.compute": 1u64,
            "entity_recognition.coordination": 1u64,
        },
        "latency_max_ms": {
            "entity_recognition.compute": 1u64,
            "entity_recognition.coordination": 1u64,
        },
        "latency_samples": {
            "entity_recognition.compute": 1,
            "entity_recognition.coordination": 1,
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
            "documents_processed": s.documents_processed,
            "entities_total": s.entities_total,
            "last_doc_id": s.last_doc_id,
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
            "documents_processed": s.documents_processed,
            "entities_total": s.entities_total,
            "last_doc_id": s.last_doc_id,
            "total_compute_ms": s.total_compute_ms,
            "total_coord_ms": s.total_coord_ms,
            "compute_pct": compute_pct,
            "coord_pct": coord_pct,
            "granularity_ratio": gran,
            "use_case": "entity_recognition",
            "orchestration": "single_actor"
        })
        .to_string()
    })
}

/// Batch process for throughput demo: `{ "documents": [ { "doc_id", "content" }, ... ] }` (max 50).
fn handle_batch(payload: &serde_json::Value) -> String {
    let application_id = resolve_application_id();
    let arr = match payload.get("documents").and_then(|v| v.as_array()) {
        Some(a) if !a.is_empty() => a,
        _ => {
            return serde_json::json!({ "error": "missing non-empty documents array" }).to_string();
        }
    };
    let n = arr.len().min(50);
    for item in arr.iter().take(n) {
        let out = handle_process_document(item);
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&out) {
            if v.get("error").is_some() && v.get("entity_count").is_none() {
                return out;
            }
        } else {
            return out;
        }
    }
    if let Err(e) = merge_application_metrics_for(
        &application_id,
        serde_json::json!({
            "message_count": 1,
            "counter_metrics": {
                "entity_recognition_batch_runs": 1,
                "entity_recognition_batch_docs_delta": n as u64,
            },
        }),
        "batch rollup",
    ) {
        return serde_json::json!({ "error": e }).to_string();
    }
    with_state(|s| {
        serde_json::json!({
            "batch_docs": n,
            "documents_processed": s.documents_processed,
            "entities_total": s.entities_total,
        })
        .to_string()
    })
}

#[gen_server_actor(wasm)]
#[derive(Default)]
struct EntityRecognitionActor;

#[plexspaces_handlers(wasm)]
impl EntityRecognitionActor {
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

    #[handler("process_document")]
    fn process_document_op(
        &mut self,
        _from_actor: &str,
        payload_json: &str,
    ) -> Result<String, String> {
        let payload: serde_json::Value =
            serde_json::from_str(payload_json).map_err(|e| format!("invalid payload: {}", e))?;
        Ok(handle_process_document(&payload))
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

struct EntityRecognitionBridge;

impl Guest for EntityRecognitionBridge {
    fn init(config_json: String) -> String {
        let mut actor = EntityRecognitionActor::default();
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
        let mut actor = EntityRecognitionActor::default();
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
        match serde_json::from_str::<PipelineState>(&state_json) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = s;
                String::new()
            }
            Err(_) => "ERROR: invalid state JSON".to_string(),
        }
    }
}

export!(EntityRecognitionBridge);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_op_reads_embedded_op() {
        assert_eq!(
            parse_op("call", r#"{"op":"process_document","doc_id":"d","content":"x"}"#)
                .expect("op"),
            "process_document"
        );
    }

    #[test]
    fn extracts_email_and_url() {
        let t = "Write alice@example.com or see https://example.org/path .";
        let e = extract_entities_from_text(t);
        let types: Vec<&str> = e.iter().map(|x| x.entity_type.as_str()).collect();
        assert!(types.contains(&"EMAIL"), "{types:?}");
        assert!(types.contains(&"URL"), "{types:?}");
    }
}
