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

//! Facet factories for journaling-related facets
//!
//! ## Purpose
//! Provides factories for creating journaling-related facet instances from configuration.
//! These factories use ServiceLocator to get runtime dependencies (JournalStorage, etc.)
//! ensuring facets use the configured services from RuntimeConfig.
//!
//! ## Design Principles
//! - **ServiceLocator-based**: All dependencies come from ServiceLocator
//! - **Runtime Configuration**: Extract priority and config from proto/config
//! - **Consistent Pattern**: All factories follow the same structure

use async_trait::async_trait;
use plexspaces_core::ServiceLocator;
use plexspaces_facet::{Facet, FacetError, FacetFactory, FacetMetadata};
use serde_json::Value;
use std::sync::Arc;
use tracing;

/// Facet factory for VirtualActorFacet
///
/// ## Purpose
/// Creates VirtualActorFacet instances. VirtualActorFacet enables automatic
/// activation/deactivation for always-addressable actors (Orleans-style).
pub struct VirtualActorFacetFactory;

#[async_trait]
impl FacetFactory for VirtualActorFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use crate::VirtualActorFacet;

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(100);

        Ok(Box::new(VirtualActorFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "virtual_actor".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: 100,
        }
    }
}

/// Facet factory for DurabilityFacet
///
/// ## Purpose
/// Creates DurabilityFacet instances by getting JournalStorage from ServiceLocator.
/// DurabilityFacet enables checkpoint-based state persistence (Restate-inspired).
pub struct DurabilityFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl DurabilityFacetFactory {
    /// Create a new DurabilityFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get JournalStorage from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for DurabilityFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use crate::DurabilityFacet;
        use plexspaces_core::JournalStorage;

        // Get JournalStorage from ServiceLocator
        let journal_storage = self.service_locator.get_journal_storage().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "JournalStorage not found in ServiceLocator. Ensure JournalStorage is registered during service initialization.".to_string()
            ))?;

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(90);

        Ok(Box::new(DurabilityFacet::new(
            journal_storage,
            config,
            priority,
        )))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "durability".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: 90,
        }
    }
}

/// Factory for creating TimerFacet instances
///
/// ## Purpose
/// Creates TimerFacet instances by getting ServiceLocator for ActorService lookup.
/// TimerFacet needs ServiceLocator to send TimerFired messages to actors.
pub struct TimerFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl TimerFacetFactory {
    /// Create a new TimerFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for TimerFacet to use
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for TimerFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use crate::timer_facet::{TimerFacet, TIMER_FACET_DEFAULT_PRIORITY};

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(TIMER_FACET_DEFAULT_PRIORITY);

        // Check if distributed locking is enabled in config
        let use_distributed_locking = config
            .get("enable_distributed_locking")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let timer_facet = if use_distributed_locking {
            #[cfg(feature = "locks")]
            {
                // Get LockManager and node_id for distributed locking
                let lock_manager = self.service_locator.get_lock_manager().await
                    .ok_or_else(|| FacetError::InvalidConfig(
                        "LockManager not found in ServiceLocator. Required for distributed timer locking.".to_string()
                    ))?;

                let node_id = self
                    .service_locator
                    .get_node_config()
                    .await
                    .and_then(|cfg| Some(cfg.id.clone()))
                    .unwrap_or_else(|| "unknown".to_string());

                TimerFacet::with_lock_manager(
                    lock_manager,
                    node_id,
                    config,
                    priority,
                    self.service_locator.clone(),
                )
            }
            #[cfg(not(feature = "locks"))]
            {
                return Err(FacetError::InvalidConfig(
                    "Distributed locking requires 'locks' feature. Either disable 'enable_distributed_locking' or enable the 'locks' feature.".to_string()
                ));
            }
        } else {
            TimerFacet::new(config, priority, self.service_locator.clone())
        };

        Ok(Box::new(timer_facet))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "timer".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: crate::timer_facet::TIMER_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating ReminderFacet instances
///
/// ## Purpose
/// Creates ReminderFacet instances by getting JournalStorage and ServiceLocator.
/// ReminderFacet needs JournalStorage for persistence and ServiceLocator for sending messages.
pub struct ReminderFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl ReminderFacetFactory {
    /// Create a new ReminderFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get JournalStorage from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for ReminderFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use crate::reminder_facet::{ReminderFacet, REMINDER_FACET_DEFAULT_PRIORITY};
        use plexspaces_core::JournalStorage;

