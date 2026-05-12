// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Chat Room (Rust WASM) — large-scale chat example demonstrating durable virtual actors,
// process groups, FSM actors, workflow actors, timers, and the object registry.
//
// Faithfully ports the Python/Go/TypeScript chat_room examples.
// Actor types in one WASM binary, routed by `actor_type` in the init config.

use prost::Message as ProstMessage;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};

wit_bindgen::generate!({
    path: "../../../../wit/plexspaces-actor",
    world: "actor-world",
});

use exports::plexspaces::actor::actor::Guest;
use plexspaces::actor::host;
use plexspaces::actor::registry;

// ============================================================================
// Actor ID helpers (mirrors Go helpers)
// ============================================================================

fn actor_application_id(actor_id: &str) -> String {
    if let Some(rest) = actor_id.strip_prefix("//").or_else(|| {
        actor_id
            .split_once("//")
            .map(|(_, r)| r)
    }) {
        if let Some(qualified) = rest.split('@').next() {
            if let Some((_, ns)) = qualified.split_once("::") {
                return ns.to_string();
            }
        }
    }
    if let Some((_, rest)) = actor_id.split_once(':') {
        if let Some((ns, _)) = rest.split_once('@') {
            return ns.to_string();
        }
    }
    String::new()
}

fn actor_instance_name(actor_id: &str) -> String {
    if let Some((name, _)) = actor_id.split_once("//") {
        return name.to_string();
    }
    actor_id.to_string()
}

fn peer(actor_type: &str, name: &str) -> String {
    let self_id = host::self_id();
    if self_id.contains("//") {
        let rest = &self_id[self_id.find("//").unwrap() + 2..];
        if let Some(at) = rest.find('@') {
            if let Some(cc) = rest.find("::") {
                if cc < at {
                    let ns = &rest[cc + 2..at];
                    let node_id = &rest[at + 1..];
                    return format!("{name}//{actor_type}::{ns}@{node_id}");
                }
            }
        }
    }
    format!("{actor_type}:{name}")
}

fn guild_actor_id(guild_id: &str) -> String {
    peer("GuildActor", guild_id)
}

fn channel_actor_id(guild_id: &str, channel_id: &str) -> String {
    peer("ChannelActor", &format!("{guild_id}__{channel_id}"))
}

fn message_store_actor_id(guild_id: &str, channel_id: &str) -> String {
    peer("MessageStoreActor", &format!("{guild_id}__{channel_id}"))
}

fn presence_actor_id(user_id: &str) -> String {
    peer("PresenceActor", user_id)
}

fn connection_fsm_actor_id(session_id: &str) -> String {
    peer("ConnectionFSM", session_id)
}

fn fanout_actor_id() -> String {
    peer("FanoutActor", "singleton")
}

fn audit_event_actor_id() -> String {
    peer("AuditEventActor", "singleton")
}

fn channel_group(guild_id: &str, channel_id: &str) -> String {
    format!("channel:{guild_id}__{channel_id}")
}

fn user_session_group(user_id: &str) -> String {
    format!("user-session:{user_id}")
}

fn decode_channel_parts(actor_id: &str) -> (String, String) {
    let name = actor_instance_name(actor_id);
    if let Some(idx) = name.find("__") {
        (name[..idx].to_string(), name[idx + 2..].to_string())
    } else {
        (name, String::new())
    }
}

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

fn safe_metrics_add(app_id: &str, counters: &[(&str, u64)]) {
    if app_id.is_empty() {
        return;
    }
    let counter_metrics: std::collections::HashMap<String, u64> =
        counters.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    let metrics = plexspaces_proto::application::v1::ApplicationMetrics {
        message_count: 1,
        counter_metrics,
        ..Default::default()
    };
    let bytes = metrics.encode_to_vec();
    let _ = host::application_metrics_add(app_id, &bytes);
}

/// Encode a RegisterRequest proto for the registry WIT interface.
fn encode_register_request(object_id: &str, object_type: u32, object_category: &str) -> Vec<u8> {
    let reg = plexspaces_proto::object_registry::v1::ObjectRegistration {
        object_id: object_id.to_string(),
        object_type: object_type as i32,
        object_category: object_category.to_string(),
        ..Default::default()
    };
    let req = plexspaces_proto::object_registry::v1::RegisterRequest {
        registration: Some(reg),
        ..Default::default()
    };
    req.encode_to_vec()
}

// object_type constants matching proto enum
const OBJECT_TYPE_ACTOR: u32 = 1;

// ============================================================================
// Per-instance actor state (OnceLock + Mutex for WASM single-threaded safety)
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "actor_type")]
enum ActorState {
    Session(SessionState),
    Guild(GuildState),
    Channel(ChannelState),
    Presence(PresenceState),
    MessageStore(MessageStoreState),
    Fanout(FanoutState),
    AuditEvent(AuditEventState),
    ConnectionFsm(ConnectionFsmState),
    ModerationWorkflow(ModerationWorkflowState),
}

