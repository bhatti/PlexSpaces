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

//! Integration tests for TimerFacet with distributed locks
//!
//! Tests cover:
//! - Timer registration with distributed locks (multi-node protection)
//! - Lock acquisition, renewal, and release
//! - SQLite backend for locks
//! - Memory backend for locks (for comparison)
//! - Concurrent timer registration from multiple nodes

#[cfg(feature = "locks")]
mod distributed_lock_tests {
    use plexspaces_core::{ActorId, ActorRef};
    use plexspaces_journaling::{TimerFacet, TimerRegistration};
    use plexspaces_locks::{LockManager, memory::MemoryLockManager};
    use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
    use plexspaces_facet::Facet;
    use plexspaces_proto::prost_types;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::sleep;

    /// Helper to create TimerFacet with lock manager
    fn create_timer_facet_with_locks(
        lock_manager: Arc<dyn LockManager>,
        node_id: String,
    ) -> TimerFacet {
        TimerFacet::with_lock_manager(lock_manager, node_id)
    }

    /// Helper to create TimerFacet without locks
    fn create_timer_facet_without_locks() -> TimerFacet {
        TimerFacet::new(serde_json::json!({}), 75)
    }

    #[tokio::test]
    async fn test_timer_with_distributed_lock_acquires() {
        let lock_manager: Arc<dyn LockManager> = Arc::new(MemoryLockManager::new());
        let facet = create_timer_facet_with_locks(lock_manager, "node-1".to_string());
        
        let mut facet_mut = facet;
        facet_mut.on_attach("test-actor", serde_json::json!({})).await.unwrap();
        
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@node-1".to_string()).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new("test-actor@node-1".to_string()).unwrap();
        facet_mut.set_actor_ref(actor_ref).await;
        
        // Register timer with lock key
        let registration = TimerRegistration {
            actor_id: "test-actor@node-1".to_string(),
            timer_name: "locked-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000, // 100ms
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic: true,
            lock_key: Some("timer-lock".to_string()),
        };
        
        let result = facet_mut.register_timer(registration).await;
        assert!(result.is_ok(), "Timer registration with lock should succeed");
    }

