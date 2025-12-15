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

//! Redis TupleSpace Storage Backend
//!
//! ## Purpose
//! Provides distributed, durable storage for TupleSpace tuples using Redis.
//! Ideal for multi-node production deployments requiring persistence.
//!
//! ## Design
//! - **Storage**: Redis keys with pattern `{prefix}:tuple:{id}`
//! - **Pattern Matching**: SCAN + client-side filtering
//! - **Leases**: Redis TTL for automatic expiry
//! - **Blocking Reads**: Redis BLPOP on notification queue
//! - **Pub/Sub**: For watch operations (future)
//!
//! ## Performance Characteristics
//! - **Write**: O(1) - Redis SET command
//! - **Read/Take**: O(n) - SCAN all keys + pattern matching
//! - **Count**: O(n) - SCAN all keys
//! - **Memory**: Depends on Redis configuration
//!
//! ## When to Use
//! - ✅ Multi-process distributed systems
//! - ✅ Persistence requirements (AOF/RDB)
//! - ✅ High availability (Redis Sentinel/Cluster)
//! - ❌ Single-process applications (use Memory)
//! - ❌ Complex transactions (use PostgreSQL)

#[cfg(feature = "redis-backend")]
use async_trait::async_trait;
#[cfg(feature = "redis-backend")]
use chrono::{DateTime, Utc};
#[cfg(feature = "redis-backend")]
use plexspaces_proto::tuplespace::v1::{RedisStorageConfig, StorageStats};
#[cfg(feature = "redis-backend")]
use redis::{aio::ConnectionManager, AsyncCommands, Client};
#[cfg(feature = "redis-backend")]
use std::time::Duration;
#[cfg(feature = "redis-backend")]
use std::sync::Arc;
#[cfg(feature = "redis-backend")]
use tracing::{debug, instrument, trace};

#[cfg(feature = "redis-backend")]
use super::{TupleSpaceStorage, WatchEventMessage};
#[cfg(feature = "redis-backend")]
use crate::{Pattern, Tuple, TupleSpaceError};

/// Stored tuple metadata for Redis
#[cfg(feature = "redis-backend")]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredTuple {
    /// Unique tuple ID (ULID)
    id: String,

    /// The actual tuple
    tuple: Tuple,

    /// When tuple was created
    created_at: DateTime<Utc>,

    /// When tuple expires (if has lease)
    expires_at: Option<DateTime<Utc>>,

    /// Is lease renewable?
    renewable: bool,
}

/// Redis-backed TupleSpace storage implementation
///
/// ## Thread Safety
/// - Uses Redis connection manager for connection pooling
/// - Redis commands are atomic (SET, GET, DEL)
/// - Pattern matching is eventually consistent (SCAN)
///
/// ## Lease Management
/// - Uses Redis EXPIRE for automatic TTL
/// - Renewal updates EXPIRE time
/// - Redis handles cleanup automatically
#[cfg(feature = "redis-backend")]
pub struct RedisStorage {
    /// Redis connection manager (handles connection pooling)
    conn: ConnectionManager,

    /// Redis client (for pub/sub subscriptions)
    client: Client,

    /// Key prefix for tuple keys (from config)
    pub(crate) key_prefix: String,

    /// Enable pub/sub for notifications
    enable_pubsub: bool,

    /// Operation statistics (for metrics)
    stats: Arc<std::sync::Mutex<RedisOperationStats>>,
}

/// Operation statistics for metrics
#[cfg(feature = "redis-backend")]
#[derive(Debug, Default)]
struct RedisOperationStats {
    /// Total operations
    total_operations: u64,
    /// Read operations
    read_operations: u64,
    /// Write operations
    write_operations: u64,
    /// Take operations
    take_operations: u64,
    /// Total latency in microseconds
    total_latency_us: u64,
}

