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

//! TupleSpaceProvider trait and capability system
//!
//! ## Purpose
//! Defines the provider abstraction that allows different TupleSpace implementations
//! (Redis, SQLite, InMemory, etc.) to declare their intrinsic capabilities and be
//! discovered via ObjectRegistry (replaces TupleSpaceRegistry).
//!
//! ## Design Principle
//! Unlike Actor Facets (which are additive/composable), TupleSpace capabilities are
//! **intrinsic properties** of the implementation. You cannot mix Redis + SQLite storage,
//! or have both eventual and strong consistency - these are mutually exclusive choices
//! determined by the provider implementation.
//!
//! ## Capabilities vs Facets
//! - **Actor Facets**: Additive (actor + mobility + metrics + tracing all on same actor)
//! - **TupleSpace Capabilities**: Intrinsic (provider IS Redis OR SQLite, not both)
//!
//! ## Multi-Tenancy
//! Each TupleSpace instance is scoped to a tenant + namespace:
//! - `tenant`: Organizational boundary (e.g., "company-a", "company-b")
//! - `namespace`: Environment/purpose (e.g., "production", "staging", "test")
//!
//! This allows:
//! - Tenant isolation (company-a cannot access company-b's tuples)
//! - Namespace isolation (production vs staging data separation)
//! - Resource quotas per tenant
//!
//! ## Example
//! ```rust
//! use plexspaces_tuplespace::provider::TupleSpaceProvider;
//! use std::collections::HashMap;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Provider declares its capabilities
//! let provider = create_redis_provider().await?;
//! let caps = provider.capabilities();
//!
//! assert_eq!(caps.get("storage"), Some(&"redis".to_string()));
//! assert_eq!(caps.get("storage.persistent"), Some(&"true".to_string()));
//! assert_eq!(caps.get("storage.distributed"), Some(&"true".to_string()));
//! assert_eq!(caps.get("replication"), Some(&"eventual".to_string()));
//! # Ok(())
//! # }
//! # async fn create_redis_provider() -> Result<Box<dyn TupleSpaceProvider>, Box<dyn std::error::Error>> {
//! #     unimplemented!()
//! # }
//! ```

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{Pattern, PatternField, Tuple, TupleSpaceError, TupleSpaceStats};

/// TupleSpaceProvider trait - abstraction over different TupleSpace implementations
///
/// ## Purpose
/// Allows different storage backends (Redis, SQLite, InMemory, PostgreSQL) to implement
/// the same TupleSpace interface while declaring their intrinsic capabilities.
///
/// ## Capabilities Map
/// The `capabilities()` method returns a HashMap describing what this provider can do:
///
/// ### Storage Capabilities
/// - `"storage"`: Backend type ("redis", "sqlite", "memory", "postgresql")
/// - `"storage.persistent"`: "true" or "false" (survives restarts)
/// - `"storage.distributed"`: "true" or "false" (multi-node)
///
/// ### Replication Capabilities
/// - `"replication"`: Strategy ("none", "eventual", "strong", "quorum")
/// - `"replication.factor"`: Number of replicas (e.g., "3")
///
/// ### Feature Capabilities
/// - `"barriers"`: "enabled" or "disabled" (N-actor synchronization)
/// - `"leases"`: "enabled" or "disabled" (TTL-based expiration)
/// - `"indexing"`: "none", "pattern", "spatial" (query optimization)
/// - `"transactions"`: "enabled" or "disabled" (atomic multi-tuple ops)
///
/// ## Example Implementations
/// ```rust
/// # use async_trait::async_trait;
/// # use std::collections::HashMap;
/// # use plexspaces_tuplespace::provider::TupleSpaceProvider;
/// # use plexspaces_tuplespace::{Tuple, Pattern, TupleSpaceError};
/// # struct RedisTupleSpace;
/// #[async_trait]
/// impl TupleSpaceProvider for RedisTupleSpace {
///     fn capabilities(&self) -> HashMap<String, String> {
///         let mut caps = HashMap::new();
///         caps.insert("storage".to_string(), "redis".to_string());
///         caps.insert("storage.persistent".to_string(), "true".to_string());
///         caps.insert("storage.distributed".to_string(), "true".to_string());
///         caps.insert("replication".to_string(), "eventual".to_string());
///         caps.insert("barriers".to_string(), "enabled".to_string());
///         caps.insert("leases".to_string(), "enabled".to_string());
///         caps
///     }
///
///     // ... implement read/write/take methods
/// #   async fn read(&self, _pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> { Ok(vec![]) }
/// #   async fn write(&self, _tuple: Tuple) -> Result<(), TupleSpaceError> { Ok(()) }
/// #   async fn take(&self, _pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> { Ok(None) }
/// #   fn tenant(&self) -> &str { "default" }
/// #   fn namespace(&self) -> &str { "default" }
/// }
/// ```
#[async_trait]
pub trait TupleSpaceProvider: Send + Sync {
    /// Returns the capabilities of this TupleSpace provider
    ///
    /// Capabilities are intrinsic properties that cannot be changed at runtime.
    /// Examples:
    /// - Storage backend type (redis, sqlite, memory)
    /// - Persistence (true/false)
    /// - Distribution (true/false)
    /// - Replication strategy (none, eventual, strong)
    /// - Feature support (barriers, leases, indexing)
    fn capabilities(&self) -> HashMap<String, String>;

