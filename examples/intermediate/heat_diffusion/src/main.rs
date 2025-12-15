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

//! Heat Diffusion Example - Dataflow Programming with PlexSpaces
//!
//! ## Purpose
//! Demonstrates parallel dataflow programming using PlexSpaces TupleSpace:
//! - Grid-based computation (Jacobi iteration for heat diffusion)
//! - Actor-based parallelism with proper granularity
//! - TupleSpace coordination (boundary exchange, barriers)
//! - Metrics tracking (compute vs coordinate cost)
//!
//! ## Granularity Design
//! Follows the core principle: **Computation Cost >> Communication Cost**
//!
//! Each actor manages a region of cells (not individual cells):
//! - 20×20 total grid
//! - 4 actors (2×2 actor grid)
//! - Each actor manages 10×10 = 100 cells
//! - Target ratio: compute_time / coordinate_time >= 100x
//!
//! ## Algorithm: Jacobi Iteration
//! Iterative solver for 2D heat equation:
//! ```
//! new_temp[i][j] = (temp[i-1][j] + temp[i+1][j] +
//!                   temp[i][j-1] + temp[i][j+1]) / 4.0
//! ```
//!
//! ## Coordination Pattern
//! 1. All actors compute Jacobi iteration on their region
//! 2. All actors exchange boundary values via TupleSpace
//! 3. Barrier synchronization (wait for all actors)
//! 4. Repeat until convergence (max change < threshold)
//!
//! ## PlexSpaces Features Demonstrated
//! - TupleSpace pattern matching (boundary exchange)
//! - Barrier synchronization (iteration coordination)
//! - Metrics tracking (granularity ratio)
//! - Dataflow programming model

mod config;
mod coordinator;
mod region_actor;
mod region_behavior;

use config::{GridConfig, HeatDiffusionConfig};
use coordinator::Coordinator;
use plexspaces_node::NodeBuilder;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(fmt::layer())
        .with(EnvFilter::from_default_env()
            .add_directive("heat_diffusion=info".parse()?))
        .init();

    tracing::info!("🔥 Heat Diffusion Example - PlexSpaces Dataflow Programming");
    tracing::info!("");

    // Load configuration using ConfigBootstrap
    let app_config: HeatDiffusionConfig = HeatDiffusionConfig::load();
    let config = app_config.to_grid_config()?;

    tracing::info!("Grid Configuration:");
    tracing::info!("  Total grid: {}×{} cells", config.grid_size.0, config.grid_size.1);
    tracing::info!("  Region size: {}×{} cells per actor", config.region_size.0, config.region_size.1);
    tracing::info!("  Actor grid: {}×{} actors",
        config.actor_grid_dimensions().0,
        config.actor_grid_dimensions().1
    );
    tracing::info!("  Cells per actor: {}", config.cells_per_actor());
    tracing::info!("  FLOPs per actor per iteration: {}", config.flops_per_actor_per_iteration());
    tracing::info!("  Boundary exchanges per actor: {}", config.boundary_exchanges_per_actor());
    tracing::info!("  Target granularity ratio: {}", config.target_ratio);
    tracing::info!("");

    // Create node using NodeBuilder
    let node = Arc::new(NodeBuilder::new("heat-node")
        .build());

    // Create coordinator
    let mut coordinator = Coordinator::new(config);

    // Initialize actors using NodeBuilder/ActorBuilder
    coordinator.initialize(node).await?;

    // Run simulation
    let result = coordinator.run().await?;

    // Visualize results
    let visualization = coordinator.visualize().await;
    println!("{}", visualization);

    // Report final metrics
    tracing::info!("Simulation complete!");
    tracing::info!("Iterations: {}", result.iterations);
    tracing::info!("Converged: {}", result.converged);

    Ok(())
}
