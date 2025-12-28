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

//! # PlexSpaces Unified Object Registry
//!
//! ## Purpose
//! Provides unified registration and discovery for all distributed objects in PlexSpaces:
//! - **Actors**: Stateful computation units (actor model)
//! - **TupleSpaces**: Coordination primitives (Linda model)
//! - **Services**: Microservices and gRPC endpoints
//!
//! ## Architecture Context
//! This crate consolidates three separate registries (ActorRegistry, TupleSpaceRegistry,
//! ServiceRegistry) into ONE unified registry following Proto-First Design principles.
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │                 ObjectRegistry                           │
//! │  register() / unregister() / lookup() / discover()      │
//! └────────────────────┬────────────────────────────────────┘
//!                      │
//!                      ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │              KeyValueStore Backend                       │
//! │  (InMemory, SQLite, Redis, PostgreSQL)                  │
//! │  Key: {tenant}:{namespace}:{type}:{object_id}           │
//! │  Value: ObjectRegistration (proto serialized)           │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`ObjectRegistry`]: Main registry struct with KeyValueStore backend
//! - [`ObjectRegistryError`]: Error types for registry operations
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_proto`]: Protocol buffer definitions (object_registry.proto)
//! - [`plexspaces_keyvalue`]: Key-value storage backend
//!
//! ## Dependents
//! This crate is used by:
//! - Node (for actor/service discovery)
//! - TupleSpace (for distributed coordination)
//! - Service mesh (for load balancing)
//!
//! ## Examples
//!
//! ### Basic Usage - Register Actor
//! ```rust,no_run
//! use plexspaces_object_registry::ObjectRegistry;
//! use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
//! use plexspaces_keyvalue::InMemoryKVStore;
//! use plexspaces_core::RequestContext;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kv = Arc::new(InMemoryKVStore::new());
//! let registry = ObjectRegistry::new(kv);
//!
//! // Create RequestContext for tenant isolation
//! let ctx = RequestContext::new_without_auth("default".to_string(), "production".to_string());
//!
//! // Register actor
//! let registration = ObjectRegistration {
//!     object_id: "counter@node1".to_string(),
//!     object_type: ObjectType::ObjectTypeActor as i32,
//!     object_category: "GenServer".to_string(),
//!     grpc_address: "http://node1:8000".to_string(),
//!     ..Default::default()
//! };
//!
//! registry.register(&ctx, registration).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Discover Objects by Type
//! ```rust,no_run
//! # use plexspaces_object_registry::ObjectRegistry;
//! # use plexspaces_proto::object_registry::v1::ObjectType;
//! # use plexspaces_keyvalue::InMemoryKVStore;
//! # use plexspaces_core::RequestContext;
//! # use std::sync::Arc;
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let kv = Arc::new(InMemoryKVStore::new());
//! # let registry = ObjectRegistry::new(kv);
//! # let ctx = RequestContext::new_without_auth("default".to_string(), "default".to_string());
//! // Discover all actors
//! let actors = registry.discover(&ctx, Some(ObjectType::ObjectTypeActor), None, None, None, None, 0, 100).await?;
//! println!("Found {} actors", actors.len());
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All data models defined in `proto/plexspaces/v1/object_registry.proto`
//!
//! ### Static vs Dynamic
//! - Static: Core registration/discovery logic (always present)
//! - Dynamic: Filtering/pagination strategies (extensible)
//!
//! ### Test-Driven
//! - Unit tests in this file (#[cfg(test)] mod tests)
//! - Integration tests in tests/ directory
//! - Target coverage: 90%+
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-object-registry
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-object-registry
//! ```
//!
//! ## Performance Characteristics
//! - Register: O(1) - single KeyValueStore write
//! - Lookup: O(1) - single KeyValueStore read
//! - Discover: O(n) - scan + filter (can use prefix for type filtering)
//! - Heartbeat: O(1) - single KeyValueStore update

#![warn(missing_docs)]
#![warn(clippy::all)]

use plexspaces_core::RequestContext;
use plexspaces_keyvalue::{KVError, KeyValueStore};
use plexspaces_proto::object_registry::v1::{
    HealthStatus, ObjectRegistration, ObjectType,
};
use prost::Message; // For encode_to_vec() and decode()
use std::sync::Arc;
use async_trait::async_trait;

/// Error types for ObjectRegistry operations
#[derive(Debug, thiserror::Error)]
pub enum ObjectRegistryError {
    /// Object not found
    #[error("Object not found: {0}")]
    ObjectNotFound(String),

