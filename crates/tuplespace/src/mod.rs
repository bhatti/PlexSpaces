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

//! TupleSpace module for universal coordination
//!
//! Based on Linda coordination language but elevated to serve as:
//! - Configuration management
//! - Service discovery
//! - State coordination
//! - Event bus
//! - Workflow coordination

// lattice_space is declared in lib.rs

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// A tuple in the space
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tuple {
    /// Tuple fields
    fields: Vec<TupleField>,
    /// Optional lease (for automatic expiry)
    lease: Option<Lease>,
    /// Metadata
    metadata: HashMap<String, String>,
}

impl std::hash::Hash for Tuple {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.fields.hash(state);
        self.lease.hash(state);
        // Don't hash metadata as it's not part of equality
    }
}

impl PartialOrd for Tuple {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Tuple {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fields
            .cmp(&other.fields)
            .then_with(|| self.lease.cmp(&other.lease))
    }
}

impl Tuple {
    /// Create a new tuple from fields
    pub fn new(fields: Vec<TupleField>) -> Self {
        Tuple {
            fields,
            lease: None,
            metadata: HashMap::new(),
        }
    }

    /// Get the fields of the tuple
    pub fn fields(&self) -> &[TupleField] {
        &self.fields
    }

    /// Get the lease of the tuple
    pub fn lease(&self) -> Option<&Lease> {
        self.lease.as_ref()
    }

    /// Create a tuple with a lease
    pub fn with_lease(mut self, lease: Lease) -> Self {
        self.lease = Some(lease);
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Check if tuple has expired
    pub fn is_expired(&self) -> bool {
        if let Some(lease) = &self.lease {
            lease.is_expired()
        } else {
            false
        }
    }

    /// Check if tuple matches a pattern
    pub fn matches(&self, pattern: &Pattern) -> bool {
        if self.fields.len() != pattern.fields.len() {
            return false;
        }

        for (field, pattern_field) in self.fields.iter().zip(pattern.fields.iter()) {
            if !pattern_field.matches(field) {
                return false;
            }
        }

        true
    }
}

/// Field in a tuple
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub enum TupleField {
    /// Integer value
    Integer(i64),
    /// String value
    String(String),
    /// Boolean value
    Boolean(bool),
    /// Binary data
    Binary(Vec<u8>),
    /// Floating point
    Float(OrderedFloat),
    /// Null value
    Null,
}

/// Ordered float for hashing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedFloat(f64);

impl OrderedFloat {
    /// Create a new OrderedFloat from a float value
    pub fn new(value: f64) -> Self {
        OrderedFloat(value)
    }

    /// Get the inner float value
    pub fn get(&self) -> f64 {
        self.0
    }
}

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for OrderedFloat {}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> Ordering {
        // Handle NaN consistently
        match (self.0.is_nan(), other.0.is_nan()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater, // NaN > numbers
            (false, true) => Ordering::Less,
            (false, false) => self.0.partial_cmp(&other.0).unwrap(),
        }
    }
}

impl std::hash::Hash for OrderedFloat {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

/// Pattern for matching tuples
#[derive(Debug, Clone)]
pub struct Pattern {
    /// Pattern fields
    fields: Vec<PatternField>,
}

impl Pattern {
    /// Create a new pattern
    pub fn new(fields: Vec<PatternField>) -> Self {
        Pattern { fields }
    }

    /// Check if a tuple matches this pattern
    pub fn matches(&self, tuple: &Tuple) -> bool {
        if self.fields.len() != tuple.fields.len() {
            return false;
        }

        for (pattern_field, tuple_field) in self.fields.iter().zip(tuple.fields.iter()) {
            if !pattern_field.matches(tuple_field) {
                return false;
            }
        }

        true
    }
}

/// Field in a pattern
#[derive(Clone)]
pub enum PatternField {
    /// Exact match
    Exact(TupleField),
    /// Wildcard (matches any value)
    Wildcard,
    /// Type constraint
    Type(FieldType),
    /// Predicate function (cannot be serialized)
    Predicate(Arc<dyn Fn(&TupleField) -> bool + Send + Sync>),
}

impl std::fmt::Debug for PatternField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PatternField::Exact(field) => f.debug_tuple("Exact").field(field).finish(),
            PatternField::Wildcard => f.debug_tuple("Wildcard").finish(),
            PatternField::Type(ty) => f.debug_tuple("Type").field(ty).finish(),
            PatternField::Predicate(_) => f.debug_tuple("Predicate").field(&"<function>").finish(),
        }
    }
}

impl PatternField {
    /// Check if a field matches this pattern
    fn matches(&self, field: &TupleField) -> bool {
        match self {
            PatternField::Exact(expected) => field == expected,
            PatternField::Wildcard => true,
            PatternField::Type(field_type) => matches_type(field, field_type),
            PatternField::Predicate(pred) => pred(field),
        }
    }
}

/// Field type for pattern matching
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum FieldType {
    /// Integer type
    Integer,
    /// String type
    String,
    /// Boolean type
    Boolean,
    /// Binary data type
    Binary,
    /// Floating point type
    Float,
    /// Null/empty type
    Null,
}

fn matches_type(field: &TupleField, field_type: &FieldType) -> bool {
    matches!(
        (field, field_type),
        (TupleField::Integer(_), FieldType::Integer)
            | (TupleField::String(_), FieldType::String)
            | (TupleField::Boolean(_), FieldType::Boolean)
            | (TupleField::Binary(_), FieldType::Binary)
            | (TupleField::Float(_), FieldType::Float)
            | (TupleField::Null, FieldType::Null)
    )
}

/// Lease for automatic expiry
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
pub struct Lease {
    /// Expiry time
    expires_at: DateTime<Utc>,
    /// Owner of the lease
    owner: Option<String>,
    /// Whether lease can be renewed
    renewable: bool,
}