#[cfg(feature = "redis-backend")]
impl RedisStorage {
    /// Create new Redis storage
    pub async fn new(config: RedisStorageConfig) -> Result<Self, TupleSpaceError> {
        let client = Client::open(config.connection_string.as_str()).map_err(|e| {
            TupleSpaceError::ConnectionError(format!("Redis connection failed: {}", e))
        })?;

        let conn = ConnectionManager::new(client.clone()).await.map_err(|e| {
            TupleSpaceError::ConnectionError(format!("Redis manager failed: {}", e))
        })?;

        Ok(RedisStorage {
            conn,
            client,
            key_prefix: if config.key_prefix.is_empty() {
                "tuplespace".to_string()
            } else {
                config.key_prefix
            },
            enable_pubsub: config.enable_pubsub,
            stats: Arc::new(std::sync::Mutex::new(RedisOperationStats::default())),
        })
    }

    /// Record operation for metrics
    fn record_operation(&self, op_type: &str, latency_us: u64) {
        let mut stats = self.stats.lock().unwrap();
        stats.total_operations += 1;
        stats.total_latency_us += latency_us;
        match op_type {
            "read" => stats.read_operations += 1,
            "write" => stats.write_operations += 1,
            "take" => stats.take_operations += 1,
            _ => {}
        }
    }

    /// Generate Redis key for tuple
    fn tuple_key(&self, tuple_id: &str) -> String {
        format!("{}:tuple:{}", self.key_prefix, tuple_id)
    }

    /// Generate pattern for scanning all tuples
    fn tuple_pattern(&self) -> String {
        format!("{}:tuple:*", self.key_prefix)
    }

    /// Generate notification queue key for blocking operations
    fn notification_queue_key(&self) -> String {
        format!("{}:notify", self.key_prefix)
    }

    /// Lua script for atomic take (scan + delete in single operation)
    /// This ensures that tuples are not taken by multiple clients simultaneously
    fn atomic_take_lua_script() -> &'static str {
        r#"
        local pattern = ARGV[1]
        local key_prefix = ARGV[2]
        local matching_keys = {}
        
        -- Scan for matching tuples (simplified - in production would use pattern matching)
        local cursor = "0"
        repeat
            local result = redis.call("SCAN", cursor, "MATCH", key_prefix .. ":tuple:*", "COUNT", 1000)
            cursor = result[1]
            local keys = result[2]
            
            for i, key in ipairs(keys) do
                local tuple_data = redis.call("GET", key)
                if tuple_data then
                    -- In a full implementation, we would deserialize and match pattern here
                    -- For now, we'll match all (can be improved)
                    table.insert(matching_keys, key)
                end
            end
        until cursor == "0"
        
        -- Delete matching keys
        if #matching_keys > 0 then
            redis.call("DEL", unpack(matching_keys))
            return matching_keys
        else
            return {}
        end
        "#
    }

    /// Notify waiting readers that a tuple was written
    async fn notify_tuple_written(&self) -> Result<(), TupleSpaceError> {
        let mut conn = self.conn.clone();
        let queue_key = self.notification_queue_key();
        
        // Push notification to queue (non-blocking, fire-and-forget)
        // Use LPUSH with a simple marker (we'll check for matching tuples after BLPOP)
        redis::cmd("LPUSH")
            .arg(&queue_key)
            .arg("1") // Simple marker
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("LPUSH failed: {}", e)))?;
        
        Ok(())
    }

    /// Scan all tuples matching pattern
    async fn scan_tuples(&self, pattern: &Pattern) -> Result<Vec<StoredTuple>, TupleSpaceError> {
        let mut conn = self.conn.clone();
        let scan_pattern = self.tuple_pattern();

        // SCAN to get all tuple keys
        // NOTE: This is a simplified version. Production should handle cursor iteration.
        let (_, keys): (i32, Vec<String>) = redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg(&scan_pattern)
            .arg("COUNT")
            .arg("1000")
            .query_async(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("SCAN failed: {}", e)))?;

        let mut matching_tuples = Vec::new();

        for key in keys {
            // Get tuple data
            let serialized: Option<String> = conn
                .get(&key)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("GET failed: {}", e)))?;

            if let Some(data) = serialized {
                let stored: StoredTuple = serde_json::from_str(&data).map_err(|e| {
                    TupleSpaceError::SerializationError(format!("Deserialization failed: {}", e))
                })?;

                // Check if tuple matches pattern
                if stored.tuple.matches(pattern) {
                    matching_tuples.push(stored);
                }
            }
        }

        Ok(matching_tuples)
    }
}

