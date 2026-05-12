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

use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use repository::{DiscoverFilter, ObjectRegistryRepository, RepositoryError};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tracing::instrument;

// Re-export commonly used types
pub use config::{create_repository_from_shared_db, create_repository_from_storage_config};

#[cfg(feature = "sql-backend")]
pub use repository::{PostgresObjectRegistryRepository, SqliteObjectRegistryRepository};

#[cfg(feature = "ddb-backend")]
pub use repository::DynamoDBObjectRegistryRepository;

// Re-export RegisterResult from service-traits via actor crate so callers don't need extra deps.
pub use plexspaces_actor::actor_context::RegisterResult;

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

// ── In-process alias LRU cache (Phase 12) ──────────────────────────────────
//
// Short-circuits `lookup_by_alias` for the hot path (actor placement checks,
// duplicate-spawn guard).  TTL is 30 s — shorter than the discovery cache
// (60 s) because actor placement decisions require fresher data.
//
// Capacity: 10_000 entries (≈ actors per node in a typical large deployment).

/// Minimal LRU cache with per-entry TTL, used for alias → ObjectRegistration.
struct AliasLruCache {
    capacity: usize,
    ttl: Duration,
    map: HashMap<String, (Option<ObjectRegistration>, SystemTime)>,
    queue: VecDeque<String>,
}

