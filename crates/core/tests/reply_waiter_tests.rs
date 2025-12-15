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

//! Comprehensive tests for ReplyWaiter and ReplyWaiterRegistry

use plexspaces_core::{ReplyWaiter, ReplyWaiterRegistry, ReplyWaiterError};
use plexspaces_mailbox::Message;
use std::time::Duration;

#[tokio::test]
async fn test_reply_waiter_new() {
    let waiter = ReplyWaiter::new();
    // Just verify it can be created
    assert!(true);
}

#[tokio::test]
async fn test_reply_waiter_wait_and_notify() {
    let waiter = ReplyWaiter::new();
    let reply_msg = Message::new(b"reply".to_vec());
    
    // Spawn task to notify after a short delay
    let waiter_clone = waiter.clone();
    let msg_clone = reply_msg.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(10)).await;
        waiter_clone.notify(msg_clone).await.unwrap();
    });
    
    // Wait for reply
    let result = waiter.wait(Duration::from_secs(1)).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), reply_msg);
}

#[tokio::test]
async fn test_reply_waiter_timeout() {
    let waiter = ReplyWaiter::new();
    
    // Wait for reply that never comes
    let result = waiter.wait(Duration::from_millis(50)).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ReplyWaiterError::Timeout => {}
        _ => panic!("Expected Timeout error"),
    }
}

#[tokio::test]
async fn test_reply_waiter_double_notify() {
    let waiter = ReplyWaiter::new();
    let reply_msg1 = Message::new(b"reply1".to_vec());
    let reply_msg2 = Message::new(b"reply2".to_vec());
    
    // First notify should succeed
    assert!(waiter.notify(reply_msg1.clone()).await.is_ok());
    
    // Second notify should fail
    let result = waiter.notify(reply_msg2).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ReplyWaiterError::AlreadySet => {}
        _ => panic!("Expected AlreadySet error"),
    }
}

#[tokio::test]
async fn test_reply_waiter_concurrent_waiters() {
    // Each waiter is a separate instance, so they each get their own reply
    // This test verifies that multiple waiters can wait concurrently
    let waiter = ReplyWaiter::new();
    let reply_msg = Message::new(b"reply".to_vec());
    
    // Spawn a single waiter (cloning creates shared state, so all clones share the same reply)
    let waiter_clone = waiter.clone();
    let handle = tokio::spawn(async move {
        waiter_clone.wait(Duration::from_secs(1)).await
    });
    
    // Notify after a short delay
    tokio::time::sleep(Duration::from_millis(10)).await;
    waiter.notify(reply_msg.clone()).await.unwrap();
    
    // Waiter should get the reply
    let result = handle.await.unwrap();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), reply_msg);
    
    // Test that multiple independent waiters work correctly
    let waiter1 = ReplyWaiter::new();
    let waiter2 = ReplyWaiter::new();
    let reply_msg1 = Message::new(b"reply1".to_vec());
    let reply_msg2 = Message::new(b"reply2".to_vec());
    
    let waiter1_clone = waiter1.clone();
    let waiter2_clone = waiter2.clone();
    let handle1 = tokio::spawn(async move {
        waiter1_clone.wait(Duration::from_secs(1)).await
    });
    let handle2 = tokio::spawn(async move {
        waiter2_clone.wait(Duration::from_secs(1)).await
    });
    
    tokio::time::sleep(Duration::from_millis(10)).await;
    waiter1.notify(reply_msg1.clone()).await.unwrap();
    waiter2.notify(reply_msg2.clone()).await.unwrap();
    
    assert_eq!(handle1.await.unwrap().unwrap(), reply_msg1);
    assert_eq!(handle2.await.unwrap().unwrap(), reply_msg2);
}

#[tokio::test]
async fn test_reply_waiter_registry_new() {
    let registry = ReplyWaiterRegistry::new();
    // Just verify it can be created
    assert!(true);
}

