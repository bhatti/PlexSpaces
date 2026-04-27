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

//! # PlexSpaces Unified Object Registry
//!
//! ## Purpose
//! Provides unified registration and discovery for all distributed objects in PlexSpaces:
//! - **Actors**: Stateful computation units (actor model)
//! - **TupleSpaces**: Coordination primitives (Linda model)
//! - **Services**: Microservices and gRPC endpoints
//! - **Nodes**: PlexSpaces node instances
//! - **Workflows**: Durable workflow definitions
//! - **Applications**: Deployed applications
//!
//! ## Architecture Context
//! This crate consolidates three separate registries (ActorRegistry, TupleSpaceRegistry,
//! ServiceRegistry) into ONE unified registry following Proto-First Design principles.
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │              ObjectRegistryImpl                          │
//! │  register() / unregister() / lookup() / discover()      │
//! └────────────────────┬────────────────────────────────────┘
//!                      │
//!                      ▼
//! ┌─────────────────────────────────────────────────────────┐
//! │         ObjectRegistryRepository (trait)                │
//! │  put() / get() / delete() / discover() / heartbeat()    │
//! └────────────────────┬────────────────────────────────────┘
//!                      │
//!         ┌───────────┼───────────┬────────────┐
//!         ▼           ▼           ▼            ▼
//!    ┌────────┐  ┌────────┐  ┌────────┐  ┌────────┐
//!    │ Memory │  │ SQLite │  │Postgres│  │DynamoDB│
//!    └────────┘  └────────┘  └────────┘  └────────┘
//! ```
//!
//! ## Key Components
//! - [`ObjectRegistryImpl`]: Main registry with repository backend
//! - [`ObjectRegistryRepository`]: Storage abstraction trait
//! - [`ObjectRegistryError`]: Error types for registry operations
//! - [`config`]: Backend configuration and factory functions
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_proto`]: Protocol buffer definitions (object_registry.proto)
//! - [`plexspaces_common`]: RequestContext for multi-tenancy
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
//! use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
//! use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
//! use plexspaces_common::RequestContext;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await?);
//! let registry = ObjectRegistryImpl::new(repo);
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
//! ### Using Configuration
//! ```rust,no_run
//! use plexspaces_object_registry::{ObjectRegistryImpl, create_repository_from_shared_db};
//! use plexspaces_proto::storage::v1::SharedDbConfig;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = SharedDbConfig {
//!     connection_string: "sqlite:///tmp/registry.db".to_string(),
//!     ..Default::default()
//! };
//! let repo = create_repository_from_shared_db(&config).await?;
//! let registry = ObjectRegistryImpl::new(repo);
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All data models defined in `proto/plexspaces/v1/registry/object_registry.proto`
//!
//! ### Repository Pattern
//! - Storage abstraction via `ObjectRegistryRepository` trait
//! - Multiple backends: InMemory (tests), SQLite (embedded), PostgreSQL (production), DynamoDB (AWS)
//! - Indexed columns for fast queries (object_type, node_id, health_status, last_heartbeat)
//! - Full ObjectRegistration preserved in blob column
//!
//! ### Test-Driven
//! - Unit tests in this file (#[cfg(test)] mod tests)
//! - Integration tests in tests/ directory
//! - Target coverage: 95%+
//!
//! ## Performance Characteristics
//! - Register: O(1) - single repository write
//! - Lookup: O(1) - single repository read
//! - Discover: O(log n + k) - indexed query + filter
//! - Heartbeat: O(1) - single column UPDATE (no blob read/write)

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod config;
pub mod repository;

use plexspaces_common::RequestContext;
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use repository::{DiscoverFilter, ObjectRegistryRepository, RepositoryError};
use std::sync::Arc;
use tracing::instrument;

// Re-export commonly used types
pub use config::{create_repository_from_shared_db, create_repository_from_storage_config};

#[cfg(feature = "sql-backend")]
pub use repository::{PostgresObjectRegistryRepository, SqliteObjectRegistryRepository};

#[cfg(feature = "ddb-backend")]
pub use repository::DynamoDBObjectRegistryRepository;

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

