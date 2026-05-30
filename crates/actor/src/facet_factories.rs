// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Facet factories for non-journaling facets
//!
//! ## Purpose
//! Provides factories for creating facet instances from configuration.
//! These factories use ServiceLocator to get runtime dependencies ensuring
//! facets use the configured services from RuntimeConfig.
//!
//! ## Design Principles
//! - **ServiceLocator-based**: All dependencies come from ServiceLocator
//! - **Runtime Configuration**: Extract priority and config from proto/config
//! - **Consistent Pattern**: All factories follow the same structure
//!
//! ## Note
//! Journaling-related factories (VirtualActorFacetFactory, DurabilityFacetFactory,
//! TimerFacetFactory, ReminderFacetFactory, EventSourcingFacetFactory) are in
//! the `plexspaces-journaling` crate to avoid circular dependencies.

use crate::{ProcessGroupService, RequestContext, ServiceLocator};
use async_trait::async_trait;
use plexspaces_facet::{Facet, FacetError, FacetFactory, FacetMetadata};
use plexspaces_proto::locks::prv::{
    AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions,
};
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use serde_json::Value;
use std::sync::Arc;

/// Factory for creating LockFacet instances
///
/// ## Purpose
/// Creates LockFacet instances by getting LockManager from ServiceLocator.
/// This ensures facets use the LockManager configured in node config/runtime config.
pub struct LockFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl LockFacetFactory {
    /// Create a new LockFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get LockManager from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for LockFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::locks::{LockFacet, LOCK_FACET_DEFAULT_PRIORITY};

        // Get LockManager from ServiceLocator
        let lock_manager = self.service_locator.get_lock_manager().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "LockManager not found in ServiceLocator. Ensure LockManager is registered during service initialization.".to_string()
            ))?;

        // Convert locks::LockManager to facet::LockManager trait
        let adapter = LockManagerAdapter {
            inner: lock_manager,
        };

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(LOCK_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(LockFacet::new(
            Arc::new(adapter),
            config,
            priority,
        )))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "locks".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::capabilities::locks::LOCK_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Adapter that converts locks::LockManager to facet::LockManager trait
struct LockManagerAdapter {
    inner: Arc<dyn plexspaces_locks::LockManager + Send + Sync>,
}

#[async_trait]
impl plexspaces_facet::capabilities::locks::LockManager for LockManagerAdapter {
    async fn acquire_lock(
        &self,
        ctx: &RequestContext,
        options: AcquireLockOptions,
    ) -> Result<Lock, String> {
        self.inner
            .acquire_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn renew_lock(
        &self,
        ctx: &RequestContext,
        options: RenewLockOptions,
    ) -> Result<Lock, String> {
        self.inner
            .renew_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn release_lock(
        &self,
        ctx: &RequestContext,
        options: ReleaseLockOptions,
    ) -> Result<(), String> {
        self.inner
            .release_lock(ctx, options)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> Result<Option<Lock>, String> {
        self.inner
            .get_lock(ctx, lock_key)
            .await
            .map_err(|e| e.to_string())
    }
}

/// Factory for creating RegistryFacet instances
///
/// ## Purpose
/// Creates RegistryFacet instances by getting ObjectRegistry from ServiceLocator.
/// This ensures facets use the ObjectRegistry configured in node config/runtime config.
pub struct RegistryFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl RegistryFacetFactory {
    /// Create a new RegistryFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get ObjectRegistry from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for RegistryFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::registry::{
            RegistryFacet, REGISTRY_FACET_DEFAULT_PRIORITY,
        };

        // Get ObjectRegistry from ServiceLocator
        let object_registry = self.service_locator.get_object_registry().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "ObjectRegistry not found in ServiceLocator. Ensure ObjectRegistry is registered during service initialization.".to_string()
            ))?;

