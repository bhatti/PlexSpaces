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

//! Workflow storage implementations
//!
//! This module provides storage backends for workflow definitions and executions:
//! - SQL-based storage (SQLite/PostgreSQL) - `sql.rs`
//! - DynamoDB-based storage - `ddb.rs` (requires `ddb-backend` feature)

pub mod sql;

#[cfg(feature = "ddb-backend")]
pub mod ddb;

// Re-export SQL storage as the default
pub use sql::WorkflowStorage;

#[cfg(feature = "ddb-backend")]
pub use ddb::{DynamoDBWorkflowStorage, WorkflowStorageTrait};