        // Get JournalStorage from ServiceLocator
        let journal_storage = self.service_locator.get_journal_storage().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "JournalStorage not found in ServiceLocator. Ensure JournalStorage is registered during service initialization.".to_string()
            ))?;

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(REMINDER_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(ReminderFacet::new(
            journal_storage,
            config,
            priority,
            self.service_locator.clone(),
        )))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "reminder".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: crate::reminder_facet::REMINDER_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating EventSourcingFacet instances
///
/// ## Purpose
/// Creates EventSourcingFacet instances by getting JournalStorage from ServiceLocator.
/// EventSourcingFacet needs JournalStorage for event persistence.
///
/// ## Note
/// EventSourcingFacet uses generics which requires concrete JournalStorage types.
/// This factory returns an error explaining that EventSourcingFacet needs to be
/// refactored to use trait objects (like DurabilityFacet) or created with a concrete type.
pub struct EventSourcingFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl EventSourcingFacetFactory {
    /// Create a new EventSourcingFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get JournalStorage from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for EventSourcingFacetFactory {
    async fn create(&self, _config: Value) -> Result<Box<dyn Facet>, FacetError> {
        // EventSourcingFacet uses generics (EventSourcingFacet<S: JournalStorage + Clone>)
        // which requires concrete types, not trait objects.
        // This is a limitation that needs to be addressed by refactoring EventSourcingFacet
        // to use Arc<dyn JournalStorage> like DurabilityFacet does.

        Err(FacetError::InvalidConfig(
            "EventSourcingFacet requires a concrete JournalStorage type. Consider using DurabilityFacet or refactoring EventSourcingFacet to use trait objects.".to_string()
        ))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "event_sourcing".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: crate::event_sourcing_facet::EVENT_SOURCING_FACET_DEFAULT_PRIORITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::SqliteJournalStorage;
    use plexspaces_core::{JournalStorage, LockManager};
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_services::ServiceLocatorImpl;

    /// Helper to create a test ServiceLocator with JournalStorage
    async fn create_test_service_locator() -> Arc<dyn ServiceLocator> {
        let service_locator = Arc::new(ServiceLocatorImpl::new());

        // Register JournalStorage
        let journal_storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
        service_locator
            .register_journal_storage(journal_storage)
            .await;

        // Register LockManager (for TimerFacet with distributed locking)
        let lock_manager: Arc<dyn LockManager + Send + Sync> =
            Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        service_locator.register_lock_manager(lock_manager).await;

        // Register NodeConfig
        let node_config = plexspaces_proto::node::v1::NodeConfig {
            id: "test-node".to_string(),
            listen_addr: "127.0.0.1:8000".to_string(),
            ..Default::default()
        };
        service_locator.register_node_config(node_config).await;

        // Register a minimal ActorService for TimerFacet/ReminderFacet
        use plexspaces_core::{ActorRef, ActorService, Message};

        struct MockActorService;

        #[async_trait::async_trait]
        impl ActorService for MockActorService {
            async fn spawn_actor(
                &self,
                _ctx: &plexspaces_core::RequestContext,
                _actor_id: &str,
                _actor_type: &str,
                _initial_state: Vec<u8>,
            ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
                Err("Not implemented for tests".into())
            }

            async fn send(
                &self,
                _ctx: &plexspaces_core::RequestContext,
                _actor_id: &str,
                _message: Message,
            ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
                Ok("message-id".to_string())
            }
        }

        let actor_service: Arc<dyn ActorService> = Arc::new(MockActorService);
        service_locator.register_actor_service(actor_service).await;

        service_locator
    }

    #[tokio::test]
    async fn test_virtual_actor_facet_factory() {
        let factory = VirtualActorFacetFactory;

        let config = serde_json::json!({
            "priority": 100,
            "idle_timeout": "5m"
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "virtual_actor");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "virtual_actor");
        assert_eq!(metadata.priority, 100);
    }

    #[tokio::test]
    async fn test_virtual_actor_facet_factory_custom_priority() {
        let factory = VirtualActorFacetFactory;

        let config = serde_json::json!({
            "priority": 150
        });

        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 150);
    }

