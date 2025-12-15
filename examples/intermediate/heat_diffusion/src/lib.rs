// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Heat Diffusion Example Library

pub mod config;
pub mod coordinator;
pub mod region_actor;
pub mod region_behavior;

pub use config::{GridConfig, HeatDiffusionConfig};
pub use region_actor::ActorPosition;
pub use region_behavior::{RegionBehavior, RegionMessage};

use std::sync::Arc;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_proto::v1::tuplespace::TupleSpaceConfig;

// ============================================================================
// Configuration Helpers
// ============================================================================
//
// These functions create configured TupleSpace instances for heat diffusion simulation

/// Create a TupleSpace instance from environment or default
///
/// ## Purpose
/// Provides a convenient way to create TupleSpace for heat diffusion that respects
/// environment configuration (for multi-process simulations) or falls back to in-memory.
///
/// ## Environment Variables
/// - `PLEXSPACES_TUPLESPACE_BACKEND`: Backend type ("in-memory", "sqlite", "redis", "postgres")
/// - `PLEXSPACES_SQLITE_PATH`: SQLite database file path
/// - Other vars per TupleSpace::from_env() documentation
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc for sharing across region actors
///
/// ## Examples
/// ```rust
/// # use heat_diffusion::create_tuplespace_from_env;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Uses env vars if set, otherwise in-memory
/// let space = create_tuplespace_from_env().await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_tuplespace_from_env() -> Result<Arc<TupleSpace>, Box<dyn std::error::Error>> {
    // Try environment variables first, fall back to in-memory
    let tuplespace = TupleSpace::from_env_or_default().await?;
    Ok(Arc::new(tuplespace))
}

/// Create a TupleSpace instance from explicit configuration
///
/// ## Purpose
/// Allows explicit backend configuration instead of relying on env vars.
/// Useful for integration tests that need specific backends.
///
/// ## Arguments
/// * `config` - TupleSpaceConfig protobuf message
///
/// ## Returns
/// Configured TupleSpace wrapped in Arc
///
/// ## Examples
/// ```rust
/// # use heat_diffusion::create_tuplespace_from_config;
/// # use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, SqliteBackend};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = TupleSpaceConfig {
///     backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Sqlite(
///         SqliteBackend { path: ":memory:".to_string() }
///     )),
///     pool_size: 1,
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
/// # use heat_diffusion::create_in_memory_tuplespace;
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let space = create_in_memory_tuplespace().await?;
/// # Ok(())
/// # }
/// ```
pub async fn create_in_memory_tuplespace() -> Result<Arc<TupleSpace>, Box<dyn std::error::Error>> {
    let tuplespace = TupleSpace::new();
    Ok(Arc::new(tuplespace))
}
