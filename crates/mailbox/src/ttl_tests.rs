// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! TTL (Time-To-Live) Tests for Message

#[cfg(test)]
mod tests {
    use crate::*;
    use std::time::Duration;

    /// Test that new_message() creates message without TTL
    #[test]
    fn test_message_new_no_ttl() {
        let msg = new_message(b"test".to_vec());
        assert_eq!(get_ttl(&msg), None);
        assert!(!is_expired(&msg));
    }

    /// Test TTL set correctly via direct field assignment
    #[test]
    fn test_message_with_ttl() {
        let ttl = Duration::from_secs(30);
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: ttl.as_secs() as i64,
            nanos: ttl.subsec_nanos() as i32,
        });
        assert_eq!(get_ttl(&msg), Some(ttl));
        assert!(!is_expired(&msg));
    }

    /// Test is_expired() returns false for non-expired message
    #[test]
    fn test_message_not_expired() {
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        });
        assert!(!is_expired(&msg));
    }

    /// Test is_expired() returns true for expired message
    #[tokio::test]
    async fn test_message_expired() {
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: 0,
            nanos: 10_000_000, // 10ms
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(is_expired(&msg));
    }

    /// Test TTL stored correctly in proto fields
    #[test]
    fn test_message_ttl_to_proto() {
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: 30,
            nanos: 0,
        });
        let proto_ttl = msg.ttl.as_ref().unwrap();
        assert_eq!(proto_ttl.seconds, 30);
        assert_eq!(proto_ttl.nanos, 0);
    }

    /// Test TTL read back from proto fields
    #[test]
    fn test_message_ttl_from_proto() {
        use plexspaces_proto::common::v1::Message as ProtoMessage;

        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: "sender".to_string(),
            receiver_id: "receiver".to_string(),
            channel: String::new(),
            message_type: "test".to_string(),
            payload: b"test".to_vec(),
            timestamp: None,
            priority: 25,
            ttl: Some(prost_types::Duration {
                seconds: 30,
                nanos: 0,
            }),
            headers: Default::default(),
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            delivery_count: 0,
            uri_path: String::new(),
            uri_method: String::new(),
        };

        assert_eq!(get_ttl(&proto_msg), Some(Duration::from_secs(30)));
    }

    /// Test message without TTL in proto
    #[test]
    fn test_message_no_ttl_from_proto() {
        use plexspaces_proto::common::v1::Message as ProtoMessage;

        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: String::new(),
            receiver_id: String::new(),
            channel: String::new(),
            message_type: String::new(),
            payload: b"test".to_vec(),
            timestamp: None,
            priority: 25,
            ttl: None,
            headers: Default::default(),
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            delivery_count: 0,
            uri_path: String::new(),
            uri_method: String::new(),
        };

        assert_eq!(get_ttl(&proto_msg), None);
    }

    /// Test TTL with nanoseconds
    #[test]
    fn test_message_ttl_with_nanos() {
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: 0,
            nanos: 500_000_000,
        });
        let proto_ttl = msg.ttl.as_ref().unwrap();
        assert_eq!(proto_ttl.seconds, 0);
        assert_eq!(proto_ttl.nanos, 500_000_000);
    }

    /// Test expired message detection
    #[tokio::test]
    async fn test_expired_message_not_processed() {
        let mut msg = new_message(b"test".to_vec());
        msg.ttl = Some(prost_types::Duration {
            seconds: 0,
            nanos: 10_000_000, // 10ms
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(is_expired(&msg));
    }
}
