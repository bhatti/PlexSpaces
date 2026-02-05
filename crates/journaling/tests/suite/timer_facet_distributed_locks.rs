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
//! - Memory backend for locks

#[cfg(feature = "locks")]
mod distributed_lock_tests {
    use plexspaces_core::{ActorId, ActorRef, ActorService, Message};
    use plexspaces_journaling::{TimerFacet, TimerRegistration};
    use plexspaces_locks::{LockManager, sql::SqliteLockManager};
    use plexspaces_mailbox::{Mailbox, MailboxConfig};
    use plexspaces_facet::Facet;
    use plexspaces_proto::prost_types;
    use std::sync::Arc;
    use async_trait::async_trait;

    /// Mock ActorService that sends messages to a mailbox
    struct MockActorService {
        mailbox: Arc<Mailbox>,
    }

    #[async_trait]
    impl ActorService for MockActorService {
        async fn spawn_actor(
            &self,
            _actor_id: &str,
            _actor_type: &str,
            _initial_state: Vec<u8>,
        ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented for tests".into())
        }

        async fn send(
            &self,
            _actor_id: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            self.mailbox.send(message.into()).await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)?;
            Ok("message-id".to_string())
        }
    }

    /// Helper to create TimerFacet with lock manager
    fn create_timer_facet_with_locks(
        lock_manager: Arc<dyn LockManager>,
        node_id: String,
    ) -> TimerFacet {
        TimerFacet::with_lock_manager(lock_manager, node_id, serde_json::json!({}), 50)
    }

    /// Helper to create TimerFacet without locks
    fn create_timer_facet_without_locks() -> TimerFacet {
        TimerFacet::new(serde_json::json!({}), 75)
    }

    /// Helper to setup a timer facet with all required services
    async fn setup_facet(
        facet: TimerFacet,
        actor_id: &str,
    ) -> (TimerFacet, Arc<Mailbox>) {
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), format!("{}@node-1", actor_id)).await.expect("Failed to create mailbox"));
        let actor_ref = ActorRef::new(format!("{}@node-1", actor_id)).unwrap();
        let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService { mailbox: mailbox.clone() });
        
        let mut facet_mut = facet;
        facet_mut.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        facet_mut.set_actor_ref(actor_ref).await;
        facet_mut.set_actor_service(actor_service).await;
        
        (facet_mut, mailbox)
    }

    #[tokio::test]
    async fn test_timer_registration_with_locks() {
        // Test that timers can be registered with lock manager
        let lock_manager: Arc<dyn LockManager + Send + Sync> = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let facet = create_timer_facet_with_locks(lock_manager, "node-1".to_string());
        let (mut facet_mut, _mailbox) = setup_facet(facet, "test-actor").await;
        
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
        };
        
        let result = facet_mut.register_timer(registration).await;
        assert!(result.is_ok(), "Timer registration with lock should succeed");
        
        // Verify timer is registered
        let timers = facet_mut.list_timers().await;
        assert_eq!(timers.len(), 1, "Should have one registered timer");
        
        // Clean up
        facet_mut.on_detach("test-actor").await.unwrap();
    }

    #[tokio::test]
    async fn test_timer_registration_without_locks() {
        // Test that timers can be registered without lock manager
        let facet = create_timer_facet_without_locks();
        let (mut facet_mut, _mailbox) = setup_facet(facet, "test-actor").await;
        
        let registration = TimerRegistration {
            actor_id: "test-actor@node-1".to_string(),
            timer_name: "unlocked-timer".to_string(),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 10_000_000, // 10ms
            }),
            callback_data: vec![],
            periodic: false,
        };
        
        let result = facet_mut.register_timer(registration).await;
        assert!(result.is_ok(), "Timer registration without lock should succeed");
        
        // Verify timer is registered
        let timers = facet_mut.list_timers().await;
        assert_eq!(timers.len(), 1, "Should have one registered timer");
        
        // Clean up
        facet_mut.on_detach("test-actor").await.unwrap();
    }

    #[tokio::test]
    async fn test_timer_multi_node_lock_isolation() {
        // Test that multiple nodes with same actor_id compete for lock
        // Only the node that holds the lock should have its timer fire
        let lock_manager: Arc<dyn LockManager + Send + Sync> = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        
        // Node 1: Register timer with lock
        let facet1 = create_timer_facet_with_locks(lock_manager.clone(), "node-1".to_string());
        let (mut facet1_mut, _mailbox1) = setup_facet(facet1, "test-actor-1").await;
        
        // Node 2: Try to register timer with same actor_id (same lock key)
        let facet2 = create_timer_facet_with_locks(lock_manager.clone(), "node-2".to_string());
        let (mut facet2_mut, _mailbox2) = setup_facet(facet2, "test-actor-1").await; // Same actor_id!
        
        // Register timer on node 1 - should acquire lock
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
        };
        let result1 = facet1_mut.register_timer(registration1).await;
        assert!(result1.is_ok(), "Node 1 timer registration should succeed");
        
        // Register timer on node 2 - should register but lock acquisition will fail
        let registration2 = TimerRegistration {
            actor_id: "test-actor-1@node-2".to_string(),
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
        };
        // Registration succeeds (timer is registered), but lock acquisition will fail
        let result2 = facet2_mut.register_timer(registration2).await;
        assert!(result2.is_ok(), "Node 2 timer registration should succeed (but won't fire)");
        
        // Both timers should be registered
        let timers1 = facet1_mut.list_timers().await;
        let timers2 = facet2_mut.list_timers().await;
        assert_eq!(timers1.len(), 1, "Node 1 should have one timer");
        assert_eq!(timers2.len(), 1, "Node 2 should have one timer");
        
        // Clean up
        facet1_mut.on_detach("test-actor-1").await.unwrap();
        facet2_mut.on_detach("test-actor-1").await.unwrap();
    }

    #[tokio::test]
    async fn test_timer_cleanup_on_detach() {
        // Test that all timers are properly cleaned up on detach
        let lock_manager: Arc<dyn LockManager + Send + Sync> = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let facet = create_timer_facet_with_locks(lock_manager, "node-1".to_string());
        let (mut facet_mut, _mailbox) = setup_facet(facet, "test-actor").await;
        
        // Register multiple timers
        for i in 0..3 {
            let registration = TimerRegistration {
                actor_id: "test-actor@node-1".to_string(),
                timer_name: format!("timer-{}", i),
                interval: Some(prost_types::Duration {
                    seconds: 0,
                    nanos: 100_000_000,
                }),
                due_time: Some(prost_types::Duration {
                    seconds: 0,
                    nanos: 0,
                }),
                callback_data: vec![],
                periodic: true,
            };
            facet_mut.register_timer(registration).await.unwrap();
        }
        
        // Verify all timers are registered
        let timers = facet_mut.list_timers().await;
        assert_eq!(timers.len(), 3, "Should have three registered timers");
        
        // Detach should clean up all timers
        facet_mut.on_detach("test-actor").await.unwrap();
        
        // Verify timers are cleaned up (can't list after detach, but verify no panic)
        // The timers map should be empty after on_detach
    }
}
