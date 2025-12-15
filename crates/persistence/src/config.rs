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

//! Configuration for journal and persistence features
//!
//! ## Purpose
//! Provides a comprehensive configuration system for all durability features:
//! - Compression settings (algorithm, level)
//! - Encryption settings (algorithm, key management)
//! - Batch write settings (buffer size, flush interval)
//! - Snapshot settings (interval, retention)
//! - Retention policies (cleanup strategies)
//!
//! ## Design Principles
//! - **Sensible Defaults**: Production-ready defaults that work for most use cases
//! - **Easy to Configure**: Builder pattern for fluent configuration
//! - **Environment Variables**: Support for 12-factor app configuration
//! - **Type Safety**: Compile-time validation of configuration
//!
//! ## Usage Example
//!
//! ```rust
//! use plexspaces_persistence::config::*;
//!
//! // Default configuration (no compression, no encryption, no batching)
//! let config = JournalConfig::default();
//!
//! // Production configuration with compression and encryption
//! let config = JournalConfig::builder()
//!     .compression(CompressionType::Zstd)
//!     .encryption(EncryptionType::Aes256Gcm)
//!     .encryption_key([42u8; 32])  // Use secure key in production!
//!     .batch_size(100)
//!     .batch_timeout_ms(50)
//!     .snapshot_interval(1000)
//!     .retention_count(10)
//!     .build();
//!
//! // High-performance configuration (Snappy + batching)
//! let config = JournalConfig::builder()
//!     .compression(CompressionType::Snappy)
//!     .batch_size(500)
//!     .build();
//!
//! // High-security configuration (encryption only)
//! let config = JournalConfig::builder()
//!     .encryption(EncryptionType::Aes256Gcm)
//!     .encryption_key_from_env("JOURNAL_ENCRYPTION_KEY")
//!     .build();
//! ```

use crate::{codec::EncryptionKey, CompressionType, EncryptionType};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Comprehensive journal configuration
///
/// ## Configuration Categories
/// 1. **Compression**: Reduce storage size
/// 2. **Encryption**: Secure sensitive data
/// 3. **Batching**: Improve write throughput
/// 4. **Snapshots**: Recovery optimization
/// 5. **Retention**: Cleanup old data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JournalConfig {
    /// Compression settings
    pub compression: CompressionConfig,

    /// Encryption settings
    pub encryption: EncryptionConfig,

    /// Batch write settings
    pub batching: BatchConfig,

    /// Snapshot settings
    pub snapshot: SnapshotConfig,

    /// Retention settings
    pub retention: RetentionConfig,
}

/// Compression configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Compression algorithm to use
    pub algorithm: CompressionType,

    /// Compression level (algorithm-specific)
    ///
    /// - Snappy: Ignored (no levels)
    /// - Zstd: 1-22 (3 = default, 1 = fastest, 22 = best compression)
    pub level: u8,

    /// Minimum size (bytes) before compressing
    ///
    /// Skip compression for small snapshots to avoid overhead.
    /// Recommended: 1KB (1024 bytes)
    pub min_size: usize,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionType::None,
            level: 3,       // Zstd default
            min_size: 1024, // 1KB minimum
        }
    }
}

/// Encryption configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptionConfig {
    /// Encryption algorithm to use
    pub algorithm: EncryptionType,

    /// Encryption key (32 bytes for AES-256)
    ///
    /// ## Security Warning
    /// In production, NEVER hardcode keys in source code!
    /// Load from:
    /// - Environment variables
    /// - Key management service (KMS)
    /// - Secure key file with restricted permissions
    ///
    /// Use `JournalConfigBuilder::encryption_key_from_env()` for environment variables.
    #[serde(skip)] // Never serialize keys
    pub key: Option<EncryptionKey>,

    /// Key derivation salt (for password-based keys)
    #[serde(skip)]
    pub salt: Option<Vec<u8>>,
}

impl Default for EncryptionConfig {
    fn default() -> Self {
        Self {
            algorithm: EncryptionType::None,
            key: None,
            salt: None,
        }
    }
}

