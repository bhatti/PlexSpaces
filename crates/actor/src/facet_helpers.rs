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

//! Helper functions for creating facets from proto configuration
//!
//! ## Purpose
//! Provides simple, extensible functions to convert proto Facet configurations
//! to facet instances. This enables automatic facet attachment during actor creation
//! from ChildSpec configurations.
//!
//! ## Design Principles
//! - **Simple**: Straightforward conversion from proto to facet instances
//! - **Extensible**: Easy to add new facet types via FacetRegistry
//! - **Debuggable**: Clear error messages when facet creation fails
//! - **Production-grade**: Handles errors gracefully, logs appropriately

use plexspaces_facet::{Facet, FacetError, FacetRegistry, FacetFactory, FacetMetadata};
use plexspaces_proto::common::v1::Facet as ProtoFacet;
use plexspaces_core::{RequestContext, ProcessGroupService};
use plexspaces_proto::locks::prv::{AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions};
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use async_trait::async_trait;
use tracing;

/// Convert proto Facet configuration to facet instance
///
/// ## Purpose
/// Creates a facet instance from proto Facet configuration using FacetRegistry.
/// This is used by supervisors to automatically attach facets from ChildSpec.
///
/// ## Arguments
/// * `proto_facet` - Proto Facet configuration
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// `Ok(Box<dyn Facet>)` if facet was created successfully
/// `Err(FacetError)` if facet type not found or creation failed
///
/// ## Example
/// ```rust,ignore
/// let proto_facet = plexspaces_proto::common::v1::Facet {
///     r#type: "timer".to_string(),
///     config: HashMap::new(),
///     priority: 100,
///     state: HashMap::new(),
///     metadata: None,
/// };
/// let facet = create_facet_from_proto(&proto_facet, &facet_registry).await?;
/// ```
pub async fn create_facet_from_proto(
    proto_facet: &ProtoFacet,
    facet_registry: &FacetRegistry,
) -> Result<Box<dyn Facet>, FacetError> {
    let facet_type = &proto_facet.r#type;
    
    // Convert proto config (map<string, string>) to serde_json::Value
    let config_value = proto_config_to_value(&proto_facet.config);
    
    // Create facet instance via registry
    let facet = facet_registry
        .create_facet(facet_type, config_value)
        .await?;
    
    // Set priority from proto (facets may have default priority, but proto overrides it)
    // Note: This requires facets to support priority setting, which is handled by FacetContainer
    // The priority is stored in FacetContainer metadata, not in the facet itself
    // So we just return the facet - priority will be set during attachment
    
    if tracing::enabled!(tracing::Level::DEBUG) {
    tracing::debug!(
        facet_type = %facet_type,
        priority = proto_facet.priority,
        "Created facet from proto configuration"
    );
    }
    
    Ok(facet)
}

/// Convert proto config map to serde_json::Value
///
/// ## Purpose
/// Converts proto's `map<string, string>` to `serde_json::Value` for facet configuration.
/// This is a simple, straightforward conversion that preserves all key-value pairs.
///
/// ## Arguments
/// * `config_map` - Proto config map (string -> string)
///
/// ## Returns
/// `serde_json::Value::Object` with all config key-value pairs
fn proto_config_to_value(config_map: &HashMap<String, String>) -> Value {
    let mut map = serde_json::Map::new();
    for (key, value) in config_map {
        // Try to parse value as JSON, fallback to string if not valid JSON
        match serde_json::from_str::<Value>(value) {
            Ok(json_value) => {
                map.insert(key.clone(), json_value);
            }
            Err(_) => {
                // Not valid JSON, treat as string
                map.insert(key.clone(), Value::String(value.clone()));
            }
        }
    }
    Value::Object(map)
}