#[cfg(feature = "redis-backend")]
#[async_trait]
impl TupleSpaceStorage for RedisStorage {
    #[instrument(skip(self, tuple), fields(tuple_id = %ulid::Ulid::new()))]
    async fn write(&self, tuple: Tuple) -> Result<String, TupleSpaceError> {
        let start = std::time::Instant::now();
        let mut conn = self.conn.clone();
        let tuple_id = ulid::Ulid::new().to_string();
        let key = self.tuple_key(&tuple_id);
        trace!(tuple_id = %tuple_id, "Writing tuple to Redis");

        // Extract lease information
        let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
            (Some(lease.expires_at()), lease.is_renewable())
        } else {
            (None, false)
        };

        // Create stored tuple
        let stored = StoredTuple {
            id: tuple_id.clone(),
            tuple,
            created_at: Utc::now(),
            expires_at,
            renewable,
        };

        // Serialize to JSON
        let serialized = serde_json::to_string(&stored).map_err(|e| {
            TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
        })?;

        // Write to Redis with TTL if lease exists
        if let Some(expires) = expires_at {
            let now = Utc::now();
            if expires > now {
                let ttl_seconds = (expires - now).num_seconds();
                if ttl_seconds > 0 {
                    // SET with EXAT (expire at timestamp)
                    redis::cmd("SET")
                        .arg(&key)
                        .arg(&serialized)
                        .arg("EX")
                        .arg(ttl_seconds as usize)
                        .query_async::<_, ()>(&mut conn)
                        .await
                        .map_err(|e| {
                            TupleSpaceError::BackendError(format!("SET with TTL failed: {}", e))
                        })?;
                } else {
                    // Already expired, don't write
                    return Err(TupleSpaceError::LeaseError(
                        "Tuple already expired".to_string(),
                    ));
                }
            } else {
                return Err(TupleSpaceError::LeaseError(
                    "Tuple already expired".to_string(),
                ));
            }
        } else {
            // No lease, set normally
            conn.set::<_, _, ()>(&key, serialized)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("SET failed: {}", e)))?;
        }

        // Notify waiting readers that a tuple was written
        self.notify_tuple_written().await?;

        // Record metrics
        let latency_us = start.elapsed().as_micros() as u64;
        self.record_operation("write", latency_us);
        debug!(tuple_id = %tuple_id, latency_us, "Tuple written to Redis");

        Ok(tuple_id)
    }

    async fn write_batch(&self, tuples: Vec<Tuple>) -> Result<Vec<String>, TupleSpaceError> {
        if tuples.is_empty() {
            return Ok(Vec::new());
        }

        let mut ids = Vec::with_capacity(tuples.len());
        let mut conn = self.conn.clone();

        // Use Redis pipeline for batch writes
        let mut pipe = redis::pipe();
        pipe.atomic();

        // Prepare all writes in pipeline
        for tuple in tuples {
            let tuple_id = ulid::Ulid::new().to_string();
            ids.push(tuple_id.clone());

            // Extract lease information
            let (expires_at, renewable) = if let Some(lease) = tuple.lease() {
                (Some(lease.expires_at()), lease.is_renewable())
            } else {
                (None, false)
            };

            // Create stored tuple
            let stored = StoredTuple {
                id: tuple_id.clone(),
                tuple,
                created_at: Utc::now(),
                expires_at,
                renewable,
            };

            // Serialize to JSON
            let serialized = serde_json::to_string(&stored).map_err(|e| {
                TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
            })?;

            let key = self.tuple_key(&tuple_id);

            // Add SET command to pipeline
            if let Some(expires) = expires_at {
                let now = Utc::now();
                if expires > now {
                    let ttl_seconds = (expires - now).num_seconds();
                    if ttl_seconds > 0 {
                        pipe.set_ex(&key, &serialized, ttl_seconds as u64);
                    }
                }
            } else {
                pipe.set(&key, &serialized);
            }
        }

        // Execute pipeline
        pipe.query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("Pipeline failed: {}", e)))?;

        // Notify waiting readers (single notification for batch)
        self.notify_tuple_written().await?;

        Ok(ids)
    }

    #[instrument(skip(self, pattern), fields(timeout = ?timeout))]
    async fn read(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        trace!("Reading tuples from Redis");
        // First, try immediate read
        let stored_tuples = self.scan_tuples(&pattern).await?;
        if !stored_tuples.is_empty() {
            let latency_us = start.elapsed().as_micros() as u64;
            self.record_operation("read", latency_us);
            debug!(count = stored_tuples.len(), latency_us, "Tuples read from Redis");
            return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
        }

        // If no matches and no timeout, return empty
        let timeout_duration = match timeout {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        // Blocking read: Use BLPOP on notification queue
        let queue_key = self.notification_queue_key();

        loop {
            // Check if timeout expired
            if start.elapsed() >= timeout_duration {
                return Ok(Vec::new());
            }

            // Calculate remaining timeout for BLPOP (in seconds)
            let remaining = timeout_duration - start.elapsed();
            let remaining_secs = remaining.as_secs().max(1); // BLPOP requires at least 1 second

            // BLPOP with timeout
            let mut blpop_conn = self.conn.clone();
            let result: Option<(String, String)> = redis::cmd("BLPOP")
                .arg(&queue_key)
                .arg(remaining_secs) // BLPOP timeout in seconds
                .query_async(&mut blpop_conn)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("BLPOP failed: {}", e)))?;

            // If BLPOP returned (tuple was written), check for matches
            if result.is_some() {
                let stored_tuples = self.scan_tuples(&pattern).await?;
                if !stored_tuples.is_empty() {
                    let latency_us = start.elapsed().as_micros() as u64;
                    self.record_operation("read", latency_us);
                    return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
                }
                // No match, continue waiting
            } else {
                // BLPOP timed out
                return Ok(Vec::new());
            }
        }
    }

    #[instrument(skip(self, pattern), fields(timeout = ?timeout))]
    async fn take(
        &self,
        pattern: Pattern,
        timeout: Option<Duration>,
    ) -> Result<Vec<Tuple>, TupleSpaceError> {
        let start = std::time::Instant::now();
        trace!("Taking tuples from Redis");
        // First, try immediate take
        let stored_tuples = self.scan_tuples(&pattern).await?;

        if !stored_tuples.is_empty() {
            // Delete tuples from Redis
            let mut conn = self.conn.clone();
            let keys: Vec<String> = stored_tuples
                .iter()
                .map(|s| self.tuple_key(&s.id))
                .collect();

            // Use DEL to remove all matching tuples
            // TODO: Make this atomic using Lua script
            redis::cmd("DEL")
                .arg(&keys)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("DEL failed: {}", e)))?;

            let latency_us = start.elapsed().as_micros() as u64;
            self.record_operation("take", latency_us);
            debug!(count = stored_tuples.len(), latency_us, "Tuples taken from Redis");
            return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
        }

        // If no matches and no timeout, return empty
        let timeout_duration = match timeout {
            Some(d) => d,
            None => return Ok(Vec::new()),
        };

        // Blocking take: Use BLPOP on notification queue
        let queue_key = self.notification_queue_key();
        let start = std::time::Instant::now();

        loop {
            // Check if timeout expired
            if start.elapsed() >= timeout_duration {
                return Ok(Vec::new());
            }

            // Calculate remaining timeout for BLPOP (in seconds)
            let remaining = timeout_duration - start.elapsed();
            let remaining_secs = remaining.as_secs().max(1); // BLPOP requires at least 1 second

            // BLPOP with timeout
            let mut blpop_conn = self.conn.clone();
            let result: Option<(String, String)> = redis::cmd("BLPOP")
                .arg(&queue_key)
                .arg(remaining_secs) // BLPOP timeout in seconds
                .query_async(&mut blpop_conn)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("BLPOP failed: {}", e)))?;

            // If BLPOP returned (tuple was written), check for matches and take
            if result.is_some() {
                let stored_tuples = self.scan_tuples(&pattern).await?;
                if !stored_tuples.is_empty() {
                    // Delete tuples from Redis
                    let mut del_conn = self.conn.clone();
                    let keys: Vec<String> = stored_tuples
                        .iter()
                        .map(|s| self.tuple_key(&s.id))
                        .collect();

                    // Use DEL to remove all matching tuples (DEL is atomic in Redis)
                    redis::cmd("DEL")
                        .arg(&keys)
                        .query_async::<_, ()>(&mut del_conn)
                        .await
                        .map_err(|e| TupleSpaceError::BackendError(format!("DEL failed: {}", e)))?;

                    let latency_us = start.elapsed().as_micros() as u64;
                    self.record_operation("take", latency_us);
                    debug!(count = stored_tuples.len(), latency_us, "Tuples taken from Redis (after blocking)");
                    return Ok(stored_tuples.into_iter().map(|s| s.tuple).collect());
                }
                // No match, continue waiting
            } else {
                // BLPOP timed out
                let latency_us = start.elapsed().as_micros() as u64;
                self.record_operation("take", latency_us);
                debug!(latency_us, "Take timeout - no tuples found");
                return Ok(Vec::new());
            }
        }
    }

    async fn count(&self, pattern: Pattern) -> Result<usize, TupleSpaceError> {
        let stored_tuples = self.scan_tuples(&pattern).await?;
        Ok(stored_tuples.len())
    }

    async fn exists(&self, pattern: Pattern) -> Result<bool, TupleSpaceError> {
        let stored_tuples = self.scan_tuples(&pattern).await?;
        Ok(!stored_tuples.is_empty())
    }

    async fn renew_lease(
        &self,
        tuple_id: &str,
        new_ttl: Option<Duration>,
    ) -> Result<DateTime<Utc>, TupleSpaceError> {
        let mut conn = self.conn.clone();
        let key = self.tuple_key(tuple_id);

        // Get current tuple
        let serialized: Option<String> = conn
            .get(&key)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("GET failed: {}", e)))?;

        let mut stored = match serialized {
            Some(data) => serde_json::from_str::<StoredTuple>(&data).map_err(|e| {
                TupleSpaceError::SerializationError(format!("Deserialization failed: {}", e))
            })?,
            None => return Err(TupleSpaceError::NotFound),
        };

        if !stored.renewable {
            return Err(TupleSpaceError::LeaseError(
                "Lease is not renewable".to_string(),
            ));
        }

        // Calculate new expiry
        let extension = if let Some(ttl) = new_ttl {
            chrono::Duration::milliseconds(ttl.as_millis() as i64)
        } else if let Some(lease) = stored.tuple.lease() {
            chrono::Duration::seconds(lease.ttl_seconds() as i64)
        } else {
            return Err(TupleSpaceError::LeaseError(
                "No lease associated with tuple".to_string(),
            ));
        };

        let new_expires = Utc::now() + extension;
        stored.expires_at = Some(new_expires);

        // Update tuple in Redis
        let updated_serialized = serde_json::to_string(&stored).map_err(|e| {
            TupleSpaceError::SerializationError(format!("Serialization failed: {}", e))
        })?;

        let ttl_seconds = extension.num_seconds();
        if ttl_seconds > 0 {
            redis::cmd("SET")
                .arg(&key)
                .arg(&updated_serialized)
                .arg("EX")
                .arg(ttl_seconds as usize)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("SET failed: {}", e)))?;
        }

        Ok(new_expires)
    }

    async fn clear(&self) -> Result<(), TupleSpaceError> {
        let mut conn = self.conn.clone();
        let scan_pattern = self.tuple_pattern();

        // SCAN to get all tuple keys
        let (_, keys): (i32, Vec<String>) = redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg(&scan_pattern)
            .arg("COUNT")
            .arg("1000")
            .query_async(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("SCAN failed: {}", e)))?;

        if !keys.is_empty() {
            // Delete all keys
            redis::cmd("DEL")
                .arg(&keys)
                .query_async::<_, ()>(&mut conn)
                .await
                .map_err(|e| TupleSpaceError::BackendError(format!("DEL failed: {}", e)))?;
        }

        Ok(())
    }

    async fn stats(&self) -> Result<StorageStats, TupleSpaceError> {
        let mut conn = self.conn.clone();
        let scan_pattern = self.tuple_pattern();

        // Count all tuples
        let (_, keys): (i32, Vec<String>) = redis::cmd("SCAN")
            .arg("0")
            .arg("MATCH")
            .arg(&scan_pattern)
            .arg("COUNT")
            .arg("1000")
            .query_async(&mut conn)
            .await
            .map_err(|e| TupleSpaceError::BackendError(format!("SCAN failed: {}", e)))?;

        // Get operation stats
        let stats = self.stats.lock().unwrap();
        let avg_latency_ms = if stats.total_operations > 0 {
            ((stats.total_latency_us as f64 / stats.total_operations as f64) / 1000.0) as f32
        } else {
            0.0f32
        };

        Ok(StorageStats {
            tuple_count: keys.len() as u64,
            memory_bytes: 0, // TODO: Get from Redis INFO
            total_operations: stats.total_operations,
            read_operations: stats.read_operations,
            write_operations: stats.write_operations,
            take_operations: stats.take_operations,
            avg_latency_ms,
        })
    }

    async fn publish_watch_event(
        &self,
        event_type: &str,
        tuple: &Tuple,
        namespace: &str,
    ) -> Result<(), TupleSpaceError> {
        if !self.enable_pubsub {
            return Ok(()); // Pub/sub disabled
        }

        // Serialize watch event message
        // Note: Pattern with Predicate variants cannot be serialized, so we skip it
        let message = WatchEventMessage {
            event_type: event_type.to_string(),
            tuple: tuple.clone(),
            pattern_json: None, // Pattern not needed for publishing
        };

        let message_json = serde_json::to_string(&message).map_err(|e| {
            TupleSpaceError::SerializationError(format!("Failed to serialize watch event: {}", e))
        })?;

        // Publish to Redis PUBSUB channel
        let channel = format!("tuplespace:watch:{}", namespace);
        let mut conn = self.conn.clone();

        redis::cmd("PUBLISH")
            .arg(&channel)
            .arg(&message_json)
            .query_async::<_, ()>(&mut conn)
            .await
            .map_err(|e| {
                TupleSpaceError::BackendError(format!("PUBLISH failed: {}", e))
            })?;

        Ok(())
    }

    async fn subscribe_watch_events(
        &self,
        namespace: &str,
    ) -> Result<tokio::sync::mpsc::Receiver<WatchEventMessage>, TupleSpaceError> {
        if !self.enable_pubsub {
            return Err(TupleSpaceError::NotSupported(
                "Pub/sub is disabled for this storage backend".to_string(),
            ));
        }

        // Create a dedicated connection for pubsub (pubsub connections can't be used for regular commands)
        let pubsub_client = self.client.clone();
        let channel = format!("tuplespace:watch:{}", namespace);
        let (tx, rx) = tokio::sync::mpsc::channel(100);

        // Spawn background task to handle pubsub subscription
        tokio::spawn(async move {
            // Get a new connection for pubsub (pubsub connections are separate from regular connections)
            let mut pubsub_conn = match pubsub_client.get_async_connection().await {
                Ok(conn) => conn,
                Err(e) => {
                    tracing::error!("Failed to create pubsub connection: {}", e);
                    return;
                }
            };

            // Subscribe to the channel using raw Redis command
            // After SUBSCRIBE, the connection enters pubsub mode and can only receive subscription messages
            let subscribe_result: Result<(), redis::RedisError> = redis::cmd("SUBSCRIBE")
                .arg(&channel)
                .query_async(&mut pubsub_conn)
                .await;

            if let Err(e) = subscribe_result {
                tracing::error!("Failed to subscribe to channel {}: {}", channel, e);
                return;
            }

            tracing::debug!("Subscribed to Redis channel: {}", channel);

            // Read messages from the subscription
            // Redis SUBSCRIBE returns messages in the format: ["message", channel, payload]
            loop {
                let result: Result<redis::Value, redis::RedisError> = redis::cmd("")
                    .query_async(&mut pubsub_conn)
                    .await;

                match result {
                    Ok(redis::Value::Bulk(values)) => {
                        // Message format: ["message", channel, payload]
                        if values.len() >= 3 {
                            if let (redis::Value::Data(msg_type), redis::Value::Data(_ch), redis::Value::Data(payload)) =
                                (&values[0], &values[1], &values[2])
                            {
                                if msg_type == b"message" {
                                    // Deserialize the watch event message from bytes
                                    match serde_json::from_slice::<WatchEventMessage>(payload) {
                                        Ok(event) => {
                                            if tx.send(event).await.is_err() {
                                                // Receiver dropped, exit
                                                tracing::debug!("Watch event receiver dropped, stopping subscription");
                                                break;
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!("Failed to deserialize watch event: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(_) => {
                        // Unexpected message format, continue
                        continue;
                    }
                    Err(e) => {
                        tracing::error!("Error reading from pubsub: {}", e);
                        // Connection error, exit
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(all(test, feature = "redis-backend"))]
mod tests {
    use super::*;
    use crate::{Lease, PatternField, TupleField};

    /// Helper to create test Redis storage
    /// NOTE: Tests require Redis running on localhost:6379
    async fn create_test_storage() -> RedisStorage {
        let config = RedisStorageConfig {
            connection_string: "redis://127.0.0.1:6379".to_string(),
            pool_size: 10,
            key_prefix: format!("test:{}", ulid::Ulid::new()),
            enable_pubsub: false,
        };

        RedisStorage::new(config)
            .await
            .expect("Failed to connect to Redis")
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_basic() {
        let storage = create_test_storage().await;

        // Write tuple
        let tuple = Tuple::new(vec![
            TupleField::String("test".to_string()),
            TupleField::Integer(42),
        ]);

        let tuple_id = storage.write(tuple.clone()).await.unwrap();
        assert!(!tuple_id.is_empty());

        // Read tuple
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields(), tuple.fields());

        // Take tuple
        let taken = storage.take(pattern, None).await.unwrap();
        assert_eq!(taken.len(), 1);

        // Should be gone
        let pattern2 = Pattern::new(vec![
            PatternField::Exact(TupleField::String("test".to_string())),
            PatternField::Wildcard,
        ]);
        let empty = storage.read(pattern2, None).await.unwrap();
        assert_eq!(empty.len(), 0);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_blocking_read() {
        let storage = create_test_storage().await;

        // Spawn task to write tuple after delay
        // Create a new storage instance for the spawned task (shares same Redis instance)
        let config = RedisStorageConfig {
            connection_string: "redis://127.0.0.1:6379".to_string(),
            pool_size: 10,
            key_prefix: storage.key_prefix.clone(),
            enable_pubsub: false,
        };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // Create new storage instance pointing to same Redis
            let write_storage = RedisStorage::new(config)
                .await
                .expect("Failed to create Redis storage");
            let tuple = Tuple::new(vec![
                TupleField::String("blocking-test".to_string()),
                TupleField::Integer(42),
            ]);
            write_storage.write(tuple).await.unwrap();
        });

        // Blocking read should wait for tuple
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("blocking-test".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .read(pattern.clone(), Some(Duration::from_secs(5)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields()[0], TupleField::String("blocking-test".to_string()));
        assert!(elapsed.as_millis() >= 50, "Should have waited for tuple");
        assert!(elapsed.as_millis() < 5000, "Should not have timed out");

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_blocking_read_timeout() {
        let storage = create_test_storage().await;

        // Blocking read with no matching tuple should timeout
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("nonexistent".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .read(pattern, Some(Duration::from_millis(500)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 0, "Should return empty on timeout");
        assert!(elapsed.as_millis() >= 450, "Should have waited for timeout");
        assert!(elapsed.as_millis() < 1000, "Should not wait longer than timeout");

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_blocking_take() {
        let storage = create_test_storage().await;

        // Spawn task to write tuple after delay
        // Create a new storage instance for the spawned task (shares same Redis instance)
        let config = RedisStorageConfig {
            connection_string: "redis://127.0.0.1:6379".to_string(),
            pool_size: 10,
            key_prefix: storage.key_prefix.clone(),
            enable_pubsub: false,
        };
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            // Create new storage instance pointing to same Redis
            let write_storage = RedisStorage::new(config)
                .await
                .expect("Failed to create Redis storage");
            let tuple = Tuple::new(vec![
                TupleField::String("blocking-take-test".to_string()),
                TupleField::Integer(99),
            ]);
            write_storage.write(tuple).await.unwrap();
        });

        // Blocking take should wait for tuple and remove it
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("blocking-take-test".to_string())),
            PatternField::Wildcard,
        ]);

        let start = std::time::Instant::now();
        let results = storage
            .take(pattern.clone(), Some(Duration::from_secs(5)))
            .await
            .unwrap();
        let elapsed = start.elapsed();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].fields()[0], TupleField::String("blocking-take-test".to_string()));
        assert!(elapsed.as_millis() >= 50, "Should have waited for tuple");
        assert!(elapsed.as_millis() < 5000, "Should not have timed out");

        // Tuple should be gone
        let empty = storage.read(pattern, None).await.unwrap();
        assert_eq!(empty.len(), 0, "Tuple should be removed after take");

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_lease() {
        let storage = create_test_storage().await;

        // Write tuple with short lease
        let tuple = Tuple::new(vec![TupleField::String("expiring".to_string())])
            .with_lease(Lease::new(chrono::Duration::seconds(2)));

        storage.write(tuple).await.unwrap();

        // Should exist immediately
        let pattern = Pattern::new(vec![PatternField::Exact(TupleField::String(
            "expiring".to_string(),
        ))]);

        let results = storage.read(pattern.clone(), None).await.unwrap();
        assert_eq!(results.len(), 1);

        // Wait for expiry
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Should be gone (Redis TTL cleanup)
        let empty = storage.read(pattern, None).await.unwrap();
        assert_eq!(empty.len(), 0);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_stats() {
        let storage = create_test_storage().await;

        // Write some tuples
        for i in 0..5 {
            let tuple = Tuple::new(vec![TupleField::Integer(i)]);
            storage.write(tuple).await.unwrap();
        }

        // Check stats
        let stats = storage.stats().await.unwrap();
        assert_eq!(stats.tuple_count, 5);

        // Cleanup
        storage.clear().await.unwrap();
    }

    #[tokio::test]
    #[ignore] // Requires Redis
    async fn test_redis_storage_pattern_matching() {
        let storage = create_test_storage().await;

        // Write multiple tuples
        storage
            .write(Tuple::new(vec![
                TupleField::String("config".to_string()),
                TupleField::String("timeout".to_string()),
                TupleField::Integer(30),
            ]))
            .await
            .unwrap();

        storage
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
            PatternField::Wildcard,
        ]);

        let all = storage.read(pattern, None).await.unwrap();
        assert_eq!(all.len(), 2);

        // Cleanup
        storage.clear().await.unwrap();
    }
}
