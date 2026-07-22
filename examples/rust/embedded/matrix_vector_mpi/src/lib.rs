// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! HPC Matrix-Vector Multiplication with MPI-style Collective Operations
//!
//! This library provides MPI-style collective operations (scatter, broadcast,
//! gather, reduce, barrier) for parallel matrix-vector multiplication.

pub mod mpi_ops;
pub mod metrics;
pub mod worker_actor;
pub mod config;

pub use mpi_ops::*;
pub use metrics::*;
pub use worker_actor::WorkerActor;
pub use config::MatrixVectorConfig;

use std::sync::Arc;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_proto::v1::tuplespace::TupleSpaceConfig;

// ============================================================================
// Configuration Helpers
// ============================================================================
//
// These functions create configured TupleSpace instances for MPI operations

/// Create a TupleSpace instance from runtime-oriented defaults
///
/// ## Purpose
/// Provides a convenient way to create TupleSpace for MPI operations while
/// keeping backend selection in the enclosing runtime configuration.
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc for sharing across workers
///
/// ## Examples
/// ```rust
/// # use matrix_vector_mpi::create_tuplespace;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let space = create_tuplespace().await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_tuplespace() -> Result<Arc<TupleSpace>, Box<dyn std::error::Error>> {
    let tuplespace = TupleSpace::from_config(TupleSpaceConfig::default()).await?;
    Ok(Arc::new(tuplespace))
}

/// Create a TupleSpace instance from explicit configuration
///
/// ## Purpose
/// Allows explicit tuple-space configuration while backend selection remains
/// owned by runtime-configured services.
///
/// ## Arguments
/// * `config` - TupleSpaceConfig protobuf message
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc
///
/// ## Examples
/// ```rust
/// # use matrix_vector_mpi::create_tuplespace_from_config;
/// # use plexspaces_proto::tuplespace::v1::TupleSpaceConfig;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = TupleSpaceConfig {
///     default_ttl_seconds: 0,
///     enable_indexing: false,
/// };
/// let space = create_tuplespace_from_config(config).await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_tuplespace_from_config(config: TupleSpaceConfig) -> Result<Arc<TupleSpace>, Box<dyn std::error::Error>> {
    let tuplespace = TupleSpace::from_config(config).await?;
    Ok(Arc::new(tuplespace))
}

/// Create an in-memory TupleSpace for single-process testing
///
/// ## Purpose
/// Convenience function for creating in-memory TupleSpace.
/// This is the default for unit tests and local development.
///
/// ## Returns
/// In-memory TupleSpace wrapped in Arc
///
/// ## Examples
/// ```rust
/// # use matrix_vector_mpi::create_in_memory_tuplespace;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let space = create_in_memory_tuplespace().await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_in_memory_tuplespace() -> Result<Arc<TupleSpace>, Box<dyn std::error::Error>> {
    let tuplespace = TupleSpace::with_tenant_namespace("internal", "system");
    Ok(Arc::new(tuplespace))
}
