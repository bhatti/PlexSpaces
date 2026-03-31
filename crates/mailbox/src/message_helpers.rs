// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Helper functions for proto Message

use plexspaces_proto::common::v1::Message;
use plexspaces_proto::mailbox::v1::MessagePriority;
use prost_types::Timestamp;
use std::time::Duration;
use ulid::Ulid;

/// Check if this is an EXIT message
pub fn is_exit(msg: &Message) -> bool {
    msg.payload == b"__EXIT__"
        || msg.message_type == "__EXIT__"
        || msg
            .headers
            .get("type")
            .map(|s| s == "__EXIT__")
            .unwrap_or(false)
}

/// Try to parse EXIT message and extract exit reason string
///
/// ## Returns
/// Some((from_actor_id, exit_reason_string)) if this is an EXIT message, None otherwise
pub fn try_parse_exit(msg: &Message) -> Option<(String, String)> {
    if !is_exit(msg) {
        return None;
    }
    let from = if msg.sender_id.is_empty() {
        "unknown".to_string()
    } else {
        msg.sender_id.clone()
    };
    let reason_str = msg
        .headers
        .get("exit_reason")
        .cloned()
        .unwrap_or_else(|| "Normal".to_string());
    Some((from, reason_str))
}

/// Get message type string for routing (extracted from message_type field or headers)
pub fn message_type_str(msg: &Message) -> &str {
    if !msg.message_type.is_empty() {
        return &msg.message_type;
    }
    msg.headers
        .get("type")
        .map(|s| s.as_str())
        .unwrap_or("cast")
}

/// Check if receiver is unset (empty or "unknown")
pub fn is_receiver_unset(msg: &Message) -> bool {
    msg.receiver_id.is_empty() || msg.receiver_id == "unknown"
}

/// Check if message has expired based on TTL and timestamp
pub fn is_expired(msg: &Message) -> bool {
    if let Some(ttl) = &msg.ttl {
        if let Some(timestamp) = &msg.timestamp {
            let created =
                std::time::UNIX_EPOCH + std::time::Duration::from_secs(timestamp.seconds as u64);
            let ttl_duration = std::time::Duration::from_secs(ttl.seconds as u64)
                + std::time::Duration::from_nanos(ttl.nanos as u64);
            if let Ok(elapsed) = std::time::SystemTime::now().duration_since(created) {
                return elapsed >= ttl_duration;
            }
        }
    }
    false
}

/// Get TTL as std::time::Duration, or None if not set
pub fn get_ttl(msg: &Message) -> Option<Duration> {
    msg.ttl
        .as_ref()
        .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
}

/// Priority integer scale (stored in proto priority field):
/// Low=0, Normal=25, High=50, System=75, Highest=100
pub fn priority_to_int(priority: MessagePriority) -> i32 {
    match priority {
        MessagePriority::MessagePriorityUnspecified => 25,
        MessagePriority::Lowest => 10,
        MessagePriority::Low => 0,
        MessagePriority::Normal => 25,
        MessagePriority::High => 50,
        MessagePriority::System => 75,
        MessagePriority::Highest => 100,
    }
}

/// Convert priority integer (0-100 scale) to MessagePriority enum (range-based)
pub fn priority_from_int(value: i32) -> MessagePriority {
    if value >= 100 {
        MessagePriority::Highest
    } else if value >= 75 {
        MessagePriority::System
    } else if value >= 50 {
        MessagePriority::High
    } else if value >= 25 {
        MessagePriority::Normal
    } else {
        MessagePriority::Low
    }
}

/// Create a new proto Message with Normal priority and a fresh ULID id
pub fn new_message(payload: Vec<u8>) -> Message {
    use chrono::Utc;
    let now = Utc::now();
    Message {
        id: Ulid::new().to_string(),
        sender_id: String::new(),
        receiver_id: String::new(),
        channel: String::new(),
        message_type: String::new(),
        payload,
        timestamp: Some(Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        }),
        headers: std::collections::HashMap::new(),
        priority: 25, // Normal (0-100 scale)
        ttl: None,
        delivery_count: 0,
        idempotency_key: String::new(),
        correlation_id: String::new(),
        reply_to: String::new(),
        partition_key: String::new(),
        uri_path: String::new(),
        uri_method: String::new(),
    }
}

/// Create a signal message with Highest priority (100)
pub fn signal_message(payload: Vec<u8>) -> Message {
    let mut msg = new_message(payload);
    msg.priority = priority_to_int(MessagePriority::Highest);
    msg
}

/// Create a system-priority message (75)
pub fn system_message(payload: Vec<u8>) -> Message {
    let mut msg = new_message(payload);
    msg.priority = priority_to_int(MessagePriority::System);
    msg
}

/// Create a timer message with timer metadata in headers
pub fn timer_message(name: &str) -> Message {
    let mut msg = new_message(name.as_bytes().to_vec());
    msg.headers.insert("type".to_string(), "timer".to_string());
    msg.headers
        .insert("timer_name".to_string(), name.to_string());
    msg
}

/// Create an EXIT message (from linked actor death)
pub fn exit_message(from: String, reason_str: &str) -> Message {
    use chrono::Utc;
    let now = Utc::now();
    let mut headers = std::collections::HashMap::new();
    headers.insert("type".to_string(), "__EXIT__".to_string());
    headers.insert("exit_from".to_string(), from.clone());
    headers.insert("exit_reason".to_string(), reason_str.to_string());
    Message {
        id: Ulid::new().to_string(),
        sender_id: from,
        receiver_id: String::new(),
        channel: String::new(),
        message_type: "__EXIT__".to_string(),
        payload: b"__EXIT__".to_vec(),
        timestamp: Some(Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        }),
        headers,
        priority: priority_to_int(MessagePriority::Highest),
        ttl: None,
        delivery_count: 0,
        idempotency_key: String::new(),
        correlation_id: String::new(),
        reply_to: String::new(),
        partition_key: String::new(),
        uri_path: String::new(),
        uri_method: String::new(),
    }
}
