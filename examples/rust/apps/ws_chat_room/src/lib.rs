// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WebSocket Chat Room — Rust WASM actors.
//
// Rust port of examples/typescript/apps/ws_chat_room.
//
// Deployed to a PlexSpaces node and driven by browser thin-node clients via
// the WsFrame binary WebSocket protocol.
//
// Actors:
//   ChatRoomActor  — per-room member registry; fans out chat_message to all
//                    member actor_ids via host.send(), routing each tell through
//                    WsActorTransportClient → WsRegistry → thin-node WS session.
//   PresenceActor  — per-user online/offline tracking with idle timeout via
//                    host::send_after (reminder facet).
//
// Note on routing: `host::send(actorId, ...)` is the correct fan-out primitive
// for thin-node clients. Their actor_ids (e.g. alice//ChatClient::ns@<thin-node>)
// are stored in ChatRoomActor state; the ActorRegistry routes each send to the
// appropriate WS session via WsActorTransportClient and WsRegistry.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host_actor::{self_id, send, send_after};
use plexspaces::actor::host_kv::kv_put;
use plexspaces::actor::host_logging::now_ms;

// ============================================================================
// Helpers
// ============================================================================

fn parse_payload(payload: &[u8]) -> Value {
    if payload.is_empty() {
        return json!({});
    }
    serde_json::from_slice(payload).unwrap_or_else(|_| json!({}))
}

fn json_bytes(v: Value) -> Vec<u8> {
    v.to_string().into_bytes()
}

fn json_err(msg: impl Into<String>) -> Vec<u8> {
    json_bytes(json!({ "error": msg.into() }))
}

/// Extract the instance name (the part before "//") from a canonical actor ID.
/// "alice//ChatClient::ns@node" → "alice"
fn actor_instance_name(actor_id: &str) -> String {
    if let Some((name, _)) = actor_id.split_once("//") {
        return name.to_string();
    }
    actor_id.to_string()
}

// ============================================================================
// Per-instance actor state (OnceLock + Mutex for WASM single-threaded safety)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "actor_type")]
enum ActorState {
    ChatRoom(ChatRoomState),
    Presence(PresenceState),
}

fn state_cell() -> &'static Mutex<Option<ActorState>> {
    static STATE: OnceLock<Mutex<Option<ActorState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn with_state<T>(f: impl FnOnce(&mut ActorState) -> T) -> Result<T, String> {
    let mut g = state_cell().lock().expect("ws_chat_room state lock poisoned");
    match g.as_mut() {
        Some(s) => Ok(f(s)),
        None => Err("actor not initialized".to_string()),
    }
}

// ============================================================================
// ChatRoomActor
// ============================================================================

const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct HistoryEntry {
    sender_actor_id: String,
    sender: String,
    text: String,
    ts: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChatRoomState {
    room_id: String,
    /// actorId → username
    members: HashMap<String, String>,
    history: Vec<HistoryEntry>,
}

fn chat_room_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    // Room ID is the instance name part of the actor ID
    let room_id = if actor_id.is_empty() {
        actor_instance_name(&self_id())
    } else {
        actor_instance_name(&actor_id)
    };
    let s = ChatRoomState {
        room_id,
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::ChatRoom(s));
    Ok(())
}