impl From<RepositoryError> for ObjectRegistryError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::NotFound(id) => ObjectRegistryError::ObjectNotFound(id),
            RepositoryError::AlreadyExists(id) => ObjectRegistryError::ObjectAlreadyRegistered(id),
            RepositoryError::Storage(msg) => ObjectRegistryError::StorageError(msg),
            RepositoryError::Serialization(msg) => ObjectRegistryError::SerializationError(msg),
            RepositoryError::InvalidInput(msg) => ObjectRegistryError::InvalidInput(msg),
            RepositoryError::Connection(msg) => ObjectRegistryError::StorageError(msg),
        }
    }
}

/// Unified ObjectRegistry implementation for actors, tuplespaces, and services
///
/// ## Purpose
/// Provides centralized registration and discovery for all distributed objects
/// in PlexSpaces using an ObjectRegistryRepository backend.
///
/// ## Design
/// - Uses ObjectRegistryRepository for persistence with indexed columns
/// - Indexed columns enable fast queries by object_type, node_id, health_status, last_heartbeat
/// - Full ObjectRegistration blob preserved for complete data
/// - No external dependencies beyond repository
///
/// ## Examples
/// ```rust,no_run
/// # use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
/// # use std::sync::Arc;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await?);
/// let registry = ObjectRegistryImpl::new(repo);
/// # Ok(())
/// # }
/// ```
pub struct ObjectRegistryImpl {
    repository: Arc<dyn ObjectRegistryRepository>,
}

