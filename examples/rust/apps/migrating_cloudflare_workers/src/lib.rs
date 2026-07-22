// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Guild Chat Server - Discord-style Real-Time Chat with Durable Objects (Rust WASM)
//
// Demonstrates Cloudflare Workers Durable Objects pattern for real-time chat:
// - ChatRoomActor: per-room state, member tracking, message fan-out, history
// - RateLimiterActor: per-user token bucket rate limiting (spam prevention)
// - AlarmDemoActor: durable alarm lifecycle (setAlarm equivalent)
//
// Inspired by:
// - Discord's guild process architecture (one Elixir GenServer per guild)
// - Cloudflare's workers-chat-demo (Durable Object per room + rate limiter)
//
// ## SDK Features Used
//
// - plexspaces_sdk::simple_actor helpers: kv_multi_get/put, kv_increment, kv_cas, alarm_*
// - host::send(): Fire-and-forget fan-out to member actors
// - host::kv_put()/host::kv_get(): Durable storage (message history persistence)
// - host::now_ms(): Timestamps for messages and token bucket refill
// - host::self_id(): Actor identity for room ID extraction
//
// ## Comparison to Cloudflare Workers / Durable Objects
//
// | Cloudflare Workers/DO             | PlexSpaces Rust                   |
// |-----------------------------------|-----------------------------------|
// | export class ChatRoom extends DO  | struct ChatRoomState + WIT binds  |
// | env.CHAT_ROOM.get(id)             | host::send(actorID, ...)          |
// | this.state.storage.put/get        | host::kv_put()/host::kv_get()     |
// | fetch(request) handler            | impl Guest { fn handle() }        |
// | blockConcurrencyWhile()           | State restored from host::kv_*    |
// | WebSocket accept/send             | host::send() fan-out to members   |
// | storage.setAlarm(timestamp)       | alarm_set(timestamp_ms)           |
// | storage.getAlarm()                | alarm_get()                       |
// | alarm() scheduled callback        | "__alarm__" handler               |
// | CAS / transactional put           | kv_cas(key, expected, new)        |
// | Atomic counter (R2/KV)            | kv_increment(key, delta)          |
// | Batch storage.get([k1,k2,...])    | kv_multi_get(keys)                |
// | Batch storage.put({k:v,...})      | kv_multi_put(entries)             |
// | wrangler.toml [[bindings]]        | app-config.toml [[children]]      |
// | Worker script routing             | actor_type prefix matching        |

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::send;
use plexspaces::actor::host_kv::{kv_get, kv_put};
use plexspaces::actor::host_logging::{log, now_ms};
use plexspaces_sdk::simple_actor::{
    alarm_delete, alarm_get, alarm_set, kv_cas, kv_increment, kv_multi_get, kv_multi_put,
};

// ============================================================================
// Helpers
// ============================================================================

fn parse_payload(payload: &[u8]) -> Value {
    if payload.is_empty() {
        return json!({});
    }
    serde_json::from_slice(payload).unwrap_or_else(|_| json!({}))
}

fn parse_op(msg_type: &str, payload: &[u8]) -> Result<String, String> {
    let v = parse_payload(payload);
    if let Some(op) = v.get("op").and_then(|o| o.as_str()) {
        return Ok(op.to_string());
    }
    if msg_type == "call" || msg_type == "cast" {
        return Err("missing op".to_string());
    }
    Ok(msg_type.to_string())
}