impl Lease {
    /// Create a new lease with duration
    pub fn new(duration: Duration) -> Self {
        Lease {
            expires_at: Utc::now() + duration,
            owner: None,
            renewable: false,
        }
    }

    /// Create a renewable lease
    pub fn renewable(mut self) -> Self {
        self.renewable = true;
        self
    }

    /// Set lease owner
    pub fn with_owner(mut self, owner: String) -> Self {
        self.owner = Some(owner);
        self
    }

    /// Check if lease has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Check if lease is renewable
    pub fn is_renewable(&self) -> bool {
        self.renewable
    }

    /// Get expiration time
    pub fn expires_at(&self) -> DateTime<Utc> {
        self.expires_at
    }

    /// Get lease owner
    pub fn owner(&self) -> Option<&String> {
        self.owner.as_ref()
    }

    /// Renew the lease
    pub fn renew(&mut self, duration: Duration) -> Result<(), String> {
        if !self.renewable {
            return Err("Lease is not renewable".to_string());
        }
        self.expires_at = Utc::now() + duration;
        Ok(())
    }

    /// Get time-to-live in seconds (for Redis TTL)
    pub fn ttl_seconds(&self) -> usize {
        let now = Utc::now();
        if self.expires_at > now {
            let duration = self.expires_at - now;
            // Round up to nearest second to handle millisecond durations
            // If duration is 500ms, we want TTL of 1 second, not 0
            let seconds = duration.num_seconds();
            let millis = duration.num_milliseconds() % 1000;
            if millis > 0 && seconds >= 0 {
                (seconds + 1).max(0) as usize
            } else {
                seconds.max(0) as usize
            }
        } else {
            0
        }
    }
}

/// Storage backend (optional - if None, uses in-memory storage)
enum TupleSpaceBackend {
    /// In-memory storage (default)
    InMemory { tuples: Arc<RwLock<Vec<Tuple>>> },
    /// Pluggable storage backend (SQL, Redis, etc.)
    External(Arc<dyn crate::storage::TupleSpaceStorage>),
}

/// TupleSpace for coordination
pub struct TupleSpace {
    /// Storage backend (in-memory or external)
    storage: TupleSpaceBackend,
    /// Watchers for pattern changes
    watchers: Arc<RwLock<HashMap<String, Watcher>>>,
    /// Barriers for synchronization
    barriers: Arc<RwLock<HashMap<String, Barrier>>>,
    /// Statistics
    stats: Arc<RwLock<TupleSpaceStats>>,
    /// Tenant this TupleSpace belongs to (for multi-tenancy)
    tenant: String,
    /// Namespace within the tenant (for environment isolation)
    namespace: String,
    /// Capabilities of this TupleSpace provider
    capabilities: HashMap<String, String>,
}

/// Watcher for tuple changes
struct Watcher {
    pattern: Pattern,
    sender: mpsc::Sender<WatchEvent>,
}

/// Watch event
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// Tuple added matching pattern
    Added(Tuple),
    /// Tuple removed matching pattern
    Removed(Tuple),
}

/// Barrier for synchronization
struct Barrier {
    pattern: Pattern,
    count: usize,
    waiters: Vec<mpsc::Sender<()>>,
}

/// Statistics
#[derive(Debug, Default, Clone)]
pub struct TupleSpaceStats {
    total_writes: u64,
    total_reads: u64,
    total_takes: u64,
    current_size: usize,
}

impl TupleSpaceStats {
    /// Get total number of write operations
    pub fn total_writes(&self) -> u64 {
        self.total_writes
    }

    /// Get total number of read operations
    pub fn total_reads(&self) -> u64 {
        self.total_reads
    }

    /// Get total number of take operations
    pub fn total_takes(&self) -> u64 {
        self.total_takes
    }

    /// Get current number of tuples in the space
    pub fn current_size(&self) -> usize {
        self.current_size
    }
}

impl Default for TupleSpace {
    fn default() -> Self {
        // Use internal tenant/namespace for default (system operations)
        // Note: Can't use RequestContext here due to circular dependency (core -> tuplespace)
        Self::with_tenant_namespace("internal", "system")
    }
}