/// Create multiple facets from proto configurations
///
/// ## Purpose
/// Creates multiple facet instances from proto Facet configurations.
/// Facets are created in the order provided, but should be sorted by priority
/// before calling this function for proper attachment order.
///
/// ## Arguments
/// * `proto_facets` - Vector of proto Facet configurations
/// * `facet_registry` - FacetRegistry to create facet instances
///
/// ## Returns
/// `Ok(Vec<Box<dyn Facet>>)` with all successfully created facets
/// Errors are logged but don't stop creation of other facets
///
/// ## Error Handling
/// If a facet fails to create, it's logged as a warning and skipped.
/// This ensures that one bad facet doesn't prevent other facets from being attached.
pub async fn create_facets_from_proto(
    proto_facets: &[ProtoFacet],
    facet_registry: &FacetRegistry,
) -> Vec<Box<dyn Facet>> {
    let mut facets = Vec::new();
    
    for proto_facet in proto_facets {
        match create_facet_from_proto(proto_facet, facet_registry).await {
            Ok(facet) => {
                facets.push(facet);
            }
            Err(e) => {
                tracing::warn!(
                    facet_type = %proto_facet.r#type,
                    error = %e,
                    "Failed to create facet from proto configuration (skipping)"
                );
            }
        }
    }
    
    if tracing::enabled!(tracing::Level::DEBUG) {
    tracing::debug!(
        total = proto_facets.len(),
        created = facets.len(),
        "Created facets from proto configurations"
    );
    }
    
    facets
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_facet::{FacetFactory, FacetMetadata};
    use async_trait::async_trait;
    use std::sync::Arc;

    // Test facet for unit tests
    struct TestFacet {
        config: Value,
        priority: i32,
    }

    #[async_trait]
    impl Facet for TestFacet {
        fn facet_type(&self) -> &str {
            "test"
        }

        fn get_config(&self) -> Value {
            self.config.clone()
        }

        fn get_priority(&self) -> i32 {
            self.priority
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }

        async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
            Ok(())
        }

        async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
            Ok(())
        }
    }

    struct TestFacetFactory;

    #[async_trait]
    impl FacetFactory for TestFacetFactory {
        async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
            Ok(Box::new(TestFacet {
                config,
                priority: 50,
            }))
        }

        fn metadata(&self) -> FacetMetadata {
            FacetMetadata {
                facet_type: "test".to_string(),
                attached_at: std::time::Instant::now(),
                config: serde_json::Value::Null,
                priority: 50,
            }
        }
    }

    #[tokio::test]
    async fn test_proto_config_to_value() {
        let mut config_map = HashMap::new();
        config_map.insert("key1".to_string(), "value1".to_string());
        config_map.insert("key2".to_string(), "123".to_string());
        config_map.insert("key3".to_string(), r#"{"nested": "value"}"#.to_string());

        let value = proto_config_to_value(&config_map);
        assert!(value.is_object());
        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("key1").unwrap().as_str().unwrap(), "value1");
        // key2 is parsed as JSON number (123), not string
        assert_eq!(obj.get("key2").unwrap().as_u64().unwrap(), 123);
        // key3 should be parsed as JSON object
        assert!(obj.get("key3").unwrap().is_object());
    }

    #[tokio::test]
    async fn test_create_facet_from_proto() {
        let mut registry = FacetRegistry::new();
        registry.register("test".to_string(), Arc::new(TestFacetFactory));

        let mut config_map = HashMap::new();
        config_map.insert("test_key".to_string(), "test_value".to_string());

        let proto_facet = ProtoFacet {
            r#type: "test".to_string(),
            config: config_map,
            priority: 100,
            state: HashMap::new(),
            metadata: None,
        };

        let facet = create_facet_from_proto(&proto_facet, &registry).await;
        assert!(facet.is_ok());
        let facet = facet.unwrap();
        assert_eq!(facet.facet_type(), "test");
    }

    #[tokio::test]
    async fn test_create_facet_from_proto_not_found() {
        let registry = FacetRegistry::new();

        let proto_facet = ProtoFacet {
            r#type: "nonexistent".to_string(),
            config: HashMap::new(),
            priority: 100,
            state: HashMap::new(),
            metadata: None,
        };

        let result = create_facet_from_proto(&proto_facet, &registry).await;
        assert!(result.is_err());
        // Check error type without requiring Debug on Ok type
        match &result {
            Ok(_) => panic!("Expected NotFound error, got success"),
            Err(FacetError::NotFound(_)) => {
                // Expected - test passes
            }
            Err(e) => {
                panic!("Expected NotFound error, got: {}", e);
            }
        }
    }

    #[tokio::test]
    async fn test_create_facets_from_proto() {
        let mut registry = FacetRegistry::new();
        registry.register("test".to_string(), Arc::new(TestFacetFactory));

        let proto_facets = vec![
            ProtoFacet {
                r#type: "test".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "test".to_string(),
                config: HashMap::new(),
                priority: 50,
                state: HashMap::new(),
                metadata: None,
            },
        ];

        let facets = create_facets_from_proto(&proto_facets, &registry).await;
        assert_eq!(facets.len(), 2);
    }

    #[tokio::test]
    async fn test_create_facets_from_proto_with_errors() {
        let registry = FacetRegistry::new(); // No facets registered

        let proto_facets = vec![
            ProtoFacet {
                r#type: "nonexistent1".to_string(),
                config: HashMap::new(),
                priority: 100,
                state: HashMap::new(),
                metadata: None,
            },
            ProtoFacet {
                r#type: "nonexistent2".to_string(),
                config: HashMap::new(),
                priority: 50,
                state: HashMap::new(),
                metadata: None,
            },
        ];

        let facets = create_facets_from_proto(&proto_facets, &registry).await;
        // Should return empty vector (all facets failed, but didn't panic)
        assert_eq!(facets.len(), 0);
    }
}

