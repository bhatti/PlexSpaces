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

//! TimerFacet Tests
//!
//! Comprehensive test suite for TimerFacet following TDD principles.
//! Tests cover registration, unregistration, firing, and lifecycle.

use plexspaces_core::{ActorId, ActorRef};
use plexspaces_journaling::{TimerFacet, TimerError, TimerRegistration};
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use plexspaces_facet::Facet;
use plexspaces_proto::prost_types;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_timer_facet_creation() {
    let facet = TimerFacet::new();
    assert_eq!(facet.facet_type(), "timer");
}

#[tokio::test]
async fn test_timer_facet_attach() {
    let mut facet = TimerFacet::new();
    let result = facet.on_attach("test-actor", serde_json::json!({})).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_timer_facet_detach() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    let result = facet.on_detach("test-actor").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_one_time_timer() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    // Create a mock actor ref (format: actor@node)
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "test-timer".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        callback_data: vec![],
        periodic: false,
    };
    
    let result = facet.register_timer(registration).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_periodic_timer() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "periodic-timer".to_string(),
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
    
    let result = facet.register_timer(registration).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_duplicate_timer_fails() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "duplicate-timer".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
        callback_data: vec![],
        periodic: false,
    };
    
    // First registration should succeed
    let result1 = facet.register_timer(registration.clone()).await;
    assert!(result1.is_ok());
    
    // Second registration with same name should fail
    let result2 = facet.register_timer(registration).await;
    assert!(result2.is_err());
    assert!(matches!(result2.unwrap_err(), TimerError::TimerExists(_)));
}

#[tokio::test]
async fn test_unregister_timer() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "unregister-timer".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
        callback_data: vec![],
        periodic: false,
    };
    
    facet.register_timer(registration).await.unwrap();
    
    let result = facet.unregister_timer("unregister-timer").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unregister_nonexistent_timer_fails() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let result = facet.unregister_timer("nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TimerError::TimerNotFound(_)));
}

#[tokio::test]
async fn test_periodic_timer_requires_interval() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "invalid-periodic".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0, // Zero interval for periodic timer (invalid)
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        callback_data: vec![],
        periodic: true,
    };
    
    let result = facet.register_timer(registration).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TimerError::InvalidRegistration(_)));
}

#[tokio::test]
async fn test_timer_fires_and_sends_message() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "fire-timer".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 50_000_000, // 50ms
        }),
        callback_data: b"test-data".to_vec(),
        periodic: false,
    };
    
    facet.register_timer(registration).await.unwrap();
    
    // Wait for timer to fire
    sleep(Duration::from_millis(100)).await;
    
    // Check if message was received
    let message = mailbox.dequeue().await;
    assert!(message.is_some());
    
    let msg = message.unwrap();
    assert_eq!(msg.metadata.get("type"), Some(&"TimerFired".to_string()));
    assert_eq!(msg.metadata.get("timer_name"), Some(&"fire-timer".to_string()));
}

#[tokio::test]
async fn test_periodic_timer_fires_multiple_times() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "periodic-fire".to_string(),
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
    
    facet.register_timer(registration).await.unwrap();
    
    // Wait for multiple fires
    sleep(Duration::from_millis(200)).await;
    
    // Should have received multiple messages
    let mut count = 0;
    while mailbox.dequeue().await.is_some() {
        count += 1;
    }
    assert!(count >= 2, "Expected at least 2 timer fires, got {}", count);
}

#[tokio::test]
async fn test_timers_cleared_on_detach() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    let registration = TimerRegistration {
        actor_id: "test-actor@test-node".to_string(),
        timer_name: "detach-timer".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
        callback_data: vec![],
        periodic: false,
    };
    
    facet.register_timer(registration).await.unwrap();
    
    // Detach should clear timers
    facet.on_detach("test-actor").await.unwrap();
    
    // Attempting to unregister should fail (timer was cleared)
    let result = facet.unregister_timer("detach-timer").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_timers_simultaneously() {
    let mut facet = TimerFacet::new();
    facet.on_attach("test-actor", serde_json::json!({})).await.unwrap();
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));
    let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
    facet.set_actor_ref(actor_ref).await;
    
    // Register multiple timers
    for i in 0..5 {
        let registration = TimerRegistration {
            actor_id: "test-actor@test-node".to_string(),
            timer_name: format!("timer-{}", i),
            interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000,
            }),
            callback_data: vec![],
            periodic: false,
        };
        facet.register_timer(registration).await.unwrap();
    }
    
    // Wait for all timers to fire
    sleep(Duration::from_millis(150)).await;
    
    // Should have received messages from all timers
    let mut count = 0;
    loop {
        match tokio::time::timeout(Duration::from_millis(10), mailbox.dequeue()).await {
            Ok(Some(_)) => count += 1,
            Ok(None) => break, // No more messages
            Err(_) => break, // Timeout
        }
    }
    assert_eq!(count, 5, "Expected 5 timer fires, got {}", count);
}