    /// Tenant this TupleSpace instance belongs to
    ///
    /// Provides organizational isolation. Different tenants cannot access each other's tuples.
    /// Example: "company-a", "company-b", "user-123"
    fn tenant(&self) -> &str;

    /// Namespace within the tenant
    ///
    /// Provides environment/purpose isolation within a tenant.
    /// Example: "production", "staging", "test", "feature-branch-1"
    fn namespace(&self) -> &str;

    /// Read tuples matching pattern (non-blocking)
    ///
    /// Returns all tuples that match the given pattern without removing them.
    /// If no matches found, returns empty Vec.
    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError>;

    /// Write a tuple to the space
    ///
    /// Inserts a new tuple. If provider supports replication, tuple will be
    /// replicated according to the replication capability.
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError>;

    /// Take a tuple matching pattern (blocking with timeout)
    ///
    /// Atomically reads and removes the first tuple matching the pattern.
    /// Returns None if no match found within timeout.
    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError>;

    /// Count tuples matching pattern
    ///
    /// Returns the number of tuples matching the pattern without reading them.
    /// Useful for checking if tuples exist without transferring data.
    async fn count(&self, pattern: &Pattern) -> Result<usize, TupleSpaceError> {
        // Default implementation using read
        let tuples = self.read(pattern).await?;
        Ok(tuples.len())
    }

    /// Distributed barrier (if supported)
    ///
    /// Blocks until `expected_count` actors have reached this barrier.
    /// Only works if capabilities include `"barriers": "enabled"`.
    ///
    /// ## Errors
    /// Returns error if barriers not supported by this provider.
    async fn barrier(
        &self,
        barrier_id: &str,
        expected_count: usize,
    ) -> Result<(), TupleSpaceError> {
        let _ = (barrier_id, expected_count);
        Err(TupleSpaceError::NotSupported(
            "Barriers not supported by this provider".to_string(),
        ))
    }

    /// Clean up expired tuples (if leases supported)
    ///
    /// Removes tuples that have exceeded their TTL.
    /// Only works if capabilities include `"leases": "enabled"`.
    async fn cleanup_expired(&self) -> Result<usize, TupleSpaceError> {
        // Default: no-op (providers with lease support override this)
        Ok(0)
    }

    /// Clear all tuples from the space
    ///
    /// Removes all tuples. Use with caution in production.
    async fn clear(&self) -> Result<(), TupleSpaceError> {
        // Default implementation: use wildcard pattern to take all tuples
        // This is inefficient but works for all providers
        let wildcard_pattern = Pattern::new(vec![PatternField::Wildcard]);
        loop {
            match self.take(&wildcard_pattern).await? {
                Some(_) => {
                    // Continue taking until no more tuples
                }
                None => {
                    // No more tuples
                    break;
                }
            }
        }
        Ok(())
    }

