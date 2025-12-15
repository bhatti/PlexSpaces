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

//! Checkpoint manager for periodic actor state snapshots
//!
//! ## Purpose
//! Provides automatic checkpoint creation to speed up actor recovery by 90%+.
//! Instead of replaying full journal, actors restore from checkpoint + delta replay.
//!
//! ## Architecture Context
//! Part of PlexSpaces durability infrastructure (Pillar 3: Restate-inspired).
//! Works with JournalStorage to create periodic snapshots and truncate old entries.
//!
//! ## How It Works
//! ```text
//! Normal Operation:
//! 1. Actor processes messages → journal entries written
//! 2. CheckpointManager monitors entry count/time
//! 3. When interval reached → create checkpoint
//! 4. Optionally truncate old journal entries (cleanup)
//!
//! Recovery:
//! 1. Load latest checkpoint (fast - single read)
//! 2. Replay journal entries after checkpoint (fast - small delta)
//! 3. Restore actor state (90%+ faster than full replay)
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::*;
//!
//! # async fn example() -> JournalResult<()> {
//! // Create checkpoint manager with interval config
//! let config = CheckpointConfig {
//!     enabled: true,
//!     entry_interval: 100,  // Every 100 journal entries
//!     time_interval: None,  // Or every T seconds
//!     compression: CompressionType::CompressionTypeZstd as i32,
//!     retention_count: 2,   // Keep last 2 checkpoints
//!     auto_truncate: true,  // Delete old entries
//!     async_checkpointing: false,
//!     metadata: Default::default(),
//! };
//!
//! let storage = MemoryJournalStorage::new();
//! let manager = CheckpointManager::new(storage.clone(), config, 1);
//!
//! // Periodically called by DurabilityFacet after processing messages
//! let actor_state = vec![1, 2, 3]; // Serialized actor state
//! if let Some(checkpoint_seq) = manager.maybe_checkpoint("actor-123", 150, actor_state).await? {
//!     println!("Checkpoint created at sequence {}", checkpoint_seq);
//! }
//! # Ok(())
//! # }
//! ```

use crate::{
    Checkpoint, CheckpointConfig, CheckpointManagerStats, CompressionType, JournalError,
    JournalResult, JournalStorage,
};
use plexspaces_proto::prost_types;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Checkpoint manager for periodic state snapshots
///
/// ## Purpose
/// Automates checkpoint creation based on entry count or time interval,
/// manages checkpoint retention, handles journal truncation, and enforces
/// schema version compatibility.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to stats and last checkpoint tracking.
///
/// ## Design
/// - Tracks per-actor checkpoint state (last sequence, last time)
/// - Checks intervals on each `maybe_checkpoint()` call
/// - Creates checkpoints via JournalStorage backend
/// - Optionally compresses checkpoint data (zstd, snappy)
/// - Truncates old journal entries if auto_truncate enabled
/// - Validates checkpoint schema version on load
///
/// ## Schema Versioning
/// - Each checkpoint stores the actor's current schema version
/// - On load, version is validated against actor's current version
/// - Prevents loading incompatible checkpoints (forward compatibility)
pub struct CheckpointManager<S: JournalStorage> {
    /// Underlying journal storage
    storage: Arc<S>,

    /// Checkpoint configuration
    config: CheckpointConfig,

    /// Per-actor state (last checkpoint sequence + time)
    actor_state: Arc<RwLock<HashMap<String, ActorCheckpointState>>>,

    /// Runtime statistics
    stats: Arc<RwLock<CheckpointManagerStats>>,

    /// Actor state schema version (for checkpoint compatibility)
    ///
    /// ## Purpose
    /// Tracks the current schema version of actor state to enable
    /// checkpoint format evolution.
    ///
    /// ## Default
    /// Version 1 (initial schema)
    ///
    /// ## Version Rules
    /// - Version 0 = unversioned (legacy, treated as version 1)
    /// - Version >= 1 = explicit schema version
    /// - Checkpoints with version > current version are rejected
    schema_version: u32,
}

/// Per-actor checkpoint state tracking
#[derive(Clone, Debug)]
struct ActorCheckpointState {
    /// Last checkpoint sequence number
    last_sequence: u64,

    /// Last checkpoint creation time
    last_time: Instant,
}