impl TupleSpace {
    /// Create a new in-memory tuple space with RequestContext
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// Create a new in-memory tuple space with specific tenant and namespace
    ///
    /// ## Arguments
    /// * `tenant` - Tenant identifier (required, no defaults)
    /// * `namespace` - Namespace identifier (required, no defaults)
    ///
    /// ## Returns
    /// New TupleSpace instance
    pub fn with_tenant_namespace(tenant: &str, namespace: &str) -> Self {
        let mut capabilities = HashMap::new();
        capabilities.insert("storage".to_string(), "memory".to_string());
        capabilities.insert("storage.persistent".to_string(), "false".to_string());
        capabilities.insert("storage.distributed".to_string(), "false".to_string());
        capabilities.insert("replication".to_string(), "none".to_string());
        capabilities.insert("barriers".to_string(), "enabled".to_string());
        capabilities.insert("leases".to_string(), "enabled".to_string());

        let tenant_str = tenant.to_string();
        let namespace_str = namespace.to_string();

        TupleSpace {
            storage: TupleSpaceBackend::InMemory {
                tuples: Arc::new(RwLock::new(Vec::new())),
            },
            watchers: Arc::new(RwLock::new(HashMap::new())),
            barriers: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(TupleSpaceStats::default())),
            tenant: tenant.to_string(),
            namespace: namespace.to_string(),
            capabilities,
        }
    }

    /// Create a TupleSpace with external storage backend (SQL, Redis, etc.)
    ///
    /// ## Purpose
    /// Creates a TupleSpace backed by an external storage implementation (SQL, Redis).
    /// This enables distributed TupleSpace across multiple processes/nodes.
    ///
    /// ## Arguments
    /// * `storage` - Storage backend implementing TupleSpaceStorage trait
    ///
    /// ## Returns
    /// TupleSpace instance using the provided storage
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_tuplespace::*;
    /// # use plexspaces_tuplespace::storage::sql::*;
    /// # async fn example() -> Result<(), TupleSpaceError> {
    /// // SQLite storage
    /// let storage = SqlStorage::new_sqlite(SqliteStorageConfig {
    ///     database_path: "/tmp/tuples.db".to_string(),
    ///     pool_size: 1,
    ///     table_name: "tuples".to_string(),
    /// }).await?;
    /// let tuplespace = TupleSpace::with_storage(Box::new(storage));
    /// # Ok(())
    /// # }
    /// ```
    /// Create a TupleSpace with external storage and specific tenant/namespace
    ///
    /// ## Arguments
    /// * `storage` - Storage backend implementation
    /// * `tenant` - Tenant identifier (required, no defaults)
    /// * `namespace` - Namespace identifier (required, no defaults)
    ///
    /// ## Returns
    /// New TupleSpace instance with the given storage backend
    pub fn with_storage_and_tenant(
        storage: Box<dyn crate::storage::TupleSpaceStorage>,
        tenant: &str,
        namespace: &str,
    ) -> Self {
        // Determine capabilities based on storage type
        let mut capabilities = HashMap::new();

        // Default capabilities for external storage
        // TODO: These should come from the storage implementation
        capabilities.insert("storage".to_string(), "external".to_string());
        capabilities.insert("storage.persistent".to_string(), "true".to_string());
        capabilities.insert("storage.distributed".to_string(), "false".to_string());
        capabilities.insert("replication".to_string(), "none".to_string());
        capabilities.insert("barriers".to_string(), "enabled".to_string());
        capabilities.insert("leases".to_string(), "enabled".to_string());

        let storage_arc: Arc<dyn crate::storage::TupleSpaceStorage> = Arc::from(storage);
        let watchers: Arc<RwLock<HashMap<String, Watcher>>> = Arc::new(RwLock::new(HashMap::new()));
        let namespace_str = namespace.to_string();

        // Initialize distributed watch subscription if supported
        let storage_clone = storage_arc.clone();
        let watchers_clone = watchers.clone();
        let namespace_str_clone = namespace_str.clone();
        tokio::spawn(async move {
            match storage_clone.subscribe_watch_events(&namespace_str_clone).await {
                Ok(mut receiver) => {
                    tracing::debug!("Subscribed to watch events for namespace: {}", namespace_str_clone);
                    // Forward distributed events to local watchers
                    while let Some(event) = receiver.recv().await {
                        let watchers_guard = watchers_clone.read().await;
                        for watcher in watchers_guard.values() {
                            // Convert WatchEventMessage to WatchEvent and check pattern match
                            match event.event_type.as_str() {
                                "Added" => {
                                    if event.tuple.matches(&watcher.pattern) {
                                        let _ = watcher
                                            .sender
                                            .send(WatchEvent::Added(event.tuple.clone()))
                                            .await;
                                    }
                                }
                                "Removed" => {
                                    if event.tuple.matches(&watcher.pattern) {
                                        let _ = watcher
                                            .sender
                                            .send(WatchEvent::Removed(event.tuple.clone()))
                                            .await;
                                    }
                                }
                                _ => {
                                    tracing::warn!("Unknown watch event type: {}", event.event_type);
                                }
                            }
                        }
                    }
                }
                Err(TupleSpaceError::NotSupported(_)) => {
                    // Storage backend doesn't support subscriptions (e.g., SQLite, or PostgreSQL not yet implemented)
                    tracing::debug!("Watch event subscription not supported for this storage backend");
                }
                Err(e) => {
                    tracing::error!("Failed to subscribe to watch events: {}", e);
                }
            }
        });

        TupleSpace {
            storage: TupleSpaceBackend::External(storage_arc),
            watchers,
            barriers: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(TupleSpaceStats::default())),
            tenant: tenant.to_string(),
            namespace: namespace_str,
            capabilities,
        }
    }

    /// Write a tuple to the space
    pub async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        // Write to storage
        self.write_to_storage(&tuple).await?;

        // Notify watchers and check barriers
        self.notify_watchers_and_barriers(&tuple).await;

        Ok(())
    }

    /// Read a tuple (non-destructive)
    pub async fn read(&self, pattern: Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let tuples_guard = tuples.read().await;
                let mut stats = self.stats.write().await;

                stats.total_reads += 1;

                // Find first matching tuple
                for tuple in tuples_guard.iter() {
                    if !tuple.is_expired() && tuple.matches(&pattern) {
                        return Ok(Some(tuple.clone()));
                    }
                }

                Ok(None)
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;
                stats.total_reads += 1;

                // Read from external storage (returns Vec)
                let tuples = storage.read(pattern, None).await?;
                Ok(tuples.into_iter().next())
            }
        }
    }

    /// Take a tuple (destructive read)
    pub async fn take(&self, pattern: Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        // Take from storage using internal helper
        let tuple = self.take_from_storage(&pattern).await?;

        // Notify watchers if tuple was taken
        if let Some(ref t) = tuple {
            self.notify_watchers_on_removal(t).await;
        }

        Ok(tuple)
    }

    /// Read all matching tuples
    pub async fn read_all(&self, pattern: Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let tuples_guard = tuples.read().await;
                let mut stats = self.stats.write().await;

                stats.total_reads += 1;

                let matching: Vec<Tuple> = tuples_guard
                    .iter()
                    .filter(|t| !t.is_expired() && t.matches(&pattern))
                    .cloned()
                    .collect();

                Ok(matching)
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;
                stats.total_reads += 1;

                // Read all from external storage
                storage.read(pattern, None).await
            }
        }
    }

    /// Take all matching tuples
    pub async fn take_all(&self, pattern: Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let mut tuples_guard = tuples.write().await;
                let mut stats = self.stats.write().await;

                // Remove expired tuples
                tuples_guard.retain(|t| !t.is_expired());

                let mut taken = Vec::new();
                let mut remaining = Vec::new();

                for tuple in tuples_guard.drain(..) {
                    if tuple.matches(&pattern) {
                        taken.push(tuple.clone());

                        // Notify watchers
                        let watchers = self.watchers.read().await;
                        for watcher in watchers.values() {
                            if tuple.matches(&watcher.pattern) {
                                let _ = watcher
                                    .sender
                                    .send(WatchEvent::Removed(tuple.clone()))
                                    .await;
                            }
                        }
                    } else {
                        remaining.push(tuple);
                    }
                }

                *tuples_guard = remaining;
                stats.total_takes += taken.len() as u64;
                stats.current_size = tuples_guard.len();

                Ok(taken)
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;

                // Take all from external storage
                let taken = storage.take(pattern.clone(), None).await?;
                stats.total_takes += taken.len() as u64;

                // Notify watchers
                let watchers = self.watchers.read().await;
                for tuple in &taken {
                    for watcher in watchers.values() {
                        if tuple.matches(&watcher.pattern) {
                            let _ = watcher
                                .sender
                                .send(WatchEvent::Removed(tuple.clone()))
                                .await;
                        }
                    }
                }

                Ok(taken)
            }
        }
    }

    /// Evaluate a function on matching tuples (like eval in Linda)
    pub async fn eval<F, R>(&self, pattern: Pattern, f: F) -> Result<Vec<R>, TupleSpaceError>
    where
        F: Fn(&Tuple) -> R + Send,
        R: Send,
    {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let tuples_guard = tuples.read().await;

                let results: Vec<R> = tuples_guard
                    .iter()
                    .filter(|t| !t.is_expired() && t.matches(&pattern))
                    .map(f)
                    .collect();

                Ok(results)
            }
            TupleSpaceBackend::External(storage) => {
                // Read all matching tuples from external storage
                let tuples = storage.read(pattern, None).await?;

                let results: Vec<R> = tuples.iter().map(f).collect();

                Ok(results)
            }
        }
    }

    /// Watch for changes matching a pattern
    pub async fn watch(&self, pattern: Pattern) -> mpsc::Receiver<WatchEvent> {
        let (tx, rx) = mpsc::channel(100);

        let watcher = Watcher {
            pattern,
            sender: tx,
        };

        let id = ulid::Ulid::new().to_string();
        self.watchers.write().await.insert(id, watcher);

        rx
    }

    /// Create a barrier for synchronization
    pub async fn barrier(
        &self,
        name: String,
        pattern: Pattern,
        count: usize,
    ) -> mpsc::Receiver<()> {
        let (tx, rx) = mpsc::channel(1);

        let mut barriers = self.barriers.write().await;

        let barrier = barriers.entry(name).or_insert_with(|| Barrier {
            pattern,
            count,
            waiters: Vec::new(),
        });

        // Update count if changed (supports dynamic participant counts across iterations)
        barrier.count = count;

        barrier.waiters.push(tx);

        rx
    }

    /// Bulk write tuples
    pub async fn write_batch(&self, tuples: Vec<Tuple>) -> Result<(), TupleSpaceError> {
        for tuple in tuples {
            self.write(tuple).await?;
        }
        Ok(())
    }

    /// Count tuples matching pattern
    pub async fn count(&self, pattern: Pattern) -> Result<usize, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let tuples_guard = tuples.read().await;
                let count = tuples_guard
                    .iter()
                    .filter(|t| !t.is_expired() && t.matches(&pattern))
                    .count();
                Ok(count)
            }
            TupleSpaceBackend::External(storage) => storage.count(pattern).await,
        }
    }

    /// Check if any tuples match pattern
    pub async fn exists(&self, pattern: Pattern) -> Result<bool, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let tuples_guard = tuples.read().await;
                let exists = tuples_guard
                    .iter()
                    .any(|t| !t.is_expired() && t.matches(&pattern));
                Ok(exists)
            }
            TupleSpaceBackend::External(storage) => storage.exists(pattern).await,
        }
    }

    /// Get space statistics
    pub async fn stats(&self) -> TupleSpaceStats {
        let guard = self.stats.read().await;
        guard.clone()
    }

    /// Clear all tuples
    pub async fn clear(&self) {
        self.clear_internal().await
    }

    /// Internal helper for clearing tuples (used by both public method and trait)
    async fn clear_internal(&self) {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let mut tuples_guard = tuples.write().await;
                let mut stats = self.stats.write().await;

                tuples_guard.clear();
                stats.current_size = 0;
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;

                // Clear external storage (ignoring errors for now)
                let _ = storage.clear().await;
                stats.current_size = 0;
            }
        }
    }

    /// Internal helper for writing tuple to storage (used by both public method and trait)
    async fn write_to_storage(&self, tuple: &Tuple) -> Result<(), TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let mut tuples_guard = tuples.write().await;
                let mut stats = self.stats.write().await;

                // Remove expired tuples
                tuples_guard.retain(|t| !t.is_expired());

                // Add new tuple
                tuples_guard.push(tuple.clone());
                stats.total_writes += 1;
                stats.current_size = tuples_guard.len();
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;

                // Write to external storage
                storage.write(tuple.clone()).await?;
                stats.total_writes += 1;
            }
        }
        Ok(())
    }

    /// Internal helper for notifying watchers and checking barriers (used by both public method and trait)
    async fn notify_watchers_and_barriers(&self, tuple: &Tuple) {
        // Notify watchers (LOCAL - same node)
        let watchers = self.watchers.read().await;
        for watcher in watchers.values() {
            if tuple.matches(&watcher.pattern) {
                let _ = watcher.sender.send(WatchEvent::Added(tuple.clone())).await;
            }
        }

        // Publish watch event to distributed storage (Redis PUBSUB, PostgreSQL NOTIFY)
        if let TupleSpaceBackend::External(storage) = &self.storage {
            let _ = storage
                .publish_watch_event("Added", tuple, &self.namespace)
                .await;
        }

        // Check barriers
        let mut barriers = self.barriers.write().await;
        for (_name, barrier) in barriers.iter_mut() {
            if tuple.matches(&barrier.pattern) && barrier.waiters.len() >= barrier.count {
                // Barrier complete, notify all waiters
                for waiter in barrier.waiters.drain(..) {
                    let _ = waiter.send(()).await;
                }
                // Auto-reset: barrier.waiters is already empty from drain()
                // Barrier remains in place for next round
            }
        }
    }

    /// Internal helper for taking tuple from storage (used by both public method and trait)
    async fn take_from_storage(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let mut tuples_guard = tuples.write().await;
                let mut stats = self.stats.write().await;

                // Remove expired tuples
                tuples_guard.retain(|t| !t.is_expired());

                // Find and remove first matching tuple
                let position = tuples_guard.iter().position(|t| t.matches(pattern));

                if let Some(pos) = position {
                    let tuple = tuples_guard.remove(pos);
                    stats.total_takes += 1;
                    stats.current_size = tuples_guard.len();
                    Ok(Some(tuple))
                } else {
                    Ok(None)
                }
            }
            TupleSpaceBackend::External(storage) => {
                let mut stats = self.stats.write().await;

                // Take from external storage (returns Vec)
                let mut tuples = storage.take(pattern.clone(), None).await?;
                let tuple = tuples.pop();

                if let Some(ref t) = tuple {
                    stats.total_takes += 1;
                }

                Ok(tuple)
            }
        }
    }

    /// Internal helper for notifying watchers on tuple removal (used by both public method and trait)
    async fn notify_watchers_on_removal(&self, tuple: &Tuple) {
        // Notify watchers (LOCAL - same node)
        let watchers = self.watchers.read().await;
        for watcher in watchers.values() {
            if tuple.matches(&watcher.pattern) {
                let _ = watcher
                    .sender
                    .send(WatchEvent::Removed(tuple.clone()))
                    .await;
            }
        }

        // Publish watch event to distributed storage (Redis PUBSUB, PostgreSQL NOTIFY)
        if let TupleSpaceBackend::External(storage) = &self.storage {
            let _ = storage
                .publish_watch_event("Removed", tuple, &self.namespace)
                .await;
        }
    }
}

