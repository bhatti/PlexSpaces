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
//!
//! TDD approach: Write tests first, then implement TTL support

#[cfg(test)]
mod tests {
    use crate::*;
    use std::time::Duration;

    /// Test that Message::new() creates message without TTL
    #[test]
    fn test_message_new_no_ttl() {
        let msg = Message::new(b"test".to_vec());
        assert_eq!(msg.ttl(), None);
        assert!(!msg.is_expired());
    }

    /// Test with_ttl() sets TTL correctly
    #[test]
    fn test_message_with_ttl() {
        let ttl = Duration::from_secs(30);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        assert_eq!(msg.ttl(), Some(ttl));
        assert!(!msg.is_expired()); // Just created, not expired yet
    }

    /// Test is_expired() returns false for non-expired message
    #[test]
    fn test_message_not_expired() {
        let ttl = Duration::from_secs(60);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        // Message just created, should not be expired
        assert!(!msg.is_expired());
    }

    /// Test is_expired() returns true for expired message
    #[tokio::test]
    async fn test_message_expired() {
        let ttl = Duration::from_millis(10);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        
        // Wait for TTL to expire
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        assert!(msg.is_expired());
    }

    /// Test TTL serialization to proto
    #[test]
    fn test_message_ttl_to_proto() {
        let ttl = Duration::from_secs(30);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        let proto = msg.to_proto();
        
        assert!(proto.ttl.is_some());
        let proto_ttl = proto.ttl.unwrap();
        assert_eq!(proto_ttl.seconds, 30);
        assert_eq!(proto_ttl.nanos, 0);
    }

    /// Test TTL deserialization from proto
    #[test]
    fn test_message_ttl_from_proto() {
        use prost_types::Duration as ProtoDuration;
        use plexspaces_proto::v1::actor::Message as ProtoMessage;
        
        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: "sender".to_string(),
            receiver_id: "receiver".to_string(),
            message_type: "test".to_string(),
            payload: b"test".to_vec(),
            timestamp: None,
            priority: 25,
            ttl: Some(ProtoDuration {
                seconds: 30,
                nanos: 0,
            }),
            headers: Default::default(),
            idempotency_key: String::new(),
            uri_path: String::new(),
            uri_method: String::new(),
        };
        
        let msg = Message::from_proto(&proto_msg);
        assert_eq!(msg.ttl(), Some(Duration::from_secs(30)));
    }

    /// Test message without TTL in proto
    #[test]
    fn test_message_no_ttl_from_proto() {
        use plexspaces_proto::v1::actor::Message as ProtoMessage;
        
        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: "sender".to_string(),
            receiver_id: "receiver".to_string(),
            message_type: "test".to_string(),
            payload: b"test".to_vec(),
            timestamp: None,
            priority: 25,
            ttl: None,
            headers: Default::default(),
            idempotency_key: String::new(),
            uri_path: String::new(),
            uri_method: String::new(),
        };
        
        let msg = Message::from_proto(&proto_msg);
        assert_eq!(msg.ttl(), None);
    }

    /// Test TTL with nanoseconds
    #[test]
    fn test_message_ttl_with_nanos() {
        let ttl = Duration::from_millis(500);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        let proto = msg.to_proto();
        
        assert!(proto.ttl.is_some());
        let proto_ttl = proto.ttl.unwrap();
        assert_eq!(proto_ttl.seconds, 0);
        assert_eq!(proto_ttl.nanos, 500_000_000);
    }

    /// Test expired message is not processed (integration test)
    #[tokio::test]
    async fn test_expired_message_not_processed() {
        // This test will be implemented after mailbox supports TTL checking
        // For now, just verify the message is marked as expired
        let ttl = Duration::from_millis(10);
        let msg = Message::new(b"test".to_vec()).with_ttl(ttl);
        
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        assert!(msg.is_expired());
        // TODO: Test that mailbox skips expired messages
    }
}

