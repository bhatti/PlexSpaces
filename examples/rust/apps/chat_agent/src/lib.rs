// SPDX-License-Identifier: AGPL-3.0-or-later
//
// ChatAgentActor — Cloudflare Agents SDK equivalent (Rust WASM)
//
// Demonstrates conversation state in KV, LLM calls via service link, and
// durable alarm for periodic summarization.
//
// Cloudflare Agents SDK vs PlexSpaces Rust:
//
//   Cloudflare Agents SDK                | PlexSpaces Rust
//   -------------------------------------|--------------------------------------------
//   this.env.AI.run(model, {messages})   | host::http_fetch("llm-link", "POST", ...)
//   await this.storage.get('history')    | kv_get_json::<Vec<ChatMessage>>("history")
//   await this.storage.put('history', v) | kv_put_json("history", &history)
//   storage.setAlarm(ts)                 | alarm_set(ts)
//   async onAlarm() { ... }              | handle "msg_type == __alarm__"
//   connection.send(reply)               | return bytes (JSON-encoded reply)
//   env.AI binding in wrangler.toml      | [service_links.llm-link] in app-config.toml
//   Durable Object per-agent             | virtual_actor + reminder facets
//
// NOTE: LLM calls require ANTHROPIC_API_KEY configured in service_links.
// test.sh validates state and alarm logic only.

use plexspaces_sdk::simple_actor::{alarm_delete, alarm_get, alarm_set, kv_get_json, kv_put_json};
use plexspaces_proto::wasm::v1::{HttpFetchRequest, HttpFetchResponse};
use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_logging::{log, now_ms};
use plexspaces::actor::host_kv::{kv_delete, kv_put};
use plexspaces::actor::host_http::http_fetch;

// ========================================================================
// Constants (equivalent to DO thresholds)
// ========================================================================

const ALARM_THRESHOLD: usize = 10;
const ALARM_DELAY_MS: u64 = 300_000; // 5 minutes
const LLM_LINK: &str = "llm-link";

// ========================================================================
// Types
// ========================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
    timestamp: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AgentState {
    actor_id: String,
    total_messages: u64,
    total_summarizations: u64,
}

// ========================================================================
// State cell
// ========================================================================

fn state_cell() -> &'static Mutex<AgentState> {
    static STATE: OnceLock<Mutex<AgentState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(AgentState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut AgentState) -> T) -> T {
    let mut g = state_cell().lock().expect("agent state lock poisoned");
    f(&mut *g)
}

// ========================================================================
// WIT plumbing
// ========================================================================

struct ChatAgentActor;