/// Batch write configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchConfig {
    /// Enable batch writes
    pub enabled: bool,

    /// Maximum batch size (number of entries)
    ///
    /// Flush batch when this many entries are buffered.
    /// Recommended: 100-500 for balanced latency/throughput
    pub max_size: usize,

    /// Batch flush timeout (milliseconds)
    ///
    /// Flush batch after this timeout even if not full.
    /// Prevents unbounded latency for low-volume workloads.
    /// Recommended: 50-100ms
    pub flush_timeout_ms: u64,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Off by default (explicit opt-in)
            max_size: 100,
            flush_timeout_ms: 50,
        }
    }
}

/// Snapshot configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotConfig {
    /// Enable automatic snapshots
    pub enabled: bool,

    /// Snapshot interval (number of journal entries)
    ///
    /// Take snapshot every N journal entries.
    /// Recommended: 1000-10000 depending on entry size
    pub interval: u64,

    /// Auto-truncate journal after snapshot
    ///
    /// Remove journal entries older than snapshot to save space.
    /// Safe because snapshot contains full state.
    pub auto_truncate: bool,
}

impl Default for SnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Off by default
            interval: 1000,
            auto_truncate: false,
        }
    }
}

/// Retention configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetentionConfig {
    /// Enable retention enforcement
    pub enabled: bool,

    /// Number of snapshots to retain (0 = keep all)
    ///
    /// Delete old snapshots beyond this limit.
    /// Recommended: 5-10 for recovery without unbounded growth
    pub retention_count: u32,

    /// Retention duration (keep snapshots newer than this)
    ///
    /// Alternative to retention_count. Delete snapshots older than this duration.
    /// If both set, keep snapshots that satisfy either condition.
    pub retention_duration: Option<Duration>,
}

#[allow(clippy::derivable_impls)]
impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            enabled: false,     // Off by default
            retention_count: 0, // Keep all
            retention_duration: None,
        }
    }
}

#[allow(clippy::derivable_impls)]
impl Default for JournalConfig {
    fn default() -> Self {
        Self {
            compression: CompressionConfig::default(),
            encryption: EncryptionConfig::default(),
            batching: BatchConfig::default(),
            snapshot: SnapshotConfig::default(),
            retention: RetentionConfig::default(),
        }
    }
}

/// Builder for JournalConfig with fluent API
///
/// ## Examples
///
/// ```rust
/// use plexspaces_persistence::config::*;
///
/// let config = JournalConfig::builder()
///     .compression(CompressionType::Zstd)
///     .encryption(EncryptionType::Aes256Gcm)
///     .encryption_key([42u8; 32])
///     .batch_size(100)
///     .snapshot_interval(1000)
///     .retention_count(10)
///     .build();
/// ```
pub struct JournalConfigBuilder {
    config: JournalConfig,
}

impl JournalConfig {
    /// Create a new builder
    pub fn builder() -> JournalConfigBuilder {
        JournalConfigBuilder {
            config: JournalConfig::default(),
        }
    }

    /// Production-ready configuration with compression and batching
    ///
    /// Enables:
    /// - Zstd compression (level 3)
    /// - Batch writes (100 entries, 50ms timeout)
    /// - Automatic snapshots (every 1000 entries)
    /// - Retention (keep last 10 snapshots)
    pub fn production() -> Self {
        Self::builder()
            .compression(CompressionType::Zstd)
            .batch_enabled(true)
            .batch_size(100)
            .snapshot_enabled(true)
            .snapshot_interval(1000)
            .retention_enabled(true)
            .retention_count(10)
            .build()
    }

    /// High-performance configuration with Snappy and aggressive batching
    ///
    /// Enables:
    /// - Snappy compression (fast)
    /// - Large batches (500 entries, 100ms timeout)
    /// - Snapshots every 5000 entries
    pub fn high_performance() -> Self {
        Self::builder()
            .compression(CompressionType::Snappy)
            .batch_enabled(true)
            .batch_size(500)
            .batch_timeout_ms(100)
            .snapshot_enabled(true)
            .snapshot_interval(5000)
            .build()
    }

