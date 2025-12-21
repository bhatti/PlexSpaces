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

//! # PlexSpaces KeyValue Store
//!
//! ## Purpose
//! Provides a multi-purpose key-value storage abstraction for the PlexSpaces framework,
//! supporting persistent state management, configuration storage, service discovery,
//! distributed locks, and more.
//!
//! ## Architecture Context
//! The KeyValue store is foundational infrastructure used throughout PlexSpaces:
//!
//! - **ActorRegistry**: Maps ActorId → ActorRef for service discovery
//! - **NodeRegistry**: Maps NodeId → NodeInfo for cluster membership
//! - **Configuration**: Framework settings with hot reload via watch
//! - **Feature Flags**: Runtime toggles for capabilities
//! - **Leases**: Distributed locks with TTL
//! - **Workflows**: Long-running process state
//! - **Snapshots**: Actor state persistence
//! - **Metrics**: Counters and observability data
//! - **Sessions**: Stateful interactions with automatic expiry
//!
//! ## Why KeyValue NOT TupleSpace?
//!
//! TupleSpace is designed for coordination (dataflow patterns) with destructive `take()`
//! operations. Registry and configuration need **non-destructive reads** with safe lookup
//! semantics. Industry standard (Consul, etcd, Redis) uses key-value for persistent state.
//!
//! See `REGISTRY_STORAGE_ANALYSIS.md` for detailed rationale.
//!
//! ## Key Components
//!
//! - [`KeyValueStore`]: Main trait defining all operations
//! - [`InMemoryKVStore`]: HashMap-based implementation for testing
//! - [`KVError`]: Error types for all operations
//! - [`KVEvent`]: Watch notification events
//! - [`KVStats`]: Storage statistics
//!
//! ## Multi-Tenancy
//!
//! All operations require a [`RequestContext`] with `tenant_id` and `namespace` for proper
//! data isolation. Keys are automatically scoped to tenant and namespace, ensuring complete
//! isolation between tenants and namespaces.
//!
//! ## Backend Support
//!
//! - **InMemory**: HashMap-based (always available)
//! - **SQLite**: Persistent, single-node (feature: `sql-backend`)
//! - **PostgreSQL**: Distributed, multi-node (feature: `sql-backend`)
//! - **Redis**: Distributed with native TTL (feature: `redis-backend`)
//!
//! ## Examples
//!
//! ### Basic Usage
//! ```rust
//! use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
//! use plexspaces_core::RequestContext;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kv = InMemoryKVStore::new();
//! let ctx = RequestContext::new("tenant".to_string()).with_namespace("default".to_string());
//!
//! // Basic operations
//! kv.put(&ctx, "key", b"value".to_vec()).await?;
//! let value = kv.get(&ctx, "key").await?;
//! assert_eq!(value, Some(b"value".to_vec()));
//!
//! kv.delete(&ctx, "key").await?;
//! assert!(!kv.exists(&ctx, "key").await?);
//! # Ok(())
//! # }
//! ```
//!
//! ### With TTL (Time-To-Live)
//! ```rust
//! use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
//! use std::time::Duration;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kv = InMemoryKVStore::new();
//!
//! // Lease with automatic expiry
//! kv.put_with_ttl("session:123", b"data".to_vec(), Duration::from_secs(30)).await?;
//!
//! // Renew lease
//! kv.refresh_ttl("session:123", Duration::from_secs(60)).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Atomic Operations
//! ```rust
//! use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kv = InMemoryKVStore::new();
//!
//! // Compare-and-swap (distributed lock acquisition)
//! let acquired = kv.cas("lock:resource", None, b"node1".to_vec()).await?;
//! assert!(acquired);
//!
//! // Atomic counter
//! let count = kv.increment("metrics:requests", 1).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Hierarchical Queries
//! ```rust
//! use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let kv = InMemoryKVStore::new();
//!
//! kv.put("config:actor.timeout", b"30s".to_vec()).await?;
//! kv.put("config:actor.max_size", b"10000".to_vec()).await?;
//! kv.put("config:mailbox.type", b"fifo".to_vec()).await?;
//!
//! // List all actor configs
//! let actor_configs = kv.list("config:actor.").await?;
//! assert_eq!(actor_configs.len(), 2);
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! While the Rust trait defines the API, backend configuration will use proto definitions
//! for type-safe, documented configuration.
//!
//! ### Static vs Dynamic
//! The KeyValueStore trait is a static abstraction (always present). Specific backends
//! are chosen at runtime via configuration.
//!
//! ### Test-Driven
//! All implementations must achieve 90%+ test coverage with comprehensive unit and
//! integration tests.
//!
//! ## Testing
//! ```bash
//! # Run tests (in-memory backend)
//! cargo test -p plexspaces-keyvalue
//!
//! # Test with SQL backend
//! cargo test -p plexspaces-keyvalue --features sql-backend
//!
//! # Test with all backends
//! cargo test -p plexspaces-keyvalue --features all-backends
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-keyvalue --fail-under 90
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

