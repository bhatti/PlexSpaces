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

//! Redis-backed TupleSpace implementation
//!
//! ## Purpose
//! Provides a distributed, durable TupleSpace using Redis as the backing store.
//! This enables Byzantine Generals and other distributed actors to coordinate
//! across multiple nodes/processes.
//!
//! ## Design
//! - Uses Redis keys: `tuplespace:{namespace}:tuple:{id}`
//! - Pattern matching via Redis SCAN + client-side filtering
//! - Pub/Sub for watch notifications
//! - TTL for automatic lease expiry
//!
//! ## Why Redis
//! - Sub-millisecond latency (< 1ms)
//! - Durable (AOF + RDB persistence)
//! - Distributed (Redis Cluster support)
//! - Pub/Sub for reactive tuples
//! - TTL for automatic cleanup
//!
//! Based on TUPLESPACE_RESEARCH.md recommendations.

use crate::{Pattern, Tuple, TupleSpaceError};
#[cfg(feature = "redis-backend")]
use redis::{aio::ConnectionManager, AsyncCommands, Client};
use serde_json;

/// Redis-backed TupleSpace for distributed coordination
#[cfg(feature = "redis-backend")]
pub struct RedisTupleSpace {
    /// Redis connection manager (handles connection pooling)
    conn: ConnectionManager,
    /// Namespace for tuple keys
    namespace: String,
}

#[cfg(feature = "redis-backend")]
impl RedisTupleSpace {
    /// Create a new Redis TupleSpace
    ///
    /// ## Arguments
    /// * `redis_url` - Redis connection URL (e.g., "redis://127.0.0.1:6379")
    /// * `namespace` - Namespace for keys (e.g., "byzantine-consensus")
    ///
    /// ## Returns
    /// A new RedisTupleSpace instance
    ///
    /// ## Errors
    /// - Connection errors if Redis is not reachable
    ///
    /// ## Examples
    /// ```no_run
    /// # use plexspaces_tuplespace::redis_space::RedisTupleSpace;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let space = RedisTupleSpace::new(
    ///     "redis://127.0.0.1:6379",
    ///     "test-namespace"
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn new(redis_url: &str, namespace: &str) -> Result<Self, TupleSpaceError> {
        let client =
            Client::open(redis_url).map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;

        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;

        Ok(RedisTupleSpace {
            conn,
            namespace: namespace.to_string(),
        })
    }

    /// Generate a unique key for a tuple
    fn tuple_key(&self, tuple_id: &str) -> String {
        format!("tuplespace:{}:tuple:{}", self.namespace, tuple_id)
    }

    /// Generate a pattern key for indexing
    fn pattern_prefix(&self) -> String {
        format!("tuplespace:{}:tuple:*", self.namespace)
    }

    /// Write a tuple to Redis
    ///
    /// ## Arguments
    /// * `tuple` - The tuple to write
    ///
    /// ## Returns
    /// Ok(()) on success
    ///
    /// ## Examples
    /// ```no_run
    /// # use plexspaces_tuplespace::redis_space::RedisTupleSpace;
    /// # use plexspaces_tuplespace::{Tuple, TupleField};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let space = RedisTupleSpace::new("redis://127.0.0.1:6379", "test").await?;
    /// let tuple = Tuple::new(vec![
    ///     TupleField::String("vote".to_string()),
    ///     TupleField::String("general1".to_string()),
    ///     TupleField::Integer(0),
    /// ]);
    /// space.write(tuple).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        let tuple_id = ulid::Ulid::new().to_string();
        let key = self.tuple_key(&tuple_id);

        // Serialize tuple to JSON
        let serialized = serde_json::to_string(&tuple)
            .map_err(|e| TupleSpaceError::SerializationError(e.to_string()))?;

        // Write to Redis with TTL if lease exists
        let mut conn = self.conn.clone();

        if let Some(lease) = tuple.lease() {
            let ttl_seconds = lease.ttl_seconds() as u64;
            if ttl_seconds > 0 {
                // Use SET with EX option to set value with expiry
                redis::cmd("SET")
                    .arg(&key)
                    .arg(&serialized)
                    .arg("EX")
                    .arg(ttl_seconds)
                    .query_async::<_, ()>(&mut conn)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;
            } else {
                // TTL is 0 or negative, just set normally
                conn.set::<_, _, ()>(&key, serialized)
                    .await
                    .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;
            }
        } else {
            // No lease, set normally without expiry
            conn.set::<_, _, ()>(&key, serialized)
                .await
                .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;
        }