impl ObjectRegistryImpl {
    /// Create new ObjectRegistry with given repository backend
    ///
    /// ## Arguments
    /// * `repository` - ObjectRegistryRepository implementation
    ///
    /// ## Returns
    /// New ObjectRegistryImpl instance
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await?);
    /// let registry = ObjectRegistryImpl::new(repo);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(repository: Arc<dyn ObjectRegistryRepository>) -> Self {
        Self { repository }
    }

    /// Register object (actor, tuplespace, or service)
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (tenant_id comes from here)
    /// * `registration` - ObjectRegistration with object details
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::InvalidInput`]: Missing required fields
    /// - [`ObjectRegistryError::StorageError`]: Repository failure
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    /// # use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    /// # use plexspaces_common::RequestContext;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await?);
    /// # let registry = ObjectRegistryImpl::new(repo);
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
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %registration.object_id))]
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

        // Get tenant_id and namespace from RequestContext
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

        if registration.object_type == ObjectType::ObjectTypeNode as i32
            && !registration.node_id.is_empty()
        {
            let existing = self
                .repository
                .discover(
                    ctx,
                    &DiscoverFilter {
                        object_type: Some(ObjectType::ObjectTypeNode),
                        node_id: Some(registration.node_id.clone()),
                        ..Default::default()
                    },
                    0,
                    2,
                )
                .await?;

            if existing
                .iter()
                .any(|item| item.object_id != registration.object_id)
            {
                return Err(ObjectRegistryError::InvalidInput(format!(
                    "node_id '{}' is already registered to a different node object",
                    registration.node_id
                )));
            }
        }

        // Set timestamps
        let now = chrono::Utc::now();
        registration.created_at = Some(prost_types::Timestamp {
            seconds: now.timestamp(),
            nanos: now.timestamp_subsec_nanos() as i32,
        });
        registration.updated_at = registration.created_at.clone();

        // Store via repository (upsert semantics)
        self.repository.put(ctx, &registration).await?;

        Ok(())
    }

    /// Unregister object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Type of object (Actor, TupleSpace, Service)
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::ObjectNotFound`]: Object doesn't exist
    /// - [`ObjectRegistryError::StorageError`]: Repository failure
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    pub async fn unregister(
        &self,
        ctx: &RequestContext,
        _object_type: ObjectType,
        object_id: &str,
    ) -> Result<(), ObjectRegistryError> {
        // Check if exists first
        if !self.repository.exists(ctx, object_id).await? {
            return Err(ObjectRegistryError::ObjectNotFound(format!(
                "Object '{}' not found in tenant '{}', namespace '{}'",
                object_id,
                ctx.tenant_id(),
                ctx.namespace()
            )));
        }

        self.repository.delete(ctx, object_id).await?;
        Ok(())
    }

    /// Lookup specific object by ID
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Type of object
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(Some(ObjectRegistration))` if found, `Ok(None)` if not found
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::StorageError`]: Repository failure
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    pub async fn lookup(
        &self,
        ctx: &RequestContext,
        _object_type: ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, ObjectRegistryError> {
        let result = self.repository.get(ctx, object_id).await?;

        // If not admin, verify tenant matches
        if let Some(ref reg) = result {
            if !ctx.is_admin() && reg.tenant_id != ctx.tenant_id() {
                return Ok(None); // Tenant mismatch - return None
            }
        }

        Ok(result)
    }

    /// Lookup object by ID (full signature matching ObjectRegistry trait)
    ///
    /// ## Purpose
    /// Wraps `lookup()` to match the ObjectRegistry trait signature.
    /// This method converts errors to `Box<dyn Error>` for trait compatibility.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Type of object
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(Some(ObjectRegistration))` if found, `Ok(None)` if not found
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
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
    /// ## Purpose
    /// Wraps `register()` to match the ObjectRegistry trait signature.
    /// This method converts errors to `Box<dyn Error>` for trait compatibility.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `registration` - ObjectRegistration with object details
    ///
    /// ## Returns
    /// `Ok(())` on success
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %registration.object_id))]
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
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Filter by type (None = all types)
    /// * `object_category` - Filter by category (None = all categories)
    /// * `capabilities` - Filter by capabilities (None = all)
    /// * `labels` - Filter by labels (None = all)
    /// * `health_status` - Filter by health status (None = all)
    /// * `offset` - Number of results to skip
    /// * `limit` - Maximum results to return
    ///
    /// ## Returns
    /// List of matching ObjectRegistrations
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::StorageError`]: Repository failure
    ///
    /// ## Performance
    /// O(log n + k) using indexed columns for filtering
    #[instrument(skip(self, ctx, capabilities, labels), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_type = ?object_type, offset = %offset, limit = %limit))]
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
        let filter = DiscoverFilter {
            object_type,
            object_category,
            health_status,
            labels,
            capabilities,
            ..Default::default()
        };

        let results = self
            .repository
            .discover(ctx, &filter, offset, limit)
            .await?;

        // Filter by tenant if not admin
        if ctx.is_admin() {
            Ok(results)
        } else {
            Ok(results
                .into_iter()
                .filter(|r| r.tenant_id == ctx.tenant_id())
                .collect())
        }
    }

    /// Update heartbeat for object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Type of object
    /// * `object_id` - Object identifier
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`ObjectRegistryError::ObjectNotFound`]: Object doesn't exist
    /// - [`ObjectRegistryError::StorageError`]: Repository failure
    ///
    /// ## Performance
    /// O(1) - single column UPDATE, no blob read/write required
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    pub async fn heartbeat(
        &self,
        ctx: &RequestContext,
        _object_type: ObjectType,
        object_id: &str,
    ) -> Result<(), ObjectRegistryError> {
        let now = chrono::Utc::now().timestamp();
        self.repository
            .update_heartbeat(ctx, object_id, now)
            .await?;
        Ok(())
    }

    /// Find stale registrations (last heartbeat older than threshold)
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `threshold_seconds` - Age threshold in seconds
    /// * `object_type` - Optional filter by type
    /// * `limit` - Maximum results to return
    ///
    /// ## Returns
    /// List of stale ObjectRegistrations
    ///
    /// ## Performance
    /// O(log n + k) using last_heartbeat index
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), threshold_seconds = %threshold_seconds, object_type = ?object_type))]
    pub async fn find_stale(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        object_type: Option<ObjectType>,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, ObjectRegistryError> {
        let threshold_time = chrono::Utc::now().timestamp() - threshold_seconds;

        let filter = DiscoverFilter {
            object_type,
            last_heartbeat_before: Some(threshold_time),
            ..Default::default()
        };

        let results = self.repository.discover(ctx, &filter, 0, limit).await?;
        Ok(results)
    }

    /// Update health status for an object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_id` - Object identifier
    /// * `status` - New health status
    ///
    /// ## Returns
    /// `Ok(())` on success
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id, status = ?status))]
    pub async fn update_health_status(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        status: HealthStatus,
    ) -> Result<(), ObjectRegistryError> {
        self.repository
            .update_health_status(ctx, object_id, status)
            .await?;
        Ok(())
    }

    /// Count objects matching filter
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Optional filter by type
    ///
    /// ## Returns
    /// Count of matching objects
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_type = ?object_type))]
    pub async fn count(
        &self,
        ctx: &RequestContext,
        object_type: Option<ObjectType>,
    ) -> Result<usize, ObjectRegistryError> {
        let filter = DiscoverFilter {
            object_type,
            ..Default::default()
        };
        let count = self.repository.count(ctx, &filter).await?;
        Ok(count)
    }

    /// List distinct tenant ids for registrations of the given object type.
    ///
    /// ## Purpose
    /// Supports tenant discovery from the registry itself so callers do not
    /// reconstruct tenant state from higher-level dashboard projections.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_type = ?object_type))]
    pub async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>, ObjectRegistryError> {
        self.repository
            .list_tenant_ids_by_object_type(ctx, object_type, offset, limit)
            .await
            .map_err(Into::into)
    }

    /// Count distinct tenant ids for registrations of the given object type.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_type = ?object_type))]
    pub async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
    ) -> Result<usize, ObjectRegistryError> {
        self.repository
            .count_tenant_ids_by_object_type(ctx, object_type)
            .await
            .map_err(Into::into)
    }
}