impl AliasLruCache {
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            map: HashMap::with_capacity(capacity),
            queue: VecDeque::with_capacity(capacity),
        }
    }

    fn get(&mut self, key: &str) -> Option<Option<ObjectRegistration>> {
        let expired = self
            .map
            .get(key)
            .map(|(_, ts)| {
                SystemTime::now()
                    .duration_since(*ts)
                    .unwrap_or_default()
                    >= self.ttl
            })
            .unwrap_or(true);

        if expired {
            if self.map.remove(key).is_some() {
                self.queue.retain(|k| k != key);
            }
            return None;
        }

        self.map.get(key).map(|(v, _)| v.clone())
    }

    fn insert(&mut self, key: String, value: Option<ObjectRegistration>) {
        let now = SystemTime::now();
        if let Some(entry) = self.map.get_mut(&key) {
            entry.0 = value;
            entry.1 = now;
            return;
        }
        // Evict LRU (insertion-order front) when at capacity.
        if self.map.len() >= self.capacity {
            if let Some(evict) = self.queue.pop_front() {
                self.map.remove(&evict);
            }
        }
        self.queue.push_back(key.clone());
        self.map.insert(key, (value, now));
    }

    fn remove(&mut self, key: &str) {
        if self.map.remove(key).is_some() {
            self.queue.retain(|k| k != key);
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
/// - In-process alias LRU cache (30 s TTL, 10 k cap) short-circuits hot-path
///   alias lookups for unique-placement checks
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
    /// In-process alias LRU cache (30 s TTL, 10_000 cap).
    alias_cache: RwLock<AliasLruCache>,
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
        Self {
            repository,
            alias_cache: RwLock::new(AliasLruCache::new(
                10_000,
                Duration::from_secs(30),
            )),
        }
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

        // Invalidate alias cache so next lookup fetches fresh data.
        if !registration.alias.is_empty() {
            self.alias_cache.write().await.remove(&registration.alias);
        }

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
        // Fetch before delete so we can evict the alias cache entry.
        let existing = self.repository.get(ctx, object_id).await?;
        if existing.is_none() {
            return Err(ObjectRegistryError::ObjectNotFound(format!(
                "Object '{}' not found in tenant '{}', namespace '{}'",
                object_id,
                ctx.tenant_id(),
                ctx.namespace()
            )));
        }

        self.repository.delete(ctx, object_id).await?;

        if let Some(reg) = existing {
            if !reg.alias.is_empty() {
                self.alias_cache.write().await.remove(&reg.alias);
            }
        }

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

    /// Update heartbeat for object, reset failure count, restore HEALTHY status.
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
    /// O(1) - targeted column UPDATEs, no blob read/write required
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
        self.repository
            .reset_heartbeat_failures(ctx, object_id)
            .await?;
        self.repository
            .update_health_status(ctx, object_id, HealthStatus::HealthStatusHealthy)
            .await?;
        Ok(())
    }

    /// Record a missed heartbeat, increment failure count, and transition health state.
    ///
    /// ## Transitions
    /// - 1st failure (< max): DEGRADED
    /// - Failures >= max_heartbeat_failures (default 3): DEAD
    /// - NODE going DEAD cascades to all objects on that node
    ///
    /// ## Returns
    /// New HealthStatus after the failure is recorded.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    pub async fn record_heartbeat_failure(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> Result<HealthStatus, ObjectRegistryError> {
        let new_count = self
            .repository
            .increment_heartbeat_failures(ctx, object_id)
            .await?;

        let reg = self
            .repository
            .get(ctx, object_id)
            .await?
            .ok_or_else(|| ObjectRegistryError::ObjectNotFound(object_id.to_string()))?;

        let max = if reg.max_heartbeat_failures == 0 {
            3
        } else {
            reg.max_heartbeat_failures
        };

        let is_dead = new_count >= max;
        let new_status = if is_dead {
            HealthStatus::HealthStatusDead
        } else {
            HealthStatus::HealthStatusDegraded
        };

        self.repository
            .update_health_status(ctx, object_id, new_status.clone())
            .await?;

        // Invalidate alias cache: health_status changed, so a cached entry would
        // return stale HEALTHY/DEGRADED data and could incorrectly block a new spawn.
        if !reg.alias.is_empty() {
            self.alias_cache.write().await.remove(&reg.alias);
        }

        // Cascade: if a NODE goes DEAD, mark all objects on that node DEAD
        if is_dead && ObjectType::try_from(reg.object_type) == Ok(ObjectType::ObjectTypeNode) {
            self.repository
                .mark_dead_by_node_id(ctx, &reg.object_id)
                .await?;
        }

        Ok(new_status)
    }

    /// Lookup an object by alias (identity-based placement key).
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `alias` - Alias string (e.g. `"{actor_type}:{name}:{namespace}:{tenant_id}"`)
    ///
    /// ## Returns
    /// `Ok(Some(registration))` if found, `Ok(None)` if not found.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), alias = %alias))]
    pub async fn lookup_by_alias(
        &self,
        ctx: &RequestContext,
        alias: &str,
    ) -> Result<Option<ObjectRegistration>, ObjectRegistryError> {
        // Fast path: check in-process alias LRU cache (30 s TTL).
        if let Some(cached) = self.alias_cache.write().await.get(alias) {
            return Ok(cached);
        }

        // Cache miss — query repository.
        let result = self
            .repository
            .get_by_alias(ctx, alias)
            .await
            .map_err(ObjectRegistryError::from)?;

        // Populate cache (store None too, to short-circuit repeated misses).
        self.alias_cache
            .write()
            .await
            .insert(alias.to_string(), result.clone());

        Ok(result)
    }

    /// Register with unique alias enforcement (Orleans grain directory pattern).
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `registration` - ObjectRegistration to store; `alias` field is used as placement key
    /// * `enforce_unique` - If true, fail (return AlreadyExists) when an active instance holds the alias
    ///
    /// ## Returns
    /// - [`RegisterResult::Registered`] on success
    /// - [`RegisterResult::AlreadyExists`] when alias conflict with an active instance is found
    ///
    /// ## Semantics
    /// - DEAD/STOPPING/UNKNOWN existing registrations are removed and replaced.
    /// - HEALTHY/DEGRADED/STARTING existing registrations block the new registration.
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %registration.object_id))]
    pub async fn register_with_unique_alias(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
        enforce_unique: bool,
    ) -> Result<RegisterResult, ObjectRegistryError> {
        if enforce_unique && !registration.alias.is_empty() {
            let alias = registration.alias.as_str();
            if let Some(existing) = self.repository.get_by_alias(ctx, alias).await? {
                let status = HealthStatus::try_from(existing.health_status)
                    .unwrap_or(HealthStatus::HealthStatusUnknown);
                match status {
                    HealthStatus::HealthStatusHealthy
                    | HealthStatus::HealthStatusDegraded
                    | HealthStatus::HealthStatusStarting => {
                        // Active instance holds the alias — refuse
                        return Ok(RegisterResult::AlreadyExists {
                            grpc_address: existing.grpc_address,
                            object_id: existing.object_id,
                        });
                    }
                    _ => {
                        // DEAD / STOPPING / UNKNOWN — stale, remove it
                        self.repository.delete(ctx, &existing.object_id).await?;
                        self.alias_cache.write().await.remove(alias);
                    }
                }
            }
        }

        self.register(ctx, registration).await?;
        Ok(RegisterResult::Registered)
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

    /// Mark all HEALTHY/DEGRADED/STARTING objects on `node_id` as DEAD.
    ///
    /// ## Purpose
    /// Called by SWIM when a node permanently leaves the cluster.  All actors,
    /// services, and workflows registered on that node are immediately marked DEAD.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext; admin context with empty tenant_id performs a
    ///   cross-tenant cascade (the correct call site is the SWIM handler)
    /// * `node_id` - The dead node's identifier
    ///
    /// ## Returns
    /// The number of registrations updated
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), node_id = %node_id))]
    pub async fn mark_objects_dead_by_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<u64, ObjectRegistryError> {
        self.repository
            .mark_dead_by_node_id(ctx, node_id)
            .await
            .map_err(Into::into)
    }

    /// Return registrations whose last heartbeat is older than `threshold_seconds`.
    ///
    /// ## Purpose
    /// Used exclusively by `HeartbeatMonitor` to find stale registrations across
    /// all tenants (admin context with empty `tenant_id`) or within a single tenant.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext; empty `tenant_id` performs a cross-tenant scan
    /// * `threshold_seconds` - Objects not seen in this many seconds are stale
    /// * `limit` - Maximum objects to return per cycle
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), threshold_seconds = %threshold_seconds, limit = %limit))]
    pub async fn find_stale_heartbeats_raw(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, ObjectRegistryError> {
        self.repository
            .find_stale_heartbeats(ctx, threshold_seconds, limit)
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
impl plexspaces_actor::Service for ObjectRegistryImpl {
    fn service_name(&self) -> String {
        "ObjectRegistry".to_string()
    }
}

#[async_trait::async_trait]
impl plexspaces_actor::actor_context::ObjectRegistry for ObjectRegistryImpl {
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<
        Option<plexspaces_actor::actor_context::ObjectRegistration>,
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
        Option<plexspaces_actor::actor_context::ObjectRegistration>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.lookup_full(ctx, object_type, object_id).await
    }

    async fn register(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_actor::actor_context::ObjectRegistration,
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
        Vec<plexspaces_actor::actor_context::ObjectRegistration>,
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

    async fn lookup_by_alias(
        &self,
        ctx: &RequestContext,
        alias: &str,
    ) -> Result<Option<plexspaces_actor::actor_context::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>
    {
        self.lookup_by_alias(ctx, alias).await.map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
    }

    async fn register_with_unique_alias(
        &self,
        ctx: &RequestContext,
        registration: plexspaces_actor::actor_context::ObjectRegistration,
        enforce_unique: bool,
    ) -> Result<RegisterResult, Box<dyn std::error::Error + Send + Sync>> {
        self.register_with_unique_alias(ctx, registration, enforce_unique)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn record_heartbeat_failure(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> Result<HealthStatus, Box<dyn std::error::Error + Send + Sync>> {
        self.record_heartbeat_failure(ctx, object_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn mark_objects_dead_by_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
        self.mark_objects_dead_by_node(ctx, node_id)
            .await
            .map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
    }

    async fn find_stale_heartbeats(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        self.find_stale_heartbeats_raw(ctx, threshold_seconds, limit)
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
            .update_health_status(&ctx, "actor-1", HealthStatus::HealthStatusDead)
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
                Some(HealthStatus::HealthStatusDead),
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

    // ----- Health lifecycle tests -----

    #[tokio::test]
    async fn test_record_heartbeat_failure_degraded_then_dead() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let mut reg = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        reg.max_heartbeat_failures = 3;
        registry.register(&ctx, reg).await.unwrap();

        // First failure → DEGRADED
        let status = registry
            .record_heartbeat_failure(&ctx, "actor-1")
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::HealthStatusDegraded);

        // Second failure → still DEGRADED (count=2 < max=3)
        let status = registry
            .record_heartbeat_failure(&ctx, "actor-1")
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::HealthStatusDegraded);

        // Third failure → DEAD (count=3 >= max=3)
        let status = registry
            .record_heartbeat_failure(&ctx, "actor-1")
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::HealthStatusDead);

        // Verify stored health status
        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.health_status, HealthStatus::HealthStatusDead as i32);
    }

    #[tokio::test]
    async fn test_heartbeat_resets_failure_count_and_restores_healthy() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let mut reg = create_test_registration("actor-2", ObjectType::ObjectTypeActor);
        reg.max_heartbeat_failures = 3;
        registry.register(&ctx, reg).await.unwrap();

        // Two failures → DEGRADED
        registry
            .record_heartbeat_failure(&ctx, "actor-2")
            .await
            .unwrap();
        registry
            .record_heartbeat_failure(&ctx, "actor-2")
            .await
            .unwrap();

        // Successful heartbeat restores HEALTHY
        registry
            .heartbeat(&ctx, ObjectType::ObjectTypeActor, "actor-2")
            .await
            .unwrap();

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            found.health_status,
            HealthStatus::HealthStatusHealthy as i32
        );
        assert_eq!(found.heartbeat_failure_count, 0);
    }

    #[tokio::test]
    async fn test_custom_max_heartbeat_failures() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let mut reg = create_test_registration("actor-3", ObjectType::ObjectTypeActor);
        reg.max_heartbeat_failures = 2;
        registry.register(&ctx, reg).await.unwrap();

        // First failure → DEGRADED
        let status = registry
            .record_heartbeat_failure(&ctx, "actor-3")
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::HealthStatusDegraded);

        // Second failure → DEAD (max=2)
        let status = registry
            .record_heartbeat_failure(&ctx, "actor-3")
            .await
            .unwrap();
        assert_eq!(status, HealthStatus::HealthStatusDead);
    }

    #[tokio::test]
    async fn test_node_dead_cascades_to_objects() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        // Register node
        let mut node_reg = create_test_registration("node-1", ObjectType::ObjectTypeNode);
        node_reg.node_id = "node-1".to_string();
        node_reg.max_heartbeat_failures = 1;
        registry.register(&ctx, node_reg).await.unwrap();

        // Register actors on that node
        let mut actor1 = create_test_registration("actor-a", ObjectType::ObjectTypeActor);
        actor1.node_id = "node-1".to_string();
        registry.register(&ctx, actor1).await.unwrap();

        let mut actor2 = create_test_registration("actor-b", ObjectType::ObjectTypeActor);
        actor2.node_id = "node-1".to_string();
        registry.register(&ctx, actor2).await.unwrap();

        // Node fails enough → DEAD, cascades to actors
        registry
            .record_heartbeat_failure(&ctx, "node-1")
            .await
            .unwrap();

        // Actors on node should be DEAD
        let actor_a = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-a")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actor_a.health_status, HealthStatus::HealthStatusDead as i32);

        let actor_b = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-b")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(actor_b.health_status, HealthStatus::HealthStatusDead as i32);
    }

    // ----- Alias placement tests -----

    #[tokio::test]
    async fn test_register_with_unique_alias_succeeds_on_first_registration() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let mut reg = create_test_registration("actor-x", ObjectType::ObjectTypeActor);
        reg.alias = "Counter:worker:ns:t1".to_string();

        let result = registry
            .register_with_unique_alias(&ctx, reg, true)
            .await
            .unwrap();

        assert!(matches!(result, RegisterResult::Registered));

        // Lookup by alias should find it
        let found = registry
            .lookup_by_alias(&ctx, "Counter:worker:ns:t1")
            .await
            .unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().object_id, "actor-x");
    }

    #[tokio::test]
    async fn test_register_with_unique_alias_conflicts_with_healthy_instance() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let alias = "Counter:worker:ns:t1";
        let mut reg1 = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        reg1.alias = alias.to_string();
        reg1.grpc_address = "http://node1:8000".to_string();
        registry
            .register_with_unique_alias(&ctx, reg1, true)
            .await
            .unwrap();

        // Second registration with same alias while first is HEALTHY → conflict
        let mut reg2 = create_test_registration("actor-2", ObjectType::ObjectTypeActor);
        reg2.alias = alias.to_string();
        let result = registry
            .register_with_unique_alias(&ctx, reg2, true)
            .await
            .unwrap();

        match result {
            RegisterResult::AlreadyExists {
                grpc_address,
                object_id,
            } => {
                assert_eq!(object_id, "actor-1");
                assert_eq!(grpc_address, "http://node1:8000");
            }
            RegisterResult::Registered => panic!("expected AlreadyExists"),
        }
    }

    #[tokio::test]
    async fn test_register_with_unique_alias_replaces_dead_instance() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let alias = "Counter:worker:ns:t1";
        let mut reg1 = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        reg1.alias = alias.to_string();
        reg1.max_heartbeat_failures = 1;
        registry
            .register_with_unique_alias(&ctx, reg1, true)
            .await
            .unwrap();

        // Kill the instance
        registry
            .record_heartbeat_failure(&ctx, "actor-1")
            .await
            .unwrap();

        // New registration with same alias should succeed (stale instance removed)
        let mut reg2 = create_test_registration("actor-2", ObjectType::ObjectTypeActor);
        reg2.alias = alias.to_string();
        let result = registry
            .register_with_unique_alias(&ctx, reg2, true)
            .await
            .unwrap();

        assert!(matches!(result, RegisterResult::Registered));

        // Lookup should find new instance
        let found = registry.lookup_by_alias(&ctx, alias).await.unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().object_id, "actor-2");
    }

    #[tokio::test]
    async fn test_register_without_enforce_unique_skips_app_level_check() {
        // enforce_unique=false skips the app-level conflict check.
        // Two registrations with NO alias, or with distinct aliases, both succeed.
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());

        let mut reg1 = create_test_registration("actor-1", ObjectType::ObjectTypeActor);
        reg1.alias = "Counter:worker-1:ns:t1".to_string();
        let result1 = registry
            .register_with_unique_alias(&ctx, reg1, false)
            .await
            .unwrap();
        assert!(matches!(result1, RegisterResult::Registered));

        let mut reg2 = create_test_registration("actor-2", ObjectType::ObjectTypeActor);
        reg2.alias = "Counter:worker-2:ns:t1".to_string();
        let result2 = registry
            .register_with_unique_alias(&ctx, reg2, false)
            .await
            .unwrap();
        assert!(matches!(result2, RegisterResult::Registered));
    }

    // ── Stale-heartbeat scan tests (formerly heartbeat_monitor tests) ─────────

    async fn make_registry_for_stale_tests() -> Arc<ObjectRegistryImpl> {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        Arc::new(ObjectRegistryImpl::new(repo))
    }

    fn healthy_actor_reg(id: &str) -> ObjectRegistration {
        ObjectRegistration {
            object_id: id.to_string(),
            object_type: ObjectType::ObjectTypeActor as i32,
            grpc_address: "http://test:8000".to_string(),
            health_status: HealthStatus::HealthStatusHealthy as i32,
            max_heartbeat_failures: 3,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_stale_scan_transitions_to_degraded() {
        let registry = make_registry_for_stale_tests().await;
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        registry
            .register(&ctx, healthy_actor_reg("actor-stale-1"))
            .await
            .unwrap();

        // threshold=0 → all objects with any heartbeat record are stale
        let stale = registry
            .find_stale_heartbeats_raw(&admin_ctx, 0, 100)
            .await
            .unwrap();
        for reg in &stale {
            let tenant_ctx = RequestContext::new_without_auth(
                reg.tenant_id.clone(),
                reg.namespace.clone(),
            );
            registry
                .record_heartbeat_failure(&tenant_ctx, &reg.object_id)
                .await
                .unwrap();
        }

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-stale-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            found.health_status,
            HealthStatus::HealthStatusDegraded as i32
        );
    }

    #[tokio::test]
    async fn test_stale_scan_max_failures_transitions_to_dead() {
        let registry = make_registry_for_stale_tests().await;
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        let mut reg = healthy_actor_reg("actor-stale-2");
        reg.max_heartbeat_failures = 2;
        registry.register(&ctx, reg).await.unwrap();

        let scan = |registry: Arc<ObjectRegistryImpl>, admin_ctx: RequestContext| async move {
            let stale = registry
                .find_stale_heartbeats_raw(&admin_ctx, 0, 100)
                .await
                .unwrap();
            for r in &stale {
                let tenant_ctx =
                    RequestContext::new_without_auth(r.tenant_id.clone(), r.namespace.clone());
                let _ = registry
                    .record_heartbeat_failure(&tenant_ctx, &r.object_id)
                    .await;
            }
        };

        scan(registry.clone(), admin_ctx.clone()).await; // count=1 → DEGRADED
        scan(registry.clone(), admin_ctx.clone()).await; // count=2 >= max=2 → DEAD

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-stale-2")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(found.health_status, HealthStatus::HealthStatusDead as i32);
    }

    #[tokio::test]
    async fn test_fresh_heartbeat_not_returned_by_stale_scan() {
        let registry = make_registry_for_stale_tests().await;
        let ctx = RequestContext::new_without_auth("t1".to_string(), "ns".to_string());
        let admin_ctx =
            RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        registry
            .register(&ctx, healthy_actor_reg("actor-stale-3"))
            .await
            .unwrap();

        // Record a fresh heartbeat for this object.
        registry
            .heartbeat(&ctx, ObjectType::ObjectTypeActor, "actor-stale-3")
            .await
            .unwrap();

        // Use a 1-hour threshold → object is not stale.
        let stale = registry
            .find_stale_heartbeats_raw(&admin_ctx, 3600, 100)
            .await
            .unwrap();
        assert!(
            stale.iter().all(|r| r.object_id != "actor-stale-3"),
            "freshly heartbeated actor must not appear in stale scan"
        );

        let found = registry
            .lookup(&ctx, ObjectType::ObjectTypeActor, "actor-stale-3")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            found.health_status,
            HealthStatus::HealthStatusHealthy as i32
        );
    }

    // ── Alias LRU cache tests (Phase 12) ─────────────────────────────────────

    #[tokio::test]
    async fn test_alias_cache_hit_on_second_lookup() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".into(), "ns1".into());

        let mut reg = create_test_registration("actor-cached", ObjectType::ObjectTypeActor);
        reg.alias = "Counter:worker:ns1:t1".to_string();
        registry.register(&ctx, reg).await.unwrap();

        // First call: cache miss, goes to DB.
        let r1 = registry
            .lookup_by_alias(&ctx, "Counter:worker:ns1:t1")
            .await
            .unwrap();
        assert!(r1.is_some());

        // Second call: cache hit (no DB read, same result).
        let r2 = registry
            .lookup_by_alias(&ctx, "Counter:worker:ns1:t1")
            .await
            .unwrap();
        assert_eq!(r1.unwrap().object_id, r2.unwrap().object_id);
    }

    #[tokio::test]
    async fn test_alias_cache_invalidated_on_unregister() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".into(), "ns1".into());

        let mut reg = create_test_registration("actor-evict", ObjectType::ObjectTypeActor);
        reg.alias = "Counter:evict:ns1:t1".to_string();
        registry.register(&ctx, reg).await.unwrap();

        // Populate cache.
        let r = registry
            .lookup_by_alias(&ctx, "Counter:evict:ns1:t1")
            .await
            .unwrap();
        assert!(r.is_some());

        // Unregister removes from DB and evicts from cache.
        registry
            .unregister(&ctx, ObjectType::ObjectTypeActor, "actor-evict")
            .await
            .unwrap();

        // Next lookup must miss — returns None from DB (not stale cache).
        let after = registry
            .lookup_by_alias(&ctx, "Counter:evict:ns1:t1")
            .await
            .unwrap();
        assert!(after.is_none());
    }

    #[tokio::test]
    async fn test_alias_cache_invalidated_on_register_update() {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry = ObjectRegistryImpl::new(repo);
        let ctx = RequestContext::new_without_auth("t1".into(), "ns1".into());

        let mut reg = create_test_registration("actor-update", ObjectType::ObjectTypeActor);
        reg.alias = "Counter:update:ns1:t1".to_string();
        registry.register(&ctx, reg).await.unwrap();

        // Populate cache.
        registry
            .lookup_by_alias(&ctx, "Counter:update:ns1:t1")
            .await
            .unwrap();

        // Re-register with updated grpc_address (upsert).
        let mut reg2 = create_test_registration("actor-update", ObjectType::ObjectTypeActor);
        reg2.alias = "Counter:update:ns1:t1".to_string();
        reg2.grpc_address = "http://new-node:9000".to_string();
        registry.register(&ctx, reg2).await.unwrap();

        // Cache was evicted on register; next lookup fetches fresh data.
        let updated = registry
            .lookup_by_alias(&ctx, "Counter:update:ns1:t1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(updated.grpc_address, "http://new-node:9000");
    }
}

/// Type alias for convenience
pub type ObjectRegistry = ObjectRegistryImpl;