#[tokio::test]
async fn test_reply_waiter_registry_register_and_notify() {
    let registry = ReplyWaiterRegistry::new();
    let correlation_id = "corr-123".to_string();
    let waiter = ReplyWaiter::new();
    let reply_msg = Message::new(b"reply".to_vec());
    
    // Register waiter
    registry.register(correlation_id.clone(), waiter.clone()).await;
    
    // Spawn task to wait for reply
    let waiter_clone = waiter.clone();
    let msg_clone = reply_msg.clone();
    let handle = tokio::spawn(async move {
        waiter_clone.wait(Duration::from_secs(1)).await
    });
    
    // Notify via registry
    tokio::time::sleep(Duration::from_millis(10)).await;
    let notified = registry.notify(&correlation_id, reply_msg.clone()).await;
    assert!(notified);
    
    // Verify waiter received the reply
    let result = handle.await.unwrap();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), reply_msg);
}

#[tokio::test]
async fn test_reply_waiter_registry_notify_unknown() {
    let registry = ReplyWaiterRegistry::new();
    let correlation_id = "corr-unknown".to_string();
    let reply_msg = Message::new(b"reply".to_vec());
    
    // Try to notify unknown correlation_id
    let notified = registry.notify(&correlation_id, reply_msg).await;
    assert!(!notified);
}

#[tokio::test]
async fn test_reply_waiter_registry_remove() {
    let registry = ReplyWaiterRegistry::new();
    let correlation_id = "corr-123".to_string();
    let waiter = ReplyWaiter::new();
    
    // Register waiter
    registry.register(correlation_id.clone(), waiter).await;
    
    // Remove waiter
    registry.remove(&correlation_id).await;
    
    // Try to notify - should fail
    let reply_msg = Message::new(b"reply".to_vec());
    let notified = registry.notify(&correlation_id, reply_msg).await;
    assert!(!notified);
}

#[tokio::test]
async fn test_reply_waiter_registry_multiple_correlation_ids() {
    let registry = ReplyWaiterRegistry::new();
    let corr1 = "corr-1".to_string();
    let corr2 = "corr-2".to_string();
    let waiter1 = ReplyWaiter::new();
    let waiter2 = ReplyWaiter::new();
    let reply_msg1 = Message::new(b"reply1".to_vec());
    let reply_msg2 = Message::new(b"reply2".to_vec());
    
    // Register both waiters
    registry.register(corr1.clone(), waiter1.clone()).await;
    registry.register(corr2.clone(), waiter2.clone()).await;
    
    // Spawn tasks to wait
    let waiter1_clone = waiter1.clone();
    let waiter2_clone = waiter2.clone();
    let msg1_clone = reply_msg1.clone();
    let msg2_clone = reply_msg2.clone();
    let handle1 = tokio::spawn(async move {
        waiter1_clone.wait(Duration::from_secs(1)).await
    });
    let handle2 = tokio::spawn(async move {
        waiter2_clone.wait(Duration::from_secs(1)).await
    });
    
    // Notify both
    tokio::time::sleep(Duration::from_millis(10)).await;
    assert!(registry.notify(&corr1, reply_msg1.clone()).await);
    assert!(registry.notify(&corr2, reply_msg2.clone()).await);
    
    // Verify both received correct replies
    assert_eq!(handle1.await.unwrap().unwrap(), reply_msg1);
    assert_eq!(handle2.await.unwrap().unwrap(), reply_msg2);
}

#[tokio::test]
async fn test_reply_waiter_registry_auto_remove_on_notify() {
    let registry = ReplyWaiterRegistry::new();
    let correlation_id = "corr-123".to_string();
    let waiter = ReplyWaiter::new();
    let reply_msg = Message::new(b"reply".to_vec());
    
    // Register waiter
    registry.register(correlation_id.clone(), waiter).await;
    
    // Notify - should remove waiter automatically
    assert!(registry.notify(&correlation_id, reply_msg.clone()).await);
    
    // Try to notify again - should fail (waiter was removed)
    let reply_msg2 = Message::new(b"reply2".to_vec());
    assert!(!registry.notify(&correlation_id, reply_msg2).await);
}

#[tokio::test]
async fn test_reply_waiter_default() {
    let waiter = ReplyWaiter::default();
    // Just verify it can be created
    assert!(true);
}

#[tokio::test]
async fn test_reply_waiter_registry_default() {
    let registry = ReplyWaiterRegistry::default();
    // Just verify it can be created
    assert!(true);
}
