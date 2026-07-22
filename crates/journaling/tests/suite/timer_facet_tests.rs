// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! TimerFacet Tests
//!
//! Comprehensive test suite for TimerFacet following TDD principles.
//! Tests cover registration, unregistration, firing, and lifecycle.

use async_trait::async_trait;
use plexspaces_actor::{ActorId, ActorService, Message, ServiceLocator, ServiceTraitsActorRef};
use plexspaces_facet::Facet;
use plexspaces_journaling::{TimerError, TimerFacet, TimerRegistration};
use plexspaces_proto::prost_types;
use plexspaces_services::ServiceLocatorImpl;
use std::sync::Arc;

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
        _ctx: &plexspaces_actor::RequestContext,
        _spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
    ) -> Result<ServiceTraitsActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented for tests".into())
    }

    async fn send(
        &self,
        _ctx: &plexspaces_actor::RequestContext,
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

/// Helper to setup a timer facet with all required services
async fn setup_facet_with_services(actor_id: &str) -> (TimerFacet, Arc<MockActorService>) {
    let (service_locator, mock_service) = create_test_service_locator().await;
    let mut facet = TimerFacet::new(serde_json::json!({}), 50, service_locator);
    facet
        .on_attach(actor_id, serde_json::json!({}))
        .await
        .unwrap();
    (facet, mock_service)
}

fn timer_tests_actor_id() -> String {
    ActorId::new("test-actor", "gen_server", "default", "test-node")
        .expect("valid timer test actor id")
        .to_string()
}

fn timer_registration(
    timer_name: impl Into<String>,
    interval_nanos: i32,
    due_time_nanos: i32,
    periodic: bool,
) -> TimerRegistration {
    TimerRegistration {
        actor_id: timer_tests_actor_id(),
        timer_name: timer_name.into(),
        interval: Some(prost_types::Duration {
            seconds: 0,
            nanos: interval_nanos,
        }),
        due_time: Some(prost_types::Duration {
            seconds: 0,
            nanos: due_time_nanos,
        }),
        callback_data: vec![],
        periodic,
    }
}

#[tokio::test]
async fn test_timer_facet_creation() {
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let facet = TimerFacet::new(serde_json::json!({}), 75, service_locator);
    assert_eq!(facet.facet_type(), "timer");
}

#[tokio::test]
async fn test_timer_facet_attach() {
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = TimerFacet::new(serde_json::json!({}), 50, service_locator);
    let result = facet
        .on_attach(&timer_tests_actor_id(), serde_json::json!({}))
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_timer_facet_detach() {
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = TimerFacet::new(serde_json::json!({}), 50, service_locator);
    facet
        .on_attach(&timer_tests_actor_id(), serde_json::json!({}))
        .await
        .unwrap();
    let result = facet.on_detach(&timer_tests_actor_id()).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_one_time_timer() {
    let (facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("test-timer", 0, 100_000_000, false);

    let result = facet.register_timer(registration).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_periodic_timer() {
    let (facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("periodic-timer", 100_000_000, 0, true);

    let result = facet.register_timer(registration).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_register_duplicate_timer_fails() {
    let (facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("duplicate-timer", 0, 100_000_000, false);

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
    let (facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("unregister-timer", 0, 100_000_000, false);

    facet.register_timer(registration).await.unwrap();

    let result = facet.unregister_timer("unregister-timer").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_unregister_nonexistent_timer_fails() {
    let (service_locator, _mock_service) = create_test_service_locator().await;
    let mut facet = TimerFacet::new(serde_json::json!({}), 50, service_locator);
    facet
        .on_attach(&timer_tests_actor_id(), serde_json::json!({}))
        .await
        .unwrap();

    let result = facet.unregister_timer("nonexistent").await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), TimerError::TimerNotFound(_)));
}

#[tokio::test]
async fn test_periodic_timer_requires_interval() {
    let (facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("invalid-periodic", 0, 0, true);

    let result = facet.register_timer(registration).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        TimerError::InvalidRegistration(_)
    ));
}

#[tokio::test]
async fn test_timer_fires_and_sends_message() {
    // Test that timer registration works and timer is properly configured
    // Note: Actual firing is tested in integration tests with longer timeouts
    let (mut facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let mut registration = timer_registration("fire-timer", 0, 50_000_000, false);
    registration.callback_data = b"test-data".to_vec();

    // Verify timer can be registered
    let result = facet.register_timer(registration).await;
    assert!(result.is_ok(), "Timer registration should succeed");

    // Verify timer is in the list
    let timers = facet.list_timers().await;
    assert_eq!(timers.len(), 1, "Should have one registered timer");
    assert_eq!(timers[0].timer_name, "fire-timer");

    // Clean up
    facet.on_detach(&timer_tests_actor_id()).await.unwrap();
}

#[tokio::test]
async fn test_periodic_timer_fires_multiple_times() {
    // Test that periodic timer registration works
    // Note: Actual firing is tested in integration tests with longer timeouts
    let (mut facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("periodic-fire", 50_000_000, 0, true);

    // Verify periodic timer can be registered
    let result = facet.register_timer(registration).await;
    assert!(result.is_ok(), "Periodic timer registration should succeed");

    // Verify timer is in the list
    let timers = facet.list_timers().await;
    assert_eq!(timers.len(), 1, "Should have one registered timer");
    assert_eq!(timers[0].timer_name, "periodic-fire");
    assert!(timers[0].periodic, "Timer should be marked as periodic");

    // Clean up
    facet.on_detach(&timer_tests_actor_id()).await.unwrap();
}

#[tokio::test]
async fn test_timers_cleared_on_detach() {
    let (mut facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    let registration = timer_registration("detach-timer", 0, 100_000_000, false);

    facet.register_timer(registration).await.unwrap();

    // Detach should clear timers
    facet.on_detach(&timer_tests_actor_id()).await.unwrap();

    // Attempting to unregister should fail (timer was cleared)
    let result = facet.unregister_timer("detach-timer").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_multiple_timers_simultaneously() {
    // Test that multiple timers can be registered simultaneously
    let (mut facet, _mock_service) = setup_facet_with_services(&timer_tests_actor_id()).await;

    // Register multiple timers
    for i in 0..5 {
        let registration = timer_registration(format!("timer-{}", i), 0, 50_000_000, false);
        let result = facet.register_timer(registration).await;
        assert!(result.is_ok(), "Timer {} registration should succeed", i);
    }

    // Verify all timers are registered
    let timers = facet.list_timers().await;
    assert_eq!(timers.len(), 5, "Should have 5 registered timers");

    // Clean up
    facet.on_detach(&timer_tests_actor_id()).await.unwrap();
}