    /// Object already registered
    #[error("Object already registered: {0}")]
    ObjectAlreadyRegistered(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Invalid input
    #[error("Invalid input: {0}")]
    InvalidInput(String),
}

impl From<KVError> for ObjectRegistryError {
    fn from(err: KVError) -> Self {
        ObjectRegistryError::StorageError(err.to_string())
    }
}

/// Unified ObjectRegistry for actors, tuplespaces, and services
///
/// ## Purpose
/// Provides centralized registration and discovery for all distributed objects
/// in PlexSpaces using a KeyValueStore backend.
///
/// ## Design
/// - Uses KeyValueStore for persistence (InMemory, SQLite, Redis, PostgreSQL)
/// - Key format: `{tenant_id}:{namespace}:{object_type}:{object_id}`
/// - Value: ObjectRegistration (protobuf serialized bytes)
/// - No external dependencies beyond KeyValueStore
///
/// ## Examples
/// ```rust,no_run
/// # use plexspaces_object_registry::ObjectRegistry;
/// # use plexspaces_keyvalue::InMemoryKVStore;
/// # use std::sync::Arc;
/// let kv = Arc::new(InMemoryKVStore::new());
/// let registry = ObjectRegistry::new(kv);
/// ```
pub struct ObjectRegistry {
    kv_store: Arc<dyn KeyValueStore>,
}

impl ObjectRegistry {
    /// Create new ObjectRegistry with given KeyValueStore backend
    ///
    /// ## Arguments
    /// * `kv_store` - KeyValueStore implementation (InMemory, SQLite, Redis, PostgreSQL)
    ///
    /// ## Returns
    /// New ObjectRegistry instance
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_object_registry::ObjectRegistry;
    /// # use plexspaces_keyvalue::InMemoryKVStore;
    /// # use std::sync::Arc;
    /// let kv = Arc::new(InMemoryKVStore::new());
    /// let registry = ObjectRegistry::new(kv);
    /// ```
    pub fn new(kv_store: Arc<dyn KeyValueStore>) -> Self {
        Self { kv_store }
    }

    /// Generate KeyValueStore key for object
    ///
    /// ## Key Format
    /// `{tenant_id}:{namespace}:{object_type}:{object_id}`
    ///
    /// Examples:
    /// - `default:production:actor:counter@node1`
    /// - `acme:staging:tuplespace:ts-redis-acme-staging`
    /// - `default:default:service:order-svc-instance-1`
    ///
    /// ## Note
    /// Empty tenant_id or namespace are defaulted to "default" to avoid double slashes in paths
    fn make_key(
        tenant_id: &str,
        namespace: &str,
        object_type: ObjectType,
        object_id: &str,
    ) -> String {
        let type_str = match object_type {
            ObjectType::ObjectTypeActor => "actor",
            ObjectType::ObjectTypeTuplespace => "tuplespace",
            ObjectType::ObjectTypeService => "service",
            ObjectType::ObjectTypeVm => "vm",
            ObjectType::ObjectTypeApplication => "application",
            ObjectType::ObjectTypeWorkflow => "workflow",
            ObjectType::ObjectTypeNode => "node",
            _ => "unknown",
        };
        // Default to "default" for empty tenant/namespace to avoid double slashes in paths
        let tenant = if tenant_id.is_empty() { "default" } else { tenant_id };
        let ns = if namespace.is_empty() { "default" } else { namespace };
        format!("{}:{}:{}:{}", tenant, ns, type_str, object_id)
    }