use async_trait::async_trait;
use plexspaces_core::RequestContext;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;

pub mod config;
pub mod error;
pub mod memory;

#[cfg(feature = "sql-backend")]
pub mod sql;

#[cfg(feature = "redis-backend")]
pub mod redis;

#[cfg(feature = "blob-backend")]
pub mod blob;

pub use config::{create_keyvalue_from_config, create_keyvalue_from_env, BackendType, KVConfig};
pub use error::{KVError, KVResult};
pub use memory::InMemoryKVStore;

#[cfg(feature = "sql-backend")]
pub use sql::{PostgreSQLKVStore, SqliteKVStore};

#[cfg(feature = "redis-backend")]
pub use redis::RedisKVStore;

#[cfg(feature = "blob-backend")]
pub use blob::BlobKVStore;

/// KeyValue store trait defining all operations.
///
/// ## Purpose
/// Provides a consistent interface for key-value storage across multiple backend
/// implementations (in-memory, SQLite, Redis, PostgreSQL).
///
/// ## Design Decisions
/// - **Non-destructive reads**: `get()` does not remove the value (unlike TupleSpace `take()`)
/// - **Hierarchical keys**: Support prefix-based queries for namespace isolation
/// - **TTL support**: Automatic expiry for leases and sessions
/// - **Atomic operations**: CAS for distributed locks, increment for counters
/// - **Watch notifications**: Hot reload for configuration changes
///
/// ## Operations Grouped by Purpose
///
/// ### Core Operations (All use cases)
/// - `get()`, `put()`, `delete()`, `exists()`
///
/// ### Query Operations (Registry, Config, Workflow)
/// - `list()`, `multi_get()`, `multi_put()`
///
/// ### TTL Operations (Leases, Sessions, Metrics)
/// - `put_with_ttl()`, `refresh_ttl()`, `get_ttl()`
///
/// ### Atomic Operations (Leases, Distributed Locks)
/// - `cas()`, `increment()`, `decrement()`
///
/// ### Watch/Notification (Config hot reload, Feature flags)
/// - `watch()`, `watch_prefix()`
///
/// ### Maintenance Operations (Cleanup, Snapshots)
/// - `clear_prefix()`, `count_prefix()`, `get_stats()`
///
/// ## Multi-Tenancy
/// All methods require a `RequestContext` parameter that provides `tenant_id` and `namespace`
/// for data isolation. Keys are automatically scoped to the tenant and namespace.
#[async_trait]
pub trait KeyValueStore: Send + Sync {
    // =========================================================================
    // Core Operations (All use cases)
    // =========================================================================