        // Convert core::ObjectRegistry to facet::ObjectRegistry trait
        let adapter = ObjectRegistryAdapter {
            inner: object_registry,
        };

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(REGISTRY_FACET_DEFAULT_PRIORITY);

        // Store ServiceLocator in RegistryFacet so it can get NodeConfig defaults and auth_enabled
        let service_locator_for_facet: Arc<
            dyn plexspaces_facet::capabilities::registry::ServiceLocatorTrait,
        > = Arc::new(ServiceLocatorAdapter {
            inner: self.service_locator.clone(),
        });
        Ok(Box::new(RegistryFacet::new_with_service_locator(
            Arc::new(adapter),
            config,
            priority,
            service_locator_for_facet,
        )))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "registry".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::capabilities::registry::REGISTRY_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Adapter that converts ServiceLocator to ServiceLocatorTrait (avoids circular dependency)
struct ServiceLocatorAdapter {
    inner: Arc<dyn ServiceLocator>,
}

#[async_trait]
impl plexspaces_facet::capabilities::registry::ServiceLocatorTrait for ServiceLocatorAdapter {
    async fn is_auth_disabled(&self) -> bool {
        self.inner.is_auth_disabled().await
    }

    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig> {
        self.inner.get_node_config().await
    }
}

/// Adapter that converts core::ObjectRegistry to facet::ObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<dyn crate::ObjectRegistry>,
}

#[async_trait]
impl plexspaces_facet::capabilities::registry::ObjectRegistry for ObjectRegistryAdapter {
    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), String> {
        self.inner
            .register(ctx, registration)
            .await
            .map_err(|e| e.to_string())
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<(), String> {
        // Convert string to ObjectType enum (required for unregister)
        let object_type_enum = object_type
            .as_ref()
            .map(|s| match s.as_str() {
                "Actor" | "actor" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor
                }
                "TupleSpace" | "tuplespace" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
                }
                "Service" | "service" => {
                    plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
                }
                _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            })
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor);

        self.inner
            .unregister(ctx, object_type_enum, object_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<Option<ObjectRegistration>, String> {
        let object_type_enum = object_type.as_ref().map(|s| match s.as_str() {
            "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            "TupleSpace" | "tuplespace" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
            }
            "Service" | "service" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
            }
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });

        self.inner
            .lookup(ctx, object_id, object_type_enum)
            .await
            .map_err(|e| e.to_string())
    }

    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<String>,
        name: Option<String>,
        labels: Option<Vec<String>>,
        health_status: Option<String>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, String> {
        let object_type_enum = object_type.as_ref().map(|s| match s.as_str() {
            "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            "TupleSpace" | "tuplespace" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace
            }
            "Service" | "service" => {
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService
            }
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });

        let health_status_enum = health_status.as_ref().map(|s| match s.as_str() {
            "Healthy" | "healthy" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy
            }
            "Unhealthy" | "unhealthy" | "Dead" | "dead" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusDead
            }
            "Unknown" | "unknown" => {
                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown
            }
            _ => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
        });

        self.inner
            .discover(
                ctx,
                object_type_enum,
                name,
                None,
                labels,
                health_status_enum,
                offset,
                limit,
            )
            .await
            .map_err(|e| e.to_string())
    }
}

/// Factory for creating ProcessGroupFacet instances
///
/// ## Purpose
/// Creates ProcessGroupFacet instances by getting ProcessGroupRegistry from ServiceLocator.
/// This ensures facets use the ProcessGroupRegistry configured in node config/runtime config.
pub struct ProcessGroupFacetFactory {
    service_locator: Arc<dyn ServiceLocator>,
}

impl ProcessGroupFacetFactory {
    /// Create a new ProcessGroupFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get ProcessGroupRegistry from
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for ProcessGroupFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::process_groups::{
            ProcessGroupFacet, PROCESS_GROUP_FACET_DEFAULT_PRIORITY,
        };

