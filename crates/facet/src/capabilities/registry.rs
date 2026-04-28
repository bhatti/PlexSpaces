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

//! Object Registry Capability Facet
//!
//! ## Purpose
//! Provides service discovery capabilities to actors as a runtime-attachable facet.
//! This follows the same pattern as KeyValueFacet and LockFacet - actors send messages
//! with specific message types, and the facet intercepts and handles them using the
//! real ObjectRegistry backend.
//!
//! ## Design Pattern
//! - **Message Interception**: Facet intercepts messages with types like `"register_object"`, `"lookup_object"`, etc.
//! - **Short-Circuit Handling**: Facet handles the operation and returns result without calling the actor
//! - **Works for Rust and WASM**: Both Rust and WASM actors send messages, facet handles them uniformly
//! - **Uses Real Backend**: Wraps ObjectRegistry from ServiceLocator (based on node config)
//!
//! ## Message Types
//! - `"register_object"`: Register an object in the registry
//! - `"unregister_object"`: Unregister an object
//! - `"lookup_object"`: Lookup an object by ID
//! - `"discover_objects"`: Discover objects with filters (`offset`, then `limit`, same as ObjectRegistry)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::{Facet, FacetError, InterceptResult};
use plexspaces_common::RequestContext;
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use tracing::{debug, error, instrument, warn};

/// Trait for ServiceLocator functionality needed by RegistryFacet
/// This avoids circular dependency with plexspaces-core
#[async_trait::async_trait]
pub trait ServiceLocatorTrait: Send + Sync {
    async fn is_auth_disabled(&self) -> bool;
    async fn get_node_config(&self) -> Option<plexspaces_proto::node::v1::NodeConfig>;
}

/// Default priority for RegistryFacet
pub const REGISTRY_FACET_DEFAULT_PRIORITY: i32 = 30;

/// Configuration for registry facet
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Default object type if not specified
    pub default_object_type: Option<String>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        RegistryConfig {
            default_object_type: Some("Actor".to_string()),
        }
    }
}

/// Trait for object registry implementations (to avoid circular dependency with plexspaces-core)
#[async_trait]
pub trait ObjectRegistry: Send + Sync {
    /// Register an object
    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), String>;

    /// Unregister an object
    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<(), String>;

    /// Lookup an object by ID
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<String>,
    ) -> Result<Option<ObjectRegistration>, String>;

    /// Discover objects with filters.
    ///
    /// Pagination matches `plexspaces_object_registry::ObjectRegistry::discover`: **`offset` first, then `limit`**.
    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<String>,
        name: Option<String>,
        labels: Option<Vec<String>>,
        health_status: Option<String>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, String>;
}

/// Object registry capability facet
///
/// ## Purpose
/// Provides service discovery to actors via message interception.
/// Actors send messages with registry operation types, facet handles them using ObjectRegistry.
pub struct RegistryFacet {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    /// Facet priority (immutable)
    priority: i32,
    /// Object registry implementation
    object_registry: Arc<dyn ObjectRegistry>,
    /// Configuration (parsed from config_value)
    config: RegistryConfig,
    /// ServiceLocator for getting NodeConfig defaults and auth_enabled (optional)
    service_locator: Option<Arc<dyn ServiceLocatorTrait>>,
    /// Tenant ID from API request (stored when facet is attached to actor, empty if not set)
    tenant_id: std::sync::Mutex<String>,
    /// Namespace from API request (stored when facet is attached to actor, empty if not set)
    namespace: std::sync::Mutex<String>,
}

impl RegistryFacet {
    /// Create a new registry facet
    ///
    /// ## Arguments
    /// * `object_registry` - Object registry backend (implements ObjectRegistry trait)
    /// * `config` - Facet configuration JSON
    /// * `priority` - Facet priority
    pub fn new(object_registry: Arc<dyn ObjectRegistry>, config: Value, priority: i32) -> Self {
        let config_clone = config.clone();
        let registry_config = serde_json::from_value::<RegistryConfig>(config_clone)
            .unwrap_or_else(|_| RegistryConfig::default());

        RegistryFacet {
            config_value: config,
            priority,
            object_registry,
            config: registry_config,
            service_locator: None,
            tenant_id: std::sync::Mutex::new(String::new()),
            namespace: std::sync::Mutex::new(String::new()),
        }
    }

