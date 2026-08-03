// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Message test helpers
//!
//! Canonical implementations of message construction helpers that were previously
//! duplicated in 38+ locations across the workspace.

use plexspaces_proto::common::v1::Message;
use ulid::Ulid;

/// Creates a test Message with the given payload and a fresh ULID id.
///
/// Replaces the 38+ identical copies of this function scattered across crates.
pub fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

/// Creates a test Message with a JSON payload serialized from the given string.
pub fn create_test_message_str(payload: &str) -> Message {
    create_test_message(payload.as_bytes().to_vec())
}

/// Creates a test Message with a TTL set.
pub fn create_test_message_with_ttl(payload: Vec<u8>, ttl: std::time::Duration) -> Message {
    let mut msg = create_test_message(payload);
    msg.ttl = Some(prost_types::Duration {
        seconds: ttl.as_secs() as i64,
        nanos: ttl.subsec_nanos() as i32,
    });
    msg
}

/// Creates a call-type (request-reply) test message with JSON payload.
pub fn create_call_message(payload: &str) -> Message {
    Message {
        id: Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: payload.as_bytes().to_vec(),
        ..Default::default()
    }
}

/// Creates a cast-type (fire-and-forget) test message with JSON payload.
pub fn create_cast_message(payload: &str) -> Message {
    Message {
        id: Ulid::new().to_string(),
        message_type: "cast".to_string(),
        payload: payload.as_bytes().to_vec(),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_test_message_has_unique_id() {
        let m1 = create_test_message(b"hello".to_vec());
        let m2 = create_test_message(b"hello".to_vec());
        assert_ne!(m1.id, m2.id, "each message must have a unique ULID");
        assert_eq!(m1.payload, b"hello");
    }

    #[test]
    fn test_create_test_message_with_ttl() {
        let ttl = std::time::Duration::from_secs(30);
        let msg = create_test_message_with_ttl(b"data".to_vec(), ttl);
        let ttl_proto = msg.ttl.expect("ttl should be set");
        assert_eq!(ttl_proto.seconds, 30);
    }

    #[test]
    fn test_message_types() {
        let call = create_call_message(r#"{"action":"ping"}"#);
        assert_eq!(call.message_type, "call");
        let cast = create_cast_message(r#"{"event":"tick"}"#);
        assert_eq!(cast.message_type, "cast");
    }
}