/// Factory for creating LockFacet instances
///
/// ## Purpose
/// Creates LockFacet instances by getting LockManager from ServiceLocator.
/// This ensures facets use the LockManager configured in node config/runtime config.
///
/// ## Usage
/// ```rust,ignore
/// let factory = LockFacetFactory::new(service_locator);
/// facet_registry.register("locks".to_string(), Arc::new(factory));
/// ```
pub struct LockFacetFactory {
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
}

impl LockFacetFactory {
    /// Create a new LockFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get LockManager from
    pub fn new(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Self {
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
        // Create adapter that wraps the real LockManager and converts LockError to String
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
///
/// ## Purpose
/// The facet crate defines its own LockManager trait to avoid circular dependencies.
/// This adapter wraps the real LockManager from locks crate and converts LockError to String.
struct LockManagerAdapter {
    inner: Arc<dyn plexspaces_locks::LockManager + Send + Sync>,
}

#[async_trait]
impl plexspaces_facet::capabilities::locks::LockManager for LockManagerAdapter {
    async fn acquire_lock(&self, ctx: &RequestContext, options: AcquireLockOptions) -> Result<Lock, String> {
        self.inner.acquire_lock(ctx, options).await
            .map_err(|e| e.to_string())
    }
    
    async fn renew_lock(&self, ctx: &RequestContext, options: RenewLockOptions) -> Result<Lock, String> {
        self.inner.renew_lock(ctx, options).await
            .map_err(|e| e.to_string())
    }
    
    async fn release_lock(&self, ctx: &RequestContext, options: ReleaseLockOptions) -> Result<(), String> {
        self.inner.release_lock(ctx, options).await
            .map_err(|e| e.to_string())
    }
    
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> Result<Option<Lock>, String> {
        self.inner.get_lock(ctx, lock_key).await
            .map_err(|e| e.to_string())
    }
}

/// Factory for creating RegistryFacet instances
///
/// ## Purpose
/// Creates RegistryFacet instances by getting ObjectRegistry from ServiceLocator.
/// This ensures facets use the ObjectRegistry configured in node config/runtime config.
pub struct RegistryFacetFactory {
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
}

impl RegistryFacetFactory {
    /// Create a new RegistryFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get ObjectRegistry from
    pub fn new(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for RegistryFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::registry::{RegistryFacet, REGISTRY_FACET_DEFAULT_PRIORITY};
        use plexspaces_proto::object_registry::v1::ObjectRegistration;
        
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
        // Implement ServiceLocatorTrait for ServiceLocator to avoid circular dependency
        let service_locator_for_facet: Arc<dyn plexspaces_facet::capabilities::registry::ServiceLocatorTrait> = 
            Arc::new(ServiceLocatorAdapter {
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
    inner: Arc<dyn plexspaces_core::ServiceLocator>,
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
    inner: Arc<dyn plexspaces_core::ObjectRegistry>,
}

#[async_trait]
impl plexspaces_facet::capabilities::registry::ObjectRegistry for ObjectRegistryAdapter {
    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), String> {
        self.inner.register(ctx, registration).await
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
                "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
                "TupleSpace" | "tuplespace" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
                "Service" | "service" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
                _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            })
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor);
        
        self.inner.unregister(ctx, object_type_enum, object_id).await
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
            "TupleSpace" | "tuplespace" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            "Service" | "service" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });
        
        self.inner.lookup(ctx, object_id, object_type_enum).await
            .map_err(|e| e.to_string())
    }
    
    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<String>,
        name: Option<String>,
        labels: Option<Vec<String>>,
        health_status: Option<String>,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ObjectRegistration>, String> {
        let object_type_enum = object_type.as_ref().map(|s| match s.as_str() {
            "Actor" | "actor" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
            "TupleSpace" | "tuplespace" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeTuplespace,
            "Service" | "service" => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeService,
            _ => plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeActor,
        });
        
        let health_status_enum = health_status.as_ref().map(|s| match s.as_str() {
            "Healthy" | "healthy" => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy,
            "Unhealthy" | "unhealthy" => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnhealthy,
            "Unknown" | "unknown" => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
            _ => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
        });
        
        self.inner.discover(
            ctx,
            object_type_enum,
            name,
            labels,
            None, // exclude_labels
            health_status_enum,
            limit,
            offset,
        ).await
            .map_err(|e| e.to_string())
    }
}

