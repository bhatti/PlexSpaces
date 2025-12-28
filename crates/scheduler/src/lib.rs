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

//! # PlexSpaces Resource-Aware Scheduling Service
//!
//! ## Purpose
//! Implements two-layer resource-aware scheduling:
//! - **Layer 1 (Actor-Level)**: Place actors on nodes that match resource requirements and labels
//! - **Layer 2 (Task-Level)**: Route tasks to appropriate actors based on groups and load
//!
//! ## Architecture Context
//! This crate provides the scheduling service that runs on every node:
//! - **gRPC Service**: SchedulingService for actor placement requests
//! - **Background Scheduler**: Processes requests asynchronously with lease-based coordination
//! - **State Store**: Tracks scheduling requests and status
//! - **Node Selector**: Selects best node based on resources and labels
//! - **Capacity Tracker**: Tracks node capacity via ObjectRegistry

pub mod service;
pub mod background;
pub mod state_store;
pub mod node_selector;
pub mod capacity_tracker;
pub mod task_router;

pub use service::SchedulingServiceImpl;
pub use task_router::{TaskRouter, RoutingStrategy, TaskRouterError, TaskRouterResult};

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
pub use state_store::sql::SqliteSchedulingStateStore;

#[cfg(feature = "memory-backend")]
pub use state_store::memory::MemorySchedulingStateStore;

#[cfg(feature = "ddb-backend")]
pub use state_store::ddb::DynamoDBSchedulingStateStore;