    /// High-security configuration with encryption
    ///
    /// Enables:
    /// - AES-256-GCM encryption (requires key)
    /// - Zstd compression (before encryption)
    /// - Retention (for compliance)
    ///
    /// **Note**: Must call `.encryption_key()` or `.encryption_key_from_env()` separately
    pub fn high_security() -> Self {
        Self::builder()
            .compression(CompressionType::Zstd)
            .encryption(EncryptionType::Aes256Gcm)
            .retention_enabled(true)
            .retention_count(10)
            .build()
    }
}

impl JournalConfigBuilder {
    /// Set compression algorithm
    pub fn compression(mut self, algorithm: CompressionType) -> Self {
        self.config.compression.algorithm = algorithm;
        self
    }

    /// Set compression level (Zstd only)
    pub fn compression_level(mut self, level: u8) -> Self {
        self.config.compression.level = level;
        self
    }

    /// Set minimum size for compression (bytes)
    pub fn compression_min_size(mut self, min_size: usize) -> Self {
        self.config.compression.min_size = min_size;
        self
    }

    /// Set encryption algorithm
    pub fn encryption(mut self, algorithm: EncryptionType) -> Self {
        self.config.encryption.algorithm = algorithm;
        self
    }

    /// Set encryption key directly
    ///
    /// ## Security Warning
    /// Only use for testing! In production, use `encryption_key_from_env()`.
    pub fn encryption_key(mut self, key: EncryptionKey) -> Self {
        self.config.encryption.key = Some(key);
        self
    }

    /// Load encryption key from environment variable
    ///
    /// ## Example
    /// ```bash
    /// export JOURNAL_ENCRYPTION_KEY="<64-character hex string>"
    /// ```
    pub fn encryption_key_from_env(mut self, env_var: &str) -> Self {
        if let Ok(key_hex) = std::env::var(env_var) {
            if let Ok(key_bytes) = hex::decode(&key_hex) {
                if key_bytes.len() == 32 {
                    let mut key = [0u8; 32];
                    key.copy_from_slice(&key_bytes);
                    self.config.encryption.key = Some(key);
                }
            }
        }
        self
    }

    /// Enable/disable batch writes
    pub fn batch_enabled(mut self, enabled: bool) -> Self {
        self.config.batching.enabled = enabled;
        self
    }

    /// Set batch size (number of entries)
    pub fn batch_size(mut self, size: usize) -> Self {
        self.config.batching.max_size = size;
        self
    }

