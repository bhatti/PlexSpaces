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

//! TTL (Time-To-Live) Support Tests
//!
//! Tests for message TTL functionality to prevent stale messages
//! from accumulating in mailboxes.

use plexspaces_actor::ActorRef;
use plexspaces_core::{Message, ServiceLocator};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use std::sync::Arc;
use std::time::Duration;
use ulid::Ulid;

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
        ..Default::default()
    }
}

/// Helper to create a test message with TTL
fn create_test_message_with_ttl(payload: Vec<u8>, ttl: Duration) -> Message {
    let mut msg = create_test_message(payload);
    msg.ttl = Some(prost_types::Duration {
        seconds: ttl.as_secs() as i64,
        nanos: ttl.subsec_nanos() as i32,
    });
    msg
}

/// Helper to check if a message is expired based on timestamp and TTL
fn is_message_expired(message: &Message) -> bool {
    if message.ttl.is_none() || message.timestamp.is_none() {
        return false;
    }
    
    let ttl = message.ttl.as_ref().unwrap();
    let timestamp = message.timestamp.as_ref().unwrap();
    
    let ttl_duration = Duration::new(ttl.seconds as u64, ttl.nanos as u32);
    let created_at = std::time::UNIX_EPOCH + Duration::new(timestamp.seconds as u64, timestamp.nanos as u32);
    let expiry = created_at + ttl_duration;
    
    std::time::SystemTime::now() > expiry
}

#[tokio::test]
async fn test_message_with_ttl_is_expired_after_ttl() {
    // Create a message with a very short TTL (10ms)
    let message = create_test_message_with_ttl(b"test".to_vec(), Duration::from_millis(10));
    
    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(20)).await;
    
    // Message should be expired
    assert!(is_message_expired(&message));
}

#[tokio::test]
async fn test_message_without_ttl_is_not_expired() {
    // Create a message without TTL
    let message = create_test_message(b"test".to_vec());
    
    // Message should not be expired (no TTL means never expires)
    assert!(!is_message_expired(&message));
}

#[tokio::test]
async fn test_message_with_ttl_not_expired_before_ttl() {
    // Create a message with TTL (100ms)
    let message = create_test_message_with_ttl(b"test".to_vec(), Duration::from_millis(100));
    
    // Wait less than TTL
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Message should not be expired yet
    assert!(!is_message_expired(&message));
}

#[tokio::test]
async fn test_message_ttl_preserved_in_proto() {
    // Create a message with TTL
    let ttl = Duration::from_secs(60);
    let message = create_test_message_with_ttl(b"test".to_vec(), ttl);
    
    // TTL should be set
    assert!(message.ttl.is_some());
    let proto_ttl = message.ttl.as_ref().unwrap();
    assert_eq!(proto_ttl.seconds, 60);
    assert_eq!(proto_ttl.nanos, 0);
}

#[tokio::test]
async fn test_actor_ref_tell_with_ttl_message() {
    // Create actor ref and mailbox with sufficient capacity
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000; // Large capacity to prevent "Mailbox is full" errors
    let mailbox = Arc::new(Mailbox::new(mailbox_config, "test@node1".to_string()).await.unwrap());
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
    let actor_ref = ActorRef::local("test@node1".to_string(), "test".to_string(), Arc::clone(&mailbox), service_locator.clone());
    
    // Register actor before calling tell()
    use plexspaces_core::{ActorRegistry, RequestContext};
    if let Some(registry) = service_locator.actor_registry().await {
        let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
        let actor_id = actor_ref.id().clone();
        let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref.clone());
        registry.register_actor(&ctx, actor_id, sender, None, None, None, None).await;
    }
    
    // Create message with TTL
    let message = create_test_message_with_ttl(b"test".to_vec(), Duration::from_secs(1));
    let message_id = message.id.clone();
    
    // Send message
    actor_ref.tell(message).await.unwrap();
    
    // Message should be in mailbox
    let received = mailbox.dequeue().await;
    assert!(received.is_some());
    let received_msg = received.unwrap();
    assert_eq!(received_msg.id, message_id);
    
    // TTL should be preserved (using getter method since ttl field is private)
    assert!(received_msg.ttl().is_some());
}

#[tokio::test]
async fn test_expired_message_detected_by_helper() {
    // Create message with very short TTL
    let message = create_test_message_with_ttl(b"test".to_vec(), Duration::from_millis(1));
    
    // Message should not be expired yet
    assert!(!is_message_expired(&message));
    
    // Wait for expiration
    tokio::time::sleep(Duration::from_millis(10)).await;
    
    // Message should now be expired
    assert!(is_message_expired(&message));
}

#[tokio::test]
async fn test_message_ttl_direct_access() {
    let message = create_test_message(b"test".to_vec());
    
    // Initially no TTL
    assert!(message.ttl.is_none());
    
    // Create message with TTL
    let ttl = Duration::from_secs(30);
    let message_with_ttl = create_test_message_with_ttl(b"test".to_vec(), ttl);
    
    // TTL should be set
    assert!(message_with_ttl.ttl.is_some());
    let proto_ttl = message_with_ttl.ttl.as_ref().unwrap();
    assert_eq!(proto_ttl.seconds, 30);
}