/// TupleSpace errors
#[derive(Debug, thiserror::Error)]
pub enum TupleSpaceError {
    /// Tuple not found in the tuplespace
    #[error("Tuple not found")]
    NotFound,

    /// Pattern matching error
    #[error("Pattern error: {0}")]
    PatternError(String),

    /// Lease-related error (expiry, renewal, etc.)
    #[error("Lease error: {0}")]
    LeaseError(String),

    /// I/O error during persistence operations
    #[error("IO error: {0}")]
    IoError(String),

    /// Backend storage error (Redis, etc.)
    #[error("Backend error: {0}")]
    BackendError(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Feature not supported by storage backend
    #[error("Not supported: {0}")]
    NotSupported(String),

    /// Feature not yet implemented
    #[error("Not implemented: {0}")]
    NotImplemented(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfiguration(String),

    /// Connection error (Redis, PostgreSQL, etc.)
    #[error("Connection error: {0}")]
    ConnectionError(String),
}

/// Helper macro for creating tuples from values
///
/// # Examples
/// ```ignore
/// # use plexspaces_tuplespace::{tuple, TupleField};
/// let t = tuple!(1, "hello", true);
/// ```
#[macro_export]
macro_rules! tuple {
    ($($field:expr),*) => {
        Tuple::new(vec![$(TupleField::from($field)),*])
    };
}

/// Helper macro for creating patterns
///
/// # Examples
/// ```ignore
/// # use plexspaces_tuplespace::{pattern, PatternField};
/// let p = pattern!(1, _, "test");  // Matches tuples with specific first/third fields
/// ```
#[macro_export]
macro_rules! pattern {
    ($($field:expr),*) => {
        Pattern::new(vec![$(PatternField::from($field)),*])
    };
}

// Conversion traits
impl From<i64> for TupleField {
    fn from(val: i64) -> Self {
        TupleField::Integer(val)
    }
}

impl From<String> for TupleField {
    fn from(val: String) -> Self {
        TupleField::String(val)
    }
}

impl From<&str> for TupleField {
    fn from(val: &str) -> Self {
        TupleField::String(val.to_string())
    }
}

impl From<bool> for TupleField {
    fn from(val: bool) -> Self {
        TupleField::Boolean(val)
    }
}

impl From<Vec<u8>> for TupleField {
    fn from(val: Vec<u8>) -> Self {
        TupleField::Binary(val)
    }
}

impl From<f64> for TupleField {
    fn from(val: f64) -> Self {
        TupleField::Float(OrderedFloat(val))
    }
}

// ============================================================================
// TupleSpaceProvider Trait Implementation
// ============================================================================

use crate::provider::TupleSpaceProvider;
use async_trait::async_trait;

#[async_trait]
impl TupleSpaceProvider for TupleSpace {
    fn capabilities(&self) -> HashMap<String, String> {
        self.capabilities.clone()
    }

    fn tenant(&self) -> &str {
        &self.tenant
    }

    fn namespace(&self) -> &str {
        &self.namespace
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        // Call read_all directly - it's a different method name, so no recursion
        self.read_all(pattern.clone()).await
    }

    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        // Use the internal helper methods to avoid recursion
        self.write_to_storage(&tuple).await?;
        self.notify_watchers_and_barriers(&tuple).await;
        Ok(())
    }

    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        // Take from storage using internal helper
        let tuple = self.take_from_storage(pattern).await?;

        // Notify watchers if tuple was taken
        if let Some(ref t) = tuple {
            self.notify_watchers_on_removal(t).await;
        }

        Ok(tuple)
    }