    #[tokio::test]
    async fn test_durability_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = DurabilityFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 90
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "durability");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "durability");
    }

    #[tokio::test]
    async fn test_durability_facet_factory_no_journal_storage() {
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        let factory = DurabilityFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("JournalStorage not found"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_timer_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = TimerFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 50
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "timer");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "timer");
    }

    #[tokio::test]
    async fn test_timer_facet_factory_with_distributed_locking() {
        let service_locator = create_test_service_locator().await;
        let factory = TimerFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 50,
            "enable_distributed_locking": true
        });

        let facet = factory.create(config).await;
        #[cfg(feature = "locks")]
        {
            assert!(facet.is_ok(), "factory.create failed: {:?}", facet.err().map(|e| e.to_string()));
            let facet = facet.unwrap();
            assert_eq!(facet.facet_type(), "timer");
        }
        #[cfg(not(feature = "locks"))]
        {
            assert!(facet.is_err());
            match facet {
                Err(FacetError::InvalidConfig(msg)) => {
                    assert!(
                        msg.contains("Distributed locking requires"),
                        "unexpected message: {}",
                        msg
                    );
                }
                Err(other) => panic!(
                    "expected InvalidConfig(distributed locking), got {:?}",
                    other
                ),
                Ok(_) => panic!("expected error when locks feature is disabled"),
            }
        }
    }

    #[tokio::test]
    async fn test_timer_facet_factory_distributed_locking_no_lock_manager() {
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        let factory = TimerFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "enable_distributed_locking": true
        });

        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                #[cfg(feature = "locks")]
                {
                    assert!(
                        msg.contains("LockManager not found"),
                        "unexpected message: {}",
                        msg
                    );
                }
                #[cfg(not(feature = "locks"))]
                {
                    assert!(
                        msg.contains("Distributed locking requires"),
                        "unexpected message: {}",
                        msg
                    );
                }
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_reminder_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = ReminderFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 50
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "reminder");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "reminder");
    }

    #[tokio::test]
    async fn test_reminder_facet_factory_no_journal_storage() {
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        let factory = ReminderFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("JournalStorage not found"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_event_sourcing_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = EventSourcingFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        // EventSourcingFacetFactory returns an error because EventSourcingFacet uses generics
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("EventSourcingFacet requires a concrete JournalStorage type"));
            }
            _ => panic!("Expected InvalidConfig error about concrete type"),
        }

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "event_sourcing");
    }

    #[tokio::test]
    async fn test_journaling_factories_extract_priority_from_config() {
        let service_locator = create_test_service_locator().await;

        // Test VirtualActorFacetFactory with custom priority
        let factory = VirtualActorFacetFactory;
        let config = serde_json::json!({ "priority": 150 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 150);

        // Test DurabilityFacetFactory with custom priority
        let factory = DurabilityFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({ "priority": 120 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 120);

        // Test TimerFacetFactory with custom priority
        let factory = TimerFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({ "priority": 75 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 75);

        // Test ReminderFacetFactory with custom priority
        let factory = ReminderFacetFactory::new(service_locator);
        let config = serde_json::json!({ "priority": 80 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 80);
    }

    #[tokio::test]
    async fn test_journaling_factories_use_default_priority() {
        let service_locator = create_test_service_locator().await;

        // Test VirtualActorFacetFactory uses default priority
        let factory = VirtualActorFacetFactory;
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 100);

        // Test DurabilityFacetFactory uses default priority
        let factory = DurabilityFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 90);

        // Test TimerFacetFactory uses default priority
        let factory = TimerFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(
            facet.get_priority(),
            crate::timer_facet::TIMER_FACET_DEFAULT_PRIORITY
        );

        // Test ReminderFacetFactory uses default priority
        let factory = ReminderFacetFactory::new(service_locator);
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(
            facet.get_priority(),
            crate::reminder_facet::REMINDER_FACET_DEFAULT_PRIORITY
        );
    }
}