    /// Register object (actor, tuplespace, or service)
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (tenant_id comes from here)
    /// * `registration` - ObjectRegistration with object details (namespace can be empty, will use ctx.namespace)
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::InvalidInput`]: Missing required fields
    /// - [`ObjectRegistryError::ObjectAlreadyRegistered`]: Object already exists
    /// - [`ObjectRegistryError::StorageError`]: KeyValueStore failure
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_object_registry::ObjectRegistry;
    /// # use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    /// # use plexspaces_keyvalue::InMemoryKVStore;
    /// # use plexspaces_core::RequestContext;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let kv = Arc::new(InMemoryKVStore::new());
    /// # let registry = ObjectRegistry::new(kv);
    /// let ctx = RequestContext::new_without_auth("default".to_string(), "production".to_string());
    /// let registration = ObjectRegistration {
    ///     object_id: "counter@node1".to_string(),
    ///     object_type: ObjectType::ObjectTypeActor as i32,
    ///     grpc_address: "http://node1:8000".to_string(),
    ///     ..Default::default()
    /// };
    /// registry.register(&ctx, registration).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn register(
        &self,
        ctx: &RequestContext,
        mut registration: ObjectRegistration,
    ) -> Result<(), ObjectRegistryError> {
        // Validation
        if registration.object_id.is_empty() {
            return Err(ObjectRegistryError::InvalidInput(
                "object_id is required".to_string(),
            ));
        }
        if registration.grpc_address.is_empty() {
            return Err(ObjectRegistryError::InvalidInput(
                "grpc_address is required".to_string(),
            ));
        }

        // Get tenant_id and namespace from RequestContext (not from registration)
        let tenant_id = ctx.tenant_id();
        let namespace = ctx.namespace();

        // Verify that if registration has tenant_id/namespace set, they match the context
        if !registration.tenant_id.is_empty() && registration.tenant_id != tenant_id {
            return Err(ObjectRegistryError::InvalidInput(format!(
                "registration.tenant_id '{}' does not match RequestContext tenant_id '{}'",
                registration.tenant_id, tenant_id
            )));
        }
        if !registration.namespace.is_empty() && registration.namespace != namespace {
            return Err(ObjectRegistryError::InvalidInput(format!(
                "registration.namespace '{}' does not match RequestContext namespace '{}'",
                registration.namespace, namespace
            )));
        }

        // Update registration with tenant_id and namespace from context
        registration.tenant_id = tenant_id.to_string();
        registration.namespace = namespace.to_string();

        // Set timestamps
        let now = chrono::Utc::now();
        registration.created_at = Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        });
        registration.updated_at = registration.created_at.clone();

        // Generate key (make_key will default empty tenant/namespace to "default" for paths)
        let object_type = ObjectType::try_from(registration.object_type)
            .unwrap_or(ObjectType::ObjectTypeUnspecified);
        let full_key = Self::make_key(
            tenant_id,
            namespace,
            object_type,
            &registration.object_id,
        );
        
        // Extract just the key part (without tenant:namespace) since put() will add it via composite_key
        // make_key returns "{tenant}:{namespace}:{type}:{id}", but put expects just "{type}:{id}"
        let key_prefix = format!("{}:{}:", tenant_id, namespace);
        let key = if full_key.starts_with(&key_prefix) {
            full_key.strip_prefix(&key_prefix).unwrap_or(&full_key).to_string()
        } else {
            full_key
        };

        // Check if already registered (optional - remove if overwrite is desired)
        if self.kv_store.get(ctx, &key).await?.is_some() {
            return Err(ObjectRegistryError::ObjectAlreadyRegistered(
                registration.object_id.clone(),
            ));
        }

        // Serialize and store
        let value = registration.encode_to_vec();

        self.kv_store.put(ctx, &key, value).await?;

        Ok(())
    }

    /// Unregister object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (tenant_id comes from here)
    /// * `object_type` - Type of object (Actor, TupleSpace, Service)
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::ObjectNotFound`]: Object doesn't exist
    /// - [`ObjectRegistryError::StorageError`]: KeyValueStore failure
    pub async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<(), ObjectRegistryError> {
        // Get tenant_id and namespace from RequestContext
        let full_key = Self::make_key(
            ctx.tenant_id(),
            ctx.namespace(),
            object_type,
            object_id,
        );
        
        // Extract just the key part (without tenant:namespace) since get()/delete() will add it via composite_key
        let key_prefix = format!("{}:{}:", ctx.tenant_id(), ctx.namespace());
        let key = if full_key.starts_with(&key_prefix) {
            full_key.strip_prefix(&key_prefix).unwrap_or(&full_key).to_string()
        } else {
            full_key
        };

        // Check if exists
        if self.kv_store.get(ctx, &key).await?.is_none() {
            return Err(ObjectRegistryError::ObjectNotFound(object_id.to_string()));
        }

        self.kv_store.delete(ctx, &key).await?;

        Ok(())
    }