        // Get ProcessGroupRegistry from ServiceLocator
        let process_group_service = self.service_locator.get_process_group_service().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "ProcessGroupService not found in ServiceLocator. Ensure ProcessGroupRegistry is registered during service initialization.".to_string()
            ))?;

        let adapter = ProcessGroupRegistryAdapter {
            inner: process_group_service,
        };

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(PROCESS_GROUP_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(ProcessGroupFacet::new(
            Arc::new(adapter),
            config,
            priority,
        )))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "process_group".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority:
                plexspaces_facet::capabilities::process_groups::PROCESS_GROUP_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Adapter that converts core::ProcessGroupService to facet::ProcessGroupRegistry trait
struct ProcessGroupRegistryAdapter {
    inner: Arc<dyn ProcessGroupService>,
}

#[async_trait]
impl plexspaces_facet::capabilities::process_groups::ProcessGroupRegistry
    for ProcessGroupRegistryAdapter
{
    async fn create_group(&self, ctx: &RequestContext, group_name: &str) -> Result<(), String> {
        self.inner
            .create_group(ctx, group_name)
            .await
            .map_err(|e| e.to_string())
    }

    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), String> {
        self.inner
            .join_group(ctx, group_name, actor_id, topics)
            .await
            .map_err(|e| e.to_string())
    }

    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), String> {
        self.inner
            .leave_group(ctx, group_name, actor_id)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        self.inner
            .get_members(ctx, group_name)
            .await
            .map_err(|e| e.to_string())
    }

    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        self.inner
            .get_local_members(ctx, group_name)
            .await
            .map_err(|e| e.to_string())
    }

    async fn list_groups(&self, ctx: &RequestContext) -> Result<Vec<String>, String> {
        self.inner.list_groups(ctx).await.map_err(|e| e.to_string())
    }

    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Vec<u8>,
    ) -> Result<Vec<String>, String> {
        // ProcessGroupService::publish_to_group takes Message, not Vec<u8>
        use crate::Message;
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: message,
            ..Default::default()
        };

        let recipient_count = self
            .inner
            .publish_to_group(ctx, group_name, topic, msg)
            .await
            .map_err(|e| e.to_string())?;

        // Get members to return as recipients
        let members = self.get_members(ctx, group_name).await?;
        // Return first N members where N = recipient_count
        Ok(members.into_iter().take(recipient_count as usize).collect())
    }
}

/// Factory for creating KeyValueFacet instances
///
/// ## Purpose
/// Creates `KeyValueFacet` instances backed by in-memory store.
/// Pass config with `"type": "redis"` or similar to use an external store.
pub struct KeyValueFacetFactory;

impl KeyValueFacetFactory {
    /// Create a new KeyValueFacetFactory.
    pub fn new(_service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self
    }
}