    /// Create a new registry facet with ServiceLocator
    ///
    /// ## Arguments
    /// * `object_registry` - Object registry backend (implements ObjectRegistry trait)
    /// * `config` - Facet configuration JSON
    /// * `priority` - Facet priority
    /// * `service_locator` - ServiceLocator for getting NodeConfig defaults and auth_enabled
    pub fn new_with_service_locator(
        object_registry: Arc<dyn ObjectRegistry>,
        config: Value,
        priority: i32,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> Self {
        let config_clone = config.clone();
        let registry_config = serde_json::from_value::<RegistryConfig>(config_clone)
            .unwrap_or_else(|_| RegistryConfig::default());

        RegistryFacet {
            config_value: config,
            priority,
            object_registry,
            config: registry_config,
            service_locator: Some(service_locator),
            tenant_id: std::sync::Mutex::new(String::new()),
            namespace: std::sync::Mutex::new(String::new()),
        }
    }

    /// Handle registry operations with observability
    #[instrument(skip(self, args), fields(operation = method))]
    async fn handle_registry_operation(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<Vec<u8>, FacetError> {
        let start = Instant::now();

        // Get tenant_id/namespace - prioritize stored values (from API request), fallback to ServiceLocator defaults
        let (tenant_id, namespace, auth_enabled) = {
            // First, try to get from stored values (set when facet was attached to actor with API tenant_id/namespace)
            let stored_tenant_id = self.tenant_id.lock().unwrap().clone();
            let stored_namespace = self.namespace.lock().unwrap().clone();

            if !stored_tenant_id.is_empty() && !stored_namespace.is_empty() {
                // Use stored values from API request
                let auth_enabled = if let Some(service_locator) = &self.service_locator {
                    !service_locator.is_auth_disabled().await
                } else {
                    false
                };
                debug!(tenant_id = %stored_tenant_id, namespace = %stored_namespace, auth_enabled = auth_enabled, "RegistryFacet: Using tenant_id/namespace from API request (stored when facet attached)");
                (stored_tenant_id, stored_namespace, auth_enabled)
            } else if let Some(service_locator) = &self.service_locator {
                // Fallback to empty strings - tenant/namespace must come from API request (auth)
                // NOTE: default_tenant_id and default_namespace have been removed from NodeConfig
                let auth_enabled = !service_locator.is_auth_disabled().await;
                debug!(
                    tenant_id = "",
                    namespace = "",
                    auth_enabled = auth_enabled,
                    "RegistryFacet: Using empty tenant_id/namespace (must come from API request)"
                );
                (String::new(), String::new(), auth_enabled)
            } else {
                // Final fallback - use empty values
                debug!(
                    tenant_id = "",
                    namespace = "",
                    auth_enabled = false,
                    "RegistryFacet: Using empty tenant_id/namespace (ServiceLocator not available)"
                );
                (String::new(), String::new(), false)
            }
        };

        // Create request context - validation will only check tenant_id if auth is enabled
        let ctx = RequestContext::new(tenant_id.clone(), namespace.clone(), auth_enabled)
            .map_err(|e| {
                error!(tenant_id = %tenant_id, namespace = %namespace, auth_enabled = auth_enabled, error = %e, "RegistryFacet: Failed to create RequestContext");
                FacetError::InvalidConfig(format!("Failed to create RequestContext: {}", e))
            })?;

        debug!(tenant_id = %tenant_id, namespace = %namespace, auth_enabled = auth_enabled, "RegistryFacet: Created RequestContext for registry operation");

        let result = match method {
            "register_object" => {
                metrics::counter!("plexspaces_facet_registry_operations_total", "operation" => "register_object").increment(1);
                #[derive(Deserialize)]
                struct RegisterArgs {
                    object_id: String,
                    object_type: Option<String>,
                    object_category: Option<String>,
                    grpc_address: String,
                    metadata: Option<std::collections::HashMap<String, String>>,
                    capabilities: Option<Vec<String>>,
                    labels: Option<Vec<String>>,
                    health_status: Option<String>,
                }

                let args: RegisterArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let object_type = args
                    .object_type
                    .or_else(|| self.config.default_object_type.clone())
                    .unwrap_or_else(|| "Actor".to_string());

                // Convert string to ObjectType enum
                let object_type_enum = match object_type.as_str() {
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
                };

                let health_status_enum = args.health_status.as_ref().map(|s| match s.as_str() {
                    "Healthy" | "healthy" => {
                        plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy
                    }
                    "Unhealthy" | "unhealthy" => {
                        plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnhealthy
                    }
                    "Unknown" | "unknown" => {
                        plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown
                    }
                    _ => plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown,
                });

                // Convert metadata HashMap to Metadata proto struct
                let metadata_proto = if let Some(meta_map) = args.metadata {
                    Some(plexspaces_proto::common::v1::Metadata {
                        labels: meta_map,
                        ..Default::default()
                    })
                } else {
                    None
                };

                let registration = ObjectRegistration {
                    object_id: args.object_id.clone(),
                    object_name: String::new(), // Optional, can be empty
                    object_type: object_type_enum as i32,
                    version: String::new(), // Optional, can be empty
                    tenant_id: ctx.tenant_id().to_string(),
                    namespace: ctx.namespace().to_string(),
                    node_id: String::new(), // Will be set by registry
                    grpc_address: args.grpc_address.clone(),
                    object_category: args.object_category.unwrap_or_default(),
                    capabilities: args.capabilities.unwrap_or_default(),
                    metadata: metadata_proto,
                    health_status: health_status_enum.map(|s| s as i32).unwrap_or(0),
                    labels: args.labels.unwrap_or_default(),
                    metrics: std::collections::HashMap::new(),
                    last_heartbeat: None,
                    created_at: None,
                    updated_at: None,
                };

                match self.object_registry.register(&ctx, registration).await {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "register_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_operations_success_total", "operation" => "register_object").increment(1);
                        debug!(object_id = %args.object_id, duration_ms = duration.as_millis(), "Object registered");

                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "register_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_errors_total", "operation" => "register_object", "error" => "registration_failed").increment(1);
                        error!(object_id = %args.object_id, error = %e, duration_ms = duration.as_millis(), "Failed to register object");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "unregister_object" => {
                metrics::counter!("plexspaces_facet_registry_operations_total", "operation" => "unregister_object").increment(1);
                #[derive(Deserialize)]
                struct UnregisterArgs {
                    object_id: String,
                    object_type: Option<String>,
                }

                let args: UnregisterArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                match self
                    .object_registry
                    .unregister(&ctx, &args.object_id, args.object_type)
                    .await
                {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "unregister_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_operations_success_total", "operation" => "unregister_object").increment(1);
                        debug!(object_id = %args.object_id, duration_ms = duration.as_millis(), "Object unregistered");

                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "unregister_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_errors_total", "operation" => "unregister_object", "error" => "unregistration_failed").increment(1);
                        error!(object_id = %args.object_id, error = %e, duration_ms = duration.as_millis(), "Failed to unregister object");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "lookup_object" => {
                metrics::counter!("plexspaces_facet_registry_operations_total", "operation" => "lookup_object").increment(1);
                #[derive(Deserialize)]
                struct LookupArgs {
                    object_id: String,
                    object_type: Option<String>,
                }

                let args: LookupArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                match self
                    .object_registry
                    .lookup(&ctx, &args.object_id, args.object_type)
                    .await
                {
                    Ok(Some(registration)) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "lookup_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_operations_success_total", "operation" => "lookup_object").increment(1);
                        debug!(object_id = %args.object_id, duration_ms = duration.as_millis(), "Object found");

                        let reg_json = object_registration_to_json(&registration);
                        serde_json::to_vec(&reg_json)
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Ok(None) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "lookup_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_operations_success_total", "operation" => "lookup_object", "result" => "not_found").increment(1);
                        debug!(object_id = %args.object_id, duration_ms = duration.as_millis(), "Object not found");

                        serde_json::to_vec(&json!({"found": false}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "lookup_object").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_errors_total", "operation" => "lookup_object", "error" => "lookup_failed").increment(1);
                        error!(object_id = %args.object_id, error = %e, duration_ms = duration.as_millis(), "Failed to lookup object");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            "discover_objects" => {
                metrics::counter!("plexspaces_facet_registry_operations_total", "operation" => "discover_objects").increment(1);
                #[derive(Deserialize)]
                struct DiscoverArgs {
                    object_type: Option<String>,
                    name: Option<String>,
                    labels: Option<Vec<String>>,
                    health_status: Option<String>,
                    offset: Option<usize>,
                    limit: Option<usize>,
                }

                let args: DiscoverArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let offset = args.offset.unwrap_or(0);
                let limit = args.limit.unwrap_or(100);

                match self
                    .object_registry
                    .discover(
                        &ctx,
                        args.object_type,
                        args.name,
                        args.labels,
                        args.health_status,
                        offset,
                        limit,
                    )
                    .await
                {
                    Ok(registrations) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "discover_objects").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_operations_success_total", "operation" => "discover_objects").increment(1);
                        metrics::gauge!("plexspaces_facet_registry_discovered_objects_count")
                            .set(registrations.len() as f64);
                        debug!(
                            count = registrations.len(),
                            duration_ms = duration.as_millis(),
                            "Objects discovered"
                        );

                        let regs_json: Vec<Value> = registrations
                            .iter()
                            .map(object_registration_to_json)
                            .collect();
                        serde_json::to_vec(&json!({"objects": regs_json}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_registry_operation_duration_seconds", "operation" => "discover_objects").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_registry_errors_total", "operation" => "discover_objects", "error" => "discovery_failed").increment(1);
                        error!(error = %e, duration_ms = duration.as_millis(), "Failed to discover objects");
                        Err(FacetError::InterceptionFailed(e.to_string()))
                    }
                }
            }
            _ => {
                warn!(method = %method, "Unknown registry operation method");
                Ok(vec![])
            }
        };

        result
    }
}

/// Convert ObjectRegistration proto struct to JSON Value
fn object_registration_to_json(reg: &ObjectRegistration) -> Value {
    json!({
        "object_id": reg.object_id,
        "object_name": reg.object_name,
        "object_type": reg.object_type,
        "version": reg.version,
        "tenant_id": reg.tenant_id,
        "namespace": reg.namespace,
        "node_id": reg.node_id,
        "grpc_address": reg.grpc_address,
        "object_category": reg.object_category,
        "capabilities": reg.capabilities,
        "metadata": reg.metadata.as_ref().map(|m| &m.labels),
        "health_status": reg.health_status,
        "labels": reg.labels,
    })
}

#[async_trait]
impl Facet for RegistryFacet {
    fn facet_type(&self) -> &str {
        "registry"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_registry_attached_total").increment(1);

        // Extract tenant_id/namespace from config if available (passed from actor context)
        // These come from the API request (HTTP/gRPC), not ServiceLocator defaults
        if let Some(config_obj) = config.as_object() {
            if let Some(tenant_id_val) = config_obj.get("_tenant_id") {
                if let Some(tenant_id) = tenant_id_val.as_str() {
                    *self.tenant_id.lock().unwrap() = tenant_id.to_string();
                    debug!(actor_id = %actor_id, tenant_id = %tenant_id, "RegistryFacet: Stored tenant_id from API request");
                }
            }
            if let Some(namespace_val) = config_obj.get("_namespace") {
                if let Some(namespace) = namespace_val.as_str() {
                    *self.namespace.lock().unwrap() = namespace.to_string();
                    debug!(actor_id = %actor_id, namespace = %namespace, "RegistryFacet: Stored namespace from API request");
                }
            }
        }

        debug!(actor_id = %actor_id, "Registry capability attached to actor");
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_registry_detached_total").increment(1);
        debug!(actor_id = %actor_id, "Registry capability detached from actor");
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        // Intercept registry operations
        if method == "register_object"
            || method == "unregister_object"
            || method == "lookup_object"
            || method == "discover_objects"
        {
            let result = self.handle_registry_operation(method, args).await?;
            return Ok(InterceptResult::ShortCircuit(result));
        }
        Ok(InterceptResult::Continue)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(serde_json::json!({}))
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // In-memory object registry for testing
    struct TestObjectRegistry {
        objects: Arc<tokio::sync::RwLock<std::collections::HashMap<String, ObjectRegistration>>>,
    }

    impl TestObjectRegistry {
        fn new() -> Self {
            Self {
                objects: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl ObjectRegistry for TestObjectRegistry {
        async fn register(
            &self,
            _ctx: &RequestContext,
            registration: ObjectRegistration,
        ) -> Result<(), String> {
            let mut objects = self.objects.write().await;
            objects.insert(registration.object_id.clone(), registration);
            Ok(())
        }

        async fn unregister(
            &self,
            _ctx: &RequestContext,
            object_id: &str,
            _object_type: Option<String>,
        ) -> Result<(), String> {
            let mut objects = self.objects.write().await;
            objects.remove(object_id);
            Ok(())
        }

        async fn lookup(
            &self,
            _ctx: &RequestContext,
            object_id: &str,
            _object_type: Option<String>,
        ) -> Result<Option<ObjectRegistration>, String> {
            let objects = self.objects.read().await;
            Ok(objects.get(object_id).cloned())
        }

        async fn discover(
            &self,
            _ctx: &RequestContext,
            _object_type: Option<String>,
            _name: Option<String>,
            _labels: Option<Vec<String>>,
            _health_status: Option<String>,
            offset: usize,
            limit: usize,
        ) -> Result<Vec<ObjectRegistration>, String> {
            let objects = self.objects.read().await;
            let mut v: Vec<ObjectRegistration> = objects.values().cloned().collect();
            v.sort_by(|a, b| a.object_id.cmp(&b.object_id));
            Ok(v.into_iter().skip(offset).take(limit).collect())
        }
    }

    #[tokio::test]
    async fn test_registry_facet_register_lookup() {
        // ARRANGE
        let registry = Arc::new(TestObjectRegistry::new());
        let mut facet = RegistryFacet::new(registry, serde_json::json!({}), 50);

        // Attach to actor
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Register object
        let register_args = serde_json::json!({
            "object_id": "service-1",
            "object_type": "Service",
            "grpc_address": "http://service-1:50051",
            "metadata": {"version": "1.0.0"}
        });

        let result = facet
            .before_method(
                "register_object",
                serde_json::to_vec(&register_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should short-circuit with success
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Lookup object
        let lookup_args = serde_json::json!({
            "object_id": "service-1",
            "object_type": "Service"
        });

        let result = facet
            .before_method(
                "lookup_object",
                serde_json::to_vec(&lookup_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return object
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["object_id"], "service-1");
                assert_eq!(response["grpc_address"], "http://service-1:50051");
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_registry_facet_unregister() {
        // ARRANGE
        let registry = Arc::new(TestObjectRegistry::new());
        let mut facet = RegistryFacet::new(registry, serde_json::json!({}), 50);

        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Register then unregister
        let register_args = serde_json::json!({
            "object_id": "service-2",
            "object_type": "Service",
            "grpc_address": "http://service-2:50051"
        });

        facet
            .before_method(
                "register_object",
                serde_json::to_vec(&register_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        let unregister_args = serde_json::json!({
            "object_id": "service-2",
            "object_type": "Service"
        });

        let result = facet
            .before_method(
                "unregister_object",
                serde_json::to_vec(&unregister_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return success
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Lookup should return not found
        let lookup_args = serde_json::json!({
            "object_id": "service-2",
            "object_type": "Service"
        });

        let result = facet
            .before_method(
                "lookup_object",
                serde_json::to_vec(&lookup_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return not found
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["found"], false);
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_registry_facet_discover() {
        // ARRANGE
        let registry = Arc::new(TestObjectRegistry::new());
        let mut facet = RegistryFacet::new(registry, serde_json::json!({}), 50);

        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Register multiple objects
        for i in 1..=3 {
            let register_args = serde_json::json!({
                "object_id": format!("service-{}", i),
                "object_type": "Service",
                "grpc_address": format!("http://service-{}:50051", i)
            });

            facet
                .before_method(
                    "register_object",
                    serde_json::to_vec(&register_args).unwrap().as_slice(),
                    &std::collections::HashMap::new(),
                )
                .await
                .unwrap();
        }

        // ACT: Discover all objects
        let discover_args = serde_json::json!({
            "object_type": "Service",
            "offset": 0,
            "limit": 10
        });

        let result = facet
            .before_method(
                "discover_objects",
                serde_json::to_vec(&discover_args).unwrap().as_slice(),
                &std::collections::HashMap::new(),
            )
            .await
            .unwrap();

        // ASSERT: Should return all objects
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                let objects = response["objects"].as_array().unwrap();
                assert_eq!(objects.len(), 3);
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_facet_type() {
        let registry = Arc::new(TestObjectRegistry::new());
        let facet = RegistryFacet::new(registry, serde_json::json!({}), 50);
        assert_eq!(facet.facet_type(), "registry");
    }
}