    async fn barrier(
        &self,
        barrier_id: &str,
        expected_count: usize,
    ) -> Result<(), TupleSpaceError> {
        // Check if barriers are supported
        if self.capabilities.get("barriers") != Some(&"enabled".to_string()) {
            return Err(TupleSpaceError::NotSupported(
                "Barriers not supported by this provider".to_string(),
            ));
        }

        // Use the existing barrier implementation
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("barrier".to_string())),
            PatternField::Exact(TupleField::String(barrier_id.to_string())),
        ]);

        let mut receiver = self
            .barrier(barrier_id.to_string(), pattern, expected_count)
            .await;

        // Wait for barrier to complete
        receiver.recv().await.ok_or_else(|| {
            TupleSpaceError::BackendError("Barrier channel closed unexpectedly".to_string())
        })?;

        Ok(())
    }

    async fn clear(&self) -> Result<(), TupleSpaceError> {
        // Use the internal helper method to avoid recursion
        self.clear_internal().await;
        Ok(())
    }

    async fn stats(&self) -> Result<TupleSpaceStats, TupleSpaceError> {
        // Get stats from the internal stats field (not the trait method to avoid recursion)
        let guard = self.stats.read().await;
        Ok(guard.clone())
    }

    async fn cleanup_expired(&self) -> Result<usize, TupleSpaceError> {
        // Check if leases are supported
        if self.capabilities.get("leases") != Some(&"enabled".to_string()) {
            return Ok(0);
        }

        // Count and remove expired tuples
        match &self.storage {
            TupleSpaceBackend::InMemory { tuples } => {
                let mut write_guard = tuples.write().await;
                let initial_count = write_guard.len();
                write_guard.retain(|t| !t.is_expired());
                let final_count = write_guard.len();
                Ok(initial_count - final_count)
            }
            TupleSpaceBackend::External(_storage) => {
                // For external storage, delegate to the storage implementation
                // For now, return 0 as we don't have a generic cleanup method
                Ok(0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::{PatternField, TupleField};
    use crate::provider::TupleSpaceProvider;

    #[tokio::test]
    async fn test_tuplespace_provider_trait() {
        // Test that TupleSpace implements TupleSpaceProvider correctly
        let space = TupleSpace::with_tenant_namespace("test-tenant", "test-namespace");

        // Check capabilities
        let caps = space.capabilities();
        assert_eq!(caps.get("storage"), Some(&"memory".to_string()));
        assert_eq!(caps.get("storage.persistent"), Some(&"false".to_string()));
        assert_eq!(caps.get("barriers"), Some(&"enabled".to_string()));
        assert_eq!(caps.get("leases"), Some(&"enabled".to_string()));

        // Check tenant/namespace
        assert_eq!(space.tenant(), "test-tenant");
        assert_eq!(space.namespace(), "test-namespace");

        // Test basic operations through provider trait
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);

        // Write through provider trait
        TupleSpaceProvider::write(&space, tuple.clone())
            .await
            .unwrap();

        // Read through provider trait
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let results = TupleSpaceProvider::read(&space, &pattern).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0], tuple);

        // Take through provider trait
        let taken = TupleSpaceProvider::take(&space, &pattern).await.unwrap();
        assert_eq!(taken, Some(tuple));

        // Verify it was removed
        let results2 = TupleSpaceProvider::read(&space, &pattern).await.unwrap();
        assert_eq!(results2.len(), 0);
    }

    #[tokio::test]
    async fn test_basic_operations() {
        let space = TupleSpace::default();

        // Write a tuple
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);
        space.write(tuple.clone()).await.unwrap();

        // Read it back
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let read = space.read(pattern.clone()).await.unwrap();
        assert!(read.is_some());

        // Take it
        let taken = space.take(pattern.clone()).await.unwrap();
        assert!(taken.is_some());

        // Should be gone now
        let read_again = space.read(pattern).await.unwrap();
        assert!(read_again.is_none());
    }

    #[tokio::test]
    async fn test_pattern_matching() {
        let space = TupleSpace::default();

        // Write multiple tuples
        space
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("timeout".to_string()),
                TupleField::Integer(30),
            ]))
            .await
            .unwrap();

        space
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("retries".to_string()),
                TupleField::Integer(3),
            ]))
            .await
            .unwrap();

        // Pattern with wildcard
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("config".to_string())),
            PatternField::Wildcard,
            PatternField::Type(FieldType::Integer),
        ]);

        let all = space.read_all(pattern).await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_lease_expiry() {
        let space = TupleSpace::default();

        // Write tuple with short lease
        let lease = Lease::new(Duration::milliseconds(100));
        let tuple = Tuple::new(vec![TupleField::String("temp".to_string())]).with_lease(lease);

        space.write(tuple).await.unwrap();

        // Should exist immediately
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "temp".to_string(),
        ))]);
        assert!(space.read(pattern.clone()).await.unwrap().is_some());

        // Wait for expiry
        tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

        // Should be expired now
        assert!(space.read(pattern).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_watch() {
        let space = TupleSpace::default();

        // Set up watcher
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("event".to_string())),
            PatternField::Wildcard,
        ]);
        let mut watcher = space.watch(pattern).await;

        // Write matching tuple
        let tuple = Tuple::new(vec![
            TupleField::String("event".to_string()),
            TupleField::String("data".to_string()),
        ]);
        space.write(tuple).await.unwrap();

        // Should receive event
        let event = watcher.recv().await.unwrap();
        match event {
            WatchEvent::Added(t) => {
                assert_eq!(t.fields[0], TupleField::String("event".to_string()));
            }
            _ => panic!("Expected Added event"),
        }
    }

    #[tokio::test]
    async fn test_configuration_as_coordination() {
        // Revolutionary idea from Orbit: config as coordination!
        let space = TupleSpace::default();

        // Write configuration as tuples
        space
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("actor.timeout".to_string()),
                TupleField::Integer(30),
            ]))
            .await
            .unwrap();

        // Read configuration
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("config".to_string())),
            PatternField::Exact(TupleField::String("actor.timeout".to_string())),
            PatternField::Wildcard,
        ]);

        let config = space.read(pattern).await.unwrap().unwrap();
        if let TupleField::Integer(timeout) = &config.fields[2] {
            assert_eq!(*timeout, 30);
        }
    }

    #[test]
    fn test_tuple_ordering() {
        // Test Ord and PartialOrd implementations (lines 58-59, 64-66)
        let tuple1 = Tuple::new(vec![TupleField::Integer(1)]);
        let tuple2 = Tuple::new(vec![TupleField::Integer(2)]);
        let tuple3 = Tuple::new(vec![TupleField::Integer(1)]);

        assert!(tuple1 < tuple2);
        assert!(tuple1 == tuple3);
        assert!(tuple2 > tuple1);
        assert_eq!(tuple1.partial_cmp(&tuple2), Some(Ordering::Less));
        assert_eq!(tuple1.cmp(&tuple3), Ordering::Equal);
    }

    #[test]
    fn test_tuple_with_metadata() {
        // Test with_metadata (lines 97-99)
        let tuple = Tuple::new(vec![TupleField::String("test".to_string())])
            .with_metadata("key1".to_string(), "value1".to_string())
            .with_metadata("key2".to_string(), "value2".to_string());

        assert_eq!(tuple.metadata.get("key1"), Some(&"value1".to_string()));
        assert_eq!(tuple.metadata.get("key2"), Some(&"value2".to_string()));
    }

    #[test]
    fn test_tuple_field_ordering() {
        // Test TupleField Ord and PartialOrd (lines 149-150, 157-158, 163, 165-169)
        let int1 = TupleField::Integer(1);
        let int2 = TupleField::Integer(2);
        let str1 = TupleField::String("a".to_string());
        let str2 = TupleField::String("b".to_string());
        let bool1 = TupleField::Boolean(false);
        let bool2 = TupleField::Boolean(true);

        // Integer ordering
        assert!(int1 < int2);
        assert_eq!(int1.partial_cmp(&int2), Some(Ordering::Less));

        // String ordering
        assert!(str1 < str2);
        assert_eq!(str1.cmp(&str2), Ordering::Less);

        // Boolean ordering
        assert!(bool1 < bool2);

        // Different types - discriminant ordering
        assert!(int1 < str1); // Integer discriminant < String discriminant
    }

    #[test]
    fn test_tuple_field_hash() {
        // Test TupleField Hash implementation (lines 175-176)
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(TupleField::Integer(1));
        set.insert(TupleField::String("test".to_string()));
        set.insert(TupleField::Boolean(true));

        assert!(set.contains(&TupleField::Integer(1)));
        assert!(set.contains(&TupleField::String("test".to_string())));
        assert!(set.contains(&TupleField::Boolean(true)));
        assert!(!set.contains(&TupleField::Integer(2)));
    }

    #[test]
    fn test_pattern_field_type_matching() {
        // Test PatternField::Type matching (lines 196, 201)
        let int_field = TupleField::Integer(42);
        let str_field = TupleField::String("test".to_string());
        let bool_field = TupleField::Boolean(true);
        let float_field = TupleField::Float(OrderedFloat(3.14));
        let binary_field = TupleField::Binary(vec![1, 2, 3]);

        let int_pattern = PatternField::Type(FieldType::Integer);
        let str_pattern = PatternField::Type(FieldType::String);
        let bool_pattern = PatternField::Type(FieldType::Boolean);
        let float_pattern = PatternField::Type(FieldType::Float);
        let binary_pattern = PatternField::Type(FieldType::Binary);

        assert!(int_pattern.matches(&int_field));
        assert!(!int_pattern.matches(&str_field));

        assert!(str_pattern.matches(&str_field));
        assert!(!str_pattern.matches(&int_field));

        assert!(bool_pattern.matches(&bool_field));
        assert!(float_pattern.matches(&float_field));
        assert!(binary_pattern.matches(&binary_field));
    }

    #[test]
    fn test_lease_ordering() {
        // Test Lease Ord and PartialOrd (lines 275-277)
        let lease1 = Lease::new(Duration::seconds(10));
        let lease2 = Lease::new(Duration::seconds(20));

        // lease2 expires later, so it should be greater than lease1
        assert!(lease1 < lease2);
        assert_eq!(lease1.partial_cmp(&lease2), Some(Ordering::Less));
        assert_eq!(lease1.cmp(&lease2), Ordering::Less);
    }

    #[test]
    fn test_lease_with_owner() {
        // Test Lease::with_owner (lines 302-304)
        let lease = Lease::new(Duration::seconds(10)).with_owner("actor-123".to_string());

        assert_eq!(lease.owner(), Some(&"actor-123".to_string()));
    }

    #[test]
    fn test_lease_renewal() {
        // Test Lease::renew (lines 323-325, 327-328)
        let mut lease = Lease::new(Duration::seconds(10)).renewable();
        let original_expires = lease.expires_at();

        // Should succeed for renewable lease
        assert!(lease.renew(Duration::seconds(20)).is_ok());
        assert!(lease.expires_at() > original_expires);

        // Test non-renewable lease
        let mut non_renewable = Lease::new(Duration::seconds(10));
        assert!(non_renewable.renew(Duration::seconds(20)).is_err());
    }

    #[test]
    fn test_lease_ttl_seconds() {
        // Test Lease::ttl_seconds (lines 343, 346)
        let lease = Lease::new(Duration::seconds(100));
        let ttl = lease.ttl_seconds();

        // TTL should be approximately 100 seconds (allow small timing difference)
        assert!(ttl >= 99 && ttl <= 100);
    }

    #[test]
    fn test_field_type_ordering() {
        // Test FieldType Ord and PartialOrd (lines 395-396)
        use FieldType::*;

        assert!(Integer < String);
        assert_eq!(Integer.partial_cmp(&String), Some(Ordering::Less));
        assert_eq!(String.cmp(&Boolean), Ordering::Less);
    }

    #[test]
    fn test_tuple_field_from_conversions() {
        // Test From implementations (lines 703-704, 715-716, 721-722, 727-728)
        let from_str: TupleField = "test".into();
        assert_eq!(from_str, TupleField::String("test".to_string()));

        let from_string: TupleField = String::from("hello").into();
        assert_eq!(from_string, TupleField::String("hello".to_string()));

        let from_bool: TupleField = true.into();
        assert_eq!(from_bool, TupleField::Boolean(true));

        let from_bytes: TupleField = vec![1u8, 2, 3].into();
        assert_eq!(from_bytes, TupleField::Binary(vec![1, 2, 3]));

        let from_float: TupleField = 3.14.into();
        assert_eq!(from_float, TupleField::Float(OrderedFloat(3.14)));

        let from_int: TupleField = 42i64.into();
        assert_eq!(from_int, TupleField::Integer(42));
    }

    #[tokio::test]
    async fn test_tuplespace_count() {
        // Test count operation (lines 436-437, 439-440, 442, 447)
        let space = TupleSpace::default();

        // Initially empty
        let pattern_all = Pattern::new(vec![PatternField::Wildcard]);
        assert_eq!(space.count(pattern_all.clone()).await.unwrap(), 0);

        // Write some tuples
        space
            .write(Tuple::new(vec![TupleField::Integer(1)]))
            .await
            .unwrap();
        space
            .write(Tuple::new(vec![TupleField::Integer(2)]))
            .await
            .unwrap();
        space
            .write(Tuple::new(vec![TupleField::Integer(3)]))
            .await
            .unwrap();

        // Count all
        assert_eq!(space.count(pattern_all).await.unwrap(), 3);

        // Count specific pattern
        let pattern_one = Pattern::new(vec![PatternField::Exact(TupleField::Integer(1))]);
        assert_eq!(space.count(pattern_one).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_tuplespace_exists() {
        // Test exists operation (lines 489-490, 496)
        let space = TupleSpace::default();

        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "test".to_string(),
        ))]);

        // Should not exist initially
        assert!(!space.exists(pattern.clone()).await.unwrap());

        // Write tuple
        space
            .write(Tuple::new(vec![TupleField::String("test".to_string())]))
            .await
            .unwrap();

        // Should exist now
        assert!(space.exists(pattern).await.unwrap());
    }

    #[tokio::test]
    async fn test_tuplespace_clear() {
        // Test clear operation (lines 517-519, 522, 524-525, 527-529, 532-535)
        let space = TupleSpace::default();

        // Write some tuples
        space
            .write(Tuple::new(vec![TupleField::Integer(1)]))
            .await
            .unwrap();
        space
            .write(Tuple::new(vec![TupleField::Integer(2)]))
            .await
            .unwrap();
        space
            .write(Tuple::new(vec![TupleField::Integer(3)]))
            .await
            .unwrap();

        let pattern_all = Pattern::new(vec![PatternField::Wildcard]);
        assert_eq!(space.count(pattern_all.clone()).await.unwrap(), 3);

        // Clear all
        space.clear().await;

        // Should be empty
        assert_eq!(space.count(pattern_all).await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_tuplespace_stats() {
        // Test stats operation (lines 539, 543-545, 547, 551, 556, 558, 560-561, 564)
        let space = TupleSpace::default();

        // Write some tuples
        for i in 0..5 {
            space
                .write(Tuple::new(vec![TupleField::Integer(i)]))
                .await
                .unwrap();
        }

        // Get stats
        let stats = space.stats().await;
        assert_eq!(stats.current_size, 5);
        assert_eq!(stats.total_writes, 5);
    }

    // NOTE: Transaction methods are only available on TupleSpaceStorage trait,
    // not on the TupleSpace struct itself. Transaction tests are in storage backend tests.

    #[test]
    fn test_pattern_field_exact_matching() {
        // Test PatternField::Exact matching edge cases (line 223-228)
        let field = TupleField::String("test".to_string());
        let pattern_exact = PatternField::Exact(TupleField::String("test".to_string()));
        let pattern_different = PatternField::Exact(TupleField::String("other".to_string()));

        assert!(pattern_exact.matches(&field));
        assert!(!pattern_different.matches(&field));
    }

    #[test]
    fn test_pattern_field_wildcard() {
        // Test PatternField::Wildcard always matches (line 240)
        let wildcard = PatternField::Wildcard;

        assert!(wildcard.matches(&TupleField::Integer(42)));
        assert!(wildcard.matches(&TupleField::String("test".to_string())));
        assert!(wildcard.matches(&TupleField::Boolean(true)));
        assert!(wildcard.matches(&TupleField::Float(OrderedFloat(3.14))));
        assert!(wildcard.matches(&TupleField::Binary(vec![1, 2, 3])));
    }

    #[test]
    fn test_tuple_matches_wrong_length() {
        // Test tuple matching with wrong pattern length (line 114)
        let tuple = Tuple::new(vec![
            TupleField::Integer(1),
            TupleField::String("test".to_string()),
        ]);

        let pattern_too_short = Pattern::new(vec![PatternField::Wildcard]);
        let pattern_too_long = Pattern::new(vec![
            PatternField::Wildcard,
            PatternField::Wildcard,
            PatternField::Wildcard,
        ]);

        assert!(!tuple.matches(&pattern_too_short));
        assert!(!tuple.matches(&pattern_too_long));
    }
}