fn chat_room_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "join" => chat_room_join(&v),
        "leave" => chat_room_leave(&v),
        "send" => chat_room_send(&v),
        "members" => chat_room_members(),
        "status" => chat_room_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn chat_room_join(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ChatRoom(s) = state {
            let actor_id = match v.get("actor_id").and_then(|x| x.as_str()) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => return json_err("actor_id required"),
            };
            let username = v
                .get("username")
                .and_then(|x| x.as_str())
                .unwrap_or(&actor_id)
                .to_string();

            // Remove stale entry for same username (reconnect with new actor_id)
            let stale: Vec<String> = s
                .members
                .iter()
                .filter(|(existing_id, existing_username)| {
                    **existing_username == username && **existing_id != actor_id
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in stale {
                s.members.remove(&k);
            }

            let existing_actor_ids: Vec<String> = s.members.keys().cloned().collect();
            s.members.insert(actor_id.clone(), username.clone());

            let all_actor_ids: Vec<String> = s.members.keys().cloned().collect();
            let member_info: HashMap<&String, &String> = s.members.iter().collect();

            // Fan-out member_joined to previously existing members
            let joined_event = json!({
                "room_id": s.room_id,
                "members": all_actor_ids,
                "member_info": member_info,
                "joined_actor_id": actor_id,
                "joined_username": username,
            });
            let joined_bytes = joined_event.to_string().into_bytes();
            for existing_id in &existing_actor_ids {
                if existing_id != &actor_id {
                    let _ = send(existing_id, "member_joined", &joined_bytes);
                }
            }

            json_bytes(json!({
                "success": true,
                "members": all_actor_ids,
                "member_info": s.members,
                "room_id": s.room_id,
                "history": s.history,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn chat_room_leave(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ChatRoom(s) = state {
            let actor_id = match v.get("actor_id").and_then(|x| x.as_str()) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => return json_bytes(json!({ "success": true })),
            };
            s.members.remove(&actor_id);

            let all_actor_ids: Vec<String> = s.members.keys().cloned().collect();
            let member_info: &HashMap<String, String> = &s.members;
            let left_event = json!({
                "room_id": s.room_id,
                "members": all_actor_ids,
                "member_info": member_info,
            });
            let left_bytes = left_event.to_string().into_bytes();
            for remaining_id in &all_actor_ids {
                let _ = send(remaining_id, "member_left", &left_bytes);
            }

            json_bytes(json!({ "success": true }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn chat_room_send(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ChatRoom(s) = state {
            let sender_actor_id = match v.get("sender_actor_id").and_then(|x| x.as_str()) {
                Some(id) if !id.is_empty() => id.to_string(),
                _ => return json_err("sender_actor_id required"),
            };
            let text = match v.get("text").and_then(|x| x.as_str()) {
                Some(t) if !t.is_empty() => t.to_string(),
                _ => return json_err("text required"),
            };

            let sender_username = s
                .members
                .get(&sender_actor_id)
                .cloned()
                .unwrap_or_else(|| sender_actor_id.clone());
            let ts = now_ms();

            let entry = HistoryEntry {
                sender_actor_id: sender_actor_id.clone(),
                sender: sender_username.clone(),
                text: text.clone(),
                ts,
            };
            s.history.push(entry);
            if s.history.len() > MAX_HISTORY {
                let drain_count = s.history.len() - MAX_HISTORY;
                s.history.drain(0..drain_count);
            }

            let event = json!({
                "sender": sender_actor_id,
                "sender_username": sender_username,
                "text": text,
                "room_id": s.room_id,
                "ts": ts,
            });
            let event_bytes = event.to_string().into_bytes();

            // Fan out to all members INCLUDING sender so they see confirmation
            let member_ids: Vec<String> = s.members.keys().cloned().collect();
            for member_id in &member_ids {
                let _ = send(member_id, "chat_message", &event_bytes);
            }

            json_bytes(json!({ "success": true, "members_notified": member_ids.len() }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn chat_room_members() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ChatRoom(s) = state {
            json_bytes(json!({
                "members": s.members.keys().collect::<Vec<_>>(),
                "usernames": s.members,
                "room_id": s.room_id,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn chat_room_status() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ChatRoom(s) = state {
            json_bytes(json!({
                "room_id": s.room_id,
                "member_count": s.members.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// PresenceActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PresenceState {
    user_id: String,
    online: bool,
    last_seen: u64,
    /// Deadline stored so stale timeout_check messages can be ignored.
    timeout_deadline_ms: u64,
}

fn presence_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let user_id = if actor_id.is_empty() {
        actor_instance_name(&self_id())
    } else {
        actor_instance_name(&actor_id)
    };
    let s = PresenceState {
        user_id,
        online: false,
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Presence(s));
    Ok(())
}

fn presence_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "online" => presence_online(&v),
        "offline" => presence_offline(&v),
        "timeout_check" => presence_timeout_check(&v),
        "status" => presence_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn presence_online(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            // Allow caller to override user_id (e.g. from payload actor_id field)
            if let Some(uid) = v.get("actor_id").and_then(|x| x.as_str()) {
                if !uid.is_empty() {
                    s.user_id = actor_instance_name(uid);
                }
            }
            let now_ms = now_ms();
            s.online = true;
            s.last_seen = now_ms;

            // Persist presence in KV with a sensible key
            let presence_val = json!({
                "online": true,
                "last_seen": s.last_seen,
            });
            let _ = kv_put(
                &format!("presence:{}", s.user_id),
                presence_val.to_string().as_bytes(),
            );

            // Schedule idle timeout check in 60 s via reminder facet
            let timeout_deadline = now_ms + 60_000;
            s.timeout_deadline_ms = timeout_deadline;
            let check_msg = json!({ "deadline_ms": timeout_deadline })
                .to_string()
                .into_bytes();
            let _ = send_after(60_000, "timeout_check", &check_msg);

            json_bytes(json!({ "success": true, "online": true, "user_id": s.user_id }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn presence_offline(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            if let Some(uid) = v.get("actor_id").and_then(|x| x.as_str()) {
                if !uid.is_empty() {
                    s.user_id = actor_instance_name(uid);
                }
            }
            s.online = false;
            s.last_seen = now_ms();

            let presence_val = json!({
                "online": false,
                "last_seen": s.last_seen,
            });
            let _ = kv_put(
                &format!("presence:{}", s.user_id),
                presence_val.to_string().as_bytes(),
            );

            json_bytes(json!({ "success": true, "online": false, "user_id": s.user_id }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn presence_timeout_check(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            // Ignore stale timeout messages (a newer online() may have updated the deadline)
            let deadline = v.get("deadline_ms").and_then(|x| x.as_u64()).unwrap_or(0);
            if deadline != s.timeout_deadline_ms {
                return json_bytes(json!({ "checked": true, "ignored": true, "reason": "stale_deadline" }));
            }

            let now_ms = now_ms();
            let idle_ms = now_ms.saturating_sub(s.last_seen);
            // Mark offline if idle for more than 55 s (5 s grace below the 60 s send_after)
            if idle_ms > 55_000 {
                s.online = false;
                let presence_val = json!({
                    "online": false,
                    "last_seen": s.last_seen,
                });
                let _ = kv_put(
                    &format!("presence:{}", s.user_id),
                    presence_val.to_string().as_bytes(),
                );
            }

            json_bytes(json!({ "checked": true, "idle_ms": idle_ms, "online": s.online }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn presence_status() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            json_bytes(json!({
                "user_id": s.user_id,
                "online": s.online,
                "last_seen": s.last_seen,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// Routing bridge — single Guest dispatches by actor_type from init config
// ============================================================================

struct WsChatRoomBridge;

impl Guest for WsChatRoomBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let v = parse_payload(&config);
        let actor_type = v
            .get("actor_type")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        match actor_type {
            "ChatRoomActor" => chat_room_init(&config),
            "PresenceActor" => presence_init(&config),
            _ => Err(format!("unknown actor_type: {actor_type:?}")),
        }
    }

    fn handle(
        from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        // For call/cast messages, extract "op" field from payload; otherwise use msg_type directly.
        let op: String = if msg_type == "call" || msg_type == "cast" {
            let v = parse_payload(&payload);
            match v.get("op").and_then(|o| o.as_str()) {
                Some(op) => op.to_string(),
                None => return Ok(json_err("missing op")),
            }
        } else {
            msg_type.clone()
        };

        // Read discriminant without holding the lock during dispatch.
        let discriminant = {
            let g = state_cell().lock().expect("handle: read discriminant");
            match g.as_ref() {
                Some(ActorState::ChatRoom(_)) => 0u8,
                Some(ActorState::Presence(_)) => 1,
                None => return Ok(json_err("actor not initialized")),
            }
        };

        let result = match discriminant {
            0 => chat_room_handle(&from_actor, &op, &payload),
            1 => presence_handle(&from_actor, &op, &payload),
            _ => unreachable!(),
        };
        result.or_else(|e| Ok(json_err(e)))
    }

    fn get_state() -> Result<Vec<u8>, String> {
        let g = state_cell().lock().expect("get_state lock");
        match g.as_ref() {
            Some(s) => serde_json::to_vec(s).map_err(|e| format!("state encode: {e}")),
            None => Ok(vec![]),
        }
    }

    fn set_state(state: Vec<u8>) -> Result<(), String> {
        if state.is_empty() {
            return Ok(());
        }
        match serde_json::from_slice::<ActorState>(&state) {
            Ok(s) => {
                let mut g = state_cell().lock().expect("set_state lock");
                *g = Some(s);
                Ok(())
            }
            Err(e) => Err(format!("state decode: {e}")),
        }
    }
}

export!(WsChatRoomBridge);

// ============================================================================
// Unit tests (pure logic, no host calls)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_instance_name_canonical_form() {
        let id = "alice//ChatClient::ns@node1";
        assert_eq!(actor_instance_name(id), "alice");
    }

    #[test]
    fn actor_instance_name_plain() {
        let id = "ChatRoomActor:lobby";
        assert_eq!(actor_instance_name(id), "ChatRoomActor:lobby");
    }

    #[test]
    fn parse_payload_empty() {
        let v = parse_payload(b"");
        assert!(v.is_object());
    }

    #[test]
    fn parse_payload_valid_json() {
        let v = parse_payload(br#"{"op":"join","username":"alice"}"#);
        assert_eq!(v["op"], "join");
        assert_eq!(v["username"], "alice");
    }

    #[test]
    fn history_trim_keeps_last_max() {
        let mut s = ChatRoomState {
            room_id: "r1".to_string(),
            ..Default::default()
        };
        s.members.insert("a//C::ns@n".to_string(), "alice".to_string());
        // Fill well past MAX_HISTORY entries
        for i in 0..(MAX_HISTORY + 10) {
            s.history.push(HistoryEntry {
                sender_actor_id: "a//C::ns@n".to_string(),
                sender: "alice".to_string(),
                text: format!("msg {i}"),
                ts: i as u64,
            });
        }
        // Trim as the send handler does
        if s.history.len() > MAX_HISTORY {
            let drain = s.history.len() - MAX_HISTORY;
            s.history.drain(0..drain);
        }
        assert_eq!(s.history.len(), MAX_HISTORY);
        // Oldest retained message should be index 10
        assert_eq!(s.history[0].text, "msg 10");
    }

    #[test]
    fn json_err_produces_error_key() {
        let bytes = json_err("something went wrong");
        let v: Value = serde_json::from_slice(&bytes).expect("valid json");
        assert_eq!(v["error"], "something went wrong");
    }
}