    /// Get value by key (non-destructive read).
    ///
    /// ## Returns
    /// - `Ok(Some(value))` if key exists
    /// - `Ok(None)` if key does not exist
    /// - `Err(...)` on storage failure
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("key", b"value".to_vec()).await?;
    /// let value = kv.get("key").await?;
    /// assert_eq!(value, Some(b"value".to_vec()));
    /// # Ok(())
    /// # }
    /// ```
    async fn get(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Vec<u8>>>;

    /// Put key-value pair.
    ///
    /// ## Behavior
    /// - Overwrites existing value if key already exists
    /// - No TTL unless set separately with `put_with_ttl()`
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("config:timeout", b"30s".to_vec()).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn put(&self, ctx: &RequestContext, key: &str, value: Vec<u8>) -> KVResult<()>;

    /// Delete key.
    ///
    /// ## Behavior
    /// - Succeeds even if key does not exist (idempotent)
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("temp", b"data".to_vec()).await?;
    /// kv.delete("temp").await?;
    /// assert!(!kv.exists("temp").await?);
    /// # Ok(())
    /// # }
    /// ```
    async fn delete(&self, ctx: &RequestContext, key: &str) -> KVResult<()>;

    /// Check if key exists.
    ///
    /// ## Returns
    /// - `Ok(true)` if key exists
    /// - `Ok(false)` if key does not exist
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("key", b"value".to_vec()).await?;
    /// assert!(kv.exists("key").await?);
    /// # Ok(())
    /// # }
    /// ```
    async fn exists(&self, ctx: &RequestContext, key: &str) -> KVResult<bool>;

    // =========================================================================
    // Query Operations (Registry, Config, Workflow)
    // =========================================================================

    /// List all keys with prefix (supports hierarchical queries).
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("actor:alice", b"ref1".to_vec()).await?;
    /// kv.put("actor:bob", b"ref2".to_vec()).await?;
    /// kv.put("node:node1", b"info".to_vec()).await?;
    ///
    /// let actors = kv.list("actor:").await?;
    /// assert_eq!(actors.len(), 2);
    /// # Ok(())
    /// # }
    /// ```
    async fn list(&self, ctx: &RequestContext, prefix: &str) -> KVResult<Vec<String>>;

    /// Get multiple keys at once (batch read).
    ///
    /// ## Returns
    /// Vector with same length as input, `None` for missing keys.
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("k1", b"v1".to_vec()).await?;
    /// kv.put("k2", b"v2".to_vec()).await?;
    ///
    /// let values = kv.multi_get(&["k1", "k2", "k3"]).await?;
    /// assert_eq!(values[0], Some(b"v1".to_vec()));
    /// assert_eq!(values[1], Some(b"v2".to_vec()));
    /// assert_eq!(values[2], None);
    /// # Ok(())
    /// # }
    /// ```
    async fn multi_get(&self, ctx: &RequestContext, keys: &[&str]) -> KVResult<Vec<Option<Vec<u8>>>>;

    /// Put multiple key-value pairs atomically (batch write).
    ///
    /// ## Atomicity
    /// Implementation-dependent. In-memory is atomic. SQL uses transactions.
    /// Redis uses pipelining (not atomic across keys).
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.multi_put(&[
    ///     ("k1", b"v1".to_vec()),
    ///     ("k2", b"v2".to_vec()),
    /// ]).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn multi_put(&self, ctx: &RequestContext, pairs: &[(&str, Vec<u8>)]) -> KVResult<()>;

    // =========================================================================
    // TTL Operations (Leases, Sessions, Metrics)
    // =========================================================================

    /// Put with time-to-live (automatic expiry).
    ///
    /// ## Behavior
    /// - Key automatically deleted after TTL expires
    /// - TTL countdown starts immediately
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put_with_ttl("session:abc", b"data".to_vec(), Duration::from_secs(60)).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn put_with_ttl(&self, ctx: &RequestContext, key: &str, value: Vec<u8>, ttl: Duration) -> KVResult<()>;

    /// Refresh TTL for existing key (lease renewal).
    ///
    /// ## Returns
    /// - `Ok(())` if TTL refreshed
    /// - `Err(KVError::KeyNotFound)` if key does not exist
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put_with_ttl("lease:db", b"node1".to_vec(), Duration::from_secs(30)).await?;
    /// kv.refresh_ttl("lease:db", Duration::from_secs(60)).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn refresh_ttl(&self, ctx: &RequestContext, key: &str, ttl: Duration) -> KVResult<()>;

    /// Get TTL remaining for key.
    ///
    /// ## Returns
    /// - `Ok(Some(duration))` if key exists with TTL
    /// - `Ok(None)` if key does not exist or has no TTL
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # use std::time::Duration;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put_with_ttl("key", b"val".to_vec(), Duration::from_secs(60)).await?;
    /// let ttl = kv.get_ttl("key").await?;
    /// assert!(ttl.is_some());
    /// # Ok(())
    /// # }
    /// ```
    async fn get_ttl(&self, ctx: &RequestContext, key: &str) -> KVResult<Option<Duration>>;

    // =========================================================================
    // Atomic Operations (Leases, Distributed Locks)
    // =========================================================================

    /// Compare-and-swap: only put if current value matches expected.
    ///
    /// ## Arguments
    /// - `expected`: `None` means key must not exist, `Some(val)` means key must equal val
    ///
    /// ## Returns
    /// - `Ok(true)` if swap succeeded
    /// - `Ok(false)` if value didn't match (no change made)
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    ///
    /// // Acquire lock (only if not held)
    /// let acquired = kv.cas("lock:resource", None, b"node1".to_vec()).await?;
    /// assert!(acquired);
    ///
    /// // Try to acquire again (fails)
    /// let acquired2 = kv.cas("lock:resource", None, b"node2".to_vec()).await?;
    /// assert!(!acquired2);
    /// # Ok(())
    /// # }
    /// ```
    async fn cas(&self, ctx: &RequestContext, key: &str, expected: Option<Vec<u8>>, new_value: Vec<u8>)
        -> KVResult<bool>;

    /// Atomic increment (for counters/metrics).
    ///
    /// ## Behavior
    /// - Interprets value as i64 (stored as 8-byte big-endian)
    /// - Initializes to 0 if key does not exist
    ///
    /// ## Returns
    /// New value after increment
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// let count = kv.increment("metrics:requests", 1).await?;
    /// assert_eq!(count, 1);
    /// let count = kv.increment("metrics:requests", 5).await?;
    /// assert_eq!(count, 6);
    /// # Ok(())
    /// # }
    /// ```
    async fn increment(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64>;

    /// Atomic decrement (for counters/metrics).
    ///
    /// ## Behavior
    /// - Interprets value as i64 (stored as 8-byte big-endian)
    /// - Initializes to 0 if key does not exist
    ///
    /// ## Returns
    /// New value after decrement
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.increment("metrics:active", 10).await?;
    /// let count = kv.decrement("metrics:active", 3).await?;
    /// assert_eq!(count, 7);
    /// # Ok(())
    /// # }
    /// ```
    async fn decrement(&self, ctx: &RequestContext, key: &str, delta: i64) -> KVResult<i64>;

    // =========================================================================
    // Watch/Notification (Config hot reload, Feature flags)
    // =========================================================================

    /// Watch for changes to a key (returns channel of change events).
    ///
    /// ## Behavior
    /// - Channel receives `KVEvent` on every change (put, delete, expire)
    /// - Channel closed when store is dropped or watch is cancelled
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// let mut watcher = kv.watch("config:timeout").await?;
    ///
    /// // In separate task
    /// tokio::spawn(async move {
    ///     while let Some(event) = watcher.recv().await {
    ///         // Handle config change
    ///     }
    /// });
    /// # Ok(())
    /// # }
    /// ```
    async fn watch(&self, ctx: &RequestContext, key: &str) -> KVResult<Receiver<KVEvent>>;

    /// Watch for changes to all keys with prefix.
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// let mut watcher = kv.watch_prefix("config:actor.").await?;
    ///
    /// // Notified on any actor config change
    /// # Ok(())
    /// # }
    /// ```
    async fn watch_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<Receiver<KVEvent>>;

    // =========================================================================
    // Maintenance Operations (Cleanup, Snapshots)
    // =========================================================================

    /// Clear all keys with prefix (namespace cleanup).
    ///
    /// ## Returns
    /// Number of keys deleted
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("temp:1", b"a".to_vec()).await?;
    /// kv.put("temp:2", b"b".to_vec()).await?;
    /// let deleted = kv.clear_prefix("temp:").await?;
    /// assert_eq!(deleted, 2);
    /// # Ok(())
    /// # }
    /// ```
    async fn clear_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize>;

    /// Count keys with prefix.
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// kv.put("actor:a", b"1".to_vec()).await?;
    /// kv.put("actor:b", b"2".to_vec()).await?;
    /// let count = kv.count_prefix("actor:").await?;
    /// assert_eq!(count, 2);
    /// # Ok(())
    /// # }
    /// ```
    async fn count_prefix(&self, ctx: &RequestContext, prefix: &str) -> KVResult<usize>;

    /// Get storage statistics.
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_keyvalue::{KeyValueStore, InMemoryKVStore};
    /// # use plexspaces_core::RequestContext;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let kv = InMemoryKVStore::new();
    /// let ctx = RequestContext::new("tenant".to_string()).with_namespace("default".to_string());
    /// let stats = kv.get_stats(&ctx).await?;
    /// assert_eq!(stats.backend_type, "InMemory");
    /// # Ok(())
    /// # }
    /// ```
    async fn get_stats(&self, ctx: &RequestContext) -> KVResult<KVStats>;
}

/// Event emitted by watch operations.
///
/// ## Purpose
/// Notifies subscribers of changes to watched keys for hot reload scenarios.
///
/// ## Fields
/// - `key`: The key that changed
/// - `event_type`: Type of change (Put, Delete, Expire)
/// - `old_value`: Previous value (if any)
/// - `new_value`: New value (if any)
#[derive(Debug, Clone)]
pub struct KVEvent {
    /// Key that changed
    pub key: String,
    /// Type of change
    pub event_type: KVEventType,
    /// Previous value (None if key was created)
    pub old_value: Option<Vec<u8>>,
    /// New value (None if key was deleted)
    pub new_value: Option<Vec<u8>>,
}

/// Type of key-value change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KVEventType {
    /// Key created or updated
    Put,
    /// Key deleted
    Delete,
    /// Key expired (TTL)
    Expire,
}

/// Storage statistics.
///
/// ## Purpose
/// Provides observability into storage usage and performance.
#[derive(Debug, Clone)]
pub struct KVStats {
    /// Total number of keys in storage
    pub total_keys: usize,
    /// Total size in bytes (approximate)
    pub total_size_bytes: usize,
    /// Backend type (e.g., "InMemory", "SQLite", "Redis")
    pub backend_type: String,
}