    /// Set batch flush timeout (milliseconds)
    pub fn batch_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.config.batching.flush_timeout_ms = timeout_ms;
        self
    }

    /// Enable/disable automatic snapshots
    pub fn snapshot_enabled(mut self, enabled: bool) -> Self {
        self.config.snapshot.enabled = enabled;
        self
    }

    /// Set snapshot interval (journal entries)
    pub fn snapshot_interval(mut self, interval: u64) -> Self {
        self.config.snapshot.interval = interval;
        self
    }

    /// Enable/disable auto-truncate after snapshot
    pub fn snapshot_auto_truncate(mut self, auto_truncate: bool) -> Self {
        self.config.snapshot.auto_truncate = auto_truncate;
        self
    }

    /// Enable/disable retention enforcement
    pub fn retention_enabled(mut self, enabled: bool) -> Self {
        self.config.retention.enabled = enabled;
        self
    }

    /// Set retention count (number of snapshots to keep)
    pub fn retention_count(mut self, count: u32) -> Self {
        self.config.retention.retention_count = count;
        self
    }

    /// Set retention duration (keep snapshots newer than this)
    pub fn retention_duration(mut self, duration: Duration) -> Self {
        self.config.retention.retention_duration = Some(duration);
        self
    }

    /// Build the configuration
    pub fn build(self) -> JournalConfig {
        self.config
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = JournalConfig::default();

        // All features disabled by default
        assert_eq!(config.compression.algorithm, CompressionType::None);
        assert_eq!(config.encryption.algorithm, EncryptionType::None);
        assert!(!config.batching.enabled);
        assert!(!config.snapshot.enabled);
        assert!(!config.retention.enabled);
    }

    #[test]
    fn test_production_config() {
        let config = JournalConfig::production();

        // Compression enabled (Zstd)
        assert_eq!(config.compression.algorithm, CompressionType::Zstd);

        // Batching enabled
        assert!(config.batching.enabled);
        assert_eq!(config.batching.max_size, 100);

        // Snapshots enabled
        assert!(config.snapshot.enabled);
        assert_eq!(config.snapshot.interval, 1000);

        // Retention enabled
        assert!(config.retention.enabled);
        assert_eq!(config.retention.retention_count, 10);
    }

    #[test]
    fn test_high_performance_config() {
        let config = JournalConfig::high_performance();

        // Snappy compression (fast)
        assert_eq!(config.compression.algorithm, CompressionType::Snappy);

        // Large batches
        assert!(config.batching.enabled);
        assert_eq!(config.batching.max_size, 500);
        assert_eq!(config.batching.flush_timeout_ms, 100);

        // Snapshots with larger interval
        assert_eq!(config.snapshot.interval, 5000);
    }

    #[test]
    fn test_high_security_config() {
        let config = JournalConfig::high_security();

        // Encryption enabled (but key not set)
        assert_eq!(config.encryption.algorithm, EncryptionType::Aes256Gcm);
        assert!(config.encryption.key.is_none()); // Must be set separately

        // Compression before encryption
        assert_eq!(config.compression.algorithm, CompressionType::Zstd);

        // Retention for compliance
        assert!(config.retention.enabled);
    }

    #[test]
    fn test_builder_compression() {
        let config = JournalConfig::builder()
            .compression(CompressionType::Zstd)
            .compression_level(5)
            .compression_min_size(2048)
            .build();

        assert_eq!(config.compression.algorithm, CompressionType::Zstd);
        assert_eq!(config.compression.level, 5);
        assert_eq!(config.compression.min_size, 2048);
    }

    #[test]
    fn test_builder_encryption() {
        let key = [42u8; 32];
        let config = JournalConfig::builder()
            .encryption(EncryptionType::Aes256Gcm)
            .encryption_key(key)
            .build();

        assert_eq!(config.encryption.algorithm, EncryptionType::Aes256Gcm);
        assert!(config.encryption.key.is_some());
        assert_eq!(config.encryption.key.unwrap(), key);
    }

    #[test]
    fn test_builder_batching() {
        let config = JournalConfig::builder()
            .batch_enabled(true)
            .batch_size(200)
            .batch_timeout_ms(75)
            .build();

        assert!(config.batching.enabled);
        assert_eq!(config.batching.max_size, 200);
        assert_eq!(config.batching.flush_timeout_ms, 75);
    }

    #[test]
    fn test_builder_snapshots() {
        let config = JournalConfig::builder()
            .snapshot_enabled(true)
            .snapshot_interval(2000)
            .snapshot_auto_truncate(true)
            .build();

        assert!(config.snapshot.enabled);
        assert_eq!(config.snapshot.interval, 2000);
        assert!(config.snapshot.auto_truncate);
    }

    #[test]
    fn test_builder_retention() {
        let config = JournalConfig::builder()
            .retention_enabled(true)
            .retention_count(5)
            .retention_duration(Duration::from_secs(86400)) // 1 day
            .build();

        assert!(config.retention.enabled);
        assert_eq!(config.retention.retention_count, 5);
        assert_eq!(
            config.retention.retention_duration,
            Some(Duration::from_secs(86400))
        );
    }

    #[test]
    fn test_builder_fluent_api() {
        let config = JournalConfig::builder()
            .compression(CompressionType::Zstd)
            .encryption(EncryptionType::Aes256Gcm)
            .encryption_key([99u8; 32])
            .batch_enabled(true)
            .batch_size(150)
            .snapshot_enabled(true)
            .snapshot_interval(1500)
            .retention_enabled(true)
            .retention_count(8)
            .build();

        // Verify all settings applied
        assert_eq!(config.compression.algorithm, CompressionType::Zstd);
        assert_eq!(config.encryption.algorithm, EncryptionType::Aes256Gcm);
        assert!(config.batching.enabled);
        assert_eq!(config.batching.max_size, 150);
        assert!(config.snapshot.enabled);
        assert_eq!(config.snapshot.interval, 1500);
        assert!(config.retention.enabled);
        assert_eq!(config.retention.retention_count, 8);
    }
}