// Debug impl for ObjectRegistryImpl
impl std::fmt::Debug for ObjectRegistryImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ObjectRegistryImpl")
            .field("repository", &"<dyn ObjectRegistryRepository>")
            .finish()
    }
}

// ObjectRegistryImpl implements the Service trait
impl plexspaces_core::Service for ObjectRegistryImpl {
    fn service_name(&self) -> String {
        "ObjectRegistry".to_string()
    }
}

#[async_trait::async_trait]
impl plexspaces_core::actor_context::ObjectRegistry for ObjectRegistryImpl {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_core::actor_context::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let obj_type = object_type
            .unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.lookup(ctx, obj_type, object_id).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<
        Option<plexspaces_core::actor_context::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.lookup_full(ctx, object_type, object_id).await
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_core::actor_context::ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.register_trait(ctx, registration).await
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
    ) -> Result<
        Vec<plexspaces_core::actor_context::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.discover(
            ctx,
            object_type,
            object_category,
            capabilities,
            labels,
            health_status,
            offset,
            limit,
        )
        .await
        .map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn unregister(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.unregister(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.list_tenant_ids_by_object_type(ctx, object_type, offset, limit)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        self.count_tenant_ids_by_object_type(ctx, object_type)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn heartbeat(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.heartbeat(ctx, object_type, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }
}

#[cfg(all(test, feature = "sql-backend"))]
mod tests {
    use super::*;
    use repository::SqliteObjectRegistryRepository;

    fn create_test_registration(object_id: &str, object_type: ObjectType) -> ObjectRegistration {
        ObjectRegistration {
            object_id: object_id.to_string(),
            object_type: object_type as i32,
            grpc_address: "http://test-node:8000".to_string(),
            object_category: "GenServer".to_string(),
            health_status: HealthStatus::HealthStatusHealthy as i32,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_register_and_lookup() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );
        let registration =
            create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
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
    async fn test_register_upsert() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );
        let mut registration =
            create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration.clone()).await.unwrap();

        // Re-register with updated address (upsert)
        registration.grpc_address = "http://new-node:9000".to_string();
        registry.register(&ctx, registration).await.unwrap();

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "test-actor@node1")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(found.grpc_address, "http://new-node:9000");
    }

    #[tokio::test]
    async fn test_unregister() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );
        let registration =
            create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
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
    async fn test_register_rejects_duplicate_node_id_for_node_objects() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let mut reg1 = create_test_registration("node-1", ObjectType::ObjectTypeNode);
        reg1.node_id = "node-1".to_string();
        reg1.object_category = "Node".to_string();
        registry.register(&ctx, reg1).await.unwrap();

        let mut reg2 = create_test_registration("_unknown_1", ObjectType::ObjectTypeNode);
        reg2.node_id = "node-1".to_string();
        reg2.object_category = "Node".to_string();

        let result = registry.register(&ctx, reg2).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_heartbeat() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );
        let registration =
            create_test_registration("test-actor@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, registration).await.unwrap();

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
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg1).await.unwrap();

        let reg2 = create_test_registration("actor2@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg2).await.unwrap();

        let reg3 = create_test_registration("ts1", ObjectType::ObjectTypeTuplespace);
        registry.register(&ctx, reg3).await.unwrap();

        let actors = registry
            .discover(
                &ctx,
                Some(ObjectType::ObjectTypeActor),
                None,
                None,
                None,
                None,
                0,
                100,
            )
            .await
            .unwrap();

        assert_eq!(actors.len(), 2);
    }

    #[tokio::test]
    async fn test_tenant_isolation() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx1 =
            RequestContext::new_without_auth("tenant-1".to_string(), "namespace-1".to_string());
        let ctx2 =
            RequestContext::new_without_auth("tenant-2".to_string(), "namespace-1".to_string());

        let reg1 = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        registry.register(&ctx1, reg1).await.unwrap();

        // Different tenant should not see the registration
        let found = registry
            .lookup(&ctx2, ObjectType::ObjectTypeActor, "actor-1")
            .await
            .unwrap();
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_count() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg1 = create_test_registration("actor1@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg1).await.unwrap();

        let reg2 = create_test_registration("actor2@node1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg2).await.unwrap();

        let count = registry
            .count(&ctx, Some(ObjectType::ObjectTypeActor))
            .await
            .unwrap();
        assert_eq!(count, 2);

        let total = registry.count(&ctx, None).await.unwrap();
        assert_eq!(total, 2);
    }

    #[tokio::test]
    async fn test_update_health_status() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let ctx = RequestContext::new_without_auth(
            "test-tenant".to_string(),
            "test-namespace".to_string(),
        );

        let reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        registry.register(&ctx, reg).await.unwrap();

        registry
            .update_health_status(&ctx, "actor-1", HealthStatus::HealthStatusUnhealthy)
            .await
            .unwrap();

        // Verify by discovering with health filter
        let unhealthy = registry
            .discover(
                &ctx,
                None,
                None,
                None,
                None,
                Some(HealthStatus::HealthStatusUnhealthy),
                0,
                100,
            )
            .await
            .unwrap();

        assert_eq!(unhealthy.len(), 1);
        assert_eq!(unhealthy[0].object_id, "actor-1");
    }

    #[tokio::test]
    async fn test_list_tenant_ids_by_object_type_for_admin_reads_distinct_application_tenants() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let tenant_a_ns1 =
            RequestContext::new_without_auth("tenant-a".to_string(), "ns-1".to_string());
        let tenant_a_ns2 =
            RequestContext::new_without_auth("tenant-a".to_string(), "ns-2".to_string());
        let tenant_b_ns1 =
            RequestContext::new_without_auth("tenant-b".to_string(), "ns-1".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        registry
            .register(
                &tenant_a_ns1,
                create_test_registration("app-a-1", ObjectType::ObjectTypeApplication),
            )
            .await
            .unwrap();
        registry
            .register(
                &tenant_a_ns2,
                create_test_registration("app-a-2", ObjectType::ObjectTypeApplication),
            )
            .await
            .unwrap();
        registry
            .register(
                &tenant_b_ns1,
                create_test_registration("app-b-1", ObjectType::ObjectTypeApplication),
            )
            .await
            .unwrap();
        registry
            .register(
                &tenant_b_ns1,
                create_test_registration("actor-b-1", ObjectType::ObjectTypeActor),
            )
            .await
            .unwrap();

        let tenant_ids = registry
            .list_tenant_ids_by_object_type(&admin_ctx, ObjectType::ObjectTypeApplication, 0, 10)
            .await
            .unwrap();

        assert_eq!(
            tenant_ids,
            vec!["tenant-a".to_string(), "tenant-b".to_string()]
        );
        assert_eq!(
            registry
                .count_tenant_ids_by_object_type(&admin_ctx, ObjectType::ObjectTypeApplication)
                .await
                .unwrap(),
            2
        );
    }

    #[tokio::test]
    async fn test_list_tenant_ids_by_object_type_for_authenticated_non_admin_returns_request_tenant(
    ) {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);

        let tenant_a_ctx =
            RequestContext::new("tenant-a".to_string(), String::new(), true).unwrap();
        let tenant_b_ctx =
            RequestContext::new_without_auth("tenant-b".to_string(), "ns-1".to_string());

        registry
            .register(
                &tenant_b_ctx,
                create_test_registration("app-b-1", ObjectType::ObjectTypeApplication),
            )
            .await
            .unwrap();

        let tenant_ids = registry
            .list_tenant_ids_by_object_type(&tenant_a_ctx, ObjectType::ObjectTypeApplication, 0, 10)
            .await
            .unwrap();

        assert_eq!(tenant_ids, vec!["tenant-a".to_string()]);
        assert_eq!(
            registry
                .count_tenant_ids_by_object_type(&tenant_a_ctx, ObjectType::ObjectTypeApplication)
                .await
                .unwrap(),
            1
        );
    }
}

/// Type alias for convenience
pub type ObjectRegistry = ObjectRegistryImpl;
