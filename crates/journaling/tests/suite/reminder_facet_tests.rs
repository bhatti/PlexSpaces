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

//! ReminderFacet Tests
//!
//! Comprehensive test suite for ReminderFacet following TDD principles.
//! Tests cover registration, unregistration, persistence, and max_occurrences.

use plexspaces_core::{ActorId, ActorRef, ActorService, Message, ServiceLocator};
use plexspaces_journaling::{JournalStorage, SqliteJournalStorage, ReminderFacet, ReminderError, ReminderRegistration, ReminderState};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_facet::Facet;
use plexspaces_proto::prost_types;
use plexspaces_services::ServiceLocatorImpl;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;
use async_trait::async_trait;

/// Mock ActorService that tracks sent messages
struct MockActorService {
    sent_messages: Arc<tokio::sync::RwLock<Vec<Message>>>,
}

impl MockActorService {
    fn new() -> Self {
        Self {
            sent_messages: Arc::new(tokio::sync::RwLock::new(Vec::new())),
        }
    }
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
        self.sent_messages.write().await.push(message);
        Ok("message-id".to_string())
    }
}

/// Helper to create a test ServiceLocator with ActorService registered
async fn create_test_service_locator() -> (Arc<dyn ServiceLocator>, Arc<MockActorService>) {
    let service_locator = Arc::new(ServiceLocatorImpl::new());
    let mock_service = Arc::new(MockActorService::new());
    let actor_service: Arc<dyn ActorService> = mock_service.clone();
    service_locator.register_actor_service(actor_service).await;
    (service_locator, mock_service)
}

/// Helper to setup a reminder facet with all required services
async fn setup_facet_with_services(
    storage: Arc<dyn JournalStorage>,
    actor_id: &str,
) -> (ReminderFacet, Arc<MockActorService>) {
    let (service_locator, mock_service) = create_test_service_locator().await;
    let mut facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();
    (facet, mock_service)
}

#[tokio::test]
async fn test_reminder_facet_creation() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    assert_eq!(facet.facet_type(), "reminder");
}

#[tokio::test]
async fn test_reminder_facet_attach() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    let result = facet.on_attach("test-actor@test-node", serde_json::json!({})).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_reminder_facet_detach() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    facet.on_attach("test-actor@test-node", serde_json::json!({})).await.unwrap();
    let result = facet.on_detach("test-actor@test-node").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_reminder() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "test-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 0, // Infinite
    };
    
    let result = facet.register_reminder(registration).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_duplicate_reminder_fails() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "duplicate-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 0,
    };
    
    // First registration should succeed
    let result1 = facet.register_reminder(registration.clone()).await;
    assert!(result1.is_ok());
    
    // Second registration with same name should fail
    let result2 = facet.register_reminder(registration).await;
    assert!(result2.is_err());
    assert!(matches!(result2.unwrap_err(), ReminderError::ReminderExists(_)));
}

#[tokio::test]
async fn test_unregister_reminder() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "unregister-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 0,
    };
    
    facet.register_reminder(registration).await.unwrap();
    
    let result = facet.unregister_reminder("unregister-reminder").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unregister_nonexistent_reminder_fails() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    facet.on_attach("test-actor@test-node", serde_json::json!({})).await.unwrap();
    
    let result = facet.unregister_reminder("nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), ReminderError::ReminderNotFound(_)));
}

#[tokio::test]
async fn test_reminder_persistence() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet1, _mock_service1) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "persistent-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 0,
    };
    
    facet1.register_reminder(registration).await.unwrap();
    
    // Detach and create new facet (simulating actor deactivation/reactivation)
    facet1.on_detach("test-actor@test-node").await.unwrap();
    
    // Create new facet and attach (simulating reactivation)
    let (service_locator2, _mock_service2) = create_test_service_locator().await;
    let mut facet2 = ReminderFacet::new(storage.clone(), serde_json::json!({}), 50, service_locator2);
    facet2.on_attach("test-actor@test-node", serde_json::json!({})).await.unwrap();
    
    // Reminder should be loaded from storage
    let reminders = facet2.list_reminders().await;
    assert_eq!(reminders.len(), 1);
    assert_eq!(reminders[0].registration.as_ref().unwrap().reminder_name, "persistent-reminder");
}

#[tokio::test]
async fn test_max_occurrences_auto_deletion() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    // Register reminder with max_occurrences = 2
    let next_fire = SystemTime::now() + Duration::from_millis(50);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "limited-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: 50_000_000, // 50ms
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 2,
    };
    
    facet.register_reminder(registration).await.unwrap();
    
    // Wait for reminder to fire twice (background task polls every 100ms, reminder fires every 50ms)
    // Need enough time for background task to check and fire twice
    sleep(Duration::from_millis(300)).await;
    
    // Reminder should be auto-deleted after 2 fires
    let reminders = facet.list_reminders().await;
    assert_eq!(reminders.len(), 0, "Reminder should be auto-deleted after max_occurrences");
}

#[tokio::test]
async fn test_multiple_reminders() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    
    // Register multiple reminders
    for i in 0..5 {
        let registration = ReminderRegistration {
            actor_id: "test-actor@test-node".to_string(),
            reminder_name: format!("reminder-{}", i),
            interval: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
            first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
            callback_data: vec![],
            persist_across_activations: true,
            max_occurrences: 0,
        };
        facet.register_reminder(registration).await.unwrap();
    }
    
    // Should have all reminders
    let reminders = facet.list_reminders().await;
    assert_eq!(reminders.len(), 5);
}

#[tokio::test]
async fn test_reminders_cleared_on_detach() {
    let storage = Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
    let (mut facet, _mock_service) = setup_facet_with_services(storage.clone(), "test-actor@test-node").await;
    
    let next_fire = SystemTime::now() + Duration::from_secs(60);
    let registration = ReminderRegistration {
        actor_id: "test-actor@test-node".to_string(),
        reminder_name: "detach-reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp::from(next_fire)),
        callback_data: vec![],
        persist_across_activations: true,
        max_occurrences: 0,
    };
    
    facet.register_reminder(registration).await.unwrap();
    
    // Detach should stop background task but reminders persist in storage
    facet.on_detach("test-actor@test-node").await.unwrap();
    
    // Reminders are persisted, so they should still be in storage
    // (but background task is stopped)
    // Note: load_reminders is part of JournalStorage trait, accessible via storage
    let reminders = storage.load_reminders("test-actor@test-node").await.unwrap();
    assert_eq!(reminders.len(), 1);
}