    /// Lookup specific object by ID
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant identifier
    /// * `namespace` - Namespace
    /// * `object_type` - Type of object
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(Some(ObjectRegistration))` if found, `Ok(None)` if not found
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::SerializationError`]: Failed to deserialize
    /// - [`ObjectRegistryError::StorageError`]: KeyValueStore failure
    pub async fn lookup(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, ObjectRegistryError> {
        let full_key = Self::make_key(ctx.tenant_id(), ctx.namespace(), object_type, object_id);
        
        // Extract just the key part (without tenant:namespace) since get() will add it via composite_key
        let key_prefix = format!("{}:{}:", ctx.tenant_id(), ctx.namespace());
        let key = if full_key.starts_with(&key_prefix) {
            full_key.strip_prefix(&key_prefix).unwrap_or(&full_key).to_string()
        } else {
            full_key
        };

        match self.kv_store.get(ctx, &key).await? {
            Some(value) => {
                let registration = ObjectRegistration::decode(&value[..])
                    .map_err(|e| ObjectRegistryError::SerializationError(e.to_string()))?;
                // If not admin, verify tenant matches
                if !ctx.is_admin() && registration.tenant_id != ctx.tenant_id() {
                    return Ok(None); // Tenant mismatch - return None
                }
                Ok(Some(registration))
            }
            None => Ok(None),
        }
    }

    /// Lookup object by ID (full signature matching ObjectRegistry trait)
    ///
    /// This wraps `lookup()` to match the ObjectRegistry trait signature.
    /// Use this when implementing the ObjectRegistry trait.
    pub async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.lookup(ctx, object_type, object_id)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    /// Register an object (trait-compatible signature)
    ///
    /// This wraps `register()` to match the ObjectRegistry trait signature.
    pub async fn register_trait(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.register(ctx, registration)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    }

    /// Discover objects with filtering
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `object_type` - Filter by type (None = all types)
    /// * `object_category` - Filter by category (None = all categories)
    /// * `capabilities` - Filter by capabilities (None = all)
    /// * `labels` - Filter by labels (None = all)
    /// * `health_status` - Filter by health status (None = all)
    /// * `limit` - Maximum results to return
    ///
    /// ## Returns
    /// List of matching ObjectRegistrations
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::StorageError`]: KeyValueStore failure
    ///
    /// ## Performance
    /// O(n) scan with filtering - use prefixes for type filtering
    ///
    /// ## Note
    /// If ctx.is_admin() is true, tenant filtering is bypassed for admin operations.
    pub async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, ObjectRegistryError> {
        // Build prefix for type filtering
        // Key format in KV store: {tenant_id}:{namespace}:{object_type}:{object_id}
        // But list() expects just the key part (without tenant:namespace) since it adds them via composite_key
        // So prefix should be just: {type}: (or empty for all types)
        let prefix = match object_type {
            Some(obj_type) => {
                let type_str = match obj_type {
                    ObjectType::ObjectTypeActor => "actor",
                    ObjectType::ObjectTypeTuplespace => "tuplespace",
                    ObjectType::ObjectTypeService => "service",
                    ObjectType::ObjectTypeVm => "vm",
                    ObjectType::ObjectTypeApplication => "application",
                    ObjectType::ObjectTypeWorkflow => "workflow",
                    ObjectType::ObjectTypeNode => "node",
                    _ => "",
                };
                if type_str.is_empty() {
                    String::new() // Empty prefix = all types
                } else {
                    format!("{}:", type_str) // Just type: prefix
                }
            }
            None => String::new(), // Empty prefix = all types
        };

        // List KeyValueStore with prefix (scoped to tenant/namespace via context)
        // list() will add tenant:namespace via composite_key internally
        let keys = self.kv_store.list(&ctx, &prefix).await?;

        let mut results = Vec::new();
        let mut skipped = 0;

        for key in keys {
            // Skip items before offset
            if skipped < offset {
                skipped += 1;
                continue;
            }
            
            // Stop if we've reached the limit
            if results.len() >= limit {
                break;
            }

            if let Some(value) = self.kv_store.get(&ctx, &key).await? {
                    if let Ok(registration) = ObjectRegistration::decode(&value[..]) {
                        // If not admin, verify tenant matches
                        if !ctx.is_admin() && registration.tenant_id != ctx.tenant_id() {
                            continue; // Tenant mismatch - skip
                        }

                        // Apply filters
                        if let Some(ref cat) = object_category {
                            if registration.object_category != *cat {
                                continue;
                            }
                        }

                        if let Some(ref caps) = capabilities {
                            if !caps
                                .iter()
                                .all(|c| registration.capabilities.contains(c))
                            {
                                continue;
                            }
                        }

                        if let Some(ref lbls) = labels {
                            if !lbls.iter().all(|l| registration.labels.contains(l)) {
                                continue;
                            }
                        }

                        if let Some(ref status) = health_status {
                            // HealthStatus enum can be cast to i32 directly
                            let status_value = match status {
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnknown => 0,
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusHealthy => 1,
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusDegraded => 2,
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusUnhealthy => 3,
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusStarting => 4,
                                plexspaces_proto::object_registry::v1::HealthStatus::HealthStatusStopping => 5,
                            };
                            if registration.health_status != status_value {
                                continue;
                            }
                        }

                        results.push(registration);
                    }
            }
        }

        Ok(results)
    }

    /// Update heartbeat for object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `object_type` - Type of object
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::ObjectNotFound`]: Object doesn't exist
    /// - [`ObjectRegistryError::StorageError`]: KeyValueStore failure
    pub async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        object_id: &str,
    ) -> Result<(), ObjectRegistryError> {
        let full_key = Self::make_key(ctx.tenant_id(), ctx.namespace(), object_type, object_id);
        
        // Extract just the key part (without tenant:namespace) since get() will add it via composite_key
        let key_prefix = format!("{}:{}:", ctx.tenant_id(), ctx.namespace());
        let key = if full_key.starts_with(&key_prefix) {
            full_key.strip_prefix(&key_prefix).unwrap_or(&full_key).to_string()
        } else {
            full_key
        };

        // Use the provided RequestContext (no need to recreate)

        // Get existing registration
        let value = self
            .kv_store
            .get(&ctx, &key)
            .await?
            .ok_or_else(|| ObjectRegistryError::ObjectNotFound(object_id.to_string()))?;

        let mut registration = ObjectRegistration::decode(&value[..])
            .map_err(|e| ObjectRegistryError::SerializationError(e.to_string()))?;

        // Update heartbeat timestamp
        let now = chrono::Utc::now();
        registration.last_heartbeat = Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        });
        registration.updated_at = Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        });

        // Re-serialize and store
        let updated_value = registration.encode_to_vec();

        // Use the provided RequestContext (no need to recreate)
        self.kv_store.put(ctx, &key, updated_value).await?;

        Ok(())
    }
}