#[async_trait]
impl FacetFactory for KeyValueFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::keyvalue::{
            KeyValueFacet, KEYVALUE_FACET_DEFAULT_PRIORITY,
        };

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(KEYVALUE_FACET_DEFAULT_PRIORITY);

        // KeyValueFacet can work with in-memory store (default) or use ServiceLocator's KeyValueStore
        // KeyValueFacet::new() handles this internally via config
        Ok(Box::new(KeyValueFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "keyvalue".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::capabilities::keyvalue::KEYVALUE_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating HttpClientFacet instances
///
/// ## Purpose
/// Creates HttpClientFacet instances. HttpClientFacet is simple and has no dependencies.
pub struct HttpClientFacetFactory;

#[async_trait]
impl FacetFactory for HttpClientFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::http_client::{
            HttpClientFacet, HTTP_CLIENT_FACET_DEFAULT_PRIORITY,
        };

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(HTTP_CLIENT_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(HttpClientFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "http_client".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority:
                plexspaces_facet::capabilities::http_client::HTTP_CLIENT_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating EventEmitterFacet instances
///
/// ## Purpose
/// Creates EventEmitterFacet instances. EventEmitterFacet is simple and has no dependencies.
pub struct EventEmitterFacetFactory;

#[async_trait]
impl FacetFactory for EventEmitterFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::{EventEmitterFacet, EVENT_EMITTER_FACET_DEFAULT_PRIORITY};

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(EVENT_EMITTER_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(EventEmitterFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "event_emitter".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::EVENT_EMITTER_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating LoggingFacet instances
///
/// ## Purpose
/// Creates LoggingFacet instances. LoggingFacet is simple and has no dependencies.
pub struct LoggingFacetFactory;

#[async_trait]
impl FacetFactory for LoggingFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::{LoggingFacet, LOGGING_FACET_DEFAULT_PRIORITY};

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(LOGGING_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(LoggingFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "logging".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::LOGGING_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating CachingFacet instances
///
/// ## Purpose
/// Creates CachingFacet instances. CachingFacet is simple and has no dependencies.
pub struct CachingFacetFactory;

#[async_trait]
impl FacetFactory for CachingFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::{CachingFacet, CACHING_FACET_DEFAULT_PRIORITY};

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(CACHING_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(CachingFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "caching".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::CACHING_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Factory for creating MetricsFacet instances
///
/// ## Purpose
/// Creates MetricsFacet instances. MetricsFacet is simple and has no dependencies.
pub struct MetricsFacetFactory;

#[async_trait]
impl FacetFactory for MetricsFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::{MetricsFacet, METRICS_FACET_DEFAULT_PRIORITY};

        let priority = config
            .get("priority")
            .and_then(|v| v.as_i64())
            .map(|p| p as i32)
            .unwrap_or(METRICS_FACET_DEFAULT_PRIORITY);

        Ok(Box::new(MetricsFacet::new(config, priority)))
    }

    fn metadata(&self) -> FacetMetadata {
        FacetMetadata {
            facet_type: "metrics".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::METRICS_FACET_DEFAULT_PRIORITY,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_context::{
        ActorService, ChannelService, ObjectRegistry, ProcessGroupService, TupleSpaceProvider,
    };
    use plexspaces_common::RequestContextExt;
    use crate::behavior_factory::BehaviorRegistry;
    use crate::facet_service_wrapper::{FacetManagerServiceWrapper, FacetRegistryServiceWrapper};
    use crate::monitoring::NodeConnectionInfo;
    use crate::{
        ActorFactory, ActorRegistry, ApplicationManager, BlobServiceTrait, GrpcConnectionManager,
        InitializableServiceLocator, KeyValueStore, LockManager, NodeRegistryTrait,
        ReplyWaiterRegistry, Service, ServiceLocator, VirtualActorManager, WasmRuntimeTrait,
    };
    use async_trait::async_trait;
    use plexspaces_keyvalue::SqliteKVStore;
    use plexspaces_locks::MemoryLockManager;
    use plexspaces_proto::node::v1::{NodeConfig, RuntimeConfig, SecurityConfig};
    use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
    use std::sync::atomic::{AtomicBool, Ordering};
    use tokio::sync::RwLock;

    #[derive(Default)]
    struct TestObjectRegistry;

    #[async_trait]
    impl ObjectRegistry for TestObjectRegistry {
        async fn lookup(
            &self,
            _ctx: &RequestContext,
            _object_id: &str,
            _object_type: Option<ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn lookup_full(
            &self,
            _ctx: &RequestContext,
            _object_type: ObjectType,
            _object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn register(
            &self,
            _ctx: &RequestContext,
            _registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn discover(
            &self,
            _ctx: &RequestContext,
            _object_type: Option<ObjectType>,
            _object_category: Option<String>,
            _capabilities: Option<Vec<String>>,
            _labels: Option<Vec<String>>,
            _health_status: Option<HealthStatus>,
            _offset: usize,
            _limit: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        async fn unregister(
            &self,
            _ctx: &RequestContext,
            _object_type: ObjectType,
            _object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn heartbeat(
            &self,
            _ctx: &RequestContext,
            _object_type: ObjectType,
            _object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct TestProcessGroupService;

    #[async_trait]
    impl ProcessGroupService for TestProcessGroupService {
        async fn create_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn delete_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn join_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
            _actor_id: &str,
            _topics: Vec<String>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn leave_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
            _actor_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn get_members(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        async fn get_local_members(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        async fn list_groups(
            &self,
            _ctx: &RequestContext,
        ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        async fn publish_to_group(
            &self,
            _ctx: &RequestContext,
            _group_name: &str,
            _topic: Option<&str>,
            _message: crate::Message,
        ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
            Ok(0)
        }
    }

    #[derive(Default)]
    struct TestServiceLocator {
        lock_manager: RwLock<Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>>>,
        object_registry: RwLock<Option<Arc<dyn ObjectRegistry>>>,
        process_group_service: RwLock<Option<Arc<dyn ProcessGroupService>>>,
        keyvalue_store: RwLock<Option<Arc<dyn KeyValueStore>>>,
        node_config: RwLock<Option<NodeConfig>>,
        security_config: RwLock<Option<SecurityConfig>>,
        runtime_config: RwLock<Option<RuntimeConfig>>,
        shutdown_requested: AtomicBool,
    }

    #[async_trait]
    impl plexspaces_service_traits::ServiceLocatorBase for TestServiceLocator {
        async fn get_actor_service(
            &self,
        ) -> Option<Arc<dyn plexspaces_service_traits::ActorService>> {
            None
        }

        async fn get_journal_storage(
            &self,
        ) -> Option<Arc<dyn plexspaces_service_traits::JournalStorage + Send + Sync>> {
            None
        }

        async fn get_keyvalue_store(&self) -> Option<Arc<dyn plexspaces_common::KeyValueStore>> {
            self.keyvalue_store.read().await.clone()
        }

        async fn get_lock_manager(
            &self,
        ) -> Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>> {
            self.lock_manager.read().await.clone()
        }

        async fn get_actor_factory(
            &self,
        ) -> Option<Arc<dyn plexspaces_service_traits::ActorFactory>> {
            None
        }

        async fn get_process_group_service(
            &self,
        ) -> Option<Arc<dyn plexspaces_service_traits::ProcessGroupService>> {
            self.process_group_service.read().await.clone()
        }

        fn is_shutdown_requested(&self) -> bool {
            self.shutdown_requested.load(Ordering::SeqCst)
        }

        async fn request_context_for_system_operations(&self) -> RequestContext {
            RequestContext::new_without_auth("system".to_string(), "default".to_string())
        }

        async fn request_context_for_system_operations_with_namespace(
            &self,
            namespace: String,
        ) -> RequestContext {
            RequestContext::new_without_auth("system".to_string(), namespace)
        }
    }

    #[async_trait]
    impl ServiceLocator for TestServiceLocator {
        async fn actor_registry(&self) -> Option<Arc<ActorRegistry>> {
            None
        }

        async fn virtual_actor_manager(&self) -> Option<Arc<VirtualActorManager>> {
            None
        }
        async fn reply_waiter_registry(&self) -> Option<Arc<ReplyWaiterRegistry>> {
            None
        }
        async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
            None
        }
        async fn get_tuplespace_provider(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
            None
        }

        async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
            self.object_registry.read().await.clone()
        }

        async fn get_metrics_prometheus_renderer(
            &self,
        ) -> Option<Arc<dyn crate::MetricsPrometheusRenderer + Send + Sync>> {
            None
        }

        async fn get_metrics_service_access(
            &self,
        ) -> Option<Arc<dyn crate::MetricsServiceAccess + Send + Sync>> {
            None
        }

        async fn get_facet_manager(&self) -> Option<Arc<FacetManagerServiceWrapper>> {
            None
        }

        async fn facet_container_for_actor(
            &self,
            _actor_id: &str,
        ) -> Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>> {
            None
        }

        async fn get_facet_registry(&self) -> Option<Arc<FacetRegistryServiceWrapper>> {
            None
        }

        async fn initialize_services(
            &self,
            _release_config: Option<plexspaces_proto::node::v1::ReleaseSpec>,
        ) {
        }

        async fn get_node_config(&self) -> Option<NodeConfig> {
            self.node_config.read().await.clone()
        }

        async fn get_security_config(&self) -> Option<SecurityConfig> {
            self.security_config.read().await.clone()
        }

        async fn get_runtime_config(&self) -> Option<RuntimeConfig> {
            self.runtime_config.read().await.clone()
        }

        async fn is_auth_disabled(&self) -> bool {
            self.security_config
                .read()
                .await
                .as_ref()
                .map(|config| config.disable_auth)
                .unwrap_or(false)
        }

        async fn get_node_connection_info(
            &self,
        ) -> Option<Arc<dyn NodeConnectionInfo + Send + Sync>> {
            None
        }

        fn request_shutdown(&self) {
            self.shutdown_requested.store(true, Ordering::SeqCst);
        }

        async fn application_manager(&self) -> Option<Arc<dyn ApplicationManager>> {
            None
        }

        async fn get_behavior_registry(&self) -> Option<Arc<BehaviorRegistry>> {
            None
        }

        async fn get_grpc_connection_manager(&self) -> Option<Arc<GrpcConnectionManager>> {
            None
        }

        async fn get_actor_service_client(
            &self,
            _node_id: &str,
        ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
            Err(Box::new(std::io::Error::other(
                "actor service client not configured in test locator",
            )))
        }

        async fn get_application_service_client(
            &self,
            _node_id: &str,
        ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
            Err(Box::new(std::io::Error::other(
                "application service client not configured in test locator",
            )))
        }

        async fn get_wasm_runtime(&self) -> Option<Arc<dyn WasmRuntimeTrait>> {
            None
        }

        async fn get_blob_service(&self) -> Option<Arc<dyn BlobServiceTrait>> {
            None
        }

        async fn get_node_registry(&self) -> Option<Arc<dyn NodeRegistryTrait>> {
            None
        }
    }

    #[async_trait]
    impl InitializableServiceLocator for TestServiceLocator {
        async fn register_service<T: Service + 'static>(&self, _service: Arc<T>)
        where
            Self: Sized,
        {
        }

        async fn get_service<T: Service + 'static>(&self) -> Option<Arc<T>>
        where
            Self: Sized,
        {
            None
        }

        async fn register_service_by_name<T: Service + 'static>(
            &self,
            _name: &str,
            _service: Arc<T>,
        ) where
            Self: Sized,
        {
        }

        async fn get_service_by_name<T: Service + 'static>(&self, _name: &str) -> Option<Arc<T>>
        where
            Self: Sized,
        {
            None
        }

        async fn register_actor_registry(&self, _registry: Arc<ActorRegistry>) {}

        async fn register_actor_service(&self, _service: Arc<dyn ActorService>) {}

        async fn register_channel_service(&self, _service: Arc<dyn ChannelService>) {}

        async fn register_tuplespace_provider(&self, _service: Arc<dyn TupleSpaceProvider>) {}

        async fn register_object_registry(&self, service: Arc<dyn ObjectRegistry>) {
            *self.object_registry.write().await = Some(service);
        }

        async fn register_journal_storage(
            &self,
            _service: Arc<dyn crate::JournalStorage + Send + Sync>,
        ) {
        }

        async fn register_lock_manager(
            &self,
            service: Arc<dyn plexspaces_locks::LockManager + Send + Sync>,
        ) {
            *self.lock_manager.write().await = Some(service);
        }

        async fn register_metrics_prometheus_renderer(
            &self,
            _renderer: Arc<dyn crate::MetricsPrometheusRenderer + Send + Sync>,
        ) {
        }

        async fn register_metrics_service_access(
            &self,
            _service: Arc<dyn crate::MetricsServiceAccess + Send + Sync>,
        ) {
        }

        async fn register_facet_manager(&self, _service: Arc<FacetManagerServiceWrapper>) {}

        async fn register_facet_registry(&self, _service: Arc<FacetRegistryServiceWrapper>) {}

        async fn register_actor_factory(&self, _factory: Arc<dyn ActorFactory>) {}

        async fn register_node_config(&self, config: NodeConfig) {
            *self.node_config.write().await = Some(config);
        }

        async fn register_security_config(&self, config: SecurityConfig) {
            *self.security_config.write().await = Some(config);
        }

        async fn register_runtime_config(&self, config: RuntimeConfig) {
            *self.runtime_config.write().await = Some(config);
        }

        async fn register_node_connection_info(
            &self,
            _accessor: Arc<dyn NodeConnectionInfo + Send + Sync>,
        ) {
        }

        async fn register_application_manager(&self, _manager: Arc<dyn ApplicationManager>) {}

        async fn register_behavior_registry(&self, _registry: Arc<BehaviorRegistry>) {}

        async fn register_grpc_connection_manager(&self, _manager: Arc<GrpcConnectionManager>) {}

        async fn register_wasm_runtime(&self, _runtime: Arc<dyn WasmRuntimeTrait>) {}

        async fn register_process_group_service(&self, service: Arc<dyn ProcessGroupService>) {
            *self.process_group_service.write().await = Some(service);
        }

        async fn register_elastic_pool_service(
            &self,
            _service: Arc<dyn crate::ElasticPoolService>,
        ) {
        }

        async fn register_blob_service(&self, _service: Arc<dyn BlobServiceTrait>) {}

        async fn register_service_link_service(
            &self,
            _service: Arc<dyn plexspaces_service_traits::ServiceLinkAccess>,
        ) {
        }

        async fn register_node_registry(&self, _registry: Arc<dyn NodeRegistryTrait>) {}

        async fn register_keyvalue_store(&self, store: Arc<dyn KeyValueStore>) {
            *self.keyvalue_store.write().await = Some(store);
        }

        async fn register_outbound_http_client(&self, _client: Arc<dyn crate::OutboundHttpClient>) {
        }

        async fn unregister_outbound_http_client(&self) {}

        async fn register_process_group_registry(
            &self,
            _registry: Arc<dyn std::any::Any + Send + Sync>,
        ) {
        }
    }

    /// Helper to create a test ServiceLocator with all required services
    async fn create_test_service_locator() -> Arc<TestServiceLocator> {
        let service_locator = Arc::new(TestServiceLocator::default());
        service_locator
            .register_node_config(NodeConfig {
                id: "test-node".to_string(),
                listen_addr: "127.0.0.1:8000".to_string(),
                ..Default::default()
            })
            .await;
        service_locator
            .register_security_config(SecurityConfig {
                disable_auth: false,
                ..Default::default()
            })
            .await;
        service_locator
            .register_lock_manager(Arc::new(MemoryLockManager::new()))
            .await;
        service_locator
            .register_object_registry(Arc::new(TestObjectRegistry))
            .await;
        service_locator
            .register_process_group_service(Arc::new(TestProcessGroupService))
            .await;
        service_locator
            .register_keyvalue_store(Arc::new(SqliteKVStore::new(":memory:").await.unwrap()))
            .await;
        service_locator
    }

    #[tokio::test]
    async fn test_lock_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = LockFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 30
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "locks");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "locks");
    }

    #[tokio::test]
    async fn test_lock_facet_factory_no_lock_manager() {
        let service_locator = Arc::new(TestServiceLocator::default());
        let factory = LockFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("LockManager not found"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_registry_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = RegistryFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 30
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "registry");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "registry");
    }

    #[tokio::test]
    async fn test_registry_facet_factory_no_object_registry() {
        let service_locator = Arc::new(TestServiceLocator::default());
        let factory = RegistryFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("ObjectRegistry not found"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_process_group_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = ProcessGroupFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 30
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "process_group");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "process_group");
    }

    #[tokio::test]
    async fn test_process_group_facet_factory_no_service() {
        let service_locator = Arc::new(TestServiceLocator::default());
        let factory = ProcessGroupFacetFactory::new(service_locator);

        let config = serde_json::json!({});
        let result = factory.create(config).await;
        assert!(result.is_err());
        match result {
            Err(FacetError::InvalidConfig(msg)) => {
                assert!(msg.contains("ProcessGroupService not found"));
            }
            _ => panic!("Expected InvalidConfig error"),
        }
    }

    #[tokio::test]
    async fn test_keyvalue_facet_factory() {
        let service_locator = create_test_service_locator().await;
        let factory = KeyValueFacetFactory::new(service_locator);

        let config = serde_json::json!({
            "priority": 30
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "keyvalue");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "keyvalue");
    }

    #[tokio::test]
    async fn test_http_client_facet_factory() {
        let factory = HttpClientFacetFactory;

        let config = serde_json::json!({
            "priority": 20
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "http_client");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "http_client");
    }

    #[tokio::test]
    async fn test_event_emitter_facet_factory() {
        let factory = EventEmitterFacetFactory;

        let config = serde_json::json!({
            "priority": 10
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "event_emitter");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "event_emitter");
    }

    #[tokio::test]
    async fn test_logging_facet_factory() {
        let factory = LoggingFacetFactory;

        let config = serde_json::json!({
            "priority": 900,
            "level": "info"
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "logging");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "logging");
    }

    #[tokio::test]
    async fn test_caching_facet_factory() {
        let factory = CachingFacetFactory;

        let config = serde_json::json!({
            "priority": 40,
            "ttl_seconds": 60
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "caching");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "caching");
    }

    #[tokio::test]
    async fn test_metrics_facet_factory() {
        let factory = MetricsFacetFactory;

        let config = serde_json::json!({
            "priority": 800
        });

        let facet = factory.create(config).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "metrics");

        let metadata = factory.metadata();
        assert_eq!(metadata.facet_type, "metrics");
    }

    #[tokio::test]
    async fn test_factories_extract_priority_from_config() {
        let service_locator = create_test_service_locator().await;

        // Test LockFacetFactory with custom priority
        let factory = LockFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({ "priority": 100 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 100);

        // Test RegistryFacetFactory with custom priority
        let factory = RegistryFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({ "priority": 200 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 200);

        // Test ProcessGroupFacetFactory with custom priority
        let factory = ProcessGroupFacetFactory::new(service_locator);
        let config = serde_json::json!({ "priority": 150 });
        let facet = factory.create(config).await.unwrap();
        assert_eq!(facet.get_priority(), 150);
    }

    #[tokio::test]
    async fn test_factories_use_default_priority() {
        let service_locator = create_test_service_locator().await;

        // Test factories use default priority when not specified
        let factory = LockFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(
            facet.get_priority(),
            plexspaces_facet::capabilities::locks::LOCK_FACET_DEFAULT_PRIORITY
        );

        let factory = RegistryFacetFactory::new(service_locator.clone());
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(
            facet.get_priority(),
            plexspaces_facet::capabilities::registry::REGISTRY_FACET_DEFAULT_PRIORITY
        );

        let factory = ProcessGroupFacetFactory::new(service_locator);
        let config = serde_json::json!({});
        let facet = factory.create(config).await.unwrap();
        assert_eq!(
            facet.get_priority(),
            plexspaces_facet::capabilities::process_groups::PROCESS_GROUP_FACET_DEFAULT_PRIORITY
        );
    }
}