    /// Get statistics about the TupleSpace
    ///
    /// Returns statistics including operation counts and current size.
    /// Default implementation returns basic stats.
    async fn stats(&self) -> Result<TupleSpaceStats, TupleSpaceError> {
        // Default implementation: return default stats
        // Providers should override this for accurate statistics
        Ok(TupleSpaceStats::default())
    }
}

/// Helper functions for working with capabilities
pub struct CapabilityHelpers;

impl CapabilityHelpers {
    /// Check if provider has a specific capability
    ///
    /// ## Example
    /// ```rust
    /// # use std::collections::HashMap;
    /// # use plexspaces_tuplespace::provider::CapabilityHelpers;
    /// let caps = HashMap::from([
    ///     ("storage".to_string(), "redis".to_string()),
    ///     ("barriers".to_string(), "enabled".to_string()),
    /// ]);
    ///
    /// assert!(CapabilityHelpers::has_capability(&caps, "barriers", "enabled"));
    /// assert!(!CapabilityHelpers::has_capability(&caps, "barriers", "disabled"));
    /// ```
    pub fn has_capability(caps: &HashMap<String, String>, key: &str, value: &str) -> bool {
        caps.get(key).map(|v| v == value).unwrap_or(false)
    }

    /// Check if provider is persistent
    pub fn is_persistent(caps: &HashMap<String, String>) -> bool {
        Self::has_capability(caps, "storage.persistent", "true")
    }

    /// Check if provider is distributed
    pub fn is_distributed(caps: &HashMap<String, String>) -> bool {
        Self::has_capability(caps, "storage.distributed", "true")
    }

    /// Check if provider supports barriers
    pub fn supports_barriers(caps: &HashMap<String, String>) -> bool {
        Self::has_capability(caps, "barriers", "enabled")
    }

    /// Check if provider supports leases (TTL)
    pub fn supports_leases(caps: &HashMap<String, String>) -> bool {
        Self::has_capability(caps, "leases", "enabled")
    }

    /// Get replication strategy
    pub fn replication_strategy(caps: &HashMap<String, String>) -> Option<&str> {
        caps.get("replication").map(|s| s.as_str())
    }

    /// Get storage backend type
    pub fn storage_type(caps: &HashMap<String, String>) -> Option<&str> {
        caps.get("storage").map(|s| s.as_str())
    }
}

/// Type alias for boxed providers
pub type BoxedProvider = Box<dyn TupleSpaceProvider>;

/// Type alias for Arc-wrapped providers (for sharing across threads)
pub type SharedProvider = Arc<dyn TupleSpaceProvider>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_helpers() {
        let caps = HashMap::from([
            ("storage".to_string(), "redis".to_string()),
            ("storage.persistent".to_string(), "true".to_string()),
            ("storage.distributed".to_string(), "true".to_string()),
            ("replication".to_string(), "eventual".to_string()),
            ("barriers".to_string(), "enabled".to_string()),
            ("leases".to_string(), "enabled".to_string()),
        ]);

        assert!(CapabilityHelpers::is_persistent(&caps));
        assert!(CapabilityHelpers::is_distributed(&caps));
        assert!(CapabilityHelpers::supports_barriers(&caps));
        assert!(CapabilityHelpers::supports_leases(&caps));
        assert_eq!(
            CapabilityHelpers::replication_strategy(&caps),
            Some("eventual")
        );
        assert_eq!(CapabilityHelpers::storage_type(&caps), Some("redis"));
    }

    #[test]
    fn test_in_memory_capabilities() {
        let caps = HashMap::from([
            ("storage".to_string(), "memory".to_string()),
            ("storage.persistent".to_string(), "false".to_string()),
            ("storage.distributed".to_string(), "false".to_string()),
            ("replication".to_string(), "none".to_string()),
            ("barriers".to_string(), "enabled".to_string()),
            ("leases".to_string(), "enabled".to_string()),
        ]);

        assert!(!CapabilityHelpers::is_persistent(&caps));
        assert!(!CapabilityHelpers::is_distributed(&caps));
        assert!(CapabilityHelpers::supports_barriers(&caps));
        assert_eq!(CapabilityHelpers::replication_strategy(&caps), Some("none"));
        assert_eq!(CapabilityHelpers::storage_type(&caps), Some("memory"));
    }
}