fn state_cell() -> &'static Mutex<Option<ActorState>> {
    static STATE: OnceLock<Mutex<Option<ActorState>>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(None))
}

fn with_state<T>(f: impl FnOnce(&mut ActorState) -> T) -> Result<T, String> {
    let mut g = state_cell().lock().expect("chat_room state lock poisoned");
    match g.as_mut() {
        Some(s) => Ok(f(s)),
        None => Err("actor not initialized".to_string()),
    }
}

// ============================================================================
// SessionActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SessionState {
    application_id: String,
    session_id: String,
    user_id: String,
    guild_id: String,
    joined_channels: Vec<String>,
    delivered_events: Vec<Value>,
    unread_by_channel: std::collections::HashMap<String, u64>,
    last_delivery_ms: u64,
}

fn session_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let mut s = SessionState {
        application_id: app_id,
        session_id: actor_instance_name(&actor_id),
        ..Default::default()
    };
    if let Some(uid) = v
        .get("args")
        .and_then(|a| a.get("user_id"))
        .and_then(|u| u.as_str())
    {
        s.user_id = uid.to_string();
    }
    if let Some(gid) = v
        .get("args")
        .and_then(|a| a.get("guild_id"))
        .and_then(|g| g.as_str())
    {
        s.guild_id = gid.to_string();
    }
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Session(s));
    Ok(())
}