fn json_bytes(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

fn json_err(msg: impl Into<String>) -> Vec<u8> {
    json_bytes(json!({ "error": msg.into() }))
}

fn room_id_from_actor_id(actor_id: &str) -> String {
    if let Some((name, _)) = actor_id.split_once("//") {
        return name.to_string();
    }
    actor_id.to_string()
}

const MAX_HISTORY: usize = 100;

// ============================================================================
// Per-instance actor state (OnceLock + Mutex for WASM single-threaded safety)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "actor_type")]
enum ActorState {
    ChatRoom(ChatRoomState),
    RateLimiter(RateLimiterState),
    AlarmDemo(AlarmDemoState),
}

fn state_cell() -> &'static Mutex<Option<ActorState>> {
    static STATE: OnceLock<Mutex<Option<ActorState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn with_state<T>(f: impl FnOnce(&mut ActorState) -> T) -> Result<T, String> {
    let mut g = state_cell().lock().expect("guild_chat state lock poisoned");
    match g.as_mut() {
        Some(s) => Ok(f(s)),
        None => Err("actor not initialized".to_string()),
    }
}

// ============================================================================
// ChatRoomActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MemberInfo {
    user_id: String,
    joined_at: u64,
    msg_count: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChatMessage {
    seq: u64,
    user_id: String,
    content: String,
    timestamp: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChatRoomState {
    room_id: String,
    members: HashMap<String, MemberInfo>,
    messages: Vec<ChatMessage>,
    msg_seq: u64,
    total_messages: u64,
    total_joins: u64,
    total_leaves: u64,
    total_broadcasts: u64,
    total_compute_ms: f64,
}

impl ChatRoomState {
    fn add_message(&mut self, user_id: &str, content: &str, timestamp: u64) -> ChatMessage {
        self.msg_seq += 1;
        let msg = ChatMessage {
            seq: self.msg_seq,
            user_id: user_id.to_string(),
            content: content.to_string(),
            timestamp,
        };
        self.messages.push(msg.clone());
        if self.messages.len() > MAX_HISTORY {
            let trim = self.messages.len() - MAX_HISTORY;
            self.messages.drain(0..trim);
        }
        msg
    }

    fn persist_history(&self) {
        if self.messages.is_empty() {
            return;
        }
        // Build entries for kv_multi_put: batch write each message under its seq key.
        // Mirrors Cloudflare DO storage.put({k1:v1, k2:v2, ...}) batch API.
        let mut entries_data: Vec<(String, Vec<u8>)> = Vec::new();
        let mut seqs: Vec<u64> = Vec::new();
        for msg in &self.messages {
            if let Ok(data) = serde_json::to_vec(msg) {
                entries_data.push((format!("room:{}:msg:{}", self.room_id, msg.seq), data));
                seqs.push(msg.seq);
            }
        }
        if let Ok(idx) = serde_json::to_vec(&seqs) {
            entries_data.push((format!("room:{}:seq_index", self.room_id), idx));
        }
        let entries: Vec<(&str, &[u8])> = entries_data
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        if kv_multi_put(&entries).is_err() {
            // Fallback: single-key write
            if let Ok(data) = serde_json::to_vec(&self.messages) {
                let _ = kv_put(&format!("room:{}:history", self.room_id), &data);
            }
        }
    }

    fn load_history(&mut self) {
        let index_key = format!("room:{}:seq_index", self.room_id);
        let index_raw = kv_get(&index_key).unwrap_or_default();
        if index_raw.is_empty() {
            // Fallback: legacy single-key blob
            let history_key = format!("room:{}:history", self.room_id);
            if let Ok(stored) = kv_get(&history_key) {
                if !stored.is_empty() {
                    if let Ok(msgs) = serde_json::from_slice::<Vec<ChatMessage>>(&stored) {
                        self.msg_seq = msgs.last().map(|m| m.seq).unwrap_or(0);
                        self.messages = msgs;
                    }
                }
            }
            return;
        }
        let seqs: Vec<u64> = match serde_json::from_slice(&index_raw) {
            Ok(s) => s,
            Err(_) => return,
        };
        if seqs.is_empty() {
            return;
        }
        let keys: Vec<String> = seqs
            .iter()
            .map(|seq| format!("room:{}:msg:{}", self.room_id, seq))
            .collect();
        let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
        if let Ok(values) = kv_multi_get(&key_refs) {
            let mut msgs = Vec::new();
            for v in values {
                if let Some(bytes) = v {
                    if let Ok(msg) = serde_json::from_slice::<ChatMessage>(&bytes) {
                        msgs.push(msg);
                    }
                }
            }
            self.msg_seq = msgs.last().map(|m| m.seq).unwrap_or(0);
            self.messages = msgs;
        }
    }
}

fn chatroom_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v.get("actor_id").and_then(|a| a.as_str()).unwrap_or("");
    let room_id = room_id_from_actor_id(actor_id);
    let mut state = ChatRoomState {
        room_id: room_id.clone(),
        ..Default::default()
    };
    // Like blockConcurrencyWhile() — restore persisted history
    state.load_history();
    let _ = log(
        "info",
        &format!(
            "ChatRoom {room_id}: restored {} messages",
            state.messages.len()
        ),
    );
    let mut g = state_cell().lock().expect("guild_chat state lock poisoned");
    *g = Some(ActorState::ChatRoom(state));
    Ok(())
}

fn chatroom_handle(msg_type: &str, payload: &[u8]) -> Vec<u8> {
    let op = match parse_op(msg_type, payload) {
        Ok(o) => o,
        Err(e) => return json_err(e),
    };
    let v = parse_payload(payload);
    match op.as_str() {
        "join" => {
            let user_id = v.get("user_id").and_then(|u| u.as_str()).unwrap_or("");
            if user_id.is_empty() {
                return json_err("user_id required");
            }
            with_state(|s| {
                let ActorState::ChatRoom(ref mut cr) = s else {
                    return json_err("wrong actor type");
                };
                let now = now_ms();
                if cr.members.contains_key(user_id) {
                    return json_bytes(json!({
                        "status": "ok",
                        "action": "already_joined",
                        "user_id": user_id,
                        "room_id": cr.room_id,
                        "members": cr.members.len()
                    }));
                }
                cr.members.insert(
                    user_id.to_string(),
                    MemberInfo {
                        user_id: user_id.to_string(),
                        joined_at: now,
                        msg_count: 0,
                    },
                );
                cr.total_joins += 1;
                cr.add_message("system", &format!("{user_id} joined the room"), now);
                json_bytes(json!({
                    "status": "ok",
                    "action": "joined",
                    "user_id": user_id,
                    "room_id": cr.room_id,
                    "members": cr.members.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "leave" => {
            let user_id = v.get("user_id").and_then(|u| u.as_str()).unwrap_or("");
            with_state(|s| {
                let ActorState::ChatRoom(ref mut cr) = s else {
                    return json_err("wrong actor type");
                };
                if !cr.members.contains_key(user_id) {
                    return json_bytes(json!({
                        "status": "ok", "action": "not_member", "user_id": user_id
                    }));
                }
                cr.members.remove(user_id);
                cr.total_leaves += 1;
                let now = now_ms();
                cr.add_message("system", &format!("{user_id} left the room"), now);
                json_bytes(json!({
                    "status": "ok",
                    "action": "left",
                    "user_id": user_id,
                    "room_id": cr.room_id,
                    "members": cr.members.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "send_message" => {
            let user_id = v.get("user_id").and_then(|u| u.as_str()).unwrap_or("");
            let content = v.get("content").and_then(|c| c.as_str()).unwrap_or("");
            if user_id.is_empty() || content.is_empty() {
                return json_err("user_id and content required");
            }
            with_state(|s| {
                let ActorState::ChatRoom(ref mut cr) = s else {
                    return json_err("wrong actor type");
                };
                if !cr.members.contains_key(user_id) {
                    return json_err("not a member of this room");
                }
                let compute_start = now_ms();
                if let Some(m) = cr.members.get_mut(user_id) {
                    m.msg_count += 1;
                }
                let now = now_ms();
                let msg = cr.add_message(user_id, content, now);
                // Fan-out: send to every other member actor via host::send()
                // Mirrors Discord's Manifold pattern for distributed fan-out
                let member_ids: Vec<String> = cr
                    .members
                    .keys()
                    .filter(|id| id.as_str() != user_id)
                    .cloned()
                    .collect();
                let out_str = json!({
                    "room_id": cr.room_id,
                    "seq": msg.seq,
                    "from": user_id,
                    "content": content,
                    "timestamp": now,
                })
                .to_string();
                let fan_out = member_ids.len();
                for mid in &member_ids {
                    let _ = send(mid, "receive_message", out_str.as_bytes());
                    cr.total_broadcasts += 1;
                }
                cr.persist_history();
                let compute_ms = (now_ms() - compute_start) as f64;
                cr.total_compute_ms += compute_ms;
                cr.total_messages += 1;
                json_bytes(json!({
                    "status": "ok",
                    "seq": msg.seq,
                    "room_id": cr.room_id,
                    "user_id": user_id,
                    "fan_out": fan_out,
                    "members": cr.members.len(),
                    "history_size": cr.messages.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "send_message_batch" => {
            let count = v
                .get("count")
                .and_then(|c| c.as_u64())
                .unwrap_or(1000) as usize;
            let user_id = v
                .get("user_id")
                .and_then(|u| u.as_str())
                .unwrap_or("bench-user")
                .to_string();
            let content = v
                .get("content")
                .and_then(|c| c.as_str())
                .unwrap_or("Benchmark message payload with enough data for realistic sizing")
                .to_string();
            with_state(|s| {
                let ActorState::ChatRoom(ref mut cr) = s else {
                    return json_err("wrong actor type");
                };
                let now0 = now_ms();
                cr.members.entry(user_id.clone()).or_insert(MemberInfo {
                    user_id: user_id.clone(),
                    joined_at: now0,
                    msg_count: 0,
                });
                for i in 0..10 {
                    let uid = format!("fan-out-member-{i}");
                    cr.members.entry(uid.clone()).or_insert(MemberInfo {
                        user_id: uid,
                        joined_at: now0,
                        msg_count: 0,
                    });
                }
                let compute_start = now_ms();
                let mut sent = 0usize;
                let mut total_fan_out = 0usize;
                for i in 0..count {
                    if let Some(m) = cr.members.get_mut(&user_id) {
                        m.msg_count += 1;
                    }
                    let now = now_ms();
                    let msg_content = format!("{content} #{i}");
                    let msg = cr.add_message(&user_id, &msg_content, now);
                    let out_str = json!({
                        "room_id": cr.room_id,
                        "seq": msg.seq,
                        "from": user_id,
                        "content": msg_content,
                        "timestamp": now,
                    })
                    .to_string();
                    let member_ids: Vec<String> = cr
                        .members
                        .keys()
                        .filter(|id| *id != &user_id)
                        .cloned()
                        .collect();
                    for mid in &member_ids {
                        let _ = send(mid, "receive_message", out_str.as_bytes());
                        cr.total_broadcasts += 1;
                        total_fan_out += 1;
                    }
                    cr.total_messages += 1;
                    sent += 1;
                }
                cr.persist_history();
                let compute_ms = (now_ms() - compute_start) as f64;
                cr.total_compute_ms += compute_ms;
                let ops_per_sec = if compute_ms > 0.0 {
                    sent as f64 / (compute_ms / 1000.0)
                } else {
                    0.0
                };
                json_bytes(json!({
                    "status": "ok",
                    "total_sent": sent,
                    "total_fan_out": total_fan_out,
                    "compute_ms": compute_ms,
                    "ops_per_sec": ops_per_sec,
                    "history_size": cr.messages.len(),
                    "active_members": cr.members.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "get_history" => {
            let limit = v
                .get("limit")
                .and_then(|l| l.as_u64())
                .unwrap_or(50) as usize;
            let after_seq = v
                .get("after_seq")
                .and_then(|a| a.as_u64())
                .unwrap_or(0);
            let limit = if limit == 0 || limit > MAX_HISTORY {
                50
            } else {
                limit
            };
            with_state(|s| {
                let ActorState::ChatRoom(ref cr) = s else {
                    return json_err("wrong actor type");
                };
                let filtered: Vec<&ChatMessage> =
                    cr.messages.iter().filter(|m| m.seq > after_seq).collect();
                let start = if filtered.len() > limit {
                    filtered.len() - limit
                } else {
                    0
                };
                let result = &filtered[start..];
                // Demonstrate kv_multi_get: batch-fetch individual message keys.
                // Mirrors Cloudflare DO storage.get([k1, k2, ...]) batch API.
                let keys: Vec<String> = result
                    .iter()
                    .map(|m| format!("room:{}:msg:{}", cr.room_id, m.seq))
                    .collect();
                let key_refs: Vec<&str> = keys.iter().map(String::as_str).collect();
                let batch_fetched = kv_multi_get(&key_refs)
                    .map(|vals| vals.iter().filter(|v| v.is_some()).count())
                    .unwrap_or(0);
                let msgs_json: Vec<Value> = result
                    .iter()
                    .map(|m| {
                        json!({
                            "seq": m.seq,
                            "user_id": m.user_id,
                            "content": m.content,
                            "timestamp": m.timestamp
                        })
                    })
                    .collect();
                json_bytes(json!({
                    "status": "ok",
                    "room_id": cr.room_id,
                    "messages": msgs_json,
                    "count": msgs_json.len(),
                    "total": cr.messages.len(),
                    "batch_fetched": batch_fetched
                }))
            })
            .unwrap_or_else(json_err)
        }
        "get_members" => with_state(|s| {
            let ActorState::ChatRoom(ref cr) = s else {
                return json_err("wrong actor type");
            };
            let members_json: Vec<Value> = cr
                .members
                .values()
                .map(|m| {
                    json!({
                        "user_id": m.user_id,
                        "joined_at": m.joined_at,
                        "msg_count": m.msg_count
                    })
                })
                .collect();
            json_bytes(json!({
                "status": "ok",
                "room_id": cr.room_id,
                "members": members_json,
                "count": members_json.len()
            }))
        })
        .unwrap_or_else(json_err),
        "stats" => with_state(|s| {
            let ActorState::ChatRoom(ref cr) = s else {
                return json_err("wrong actor type");
            };
            let ops_per_sec = if cr.total_compute_ms > 0.0 {
                cr.total_messages as f64 / (cr.total_compute_ms / 1000.0)
            } else {
                0.0
            };
            let memory_kb =
                (cr.messages.len() * 128 + cr.members.len() * 64) as f64 / 1024.0;
            json_bytes(json!({
                "status": "ok",
                "room_id": cr.room_id,
                "config": { "max_history": MAX_HISTORY },
                "counters": {
                    "total_messages": cr.total_messages,
                    "total_joins": cr.total_joins,
                    "total_leaves": cr.total_leaves,
                    "total_broadcasts": cr.total_broadcasts,
                    "active_members": cr.members.len(),
                    "history_size": cr.messages.len(),
                    "message_seq": cr.msg_seq
                },
                "benchmarks": {
                    "total_compute_ms": cr.total_compute_ms,
                    "msgs_per_sec": ops_per_sec,
                    "memory_kb": memory_kb
                }
            }))
        })
        .unwrap_or_else(json_err),
        _ => json_err(format!("unknown operation: {op}")),
    }
}

// ============================================================================
// RateLimiterActor
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenBucket {
    tokens: i64,
    last_refill: u64,
    allowed: u64,
    denied: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RateLimiterState {
    max_tokens: i64,
    refill_rate_ms: u64,
    buckets: HashMap<String, TokenBucket>,
    total_checks: u64,
    total_allowed: u64,
    total_denied: u64,
}

impl Default for RateLimiterState {
    fn default() -> Self {
        RateLimiterState {
            max_tokens: 5,
            refill_rate_ms: 1000,
            buckets: HashMap::new(),
            total_checks: 0,
            total_allowed: 0,
            total_denied: 0,
        }
    }
}

fn ratelimiter_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let mut state = RateLimiterState::default();
    if let Some(args) = v.get("args").and_then(|a| a.as_object()) {
        if let Some(mt) = args
            .get("max_tokens")
            .and_then(|t| t.as_str().and_then(|s| s.parse().ok()).or_else(|| t.as_i64()))
        {
            state.max_tokens = mt;
        }
        if let Some(rm) = args.get("refill_rate_ms").and_then(|r| {
            r.as_str()
                .and_then(|s| s.parse().ok())
                .or_else(|| r.as_u64())
        }) {
            state.refill_rate_ms = rm;
        }
    }
    let mut g = state_cell().lock().expect("guild_chat state lock poisoned");
    *g = Some(ActorState::RateLimiter(state));
    Ok(())
}

fn ratelimiter_handle(msg_type: &str, payload: &[u8]) -> Vec<u8> {
    let op = match parse_op(msg_type, payload) {
        Ok(o) => o,
        Err(e) => return json_err(e),
    };
    let v = parse_payload(payload);
    match op.as_str() {
        "check" => {
            let user_id = v.get("user_id").and_then(|u| u.as_str()).unwrap_or("");
            if user_id.is_empty() {
                return json_err("user_id required");
            }
            with_state(|s| {
                let ActorState::RateLimiter(ref mut rl) = s else {
                    return json_err("wrong actor type");
                };
                let now = now_ms();
                // Atomic distributed counter: counts total lifetime requests.
                // Equivalent to Cloudflare KV atomic increment for distributed rate limiting.
                let _ = kv_increment(&format!("rate:{user_id}:total"), 1);

                let bucket = rl.buckets.entry(user_id.to_string()).or_insert(TokenBucket {
                    tokens: rl.max_tokens,
                    last_refill: now,
                    allowed: 0,
                    denied: 0,
                });
                // Refill tokens based on elapsed time
                let elapsed = now.saturating_sub(bucket.last_refill);
                if rl.refill_rate_ms > 0 {
                    let new_tokens = (elapsed / rl.refill_rate_ms) as i64;
                    if new_tokens > 0 {
                        bucket.tokens = (bucket.tokens + new_tokens).min(rl.max_tokens);
                        bucket.last_refill = now;
                    }
                }
                // Atomic CAS: attempt to reserve a token slot.
                // Equivalent to Cloudflare DO transactional storage — read-modify-write
                // atomically to prevent double-spend.
                let cas_key = format!("rate:{user_id}:window");
                let current_bytes = kv_get(&cas_key).unwrap_or_default();
                let next_bytes = now.to_string().into_bytes();
                let _ = kv_cas(
                    &cas_key,
                    if current_bytes.is_empty() {
                        None
                    } else {
                        Some(current_bytes.as_slice())
                    },
                    &next_bytes,
                );
                let allowed = bucket.tokens > 0;
                let retry_after_ms = if !allowed && rl.refill_rate_ms > 0 {
                    rl.refill_rate_ms - (elapsed % rl.refill_rate_ms)
                } else {
                    0
                };
                if allowed {
                    bucket.tokens -= 1;
                    bucket.allowed += 1;
                    rl.total_allowed += 1;
                } else {
                    bucket.denied += 1;
                    rl.total_denied += 1;
                }
                rl.total_checks += 1;
                let remaining = bucket.tokens;
                json_bytes(json!({
                    "status": "ok",
                    "allowed": allowed,
                    "user_id": user_id,
                    "remaining": remaining,
                    "limit": rl.max_tokens,
                    "retry_after_ms": retry_after_ms
                }))
            })
            .unwrap_or_else(json_err)
        }
        "check_batch" => {
            let user_id = v
                .get("user_id")
                .and_then(|u| u.as_str())
                .unwrap_or("batch-user")
                .to_string();
            let count = v.get("count").and_then(|c| c.as_u64()).unwrap_or(1000) as usize;
            with_state(|s| {
                let ActorState::RateLimiter(ref mut rl) = s else {
                    return json_err("wrong actor type");
                };
                let num_users = 50usize;
                let requests_per_user = (count / num_users).max(1);
                let batch_start = now_ms();
                let mut allowed = 0usize;
                let mut denied = 0usize;
                for u in 0..num_users {
                    let uid = format!("{user_id}-{u}");
                    let now = now_ms();
                    let bucket = rl.buckets.entry(uid).or_insert(TokenBucket {
                        tokens: rl.max_tokens,
                        last_refill: now,
                        allowed: 0,
                        denied: 0,
                    });
                    for _ in 0..requests_per_user {
                        let now = now_ms();
                        let elapsed = now.saturating_sub(bucket.last_refill);
                        if rl.refill_rate_ms > 0 {
                            let new_tokens = (elapsed / rl.refill_rate_ms) as i64;
                            if new_tokens > 0 {
                                bucket.tokens = (bucket.tokens + new_tokens).min(rl.max_tokens);
                                bucket.last_refill = now;
                            }
                        }
                        if bucket.tokens > 0 {
                            bucket.tokens -= 1;
                            bucket.allowed += 1;
                            rl.total_allowed += 1;
                            allowed += 1;
                        } else {
                            bucket.denied += 1;
                            rl.total_denied += 1;
                            denied += 1;
                        }
                        rl.total_checks += 1;
                    }
                }
                let duration_ms = (now_ms() - batch_start) as f64;
                let total = allowed + denied;
                let ops_per_sec = if duration_ms > 0.0 {
                    total as f64 / (duration_ms / 1000.0)
                } else {
                    0.0
                };
                json_bytes(json!({
                    "status": "ok",
                    "total_requests": total,
                    "allowed": allowed,
                    "denied": denied,
                    "duration_ms": duration_ms,
                    "ops_per_sec": ops_per_sec,
                    "unique_users": num_users,
                    "reqs_per_user": requests_per_user
                }))
            })
            .unwrap_or_else(json_err)
        }
        "status" | "stats" => with_state(|s| {
            let ActorState::RateLimiter(ref rl) = s else {
                return json_err("wrong actor type");
            };
            let deny_rate = if rl.total_checks > 0 {
                rl.total_denied as f64 / rl.total_checks as f64 * 100.0
            } else {
                0.0
            };
            json_bytes(json!({
                "status": "ok",
                "config": {
                    "max_tokens": rl.max_tokens,
                    "refill_rate_ms": rl.refill_rate_ms
                },
                "counters": {
                    "total_checks": rl.total_checks,
                    "total_allowed": rl.total_allowed,
                    "total_denied": rl.total_denied,
                    "deny_rate_pct": deny_rate,
                    "active_users": rl.buckets.len()
                }
            }))
        })
        .unwrap_or_else(json_err),
        _ => json_err(format!("unknown operation: {op}")),
    }
}

// ============================================================================
// AlarmDemoActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AlarmDemoState {
    pending_requests: Vec<String>,
    total_alarms_set: u64,
    total_alarms_fired: u64,
    total_processed: u64,
}

fn alarmdemo_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|a| a.as_str())
        .unwrap_or("AlarmDemo");
    let _ = log("info", &format!("AlarmDemo {actor_id}: initialized"));
    let state = AlarmDemoState::default();
    let mut g = state_cell().lock().expect("guild_chat state lock poisoned");
    *g = Some(ActorState::AlarmDemo(state));
    Ok(())
}

fn alarmdemo_handle(msg_type: &str, payload: &[u8]) -> Vec<u8> {
    let op = match parse_op(msg_type, payload) {
        Ok(o) => o,
        Err(e) => return json_err(e),
    };
    let v = parse_payload(payload);
    match op.as_str() {
        "start" => {
            let delay_ms = v
                .get("delay_ms")
                .and_then(|d| d.as_u64())
                .unwrap_or(30000);
            let delay_ms = if delay_ms == 0 { 30000 } else { delay_ms };
            with_state(|s| {
                let ActorState::AlarmDemo(ref mut ad) = s else {
                    return json_err("wrong actor type");
                };
                let fire_at = now_ms() + delay_ms;
                if let Err(e) = alarm_set(fire_at) {
                    return json_err(format!("alarm_set failed: {e}"));
                }
                ad.total_alarms_set += 1;
                let _ = log(
                    "info",
                    &format!("AlarmDemo: alarm set, fires in {delay_ms}ms at ts={fire_at}"),
                );
                json_bytes(json!({
                    "status": "ok",
                    "action": "alarm_scheduled",
                    "fire_at_ms": fire_at,
                    "delay_ms": delay_ms,
                    "total_alarms_set": ad.total_alarms_set,
                    "pending_requests": ad.pending_requests.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "enqueue" => {
            let data = v.get("data").and_then(|d| d.as_str()).unwrap_or("");
            with_state(|s| {
                let ActorState::AlarmDemo(ref mut ad) = s else {
                    return json_err("wrong actor type");
                };
                let data = if data.is_empty() {
                    format!("item-{}", ad.pending_requests.len())
                } else {
                    data.to_string()
                };
                ad.pending_requests.push(data.clone());
                // Auto-schedule alarm on first enqueue
                if ad.pending_requests.len() == 1 {
                    let fire_at = now_ms() + 10000;
                    if alarm_set(fire_at).is_ok() {
                        ad.total_alarms_set += 1;
                    }
                }
                json_bytes(json!({
                    "status": "ok",
                    "action": "enqueued",
                    "data": data,
                    "pending_requests": ad.pending_requests.len()
                }))
            })
            .unwrap_or_else(json_err)
        }
        "__alarm__" => {
            // Invoked by the framework when the scheduled alarm fires.
            // Equivalent to Cloudflare DO: async alarm() { ... }
            with_state(|s| {
                let ActorState::AlarmDemo(ref mut ad) = s else {
                    return json_err("wrong actor type");
                };
                ad.total_alarms_fired += 1;
                let processed = ad.pending_requests.len();
                ad.total_processed += processed as u64;
                let _ = log(
                    "info",
                    &format!("AlarmDemo: alarm fired, processing {processed} pending requests"),
                );
                let results: Vec<String> = ad
                    .pending_requests
                    .iter()
                    .map(|r| format!("processed:{r}"))
                    .collect();
                ad.pending_requests.clear();
                json_bytes(json!({
                    "status": "ok",
                    "action": "alarm_fired",
                    "processed": processed,
                    "results": results,
                    "total_alarms_fired": ad.total_alarms_fired,
                    "total_processed": ad.total_processed
                }))
            })
            .unwrap_or_else(json_err)
        }
        "status" => with_state(|s| {
            let ActorState::AlarmDemo(ref ad) = s else {
                return json_err("wrong actor type");
            };
            let (fire_at, err_msg) = match alarm_get() {
                Ok(ts) => (ts, String::new()),
                Err(e) => (0u64, e),
            };
            json_bytes(json!({
                "status": "ok",
                "alarm_fire_at_ms": fire_at,
                "alarm_set": fire_at > 0,
                "pending_requests": ad.pending_requests.len(),
                "total_alarms_set": ad.total_alarms_set,
                "total_alarms_fired": ad.total_alarms_fired,
                "total_processed": ad.total_processed,
                "error": err_msg
            }))
        })
        .unwrap_or_else(json_err),
        "cancel" => with_state(|s| {
            let ActorState::AlarmDemo(_) = s else {
                return json_err("wrong actor type");
            };
            if let Err(e) = alarm_delete() {
                return json_err(format!("alarm_delete failed: {e}"));
            }
            let _ = log("info", "AlarmDemo: alarm cancelled");
            json_bytes(json!({ "status": "ok", "action": "alarm_cancelled" }))
        })
        .unwrap_or_else(json_err),
        "reset" => with_state(|s| {
            let ActorState::AlarmDemo(ref mut ad) = s else {
                return json_err("wrong actor type");
            };
            *ad = AlarmDemoState::default();
            json_bytes(json!({ "status": "ok", "action": "reset" }))
        })
        .unwrap_or_else(json_err),
        _ => json_err(format!("unknown operation: {op}")),
    }
}

// ============================================================================
// WIT guest implementation — routes init/handle to the correct actor type
// ============================================================================

fn do_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_type = v
        .get("actor_type")
        .or_else(|| v.get("behavior_kind"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    match actor_type {
        "ChatRoomActor" | "ChatRoom" => chatroom_init(config),
        "RateLimiterActor" | "RateLimiter" => ratelimiter_init(config),
        "AlarmDemoActor" | "AlarmDemo" => alarmdemo_init(config),
        other => Err(format!("unknown actor_type: {other}")),
    }
}

fn do_handle(msg_type: &str, payload: &[u8]) -> Vec<u8> {
    let state_type = {
        let g = state_cell().lock().expect("guild_chat state lock");
        match g.as_ref() {
            Some(ActorState::ChatRoom(_)) => "ChatRoom",
            Some(ActorState::RateLimiter(_)) => "RateLimiter",
            Some(ActorState::AlarmDemo(_)) => "AlarmDemo",
            None => "None",
        }
    };
    match state_type {
        "ChatRoom" => chatroom_handle(msg_type, payload),
        "RateLimiter" => ratelimiter_handle(msg_type, payload),
        "AlarmDemo" => alarmdemo_handle(msg_type, payload),
        _ => json_err("actor not initialized"),
    }
}

struct GuildChatGuest;

impl Guest for GuildChatGuest {
    fn init(config: Vec<u8>) -> Result<(), String> {
        do_init(&config)
    }

    fn handle(_from: String, msg_type: String, payload: Vec<u8>) -> Result<Vec<u8>, String> {
        Ok(do_handle(&msg_type, &payload))
    }

    fn get_state() -> Result<Vec<u8>, String> {
        let g = state_cell().lock().expect("guild_chat state lock");
        match g.as_ref() {
            Some(s) => {
                serde_json::to_vec(s).map_err(|e| format!("state encode failed: {e}"))
            }
            None => Ok(vec![]),
        }
    }

    fn set_state(bytes: Vec<u8>) -> Result<(), String> {
        if bytes.is_empty() {
            return Ok(());
        }
        match serde_json::from_slice::<ActorState>(&bytes) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("guild_chat state lock");
                *g = Some(s);
                Ok(())
            }
            Err(e) => Err(format!("invalid state JSON: {e}")),
        }
    }
}

export!(GuildChatGuest);
