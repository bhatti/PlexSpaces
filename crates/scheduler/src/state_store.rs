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

//! State store for scheduling requests.
//!
//! ## Purpose
//! Stores scheduling requests and their status for tracking and recovery.
//! Uses SQL database for efficient queries by status (especially PENDING for recovery).
//!
//! ## Design
//! Following the workflow_executions pattern:
//! - SQL table for queryable metadata
//! - Indexed on status for efficient PENDING queries
//! - Atomic updates via SQL transactions
//! - Recovery support via query_pending_requests()

use async_trait::async_trait;
use plexspaces_proto::scheduling::v1::SchedulingRequest;
use std::error::Error;

/// Trait for scheduling state store.
///
/// ## Purpose
/// Stores scheduling requests and their status for tracking and recovery.
///
/// ## Backend Support
/// - InMemory: For testing
/// - SQL: PostgreSQL/SQLite for persistence (RECOMMENDED)
#[async_trait]
pub trait SchedulingStateStore: Send + Sync {
    /// Store a scheduling request
    async fn store_request(&self, request: SchedulingRequest) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Get a scheduling request by ID
    async fn get_request(&self, request_id: &str) -> Result<Option<SchedulingRequest>, Box<dyn Error + Send + Sync>>;

    /// Update a scheduling request
    async fn update_request(&self, request: SchedulingRequest) -> Result<(), Box<dyn Error + Send + Sync>>;

    /// Query PENDING requests (for recovery on startup)
    async fn query_pending_requests(&self) -> Result<Vec<SchedulingRequest>, Box<dyn Error + Send + Sync>>;
}

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
pub mod sql;

#[cfg(feature = "memory-backend")]
pub mod memory;
