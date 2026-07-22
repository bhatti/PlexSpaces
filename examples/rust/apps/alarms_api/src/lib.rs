// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Alarms API Example — Rust WASM actor
//
// Demonstrates the Cloudflare Durable Objects alarm() pattern: a RequestQueue
// actor that batches incoming requests and processes them 10 seconds after the
// first write, using a durable alarm that survives actor deactivation.
//
// ## Cloudflare DO vs PlexSpaces Rust
//
// | Cloudflare DO                             | PlexSpaces Rust                        |
// |-------------------------------------------|----------------------------------------|
// | export class RequestQueue extends DO      | struct RequestQueueState (WASM actor)  |
// | this.ctx.storage.get('count')             | host::kv_get("count")                  |
// | this.ctx.storage.put('count', n)          | host::kv_put("count", ...)             |
// | this.ctx.storage.setAlarm(Date.now()+10s) | host::alarm_set(now_ms + 10_000)       |
// | this.ctx.storage.getAlarm()               | host::alarm_get()                      |
// | async alarm() { ... }                     | "__alarm__" message handler            |
// | new Response(JSON.stringify(result))      | serde_json::to_vec(&json!({...}))      |
// | wrangler.toml [[durable_objects]]         | app-config.toml [[supervisor.children]] |

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::self_id;
use plexspaces::actor::host_kv::{alarm_delete, alarm_get, alarm_set};
use plexspaces::actor::host_logging::{log, now_ms};

// ============================================================================
// Actor state
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct QueueItem {
    id: u64,
    data: Value,
    enqueued_at: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct RequestQueueState {
    actor_id: String,
    items: Vec<QueueItem>,
    count: u64,
    total_processed: u64,
    total_alarm_fires: u64,
}

fn state_cell() -> &'static Mutex<RequestQueueState> {
    static STATE: OnceLock<Mutex<RequestQueueState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(RequestQueueState::default()))
}

fn with_state<T>(f: impl FnOnce(&mut RequestQueueState) -> T) -> T {
    let mut guard = state_cell().lock().expect("state lock poisoned");
    f(&mut guard)
}

// ============================================================================
// Helpers
// ============================================================================

fn json_bytes(v: Value) -> Vec<u8> {
    serde_json::to_vec(&v).unwrap_or_else(|_| b"{\"error\":\"serialize failed\"}".to_vec())
}

fn json_err(msg: &str) -> Vec<u8> {
    json_bytes(json!({"error": msg}))
}

fn parse_payload(bytes: &[u8]) -> Value {
    serde_json::from_slice(bytes).unwrap_or(Value::Null)
}

// ============================================================================
// Handlers
// ============================================================================

/// Add an item to the queue.
/// Sets a durable alarm 10 seconds from now on the FIRST item only.
/// Equivalent to: if (count === 0) { this.ctx.storage.setAlarm(Date.now() + 10_000) }
fn handle_enqueue(payload: &Value) -> Vec<u8> {
    with_state(|s| {
        let was_empty = s.count == 0;
        let item_data = payload.get("item").cloned().unwrap_or(payload.clone());
        let now_ms = now_ms();
        let item = QueueItem {
            id: s.count + 1,
            data: item_data,
            enqueued_at: now_ms,
        };
        s.items.push(item.clone());
        s.count += 1;

        let mut did_set_alarm = false;
        if was_empty {
            // First item: schedule alarm 10 seconds from now.
            let fire_at = now_ms + 10_000;
            match alarm_set(fire_at) {
                Ok(_) => {
                    did_set_alarm = true;
                    let _ = log("info", &format!(
                        "RequestQueue {}: first item, alarm set for 10s at {}",
                        s.actor_id, fire_at
                    ));
                }
                Err(e) => {
                    return json_err(&format!("alarm_set failed: {:?}", e));
                }
            }
        }

        json_bytes(json!({
            "status": "ok",
            "queued": s.count,
            "item_id": item.id,
            "alarm_set": did_set_alarm,
        }))
    })
}

/// Return current queue depth and next alarm timestamp.
/// Equivalent to Cloudflare DO: this.ctx.storage.getAlarm()
fn handle_status() -> Vec<u8> {
    with_state(|s| {
        let (alarm_at, alarm_set) = match alarm_get() {
            Ok(ts) => (ts, ts > 0),
            Err(_) => (0u64, false),
        };
        json_bytes(json!({
            "status": "ok",
            "count": s.count,
            "alarm_at": alarm_at,
            "alarm_set": alarm_set,
            "total_processed": s.total_processed,
            "total_alarm_fires": s.total_alarm_fires,
        }))
    })
}

/// Clear queue and cancel pending alarm (for test repeatability).
fn handle_reset() -> Vec<u8> {
    with_state(|s| {
        s.items.clear();
        s.count = 0;
        let _ = alarm_delete();
        let _ = log("info", &format!("RequestQueue {}: queue reset", s.actor_id));
        json_bytes(json!({"status": "ok", "reset": true}))
    })
}

/// Process all queued items when the alarm fires.
/// Equivalent to Cloudflare DO: async alarm() { ... }
/// Delivered by the PlexSpaces reminder facet as the "__alarm__" message type.
fn handle_alarm() -> Vec<u8> {
    with_state(|s| {
        let processed = s.count;
        s.total_alarm_fires += 1;
        s.total_processed += processed;

        let _ = log("info", &format!(
            "RequestQueue {}: alarm fired, processing {} items",
            s.actor_id, processed
        ));

        for item in &s.items {
            let _ = log("info", &format!(
                "RequestQueue {}: processing item {}: {}",
                s.actor_id, item.id, item.data
            ));
        }

        // Clear the queue
        s.items.clear();
        s.count = 0;

        json_bytes(json!({
            "status": "ok",
            "processed": processed,
            "total_processed": s.total_processed,
            "total_alarm_fires": s.total_alarm_fires,
        }))
    })
}

// ============================================================================
// WIT Guest implementation
// ============================================================================

struct RequestQueueActor;

impl Guest for RequestQueueActor {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let v = parse_payload(&config);
        let actor_id = v
            .get("actor_id")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        with_state(|s| {
            s.actor_id = if actor_id.is_empty() {
                self_id()
            } else {
                actor_id
            };
        });
        let _ = log("info", &format!(
            "RequestQueue {}: initialized",
            with_state(|s| s.actor_id.clone())
        ));
        Ok(())
    }

    fn handle(
        _from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let v = parse_payload(&payload);
        let result = match msg_type.as_str() {
            "enqueue" => handle_enqueue(&v),
            "status" => handle_status(),
            "reset" => handle_reset(),
            "__alarm__" => handle_alarm(),
            other => json_err(&format!("unknown operation: {}", other)),
        };
        Ok(result)
    }

    fn get_state() -> Result<Vec<u8>, String> {
        Ok(with_state(|s| serde_json::to_vec(s).unwrap_or_default()))
    }

    fn set_state(state_bytes: Vec<u8>) -> Result<(), String> {
        if let Ok(loaded) = serde_json::from_slice::<RequestQueueState>(&state_bytes) {
            with_state(|s| *s = loaded);
        }
        Ok(())
    }
}

export!(RequestQueueActor);