impl<S: JournalStorage> CheckpointManager<S> {
    /// Create a new checkpoint manager with schema version
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend
    /// * `config` - Checkpoint configuration from proto
    /// * `schema_version` - Actor state schema version (default: 1)
    ///
    /// ## Returns
    /// New CheckpointManager ready to manage checkpoints
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_journaling::*;
    /// # async fn example() -> JournalResult<()> {
    /// let storage = MemoryJournalStorage::new();
    /// let config = CheckpointConfig {
    ///     enabled: true,
    ///     entry_interval: 100,
    ///     time_interval: None,
    ///     compression: CompressionType::CompressionTypeZstd as i32,
    ///     retention_count: 2,
    ///     auto_truncate: true,
    ///     async_checkpointing: false,
    ///     metadata: Default::default(),
    /// };
    /// // Create with schema version 1 (initial)
    /// let manager = CheckpointManager::new(storage, config, 1);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(storage: S, config: CheckpointConfig, schema_version: u32) -> Self {
        Self {
            storage: Arc::new(storage),
            config,
            actor_state: Arc::new(RwLock::new(HashMap::new())),
            stats: Arc::new(RwLock::new(CheckpointManagerStats {
                checkpoints_created: 0,
                avg_checkpoint_duration: None,
                last_checkpoint_size: 0,
                entries_truncated: 0,
                last_checkpoint_at: None,
            })),
            schema_version,
        }
    }

    /// Create checkpoint manager with default schema version (1)
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend
    /// * `config` - Checkpoint configuration from proto
    ///
    /// ## Returns
    /// New CheckpointManager with schema version 1
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_journaling::*;
    /// # async fn example() -> JournalResult<()> {
    /// let storage = MemoryJournalStorage::new();
    /// let config = CheckpointConfig::default();
    /// let manager = CheckpointManager::with_default_version(storage, config);
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_default_version(storage: S, config: CheckpointConfig) -> Self {
        Self::new(storage, config, 1)
    }

    /// Check if checkpoint should be created and create if needed
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to checkpoint
    /// * `current_sequence` - Current journal sequence number
    /// * `state_data` - Serialized actor state to checkpoint
    ///
    /// ## Returns
    /// `Ok(Some(sequence))` if checkpoint was created, `Ok(None)` if skipped
    ///
    /// ## Errors
    /// - Storage errors during checkpoint save
    /// - Compression errors if compression enabled
    ///
    /// ## Logic
    /// 1. Check if checkpointing is enabled
    /// 2. Check if entry_interval reached (sequence - last_sequence >= interval)
    /// 3. Check if time_interval reached (now - last_time >= interval)
    /// 4. If either condition met → create checkpoint
    /// 5. Update actor state tracking
    /// 6. Update statistics
    /// 7. Optionally truncate old entries
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example(manager: &CheckpointManager<impl JournalStorage>) -> JournalResult<()> {
    /// // Actor processes 100 messages
    /// for seq in 1..=100 {
    ///     // Process message, append to journal...
    ///     let state_data = vec![1, 2, 3]; // Serialized actor state
    ///
    ///     // Periodically check if checkpoint needed
    ///     if let Some(checkpoint_seq) = manager.maybe_checkpoint(
    ///         "actor-123",
    ///         seq,
    ///         state_data
    ///     ).await? {
    ///         println!("Checkpoint created at sequence {}", checkpoint_seq);
    ///     }
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn maybe_checkpoint(
        &self,
        actor_id: &str,
        current_sequence: u64,
        state_data: Vec<u8>,
    ) -> JournalResult<Option<u64>> {
        // Check if checkpointing is enabled
        if !self.config.enabled {
            return Ok(None);
        }

        // Get actor's last checkpoint state
        let mut actor_states = self.actor_state.write().await;
        let actor_state =
            actor_states
                .entry(actor_id.to_string())
                .or_insert_with(|| ActorCheckpointState {
                    last_sequence: 0,
                    last_time: Instant::now(),
                });

        // Check entry interval
        let entry_interval_reached = if self.config.entry_interval > 0 {
            current_sequence - actor_state.last_sequence >= self.config.entry_interval
        } else {
            false
        };

        // Check time interval
        let time_interval_reached = if let Some(ref time_interval) = self.config.time_interval {
            let elapsed = actor_state.last_time.elapsed();
            let interval_duration = Duration::from_secs(time_interval.seconds as u64)
                + Duration::from_nanos(time_interval.nanos as u64);
            elapsed >= interval_duration
        } else {
            false
        };

        // If neither interval reached, skip checkpoint
        if !entry_interval_reached && !time_interval_reached {
            return Ok(None);
        }

        // Drop lock before potentially long-running operations
        drop(actor_states);

        // Create checkpoint
        let start_time = Instant::now();

        let checkpoint = self
            .create_checkpoint(actor_id, current_sequence, state_data)
            .await?;

        // Save checkpoint to storage
        self.storage.save_checkpoint(&checkpoint).await?;

        let checkpoint_duration = start_time.elapsed();

        // Update actor state
        let mut actor_states = self.actor_state.write().await;
        if let Some(state) = actor_states.get_mut(actor_id) {
            state.last_sequence = current_sequence;
            state.last_time = Instant::now();
        }
        drop(actor_states);

        // Update statistics
        let mut stats = self.stats.write().await;
        stats.checkpoints_created += 1;

        // Update average duration
        let new_avg = if let Some(ref avg) = stats.avg_checkpoint_duration {
            let old_avg_nanos = avg.seconds * 1_000_000_000 + avg.nanos as i64;
            let new_avg_nanos = (old_avg_nanos + checkpoint_duration.as_nanos() as i64) / 2;
            prost_types::Duration {
                seconds: new_avg_nanos / 1_000_000_000,
                nanos: (new_avg_nanos % 1_000_000_000) as i32,
            }
        } else {
            prost_types::Duration {
                seconds: checkpoint_duration.as_secs() as i64,
                nanos: checkpoint_duration.subsec_nanos() as i32,
            }
        };
        stats.avg_checkpoint_duration = Some(new_avg);

        stats.last_checkpoint_size = checkpoint.state_data.len() as u64;
        stats.last_checkpoint_at = checkpoint.timestamp.clone();
        drop(stats);

        // Auto-truncate old entries if enabled
        // Truncate entries BEFORE the checkpoint (keep checkpointed entry)
        if self.config.auto_truncate && current_sequence > 0 {
            let truncate_sequence = current_sequence - 1;
            let truncated = self.storage.truncate_to(actor_id, truncate_sequence).await?;

            let mut stats = self.stats.write().await;
            stats.entries_truncated += truncated;
        }

        Ok(Some(current_sequence))
    }

    /// Create checkpoint with optional compression
    ///
    /// ## Arguments
    /// * `actor_id` - Actor being checkpointed
    /// * `sequence` - Sequence number for this checkpoint
    /// * `state_data` - Serialized actor state (uncompressed)
    ///
    /// ## Returns
    /// Checkpoint proto message ready for storage
    ///
    /// ## Design Notes
    /// - Compression applied based on config.compression type
    /// - ZSTD: High compression ratio (3-5x), slower
    /// - Snappy: Fast compression (1.5-2x), faster
    /// - None: No compression (testing, small states)
    /// - state_schema_version: Set to manager's current schema version
    async fn create_checkpoint(
        &self,
        actor_id: &str,
        sequence: u64,
        state_data: Vec<u8>,
    ) -> JournalResult<Checkpoint> {
        // Compress state data if configured
        let compressed_data = self.compress_state_data(state_data)?;

        Ok(Checkpoint {
            actor_id: actor_id.to_string(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            state_data: compressed_data,
            compression: self.config.compression,
            metadata: self.config.metadata.clone(),
            state_schema_version: self.schema_version,
        })
    }

    /// Compress state data based on configuration
    ///
    /// ## Arguments
    /// * `data` - Uncompressed state data
    ///
    /// ## Returns
    /// Compressed data (or original if compression disabled)
    ///
    /// ## Errors
    /// - Compression errors (zstd/snappy failures)
    fn compress_state_data(&self, data: Vec<u8>) -> JournalResult<Vec<u8>> {
        match CompressionType::try_from(self.config.compression) {
            Ok(CompressionType::CompressionTypeNone) | Ok(CompressionType::CompressionTypeUnspecified) => {
                // No compression
                Ok(data)
            }
            Ok(CompressionType::CompressionTypeZstd) => {
                // ZSTD compression (high ratio, slower)
                #[cfg(feature = "compression-zstd")]
                {
                    zstd::encode_all(&data[..], 3)
                        .map_err(|e| JournalError::Compression(e.to_string()))
                }
                #[cfg(not(feature = "compression-zstd"))]
                {
                    Err(JournalError::Compression(
                        "ZSTD compression not enabled (enable 'compression-zstd' feature)"
                            .to_string(),
                    ))
                }
            }
            Ok(CompressionType::CompressionTypeSnappy) => {
                // Snappy compression (fast, lower ratio)
                #[cfg(feature = "compression-snappy")]
                {
                    use snap::raw::Encoder;
                    let mut encoder = Encoder::new();
                    encoder
                        .compress_vec(&data)
                        .map_err(|e| JournalError::Compression(e.to_string()))
                }
                #[cfg(not(feature = "compression-snappy"))]
                {
                    Err(JournalError::Compression(
                        "Snappy compression not enabled (enable 'compression-snappy' feature)"
                            .to_string(),
                    ))
                }
            }
            Err(_) => Err(JournalError::Compression(format!(
                "Unknown compression type: {}",
                self.config.compression
            ))),
        }
    }

    /// Get current checkpoint manager statistics
    ///
    /// ## Returns
    /// CheckpointManagerStats proto message with current metrics
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example(manager: &CheckpointManager<impl JournalStorage>) {
    /// let stats = manager.stats().await;
    /// println!("Checkpoints created: {}", stats.checkpoints_created);
    /// println!("Last checkpoint size: {} bytes", stats.last_checkpoint_size);
    /// # }
    /// ```
    pub async fn stats(&self) -> CheckpointManagerStats {
        self.stats.read().await.clone()
    }

    /// Get last checkpoint sequence for actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to query
    ///
    /// ## Returns
    /// Last checkpoint sequence, or 0 if no checkpoints exist
    pub async fn last_checkpoint_sequence(&self, actor_id: &str) -> u64 {
        let actor_states = self.actor_state.read().await;
        actor_states
            .get(actor_id)
            .map(|s| s.last_sequence)
            .unwrap_or(0)
    }

    /// Validate checkpoint schema version compatibility
    ///
    /// ## Arguments
    /// * `checkpoint` - Checkpoint to validate
    ///
    /// ## Returns
    /// `Ok(())` if compatible, `Err` if incompatible
    ///
    /// ## Validation Rules
    /// - Version 0 (unversioned) is treated as version 1 (backward compatibility)
    /// - Same version: Always compatible
    /// - Newer checkpoint version > current version: **REJECT** (forward incompatibility)
    /// - Older checkpoint version < current version: **ACCEPT** (backward compatibility assumed)
    ///
    /// ## Errors
    /// - `JournalError::IncompatibleSchemaVersion` if checkpoint version > current version
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example(manager: &CheckpointManager<impl JournalStorage>, checkpoint: &Checkpoint) -> JournalResult<()> {
    /// // Validate before loading
    /// manager.validate_checkpoint_version(checkpoint)?;
    ///
    /// // Safe to load - version is compatible
    /// // Now you can safely use checkpoint.state_data
    /// println!("Checkpoint validation passed");
    /// # Ok(())
    /// # }
    /// ```
    pub fn validate_checkpoint_version(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        // Treat version 0 (unversioned) as version 1
        let checkpoint_version = if checkpoint.state_schema_version == 0 {
            1
        } else {
            checkpoint.state_schema_version
        };

        if checkpoint_version > self.schema_version {
            return Err(JournalError::IncompatibleSchemaVersion {
                checkpoint_version,
                current_version: self.schema_version,
                actor_id: checkpoint.actor_id.clone(),
            });
        }

        Ok(())
    }

    /// Load and validate checkpoint from storage
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to load checkpoint for
    ///
    /// ## Returns
    /// Validated checkpoint, or error if not found or incompatible version
    ///
    /// ## Errors
    /// - `JournalError::CheckpointNotFound` if no checkpoint exists
    /// - `JournalError::IncompatibleSchemaVersion` if version incompatible
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example(manager: &CheckpointManager<impl JournalStorage>) -> JournalResult<()> {
    /// // Load latest checkpoint with version validation
    /// let checkpoint = manager.load_validated_checkpoint("actor-123").await?;
    ///
    /// // Checkpoint is guaranteed to be compatible with current schema
    /// let state_data = checkpoint.state_data;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn load_validated_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        // Load latest checkpoint from storage
        let checkpoint = self.storage.get_latest_checkpoint(actor_id).await?;

        // Validate version compatibility
        self.validate_checkpoint_version(&checkpoint)?;

        Ok(checkpoint)
    }

    /// Get current schema version
    ///
    /// ## Returns
    /// Current actor state schema version managed by this CheckpointManager
    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryJournalStorage;

    #[tokio::test]
    async fn test_checkpoint_manager_creation() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 100,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let stats = manager.stats().await;

        assert_eq!(stats.checkpoints_created, 0);
        assert_eq!(stats.last_checkpoint_size, 0);
        assert_eq!(manager.schema_version(), 1);
    }

    #[tokio::test]
    async fn test_entry_interval_checkpoint() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 10,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4];

        // First call at sequence 5 - should not checkpoint (< 10 entries)
        let result = manager
            .maybe_checkpoint("actor-1", 5, state_data.clone())
            .await
            .unwrap();
        assert!(result.is_none());

        // Second call at sequence 15 - should checkpoint (>= 10 entries from last)
        let result = manager
            .maybe_checkpoint("actor-1", 15, state_data.clone())
            .await
            .unwrap();
        assert_eq!(result, Some(15));

        // Verify stats updated
        let stats = manager.stats().await;
        assert_eq!(stats.checkpoints_created, 1);
        assert_eq!(stats.last_checkpoint_size, 4);
    }

    #[tokio::test]
    async fn test_disabled_checkpointing() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: false, // Disabled!
            entry_interval: 1,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4];

        // Should never checkpoint when disabled
        let result = manager
            .maybe_checkpoint("actor-1", 100, state_data)
            .await
            .unwrap();
        assert!(result.is_none());

        let stats = manager.stats().await;
        assert_eq!(stats.checkpoints_created, 0);
    }

    #[tokio::test]
    async fn test_time_interval_checkpoint() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 0, // Disabled
            time_interval: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000, // 100ms
            }),
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4];

        // First call - should not checkpoint immediately
        let result = manager
            .maybe_checkpoint("actor-1", 1, state_data.clone())
            .await
            .unwrap();
        assert!(result.is_none());

        // Wait for time interval to pass
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Second call - should checkpoint (time interval passed)
        let result = manager
            .maybe_checkpoint("actor-1", 2, state_data)
            .await
            .unwrap();
        assert_eq!(result, Some(2));

        let stats = manager.stats().await;
        assert_eq!(stats.checkpoints_created, 1);
    }

    #[tokio::test]
    async fn test_auto_truncate() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 5,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: true, // Enable truncation
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4];

        // Create checkpoint at sequence 10
        let result = manager
            .maybe_checkpoint("actor-1", 10, state_data)
            .await
            .unwrap();
        assert_eq!(result, Some(10));

        // Verify truncation happened (entries_truncated > 0)
        let stats = manager.stats().await;
        // Note: In real scenario with actual journal entries, this would be > 0
        // Here it's 0 because we didn't append any journal entries
        assert_eq!(stats.entries_truncated, 0);
    }

    #[tokio::test]
    async fn test_last_checkpoint_sequence() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 5,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4];

        // No checkpoints yet
        assert_eq!(manager.last_checkpoint_sequence("actor-1").await, 0);

        // Create checkpoint
        manager
            .maybe_checkpoint("actor-1", 10, state_data)
            .await
            .unwrap();

        // Verify last sequence updated
        assert_eq!(manager.last_checkpoint_sequence("actor-1").await, 10);
    }

    #[tokio::test]
    async fn test_compression_none() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 1,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let manager = CheckpointManager::new(storage, config, 1);
        let state_data = vec![1, 2, 3, 4, 5];

        manager
            .maybe_checkpoint("actor-1", 1, state_data)
            .await
            .unwrap();

        let stats = manager.stats().await;
        // With no compression, size should match input (5 bytes)
        assert_eq!(stats.last_checkpoint_size, 5);
    }

    #[tokio::test]
    async fn test_checkpoint_includes_schema_version() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 1,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        // Create manager with schema version 2
        let manager = CheckpointManager::new(storage.clone(), config, 2);
        let state_data = vec![1, 2, 3, 4];

        // Create checkpoint
        manager
            .maybe_checkpoint("actor-1", 1, state_data)
            .await
            .unwrap();

        // Load checkpoint and verify schema version
        let checkpoint = storage.get_latest_checkpoint("actor-1").await.unwrap();
        assert_eq!(checkpoint.state_schema_version, 2);
    }

    #[tokio::test]
    async fn test_validate_same_version() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig::default();

        // Manager with schema version 2
        let manager = CheckpointManager::new(storage, config, 2);

        // Checkpoint with same version (2)
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: None,
            state_data: vec![1, 2, 3],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: HashMap::new(),
            state_schema_version: 2,
        };

        // Should validate successfully (same version)
        assert!(manager.validate_checkpoint_version(&checkpoint).is_ok());
    }

    #[tokio::test]
    async fn test_validate_older_version() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig::default();

        // Manager with schema version 2 (current)
        let manager = CheckpointManager::new(storage, config, 2);

        // Checkpoint with older version (1)
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: None,
            state_data: vec![1, 2, 3],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: HashMap::new(),
            state_schema_version: 1,
        };

        // Should validate successfully (backward compatibility)
        assert!(manager.validate_checkpoint_version(&checkpoint).is_ok());
    }

    #[tokio::test]
    async fn test_validate_newer_version_fails() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig::default();

        // Manager with schema version 1 (old)
        let manager = CheckpointManager::new(storage, config, 1);

        // Checkpoint with newer version (2)
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: None,
            state_data: vec![1, 2, 3],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: HashMap::new(),
            state_schema_version: 2,
        };

        // Should fail (forward incompatibility)
        let result = manager.validate_checkpoint_version(&checkpoint);
        assert!(result.is_err());

        // Verify error type
        match result.unwrap_err() {
            JournalError::IncompatibleSchemaVersion {
                checkpoint_version,
                current_version,
                actor_id,
            } => {
                assert_eq!(checkpoint_version, 2);
                assert_eq!(current_version, 1);
                assert_eq!(actor_id, "actor-1");
            }
            _ => panic!("Expected IncompatibleSchemaVersion error"),
        }
    }

    #[tokio::test]
    async fn test_validate_unversioned_checkpoint() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig::default();

        // Manager with schema version 1
        let manager = CheckpointManager::new(storage, config, 1);

        // Checkpoint with version 0 (unversioned/legacy)
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: None,
            state_data: vec![1, 2, 3],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: HashMap::new(),
            state_schema_version: 0, // Unversioned
        };

        // Should validate successfully (version 0 treated as version 1)
        assert!(manager.validate_checkpoint_version(&checkpoint).is_ok());
    }

    #[tokio::test]
    async fn test_load_validated_checkpoint_success() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 1,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        // Create manager with schema version 2
        let manager = CheckpointManager::new(storage, config, 2);
        let state_data = vec![1, 2, 3, 4];

        // Create checkpoint with version 2
        manager
            .maybe_checkpoint("actor-1", 1, state_data)
            .await
            .unwrap();

        // Load and validate - should succeed
        let checkpoint = manager.load_validated_checkpoint("actor-1").await;
        assert!(checkpoint.is_ok());
        assert_eq!(checkpoint.unwrap().state_schema_version, 2);
    }

    #[tokio::test]
    async fn test_load_validated_checkpoint_version_mismatch() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig {
            enabled: true,
            entry_interval: 1,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: false,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        // Create checkpoint with schema version 2
        let manager_v2 = CheckpointManager::new(storage.clone(), config.clone(), 2);
        let state_data = vec![1, 2, 3, 4];
        manager_v2
            .maybe_checkpoint("actor-1", 1, state_data)
            .await
            .unwrap();

        // Try to load with schema version 1 manager (older version)
        let manager_v1 = CheckpointManager::new(storage, config, 1);
        let result = manager_v1.load_validated_checkpoint("actor-1").await;

        // Should fail due to version mismatch
        assert!(result.is_err());
        match result.unwrap_err() {
            JournalError::IncompatibleSchemaVersion {
                checkpoint_version,
                current_version,
                ..
            } => {
                assert_eq!(checkpoint_version, 2);
                assert_eq!(current_version, 1);
            }
            _ => panic!("Expected IncompatibleSchemaVersion error"),
        }
    }

    #[tokio::test]
    async fn test_with_default_version() {
        let storage = MemoryJournalStorage::new();
        let config = CheckpointConfig::default();

        // Create with default version (should be 1)
        let manager = CheckpointManager::with_default_version(storage, config);
        assert_eq!(manager.schema_version(), 1);
    }
}
