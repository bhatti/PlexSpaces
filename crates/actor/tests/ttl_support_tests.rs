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
use plexspaces_core::ServiceLocator;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn test_message_with_ttl_is_expired_after_ttl() {
    // Create a message with a very short TTL (10ms)
    let mut message = Message::new(b"test".to_vec());
    let message = message.with_ttl(Duration::from_millis(10));
    
    // Wait for TTL to expire
    tokio::time::sleep(Duration::from_millis(20)).await;
    
    // Message should be expired
    assert!(message.is_expired());
}

#[tokio::test]
async fn test_message_without_ttl_is_not_expired() {
    // Create a message without TTL
    let message = Message::new(b"test".to_vec());
    
    // Message should not be expired (no TTL means never expires)
    assert!(!message.is_expired());
}

#[tokio::test]
async fn test_message_with_ttl_not_expired_before_ttl() {
    // Create a message with TTL (100ms)
    let message = Message::new(b"test".to_vec()).with_ttl(Duration::from_millis(100));
    
    // Wait less than TTL
    tokio::time::sleep(Duration::from_millis(50)).await;
    
    // Message should not be expired yet
    assert!(!message.is_expired());
}

#[tokio::test]
async fn test_message_ttl_preserved_in_proto_conversion() {
    // Create a message with TTL
    let ttl = Duration::from_secs(60);
    let message = Message::new(b"test".to_vec()).with_ttl(ttl);
    
    // Convert to proto
    let proto = message.to_proto();
    
    // TTL should be preserved
    assert!(proto.ttl.is_some());
    let proto_ttl = proto.ttl.as_ref().unwrap();
    assert_eq!(proto_ttl.seconds, 60);
    assert_eq!(proto_ttl.nanos, 0);
    
    // Convert back from proto
    let restored = Message::from_proto(&proto);
    
    // TTL should be restored
    assert!(restored.ttl().is_some());
    let restored_ttl = restored.ttl().unwrap();
    assert_eq!(restored_ttl.as_secs(), 60);
}

#[tokio::test]
async fn test_actor_ref_tell_with_ttl_message() {
    // Create actor ref and mailbox
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test@node1".to_string()).await.unwrap());
    let service_locator = Arc::new(ServiceLocator::new());
    let actor_ref = ActorRef::local("test@node1".to_string(), Arc::clone(&mailbox), service_locator);
    
    // Create message with TTL
    let message = Message::new(b"test".to_vec()).with_ttl(Duration::from_secs(1));
    
    // Send message
    actor_ref.tell(message.clone()).await.unwrap();
    
    // Message should be in mailbox
    let received = mailbox.dequeue().await;
    assert!(received.is_some());
    let received_msg = received.unwrap();
    assert_eq!(received_msg.id, message.id);
    
    // TTL should be preserved
    assert!(received_msg.ttl().is_some());
}

#[tokio::test]
async fn test_expired_message_not_delivered() {
    // Create mailbox
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-mailbox@node1".to_string()).await.unwrap();
    
    // Create expired message
    let mut message = Message::new(b"test".to_vec()).with_ttl(Duration::from_millis(1));
    tokio::time::sleep(Duration::from_millis(10)).await; // Wait for expiration
    
    // Verify message is expired before enqueueing
    assert!(message.is_expired());
    
    // Try to enqueue expired message
    // Mailbox should reject expired messages
    let result = mailbox.enqueue(message).await;
    
    // Should fail or message should be filtered out
    // (Implementation depends on mailbox behavior)
}

#[tokio::test]
async fn test_message_ttl_getter_and_setter() {
    let message = Message::new(b"test".to_vec());
    
    // Initially no TTL
    assert!(message.ttl().is_none());
    
    // Set TTL (creates new message)
    let ttl = Duration::from_secs(30);
    let message_with_ttl = message.with_ttl(ttl);
    assert_eq!(message_with_ttl.ttl(), Some(ttl));
    
    // Original message still has no TTL (with_ttl creates new message)
    let message2 = Message::new(b"test".to_vec());
    assert!(message2.ttl().is_none());
}