fn session_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "connect" => session_connect(&v),
        "send_channel_message" => session_send_channel_message(&v),
        "set_typing" => session_set_typing(&v),
        "deliver_channel_event" => session_deliver_channel_event(&v),
        "read_channel" => session_read_channel(&v),
        "inbox" => session_inbox(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn session_connect(v: &Value) -> Result<Vec<u8>, String> {
    let user_id = v
        .get("user_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let guild_id = v
        .get("guild_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    if user_id.is_empty() || guild_id.is_empty() {
        return Ok(json_err("user_id and guild_id are required"));
    }
    let ttl_ms = v
        .get("ttl_ms")
        .and_then(|x| x.as_u64())
        .unwrap_or(60000);
    let channels: Vec<String> = v
        .get("channels")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();

    with_state(|state| {
        if let ActorState::Session(s) = state {
            s.user_id = user_id.clone();
            s.guild_id = guild_id.clone();
            s.joined_channels = channels.clone();

            let _ = host::pg_join(&user_session_group(&user_id));
            for ch in &channels {
                let _ = host::pg_join(&channel_group(&guild_id, ch));
                let msg = json!({ "user_id": user_id, "session_id": s.session_id })
                    .to_string()
                    .into_bytes();
                let _ = host::send(&channel_actor_id(&guild_id, ch), "join_member", &msg);
            }

            let reg_msg = json!({
                "user_id": user_id,
                "session_id": s.session_id,
                "channels": channels,
            })
            .to_string()
            .into_bytes();
            let _ = host::send(&guild_actor_id(&guild_id), "register_session", &reg_msg);

            let presence_msg = json!({
                "user_id": user_id,
                "guild_id": guild_id,
                "status": "online",
                "ttl_ms": ttl_ms,
            })
            .to_string()
            .into_bytes();
            let _ = host::send(&presence_actor_id(&user_id), "set_presence", &presence_msg);

            let to_connected =
                json!({ "to": "connected" }).to_string().into_bytes();
            let to_joined = json!({ "to": "joined" }).to_string().into_bytes();
            let _ = host::send(
                &connection_fsm_actor_id(&s.session_id),
                "transition",
                &to_connected,
            );
            let _ = host::send(
                &connection_fsm_actor_id(&s.session_id),
                "transition",
                &to_joined,
            );

            safe_metrics_add(&s.application_id, &[("chat_sessions_connected", 1)]);
            json_bytes(json!({
                "status": "connected",
                "session_id": s.session_id,
                "user_id": user_id,
                "guild_id": guild_id,
                "channels": channels,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn session_send_channel_message(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Session(s) = state {
            if s.user_id.is_empty() {
                return json_err("session_not_connected");
            }
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
            if channel_id.is_empty() {
                return json_err("channel_id is required");
            }
            let msg = json!({
                "user_id": s.user_id,
                "session_id": s.session_id,
                "text": text,
            })
            .to_string()
            .into_bytes();
            match host::ask(&channel_actor_id(&s.guild_id, channel_id), "post_message", &msg, 5000) {
                Ok(resp) => resp,
                Err(e) => json_err(e),
            }
        } else {
            json_err("wrong actor type")
        }
    })
}

fn session_set_typing(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Session(s) = state {
            if s.user_id.is_empty() {
                return json_err("session_not_connected");
            }
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let ttl_ms = v.get("ttl_ms").and_then(|x| x.as_u64()).unwrap_or(2000);
            if channel_id.is_empty() {
                return json_err("channel_id is required");
            }
            let msg = json!({ "user_id": s.user_id, "ttl_ms": ttl_ms })
                .to_string()
                .into_bytes();
            match host::ask(
                &channel_actor_id(&s.guild_id, channel_id),
                "mark_typing",
                &msg,
                5000,
            ) {
                Ok(resp) => resp,
                Err(e) => json_err(e),
            }
        } else {
            json_err("wrong actor type")
        }
    })
}

fn session_deliver_channel_event(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Session(s) = state {
            let evt_type = v
                .get("event_type")
                .and_then(|x| x.as_str())
                .unwrap_or("message")
                .to_string();
            let from_user = v
                .get("from_user")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let event = json!({
                "event_type": evt_type,
                "guild_id": v.get("guild_id").and_then(|x| x.as_str()).unwrap_or(""),
                "channel_id": channel_id,
                "message_id": v.get("message_id").and_then(|x| x.as_str()).unwrap_or(""),
                "from_user": from_user,
                "text": v.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                "delivered_at_ms": v.get("delivered_at_ms").and_then(|x| x.as_u64()).unwrap_or(0),
            });
            s.delivered_events.push(event);
            if s.delivered_events.len() > 50 {
                let len = s.delivered_events.len();
                s.delivered_events.drain(0..len - 50);
            }
            if from_user != s.user_id && evt_type == "message" {
                *s.unread_by_channel.entry(channel_id).or_insert(0) += 1;
            }
            json_bytes(json!({ "status": "delivered", "session_id": s.session_id }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn session_read_channel(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Session(s) = state {
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if channel_id.is_empty() {
                return json_err("channel_id is required");
            }
            s.unread_by_channel.insert(channel_id.to_string(), 0);
            let idle_msg = json!({ "to": "idle" }).to_string().into_bytes();
            let _ = host::ask(
                &connection_fsm_actor_id(&s.session_id),
                "transition",
                &idle_msg,
                5000,
            );
            let remaining: Value = serde_json::to_value(&s.unread_by_channel)
                .unwrap_or_else(|_| json!({}));
            json_bytes(json!({
                "status": "read",
                "channel_id": channel_id,
                "session_id": s.session_id,
                "remaining_unread": remaining,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn session_inbox() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Session(s) = state {
            let unread: Value =
                serde_json::to_value(&s.unread_by_channel).unwrap_or_else(|_| json!({}));
            json_bytes(json!({
                "session_id": s.session_id,
                "user_id": s.user_id,
                "guild_id": s.guild_id,
                "joined_channels": s.joined_channels,
                "delivered_events": s.delivered_events,
                "unread_by_channel": unread,
                "last_delivery_ms": s.last_delivery_ms,
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
    application_id: String,
    user_id: String,
    guild_id: String,
    status: String,
    last_seen_ms: u64,
    expiry_deadline_ms: u64,
}

fn presence_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let s = PresenceState {
        application_id: app_id,
        user_id: actor_instance_name(&actor_id),
        status: "offline".to_string(),
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Presence(s));
    Ok(())
}

fn presence_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "set_presence" => presence_set(&v),
        "expire_presence" => presence_expire(&v),
        "status" => presence_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn presence_set(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            if let Some(uid) = v.get("user_id").and_then(|x| x.as_str()) {
                s.user_id = uid.to_string();
            }
            if let Some(gid) = v.get("guild_id").and_then(|x| x.as_str()) {
                s.guild_id = gid.to_string();
            }
            let status = v
                .get("status")
                .and_then(|x| x.as_str())
                .unwrap_or("online")
                .to_string();
            let ttl_ms = v.get("ttl_ms").and_then(|x| x.as_u64()).unwrap_or(60000);
            let now_ms = host::now_ms();
            s.status = status.clone();
            s.last_seen_ms = now_ms;
            s.expiry_deadline_ms = now_ms + ttl_ms;
            let deadline = s.expiry_deadline_ms;
            let expire_msg = json!({ "deadline_ms": deadline }).to_string().into_bytes();
            let _ = host::send_after(ttl_ms, "expire_presence", &expire_msg);
            safe_metrics_add(&s.application_id, &[("chat_presence_updates", 1)]);
            json_bytes(json!({
                "user_id": s.user_id,
                "guild_id": s.guild_id,
                "status": status,
                "expires_at_ms": deadline,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn presence_expire(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Presence(s) = state {
            let deadline = v.get("deadline_ms").and_then(|x| x.as_u64()).unwrap_or(0);
            if deadline != s.expiry_deadline_ms {
                return json_bytes(json!({ "status": "ignored", "reason": "stale_deadline" }));
            }
            s.status = "offline".to_string();
            safe_metrics_add(&s.application_id, &[("chat_presence_expirations", 1)]);
            json_bytes(json!({ "status": "expired", "user_id": s.user_id }))
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
                "guild_id": s.guild_id,
                "status": s.status,
                "last_seen_ms": s.last_seen_ms,
                "expires_at_ms": s.expiry_deadline_ms,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// ConnectionFSM
// ============================================================================

fn fsm_allowed(from: &str) -> &'static [&'static str] {
    match from {
        "offline" => &["connected"],
        "connected" => &["joined"],
        "joined" => &["idle", "disconnected"],
        "idle" => &["joined", "disconnected"],
        "disconnected" => &["connected"],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ConnectionFsmState {
    application_id: String,
    session_id: String,
    fsm_state: String,
    transition_count: u64,
}

fn fsm_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let s = ConnectionFsmState {
        application_id: app_id,
        session_id: actor_instance_name(&actor_id),
        fsm_state: "offline".to_string(),
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::ConnectionFsm(s));
    Ok(())
}

fn fsm_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "transition" => fsm_transition(&v),
        "status" => fsm_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn fsm_transition(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ConnectionFsm(s) = state {
            let to = v.get("to").and_then(|x| x.as_str()).unwrap_or("");
            if to.is_empty() {
                return json_err("to is required");
            }
            if !fsm_allowed(&s.fsm_state).contains(&to) {
                return json_bytes(json!({
                    "status": "ignored",
                    "from": s.fsm_state,
                    "to": to,
                    "allowed": fsm_allowed(&s.fsm_state),
                }));
            }
            let previous = s.fsm_state.clone();
            s.fsm_state = to.to_string();
            s.transition_count += 1;
            safe_metrics_add(&s.application_id, &[("chat_connection_transitions", 1)]);
            json_bytes(json!({
                "status": "ok",
                "session_id": s.session_id,
                "from": previous,
                "to": s.fsm_state,
                "transition_count": s.transition_count,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn fsm_status() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ConnectionFsm(s) = state {
            json_bytes(json!({
                "session_id": s.session_id,
                "state": s.fsm_state,
                "transition_count": s.transition_count,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// GuildActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct GuildState {
    application_id: String,
    guild_id: String,
    members: Vec<String>,
    channels: Vec<String>,
    session_index: std::collections::HashMap<String, Value>,
}

fn guild_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let s = GuildState {
        application_id: app_id,
        guild_id: actor_instance_name(&actor_id),
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Guild(s));
    Ok(())
}

fn guild_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "register_session" => guild_register_session(&v),
        "create_channel" => guild_create_channel(&v),
        "topology" => guild_topology(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn guild_register_session(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Guild(s) = state {
            let user_id = v
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let session_id = v
                .get("session_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let channels: Vec<String> = v
                .get("channels")
                .and_then(|c| c.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();

            if !s.members.contains(&user_id) {
                s.members.push(user_id.clone());
            }
            for ch in &channels {
                if !s.channels.contains(ch) {
                    s.channels.push(ch.clone());
                }
            }
            s.session_index.insert(
                session_id,
                json!({ "user_id": user_id, "channels": channels }),
            );
            safe_metrics_add(&s.application_id, &[("chat_guild_registrations", 1)]);
            json_bytes(json!({
                "guild_id": s.guild_id,
                "member_count": s.members.len(),
                "session_count": s.session_index.len(),
                "channels": s.channels,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn guild_create_channel(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Guild(s) = state {
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if channel_id.is_empty() {
                return json_err("channel_id is required");
            }
            if !s.channels.contains(&channel_id.to_string()) {
                s.channels.push(channel_id.to_string());
            }
            let kv_val = serde_json::to_string(&s.channels).unwrap_or_default();
            let _ = host::kv_put(
                &format!("guild:{}:channels", s.guild_id),
                kv_val.as_bytes(),
            );
            json_bytes(json!({
                "guild_id": s.guild_id,
                "channel_id": channel_id,
                "channels": s.channels,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn guild_topology() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Guild(s) = state {
            json_bytes(json!({
                "guild_id": s.guild_id,
                "members": s.members,
                "channels": s.channels,
                "session_index": s.session_index,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// ChannelActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ChannelState {
    application_id: String,
    guild_id: String,
    channel_id: String,
    member_index: std::collections::HashMap<String, Value>,
    typing_deadlines: std::collections::HashMap<String, u64>,
    messages: Vec<Value>,
    last_message_id: String,
    total_messages: u64,
}

fn channel_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let (guild_id, channel_id) = decode_channel_parts(&actor_id);
    let s = ChannelState {
        application_id: app_id,
        guild_id,
        channel_id,
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Channel(s));
    Ok(())
}

fn channel_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "join_member" => channel_join_member(&v),
        "mark_typing" => channel_mark_typing(&v),
        "clear_typing" => channel_clear_typing(&v),
        "post_message" => channel_post_message(&v),
        "history" => channel_history(&v),
        "status" => channel_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn channel_join_member(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let user_id = v
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if user_id.is_empty() {
                return json_err("user_id is required");
            }
            let session_id = v
                .get("session_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            s.member_index.insert(
                user_id.to_string(),
                json!({ "session_id": session_id }),
            );
            let members: Vec<&str> = s.member_index.keys().map(|k| k.as_str()).collect();
            let kv_val = serde_json::to_string(&members).unwrap_or_default();
            let _ = host::kv_put(
                &format!("channel:{}:{}:members", s.guild_id, s.channel_id),
                kv_val.as_bytes(),
            );
            json_bytes(json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "member_count": s.member_index.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn channel_mark_typing(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let user_id = v
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if user_id.is_empty() {
                return json_err("user_id is required");
            }
            let ttl_ms = v.get("ttl_ms").and_then(|x| x.as_u64()).unwrap_or(2000);
            let deadline_ms = host::now_ms() + ttl_ms;
            s.typing_deadlines
                .insert(user_id.to_string(), deadline_ms);
            let clear_msg = json!({ "user_id": user_id, "deadline_ms": deadline_ms })
                .to_string()
                .into_bytes();
            let _ = host::send_after(ttl_ms, "clear_typing", &clear_msg);
            let typing_users: Vec<&str> =
                s.typing_deadlines.keys().map(|k| k.as_str()).collect();
            json_bytes(json!({
                "status": "typing",
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "user_id": user_id,
                "typing_users": typing_users,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn channel_clear_typing(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let user_id = v
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let deadline_ms = v
                .get("deadline_ms")
                .and_then(|x| x.as_u64())
                .unwrap_or(0);
            match s.typing_deadlines.get(user_id) {
                Some(&current) if current == deadline_ms => {
                    s.typing_deadlines.remove(user_id);
                    json_bytes(json!({ "status": "cleared", "user_id": user_id }))
                }
                _ => json_bytes(json!({ "status": "ignored", "reason": "stale_deadline" })),
            }
        } else {
            json_err("wrong actor type")
        }
    })
}

fn channel_post_message(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let user_id = v
                .get("user_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if user_id.is_empty() {
                return json_err("user_id is required");
            }
            if !s.member_index.contains_key(user_id) {
                return json_bytes(json!({ "error": "user_not_in_channel", "user_id": user_id }));
            }
            let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("");
            let session_id = v
                .get("session_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let next_seq = s.total_messages + 1;
            let message_id = format!("{}-{}", s.channel_id, next_seq);
            let stored_at_ms = host::now_ms();

            let store_msg = json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "user_id": user_id,
                "text": text,
                "session_id": session_id,
                "message_id": message_id,
                "stored_at_ms": stored_at_ms,
            })
            .to_string()
            .into_bytes();
            let _ = host::send(
                &message_store_actor_id(&s.guild_id, &s.channel_id),
                "append_message",
                &store_msg,
            );

            let event = json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "message_id": message_id,
                "from_user": user_id,
                "text": text,
                "delivered_at_ms": stored_at_ms,
                "event_type": "message",
            });
            s.messages.push(event.clone());
            if s.messages.len() > 200 {
                let len = s.messages.len();
                s.messages.drain(0..len - 200);
            }

            let fanout_msg = event.to_string().into_bytes();
            let _ = host::send(&fanout_actor_id(), "deliver_channel_event", &fanout_msg);

            let audit_msg = json!({
                "event_type": "channel_message",
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "message_id": message_id,
                "user_id": user_id,
            })
            .to_string()
            .into_bytes();
            let _ = host::send(&audit_event_actor_id(), "record_event", &audit_msg);

            s.last_message_id = message_id.clone();
            s.total_messages = next_seq;
            safe_metrics_add(&s.application_id, &[("chat_channel_messages", 1)]);
            json_bytes(json!({
                "status": "ok",
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "message_id": message_id,
                "recipient_count": s.member_index.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn channel_history(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let limit = v
                .get("limit")
                .and_then(|x| x.as_u64())
                .unwrap_or(50) as usize;
            let limit = if limit == 0 { 50 } else { limit };
            let start = s.messages.len().saturating_sub(limit);
            let recent = &s.messages[start..];
            json_bytes(json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "messages": recent,
                "count": recent.len(),
                "message_count": s.messages.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn channel_status() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Channel(s) = state {
            let members: Vec<&str> = s.member_index.keys().map(|k| k.as_str()).collect();
            let typing_users: Vec<&str> =
                s.typing_deadlines.keys().map(|k| k.as_str()).collect();
            json_bytes(json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "members": members,
                "typing_users": typing_users,
                "last_message_id": s.last_message_id,
                "total_messages": s.total_messages,
                "channel_group": channel_group(&s.guild_id, &s.channel_id),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// MessageStoreActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct MessageStoreState {
    application_id: String,
    guild_id: String,
    channel_id: String,
    messages: Vec<Value>,
    next_message_seq: u64,
}

fn message_store_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let (guild_id, channel_id) = decode_channel_parts(&actor_id);
    let s = MessageStoreState {
        application_id: app_id,
        guild_id,
        channel_id,
        next_message_seq: 1,
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::MessageStore(s));
    Ok(())
}

fn message_store_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "append_message" => message_store_append(&v),
        "history" => message_store_history(&v),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn message_store_append(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::MessageStore(s) = state {
            if let Some(gid) = v.get("guild_id").and_then(|x| x.as_str()) {
                s.guild_id = gid.to_string();
            }
            if let Some(cid) = v.get("channel_id").and_then(|x| x.as_str()) {
                s.channel_id = cid.to_string();
            }
            let message_id = v
                .get("message_id")
                .and_then(|x| x.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{}-{}", s.channel_id, s.next_message_seq));
            let stored_at_ms = v
                .get("stored_at_ms")
                .and_then(|x| x.as_u64())
                .unwrap_or_else(|| host::now_ms());
            let message = json!({
                "message_id": message_id,
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "user_id": v.get("user_id").and_then(|x| x.as_str()).unwrap_or(""),
                "text": v.get("text").and_then(|x| x.as_str()).unwrap_or(""),
                "session_id": v.get("session_id").and_then(|x| x.as_str()).unwrap_or(""),
                "stored_at_ms": stored_at_ms,
            });
            s.messages.push(message);
            s.next_message_seq = s.messages.len() as u64 + 1;
            safe_metrics_add(&s.application_id, &[("chat_messages_stored", 1)]);
            json_bytes(json!({
                "status": "stored",
                "message_id": message_id,
                "stored_at_ms": stored_at_ms,
                "message_count": s.messages.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn message_store_history(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::MessageStore(s) = state {
            let limit = v
                .get("limit")
                .and_then(|x| x.as_u64())
                .unwrap_or(50) as usize;
            let limit = if limit == 0 { 50 } else { limit };
            let start = s.messages.len().saturating_sub(limit);
            let recent = &s.messages[start..];
            json_bytes(json!({
                "guild_id": s.guild_id,
                "channel_id": s.channel_id,
                "messages": recent,
                "count": recent.len(),
                "message_count": s.messages.len(),
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// FanoutActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct FanoutState {
    application_id: String,
    actor_name: String,
    deliveries: u64,
}

fn fanout_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let actor_name = actor_instance_name(&actor_id);
    let s = FanoutState {
        application_id: app_id,
        actor_name,
        ..Default::default()
    };
    // Best-effort registry registration
    let req = encode_register_request(&actor_id, OBJECT_TYPE_ACTOR, "fanout");
    let _ = registry::register(&req);
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::Fanout(s));
    Ok(())
}

fn fanout_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "deliver_channel_event" => fanout_deliver(&v, payload),
        "stats" => fanout_stats(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn fanout_deliver(v: &Value, raw: &[u8]) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Fanout(s) = state {
            let guild_id = v
                .get("guild_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let channel_id = v
                .get("channel_id")
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let group = channel_group(guild_id, channel_id);
            let members = host::pg_members(&group).unwrap_or_default();
            let _ = host::pg_broadcast(&group, "deliver_channel_event", raw);
            s.deliveries += 1;
            safe_metrics_add(&s.application_id, &[("chat_fanout_events", 1)]);
            json_bytes(json!({
                "status": "broadcast",
                "group": group,
                "recipient_count": members.len(),
                "recipients": members,
                "deliveries": s.deliveries,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn fanout_stats() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::Fanout(s) = state {
            json_bytes(json!({ "deliveries": s.deliveries, "actor_name": s.actor_name }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// AuditEventActor
// ============================================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct AuditEventState {
    application_id: String,
    actor_name: String,
    recent_events: Vec<Value>,
}

fn audit_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let actor_name = actor_instance_name(&actor_id);
    let s = AuditEventState {
        application_id: app_id,
        actor_name,
        ..Default::default()
    };
    // Best-effort registry registration
    let req = encode_register_request(&actor_id, OBJECT_TYPE_ACTOR, "audit_event");
    let _ = registry::register(&req);
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::AuditEvent(s));
    Ok(())
}

fn audit_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "record_event" => audit_record(&v),
        "stats" => audit_stats(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn audit_record(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::AuditEvent(s) = state {
            let event = json!({
                "event_type": v.get("event_type").and_then(|x| x.as_str()).unwrap_or(""),
                "guild_id": v.get("guild_id").and_then(|x| x.as_str()).unwrap_or(""),
                "channel_id": v.get("channel_id").and_then(|x| x.as_str()).unwrap_or(""),
                "message_id": v.get("message_id").and_then(|x| x.as_str()).unwrap_or(""),
                "user_id": v.get("user_id").and_then(|x| x.as_str()).unwrap_or(""),
                "recorded_at_ms": host::now_ms(),
            });
            s.recent_events.push(event);
            if s.recent_events.len() > 100 {
                let len = s.recent_events.len();
                s.recent_events.drain(0..len - 100);
            }
            safe_metrics_add(&s.application_id, &[("chat_audit_events", 1)]);
            json_bytes(json!({}))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn audit_stats() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::AuditEvent(s) = state {
            json_bytes(json!({
                "actor_name": s.actor_name,
                "event_count": s.recent_events.len(),
                "recent_events": s.recent_events,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// ModerationWorkflow
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ModerationWorkflowState {
    application_id: String,
    report_id: String,
    status: String,
    message_id: String,
    reason: String,
    reporter_id: String,
    resolution: String,
    signals: Vec<String>,
}

impl Default for ModerationWorkflowState {
    fn default() -> Self {
        Self {
            application_id: String::new(),
            report_id: String::new(),
            status: "pending".to_string(),
            message_id: String::new(),
            reason: String::new(),
            reporter_id: String::new(),
            resolution: String::new(),
            signals: Vec::new(),
        }
    }
}

fn workflow_init(config: &[u8]) -> Result<(), String> {
    let v = parse_payload(config);
    let actor_id = v
        .get("actor_id")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string();
    let app_id = if actor_id.is_empty() {
        actor_application_id(&host::self_id())
    } else {
        actor_application_id(&actor_id)
    };
    let s = ModerationWorkflowState {
        application_id: app_id,
        ..Default::default()
    };
    let mut g = state_cell().lock().expect("lock");
    *g = Some(ActorState::ModerationWorkflow(s));
    Ok(())
}

fn workflow_handle(_from: &str, op: &str, payload: &[u8]) -> Result<Vec<u8>, String> {
    let v = parse_payload(payload);
    match op {
        "workflow_run" => workflow_run(&v),
        op if op.starts_with("workflow_signal:") => {
            let name = &op["workflow_signal:".len()..];
            workflow_signal(name, &v);
            Ok(json_bytes(json!({})))
        }
        op if op.starts_with("workflow_query:") => {
            let name = &op["workflow_query:".len()..];
            workflow_query(name)
        }
        "status" => workflow_current_status(),
        _ => Ok(json_err(format!("unknown op: {op}"))),
    }
}

fn workflow_run(v: &Value) -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ModerationWorkflow(s) = state {
            if s.application_id.is_empty() {
                s.application_id = actor_application_id(&host::self_id());
            }
            let report_id = v
                .get("report_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            if s.report_id.is_empty() {
                s.report_id = if report_id.is_empty() {
                    actor_instance_name(&host::self_id())
                } else {
                    report_id
                };
            }
            s.message_id = v
                .get("message_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            s.reporter_id = v
                .get("reporter_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            s.reason = v
                .get("reason")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            s.status = "under_review".to_string();
            safe_metrics_add(&s.application_id, &[("chat_moderation_reports", 1)]);
            json_bytes(json!({
                "report_id": s.report_id,
                "status": s.status,
                "message_id": s.message_id,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

fn workflow_signal(name: &str, v: &Value) {
    let _ = with_state(|state| {
        if let ActorState::ModerationWorkflow(s) = state {
            match name {
                "review" => {
                    let mod_id = v
                        .get("moderator_id")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    let resolution = v
                        .get("resolution")
                        .and_then(|x| x.as_str())
                        .unwrap_or("");
                    s.resolution = resolution.to_string();
                    s.status = "reviewed".to_string();
                    s.signals
                        .push(format!("review:{mod_id}:{resolution}"));
                }
                "close" => {
                    let resolution = v
                        .get("resolution")
                        .and_then(|x| x.as_str())
                        .unwrap_or("dismissed");
                    s.resolution = resolution.to_string();
                    s.status = "closed".to_string();
                    s.signals.push(format!("close::{resolution}"));
                }
                _ => {}
            }
        }
    });
}

fn workflow_query(name: &str) -> Result<Vec<u8>, String> {
    if name != "status" {
        return Ok(json_err(format!("unknown query: {name}")));
    }
    workflow_current_status()
}

fn workflow_current_status() -> Result<Vec<u8>, String> {
    with_state(|state| {
        if let ActorState::ModerationWorkflow(s) = state {
            json_bytes(json!({
                "report_id": s.report_id,
                "status": s.status,
                "message_id": s.message_id,
                "reporter_id": s.reporter_id,
                "reason": s.reason,
                "resolution": s.resolution,
                "signals": s.signals,
            }))
        } else {
            json_err("wrong actor type")
        }
    })
}

// ============================================================================
// Routing bridge — single Guest dispatches by actor_type from init config
// ============================================================================

struct ChatRoomBridge;

impl Guest for ChatRoomBridge {
    fn init(config: Vec<u8>) -> Result<(), String> {
        let v = parse_payload(&config);
        let actor_type = v
            .get("actor_type")
            .and_then(|x| x.as_str())
            .unwrap_or("");
        match actor_type {
            "SessionActor" => session_init(&config),
            "GuildActor" => guild_init(&config),
            "ChannelActor" => channel_init(&config),
            "PresenceActor" => presence_init(&config),
            "MessageStoreActor" => message_store_init(&config),
            "FanoutActor" => fanout_init(&config),
            "AuditEventActor" => audit_init(&config),
            "ConnectionFSM" => fsm_init(&config),
            "ModerationWorkflow" => workflow_init(&config),
            _ => Err(format!("unknown actor_type: {actor_type:?}")),
        }
    }

    fn handle(
        from_actor: String,
        msg_type: String,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, String> {
        let op = match parse_op(&msg_type, &payload) {
            Ok(op) => op,
            Err(e) => return Ok(json_err(e)),
        };
        // Read the discriminant without holding the lock during dispatch.
        // Each handler re-acquires the lock internally; WASM is single-threaded
        // so this is safe, but the mutex still panics on re-entrancy.
        let discriminant = {
            let g = state_cell().lock().expect("handle: read discriminant");
            match g.as_ref() {
                Some(ActorState::Session(_)) => 0u8,
                Some(ActorState::Guild(_)) => 1,
                Some(ActorState::Channel(_)) => 2,
                Some(ActorState::Presence(_)) => 3,
                Some(ActorState::MessageStore(_)) => 4,
                Some(ActorState::Fanout(_)) => 5,
                Some(ActorState::AuditEvent(_)) => 6,
                Some(ActorState::ConnectionFsm(_)) => 7,
                Some(ActorState::ModerationWorkflow(_)) => 8,
                None => return Ok(json_err("actor not initialized")),
            }
        };
        let result = match discriminant {
            0 => session_handle(&from_actor, &op, &payload),
            1 => guild_handle(&from_actor, &op, &payload),
            2 => channel_handle(&from_actor, &op, &payload),
            3 => presence_handle(&from_actor, &op, &payload),
            4 => message_store_handle(&from_actor, &op, &payload),
            5 => fanout_handle(&from_actor, &op, &payload),
            6 => audit_handle(&from_actor, &op, &payload),
            7 => fsm_handle(&from_actor, &op, &payload),
            8 => workflow_handle(&from_actor, &op, &payload),
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

export!(ChatRoomBridge);

// ============================================================================
// Unit tests (pure logic, no host calls)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actor_application_id_canonical_form() {
        let id = "alice//SessionActor::chat-room@node1";
        assert_eq!(actor_application_id(id), "chat-room");
    }

    #[test]
    fn actor_instance_name_canonical_form() {
        let id = "alice//SessionActor::chat-room@node1";
        assert_eq!(actor_instance_name(id), "alice");
    }

    #[test]
    fn actor_instance_name_fallback() {
        let id = "SessionActor:alice";
        assert_eq!(actor_instance_name(id), "SessionActor:alice");
    }

    #[test]
    fn decode_channel_parts_with_separator() {
        let id = "guild-acme//ChannelActor::ns@node";
        let (g, c) = decode_channel_parts(id);
        assert_eq!(g, "guild-acme");
        assert_eq!(c, "");
    }

    #[test]
    fn decode_channel_parts_with_double_underscore() {
        // Instance name contains "__"
        let id = "guild-acme__general//ChannelActor::ns@node";
        let (g, c) = decode_channel_parts(id);
        assert_eq!(g, "guild-acme");
        assert_eq!(c, "general");
    }

    #[test]
    fn channel_group_format() {
        assert_eq!(channel_group("acme", "general"), "channel:acme__general");
    }

    #[test]
    fn user_session_group_format() {
        assert_eq!(user_session_group("alice"), "user-session:alice");
    }

    #[test]
    fn parse_op_from_payload() {
        let payload = br#"{"op":"connect"}"#;
        assert_eq!(parse_op("call", payload).unwrap(), "connect");
    }

    #[test]
    fn parse_op_fallback_to_msg_type() {
        // Non-call/cast msg_type falls through to msg_type itself
        assert_eq!(parse_op("session_idle", b"{}").unwrap(), "session_idle");
    }

    #[test]
    fn parse_op_missing_op_in_call_errors() {
        let err = parse_op("call", b"{}");
        assert!(err.is_err());
    }

    #[test]
    fn fsm_allowed_transitions() {
        assert!(fsm_allowed("offline").contains(&"connected"));
        assert!(!fsm_allowed("offline").contains(&"joined"));
        assert!(fsm_allowed("joined").contains(&"idle"));
        assert!(fsm_allowed("joined").contains(&"disconnected"));
    }

    #[test]
    fn encode_register_request_produces_nonempty_bytes() {
        let bytes = encode_register_request("actor@node", OBJECT_TYPE_ACTOR, "fanout");
        assert!(!bytes.is_empty());
    }

    #[test]
    fn encode_register_request_decodes_correctly() {
        use plexspaces_proto::object_registry::v1::RegisterRequest;
        let bytes = encode_register_request("fanout-singleton@node", OBJECT_TYPE_ACTOR, "fanout");
        let req = RegisterRequest::decode(bytes.as_slice()).expect("decode");
        let reg = req.registration.expect("registration");
        assert_eq!(reg.object_id, "fanout-singleton@node");
        assert_eq!(reg.object_type, OBJECT_TYPE_ACTOR as i32);
        assert_eq!(reg.object_category, "fanout");
    }

    #[test]
    fn safe_metrics_add_skips_empty_app_id() {
        // Should not panic when app_id is empty
        safe_metrics_add("", &[("counter", 1)]);
    }
}