// ObjectRegistry now implements the trait directly - no wrapper needed!
impl plexspaces_core::Service for ObjectRegistry {}

#[async_trait::async_trait]
impl plexspaces_core::actor_context::ObjectRegistry for ObjectRegistry {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<plexspaces_core::actor_context::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.lookup(ctx, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<plexspaces_core::actor_context::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.lookup_full(ctx, object_type, object_id)
            .await
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_core::actor_context::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // ObjectRegistration in core is a type alias to proto::ObjectRegistration, so no conversion needed
        self.register_trait(ctx, registration)
            .await
    }

    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<plexspaces_core::actor_context::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.discover(ctx, object_type, object_category, capabilities, labels, health_status, offset, limit)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_keyvalue::InMemoryKVStore;

    fn create_test_registration(object_id: &str, object_type: ObjectType) -> ObjectRegistration {
        ObjectRegistration {
            object_id: object_id.to_string(),
            object_type: object_type as i32,
            grpc_address: format!("http://test-node:8000"),
            // tenant_id and namespace are ignored - they come from RequestContext
            object_category: "GenServer".to_string(),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_register_and_lookup() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = ObjectRegistry::new(kv);

        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
        let registration = create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration.clone()).await.unwrap();

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap();

        assert!(found.is_some());
        let found_reg = found.unwrap();
        assert_eq!(found_reg.object_id, "test-actor@node1");
        assert_eq!(found_reg.grpc_address, "http://test-node:8000");
    }

    #[tokio::test]
    async fn test_register_duplicate_fails() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = ObjectRegistry::new(kv);

        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
        let registration = create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration.clone()).await.unwrap();

        // Try to register again - should fail
        let result = registry.register(&ctx, registration).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ObjectRegistryError::ObjectAlreadyRegistered(_)
        ));
    }

    #[tokio::test]
    async fn test_unregister() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = ObjectRegistry::new(kv);

        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
        let registration = create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration).await.unwrap();

        registry
            .unregister(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap();

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_heartbeat() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = ObjectRegistry::new(kv);

        let ctx = RequestContext::new_without_auth("test-tenant".to_string(), "test-namespace".to_string());
        let registration = create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration).await.unwrap();

        // Wait a bit
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Update heartbeat
        registry
            .heartbeat(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap();

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap()
            .unwrap();

        assert!(found.last_heartbeat.is_some());
    }

    #[tokio::test]
    async fn test_discover_by_type() {
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = ObjectRegistry::new(kv);

        // Note: discover() uses default:default prefix, so we need to register with default tenant/namespace
        let ctx = RequestContext::new_without_auth("default".to_string(), "default".to_string());
        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg1).await.unwrap();

        let reg2 = create_test_registration("actor2@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg2).await.unwrap();

        let reg3 = create_test_registration("ts1", ObjectType::ObjectTypeTuplespace);
        registry.register(&ctx, reg3).await.unwrap();

        let actors = registry
            .discover(&ctx, Some(ObjectType::ObjectTypeActor), None, None, None, None, 0, 100)
            .await
            .unwrap();

        assert_eq!(actors.len(), 2);
    }
}