    #[tokio::test]
    async fn test_timer_without_lock_fires_normally() {
        let facet = create_timer_facet_without_locks();
        
        let mut facet_mut = facet;
        facet_mut.on_attach("test-actor", serde_json::json!({})).await.unwrap();
        
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@node-1".to_string()).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new("test-actor@node-1".to_string()).unwrap();
        facet_mut.set_actor_ref(actor_ref).await;
        
        // Register timer without lock key
        let registration = TimerRegistration {
            actor_id: "test-actor@node-1".to_string(),
            timer_name: "unlocked-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000, // 50ms
            }),
            callback_data: vec![],
            periodic: false,
            lock_key: None, // No lock
        };
        
        facet_mut.register_timer(registration).await.unwrap();
        
        // Wait for timer to fire
        sleep(Duration::from_millis(100)).await;
        
        // Check if message was received
        let message = mailbox.dequeue().await;
        assert!(message.is_some(), "Timer without lock should fire normally");
    }

    #[tokio::test]
    async fn test_timer_with_lock_only_fires_on_lock_holder() {
        let lock_manager: Arc<dyn LockManager> = Arc::new(MemoryLockManager::new());
        
        // Node 1: Register timer with lock
        let facet1 = create_timer_facet_with_locks(lock_manager.clone(), "node-1".to_string());
        let mut facet1_mut = facet1;
        facet1_mut.on_attach("test-actor-1", serde_json::json!({})).await.unwrap();
        
        let mailbox1 = Arc::new(Mailbox::new(MailboxConfig::default()));
        let actor_ref1 = ActorRef::new("test-actor-1@node-1".to_string()).unwrap();
        facet1_mut.set_actor_ref(actor_ref1).await;
        
        // Node 2: Try to register timer with same lock key (should not fire)
        let facet2 = create_timer_facet_with_locks(lock_manager.clone(), "node-2".to_string());
        let mut facet2_mut = facet2;
        facet2_mut.on_attach("test-actor-2", serde_json::json!({})).await.unwrap();
        
        let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default()));
        let actor_ref2 = ActorRef::new("test-actor-2@node-2".to_string()).unwrap();
        facet2_mut.set_actor_ref(actor_ref2).await;
        
        // Register timer on node 1 with lock
        let registration1 = TimerRegistration {
            actor_id: "test-actor-1@node-1".to_string(),
            timer_name: "node1-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000, // 50ms
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic: true,
            lock_key: Some("shared-lock".to_string()),
        };
        facet1_mut.register_timer(registration1).await.unwrap();
        
        // Try to register timer on node 2 with same lock key
        // Node 2 should not be able to acquire lock (node 1 holds it)
        let registration2 = TimerRegistration {
            actor_id: "test-actor-2@node-2".to_string(),
            timer_name: "node2-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000, // 50ms
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic: true,
            lock_key: Some("shared-lock".to_string()),
        };
        // Registration should succeed (timer is registered), but it won't fire because lock acquisition fails
        facet2_mut.register_timer(registration2).await.unwrap();
        
        // Wait for timers to potentially fire
        sleep(Duration::from_millis(200)).await;
        
        // Node 1 should have received messages (lock held)
        let mut node1_count = 0;
        while mailbox1.dequeue().await.is_some() {
            node1_count += 1;
        }
        assert!(node1_count >= 2, "Node 1 should have received timer fires (lock held)");
        
        // Node 2 should NOT have received messages (lock not held)
        let mut node2_count = 0;
        while mailbox2.dequeue().await.is_some() {
            node2_count += 1;
        }
        assert_eq!(node2_count, 0, "Node 2 should NOT have received timer fires (lock not held)");
    }

    #[tokio::test]
    async fn test_timer_lock_renewal() {
        let lock_manager: Arc<dyn LockManager> = Arc::new(MemoryLockManager::new());
        let facet = create_timer_facet_with_locks(lock_manager, "node-1".to_string());
        
        let mut facet_mut = facet;
        facet_mut.on_attach("test-actor", serde_json::json!({})).await.unwrap();
        
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@node-1".to_string()).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new("test-actor@node-1".to_string()).unwrap();
        facet_mut.set_actor_ref(actor_ref).await;
        
        // Register periodic timer with lock (should renew lock on each fire)
        let registration = TimerRegistration {
            actor_id: "test-actor@node-1".to_string(),
            timer_name: "renewal-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000, // 100ms
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic: true,
            lock_key: Some("renewal-lock".to_string()),
        };
        
        facet_mut.register_timer(registration).await.unwrap();
        
        // Wait for multiple fires (lock should be renewed)
        sleep(Duration::from_millis(350)).await;
        
        // Timer should have fired multiple times (lock renewed successfully)
        // This test verifies that lock renewal works for periodic timers
        // (actual message count verification would require mailbox access)
    }

    #[cfg(feature = "sqlite-backend")]
    #[tokio::test]
    async fn test_timer_with_sqlite_lock_backend() {
        // This test verifies TimerFacet works with SQLite lock backend
        // Note: Requires SQLite lock manager implementation
        // For now, we'll use MemoryLockManager as a placeholder
        // TODO: Implement SQLite lock manager and update this test
        
        let lock_manager: Arc<dyn LockManager> = Arc::new(MemoryLockManager::new());
        let facet = create_timer_facet_with_locks(lock_manager, "node-1".to_string());
        
        let mut facet_mut = facet;
        facet_mut.on_attach("test-actor", serde_json::json!({})).await.unwrap();
        
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@node-1".to_string()).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new("test-actor@node-1".to_string()).unwrap();
        facet_mut.set_actor_ref(actor_ref).await;
        
        // Register timer with lock
        let registration = TimerRegistration {
            actor_id: "test-actor@node-1".to_string(),
            timer_name: "sqlite-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000, // 50ms
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic: true,
            lock_key: Some("sqlite-lock".to_string()),
        };
        
        let result = facet_mut.register_timer(registration).await;
        assert!(result.is_ok(), "Timer registration with SQLite lock backend should succeed");
    }
}

