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

//! # PlexSpaces Distributed Locks
//!
//! ## Purpose
//! Provides distributed lock/lease coordination for background schedulers and
//! other coordination tasks. Uses version-based optimistic locking for atomic
//! operations, similar to db-locks implementation pattern.
//!
//! ## Architecture Context
//! This crate is used internally by:
//! - **Background Scheduler**: Lease-based coordination to ensure only one
//!   scheduler processes requests at a time
//! - **Future Coordination Tasks**: Any component that needs distributed locking
//!
//! ## Design Decisions
//! - **Version-based optimistic locking**: Prevents lost updates in concurrent scenarios
//! - **Timestamp-based expiration**: Stale lock detection for fault tolerance
//! - **Heartbeat mechanism**: Renews lease expiration timestamp periodically
//! - **Backend-agnostic**: Supports SQL (PostgreSQL/SQLite) or Redis backends
//!
//! ## Backend Support
//!
//! - **SQLite**: Persistent, single-node (feature: `sqlite-backend`) - use `:memory:` for in-memory
//! - **PostgreSQL**: Distributed, multi-node (feature: `postgres-backend`)
//! - **Redis**: Distributed with native TTL (feature: `redis-backend`)
//! - **DynamoDB**: AWS managed (feature: `ddb-backend`)
//!
//! ## Examples
//!
//! ### Basic Usage with SQLite :memory:
//! ```rust,no_run
//! use plexspaces_common::RequestContext;
//! use plexspaces_locks::{LockManager, sql::SqliteLockManager, AcquireLockOptions, RenewLockOptions, ReleaseLockOptions};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let ctx = RequestContext::new_without_auth("tenant".to_string(), "namespace".to_string());
//! let manager = SqliteLockManager::new(":memory:").await?;
//!
//! // Acquire lock
//! let lock = manager.acquire_lock(&ctx, AcquireLockOptions {
//!     lock_key: "scheduler:background:lease".to_string(),
//!     holder_id: "node-1".to_string(),
//!     lease_duration_secs: 30,
//!     additional_wait_time_ms: 0,
//!     refresh_period_ms: 100,
//!     metadata: Default::default(),
//! }).await?;
//!
//! // Renew lock (heartbeat)
//! let renewed = manager.renew_lock(&ctx, RenewLockOptions {
//!     lock_key: "scheduler:background:lease".to_string(),
//!     holder_id: "node-1".to_string(),
//!     version: lock.version.clone(),
//!     lease_duration_secs: 30,
//!     metadata: Default::default(),
//! }).await?;
//!
//! // Release lock
//! manager.release_lock(&ctx, ReleaseLockOptions {
//!     lock_key: "scheduler:background:lease".to_string(),
//!     holder_id: "node-1".to_string(),
//!     version: renewed.version,
//!     delete_lock: false,
//! }).await?;
//! # Ok(())
//! # }
//! ```

pub mod error;
pub mod factory;
pub mod manager;

pub mod memory;

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
pub mod sql;

#[cfg(feature = "redis-backend")]
pub mod redis;

#[cfg(feature = "ddb-backend")]
pub mod ddb;

pub use error::{LockError, LockResult};
pub use factory::{
    create_lock_manager, create_lock_manager_from_runtime, create_lock_manager_from_storage_config,
    LockBackend,
};
pub use manager::LockManager;
pub use memory::MemoryLockManager;

// Re-export proto types for convenience
pub use plexspaces_proto::locks::prv::{
    AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions,
};