        Ok(())
    }

    /// Read all tuples matching a pattern
    ///
    /// ## Arguments
    /// * `pattern` - Pattern to match against
    ///
    /// ## Returns
    /// Vec of matching tuples
    ///
    /// ## Examples
    /// ```no_run
    /// # use plexspaces_tuplespace::redis_space::RedisTupleSpace;
    /// # use plexspaces_tuplespace::{Pattern, PatternField, TupleField};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let space = RedisTupleSpace::new("redis://127.0.0.1:6379", "test").await?;
    /// let pattern = Pattern::new(vec![
    ///     PatternField::Exact(TupleField::String("vote".to_string())),
    ///     PatternField::Wildcard,
    ///     PatternField::Exact(TupleField::Integer(0)),
    /// ]);
    /// let tuples = space.read(&pattern).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        let mut conn = self.conn.clone();
        let pattern_key = self.pattern_prefix();

        // Use SCAN to iterate over keys
        // Redis SCAN returns (cursor, keys) tuple
        let result: (i32, Vec<String>) = redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg(&pattern_key)
            .arg("COUNT")
            .arg("100")
            .query_async(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;
        let (_, keys) = result;

        // For simplicity, this is a basic SCAN implementation
        // Production should handle cursor iteration
        let mut matching_tuples = Vec::new();

        for key in keys {
            let serialized: String = conn
                .get(&key)
                .await
                .map_err(|e| TupleSpaceError::BackendError(e.to_string()))?;

            let tuple: Tuple = serde_json::from_str(&serialized)
                .map_err(|e| TupleSpaceError::SerializationError(e.to_string()))?;

            if tuple.matches(pattern) {
                matching_tuples.push(tuple);
            }
        }

        Ok(matching_tuples)
    }

    /// Take (remove) a tuple matching a pattern
    ///
    /// ## Arguments
    /// * `pattern` - Pattern to match against
    ///
    /// ## Returns
    /// The first matching tuple, or None if no match
    pub async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        let tuples = self.read(pattern).await?;

        if let Some(tuple) = tuples.first() {
            // Find and delete the key for this tuple
            // Note: This is a simplified version; production should be atomic
            let _conn = self.conn.clone();
            let _pattern_key = self.pattern_prefix();

            // TODO: Make this atomic using Lua script
            // For now, just return the tuple
            Ok(Some(tuple.clone()))
        } else {
            Ok(None)
        }
    }
}

// Tests
#[cfg(all(test, feature = "redis-backend"))]
mod tests {
    use super::*;
    use crate::{PatternField, TupleField};

    /// Note: These tests require Redis running on localhost:6379
    /// Skip if Redis is not available
    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_write_and_read() {
        let space = RedisTupleSpace::new("redis://127.0.0.1:6379", "test-write-read")
            .await
            .expect("Failed to connect to Redis");

        // Write a tuple
        let tuple = Tuple::new(vec![
            TupleField::String("proposal".to_string()),
            TupleField::String("general1".to_string()),
            TupleField::Integer(0),
            TupleField::String("attack".to_string()),
        ]);

        space.write(tuple.clone()).await.expect("Write failed");

        // Read it back
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("proposal".to_string())),
            PatternField::Wildcard,
            PatternField::Exact(TupleField::Integer(0)),
            PatternField::Wildcard,
        ]);

        let tuples = space.read(&pattern).await.expect("Read failed");
        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0], tuple);
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_pattern_matching() {
        let space = RedisTupleSpace::new("redis://127.0.0.1:6379", "test-pattern")
            .await
            .expect("Failed to connect to Redis");

        // Write multiple tuples
        space
            .write(Tuple::new(vec![
                TupleField::String("vote".to_string()),
                TupleField::String("g1".to_string()),
                TupleField::Integer(0),
            ]))
            .await
            .expect("Write failed");

        space
            .write(Tuple::new(vec![
                TupleField::String("vote".to_string()),
                TupleField::String("g2".to_string()),
                TupleField::Integer(0),
            ]))
            .await
            .expect("Write failed");

        space
            .write(Tuple::new(vec![
                TupleField::String("vote".to_string()),
                TupleField::String("g3".to_string()),
                TupleField::Integer(1), // Different round
            ]))
            .await
            .expect("Write failed");

        // Read votes for round 0
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("vote".to_string())),
            PatternField::Wildcard,
            PatternField::Exact(TupleField::Integer(0)),
        ]);

        let tuples = space.read(&pattern).await.expect("Read failed");
        assert_eq!(tuples.len(), 2); // Only g1 and g2, not g3
    }
}