impl Guest for ChatAgentActor {
    fn init(config: Vec<u8>) -> Result<(), String> {
        // Parse JSON config from the framework
        if let Ok(cfg) = serde_json::from_slice::<Value>(&config) {
            let actor_id = cfg
                .get("actor_id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            with_state(|s| s.actor_id = actor_id.clone());
            log("info", &format!("ChatAgentActor init actor_id={actor_id}"));
        }
        Ok(())
    }

    fn handle(
        _from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let payload_json: Value = serde_json::from_slice(&payload).unwrap_or(json!({}));

        let reply = match msg_type.as_str() {
            "chat" => handle_chat(&payload_json),
            "get_history" => handle_get_history(),
            "clear" => handle_clear(),
            "__alarm__" => handle_alarm(),
            other => json!({ "error": format!("unknown op: {other}") }),
        };

        Ok(serde_json::to_vec(&reply).unwrap_or_default())
    }

    fn get_state() -> Result<Vec<u8>, String> {
        let s = state_cell().lock().expect("state lock");
        serde_json::to_vec(&*s).map_err(|e| e.to_string())
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        let loaded: AgentState = serde_json::from_slice(&state).map_err(|e| e.to_string())?;
        let mut g = state_cell().lock().expect("state lock");
        *g = loaded;
        Ok(())
    }
}

// ========================================================================
// Handlers
// ========================================================================

/// chat — equivalent to Cloudflare Agents SDK onMessage():
///   1. Load history from KV (this.storage.get)
///   2. Append user message
///   3. Call LLM via service link (this.env.AI.run)
///   4. Append response, persist to KV (this.storage.put)
///   5. Schedule alarm after threshold (storage.setAlarm)
fn handle_chat(payload: &Value) -> Value {
    let message = match payload.get("message").and_then(Value::as_str) {
        Some(m) if !m.is_empty() => m.to_string(),
        _ => return json!({ "error": "message is required" }),
    };

    // Load history — equivalent to: await this.storage.get('history')
    let mut history: Vec<ChatMessage> = kv_get_json("history")
        .unwrap_or(None)
        .unwrap_or_default();

    let now = now_ms();
    history.push(ChatMessage { role: "user".into(), content: message, timestamp: now });

    // Call LLM via service link — equivalent to: await this.env.AI.run(model, {messages})
    let assistant_reply = call_llm(&history);

    history.push(ChatMessage {
        role: "assistant".into(),
        content: assistant_reply.clone(),
        timestamp: now_ms(),
    });

    // Persist history — equivalent to: await this.storage.put('history', history)
    let _ = kv_put_json("history", &history);

    with_state(|s| s.total_messages += 1);

    // Schedule alarm after threshold — equivalent to: storage.setAlarm(ts)
    if history.len() > ALARM_THRESHOLD {
        if alarm_get().unwrap_or(0) == 0 {
            let _ = alarm_set(now_ms() + ALARM_DELAY_MS);
            log("info", "ChatAgentActor: alarm set for summarization in 5 minutes");
        }
    }

    json!({
        "status": "ok",
        "reply": assistant_reply,
        "history_length": history.len(),
    })
}

/// get_history — returns stored conversation history from KV.
fn handle_get_history() -> Value {
    let history: Vec<ChatMessage> = kv_get_json("history")
        .unwrap_or(None)
        .unwrap_or_default();
    json!({
        "status": "ok",
        "history": history,
        "count": history.len(),
    })
}

/// clear — clear history, summary, and pending alarm.
/// Equivalent to: storage.delete('history'); storage.deleteAlarm()
fn handle_clear() -> Value {
    let _ = kv_delete("history");
    let _ = kv_delete("summary");
    let _ = alarm_delete();
    json!({ "status": "ok", "cleared": true })
}

/// __alarm__ — durable alarm callback, equivalent to Cloudflare Agents SDK onAlarm().
/// Summarizes conversation history and stores a summary KV key, then clears history.
fn handle_alarm() -> Value {
    log("info", "ChatAgentActor: alarm fired — summarizing history");

    let history: Vec<ChatMessage> = kv_get_json("history")
        .unwrap_or(None)
        .unwrap_or_default();

    if history.is_empty() {
        return json!({ "status": "ok", "action": "no_history_to_summarize" });
    }

    let count = history.len();

    // Summarize via LLM
    let summary_content = format!(
        "Summarize this conversation concisely (2-3 sentences): {}",
        serde_json::to_string(
            &history
                .iter()
                .map(|m| json!({ "role": m.role, "content": m.content }))
                .collect::<Vec<_>>()
        )
        .unwrap_or_default()
    );
    let summary_msgs = vec![ChatMessage {
        role: "user".into(),
        content: summary_content,
        timestamp: now_ms(),
    }];
    let summary = call_llm(&summary_msgs);

    // Persist summary, clear history
    let _ = kv_put("summary", summary.as_bytes());
    let _ = kv_delete("history");

    with_state(|s| s.total_summarizations += 1);

    log("info", &format!("ChatAgentActor: summarized {count} messages"));

    json!({
        "status": "ok",
        "action": "summarized",
        "messages_summarized": count,
    })
}

// ========================================================================
// LLM helper
// ========================================================================

/// Call the LLM via the `llm-link` service link.
/// Equivalent to: await this.env.AI.run(model, { messages })
fn call_llm(messages: &[ChatMessage]) -> String {
    let body = json!({
        "model": "claude-3-5-haiku-20241022",
        "max_tokens": 1024,
        "messages": messages.iter().map(|m| json!({ "role": m.role, "content": m.content })).collect::<Vec<_>>(),
    });
    let body_bytes = serde_json::to_vec(&body).unwrap_or_default();

    let request = HttpFetchRequest {
        request_id: String::new(),
        headers: Default::default(),
        body: body_bytes,
    };
    let encoded = request.encode_to_vec();

    match http_fetch(LLM_LINK, "POST", "/v1/messages", &encoded) {
        Err(e) => {
            log("warn", &format!("ChatAgentActor: LLM call failed: {e}"));
            "[LLM unavailable — message stored]".to_string()
        }
        Ok(raw) => {
            let resp = match HttpFetchResponse::decode(raw.as_slice()) {
                Ok(r) => r,
                Err(_) => return "[no response]".to_string(),
            };
            // Parse JSON body — Anthropic: content[0].text; OpenAI: choices[0].message.content
            if let Ok(parsed) = serde_json::from_slice::<Value>(&resp.body) {
                if let Some(text) = parsed
                    .get("content")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(|block| block.get("text"))
                    .and_then(Value::as_str)
                {
                    return text.to_string();
                }
                if let Some(text) = parsed
                    .get("choices")
                    .and_then(Value::as_array)
                    .and_then(|arr| arr.first())
                    .and_then(|c| c.get("message"))
                    .and_then(|m| m.get("content"))
                    .and_then(Value::as_str)
                {
                    return text.to_string();
                }
            }
            "[no response]".to_string()
        }
    }
}

export!(ChatAgentActor);