/// Factory for creating ProcessGroupFacet instances
///
/// ## Purpose
/// Creates ProcessGroupFacet instances by getting ProcessGroupRegistry from ServiceLocator.
/// This ensures facets use the ProcessGroupRegistry configured in node config/runtime config.
pub struct ProcessGroupFacetFactory {
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
}

impl ProcessGroupFacetFactory {
    /// Create a new ProcessGroupFacetFactory
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator to get ProcessGroupRegistry from
    pub fn new(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl FacetFactory for ProcessGroupFacetFactory {
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError> {
        use plexspaces_facet::capabilities::process_groups::{ProcessGroupFacet, PROCESS_GROUP_FACET_DEFAULT_PRIORITY};
        
        // Get ProcessGroupRegistry from ServiceLocator
        // ProcessGroupRegistry is registered as ProcessGroupService in ServiceLocator
        use plexspaces_core::ProcessGroupService;
        let process_group_service = self.service_locator.get_process_group_service().await
            .ok_or_else(|| FacetError::InvalidConfig(
                "ProcessGroupService not found in ServiceLocator. Ensure ProcessGroupRegistry is registered during service initialization.".to_string()
            ))?;
        
        // ProcessGroupService trait doesn't expose all methods we need, so we need to get the concrete type
        // For now, we'll create an adapter that uses ProcessGroupService methods
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
            facet_type: "process_groups".to_string(),
            attached_at: std::time::Instant::now(),
            config: serde_json::Value::Null,
            priority: plexspaces_facet::capabilities::process_groups::PROCESS_GROUP_FACET_DEFAULT_PRIORITY,
        }
    }
}

/// Adapter that converts core::ProcessGroupService to facet::ProcessGroupRegistry trait
struct ProcessGroupRegistryAdapter {
    inner: Arc<dyn ProcessGroupService>,
}

#[async_trait]
impl plexspaces_facet::capabilities::process_groups::ProcessGroupRegistry for ProcessGroupRegistryAdapter {
    async fn create_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<(), String> {
        self.inner.create_group(ctx, group_name).await
            .map_err(|e| e.to_string())
    }
    
    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), String> {
        self.inner.join_group(ctx, group_name, actor_id, topics).await
            .map_err(|e| e.to_string())
    }
    
    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), String> {
        self.inner.leave_group(ctx, group_name, actor_id).await
            .map_err(|e| e.to_string())
    }
    
    async fn get_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        self.inner.get_members(ctx, group_name).await
            .map_err(|e| e.to_string())
    }
    
    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, String> {
        self.inner.get_local_members(ctx, group_name).await
            .map_err(|e| e.to_string())
    }
    
    async fn list_groups(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<String>, String> {
        self.inner.list_groups(ctx).await
            .map_err(|e| e.to_string())
    }
    
    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Vec<u8>,
    ) -> Result<Vec<String>, String> {
        // ProcessGroupService::publish_to_group takes Message, not Vec<u8>
        // We need to convert Vec<u8> to Message
        use plexspaces_core::Message;
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: message,
            ..Default::default()
        };
        
        let recipient_count = self.inner.publish_to_group(ctx, group_name, topic, msg).await
            .map_err(|e| e.to_string())?;
        
        // Get members to return as recipients
        let members = self.get_members(ctx, group_name).await?;
        // Return first N members where N = recipient_count
        Ok(members.into_iter().take(recipient_count as usize).collect())
    }
}



