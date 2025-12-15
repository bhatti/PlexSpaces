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

//! Grid configuration for heat diffusion example
//!
//! ## Granularity Design
//! Following the core principle: **Computation Cost >> Communication Cost**
//!
//! This configuration allows tuning the granularity of parallelism by adjusting
//! the region size per actor. Each actor manages a region of cells, not individual cells.
//!
//! ### Golden Rule
//! `computation_cost / communication_cost >= 10x` (minimum), better 100x+
//!
//! ### Example Configurations
//!
//! **Too Fine-Grained** (BAD):
//! ```
//! # use heat_diffusion::config::GridConfig;
//! let config = GridConfig::new(
//!     (100, 100),     // 10,000 cells total
//!     (1, 1),         // 10,000 actors (one per cell)
//! );
//! // Result: Communication dominates, poor performance
//! # assert!(config.is_ok());
//! ```
//!
//! **Properly Granular** (GOOD):
//! ```
//! # use heat_diffusion::config::GridConfig;
//! let config = GridConfig::new(
//!     (100, 100),     // 10,000 cells total
//!     (10, 10),       // 100 actors (100 cells per actor)
//! );
//! // Result: Computation dominates, good performance
//! # assert!(config.is_ok());
//! ```

use serde::{Deserialize, Serialize};
use plexspaces_node::ConfigBootstrap;

/// Grid configuration with tunable granularity
///
/// ## Purpose
/// Defines the overall grid dimensions and how it's divided into regions
/// managed by actors. Enables tracking and tuning the computation vs
/// communication cost ratio.
///
/// ## Design Principles
/// - Total cells = `grid_size.0 * grid_size.1`
/// - Actor count = `(grid_size.0 / region_size.0) * (grid_size.1 / region_size.1)`
/// - Cells per actor = `region_size.0 * region_size.1`
/// - Larger regions = fewer actors = better compute/communicate ratio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridConfig {
    /// Total grid dimensions (rows, cols)
    ///
    /// Example: (20, 20) = 400 total cells
    pub grid_size: (usize, usize),

    /// Region size per actor (rows, cols)
    ///
    /// Example: (10, 10) = 100 cells per actor
    /// With grid_size (20, 20), this creates 4 actors in 2x2 layout
    pub region_size: (usize, usize),

    /// Target ratio: compute_time / coordinate_time
    ///
    /// Recommended: 100.0
    /// Minimum acceptable: 10.0
    ///
    /// If actual ratio < target, system should recommend larger regions
    pub target_ratio: f64,

    /// Convergence threshold
    ///
    /// Iteration stops when max temperature change < threshold
    pub convergence_threshold: f64,

    /// Maximum iterations
    ///
    /// Safety limit to prevent infinite loops
    pub max_iterations: usize,

    /// Initial boundary temperatures
    ///
    /// (top, bottom, left, right)
    pub boundary_temps: (f64, f64, f64, f64),
}

impl GridConfig {
    /// Create a new grid configuration
    pub fn new(
        grid_size: (usize, usize),
        region_size: (usize, usize),
    ) -> anyhow::Result<Self> {
        // Validate grid can be evenly divided into regions
        if grid_size.0 % region_size.0 != 0 {
            anyhow::bail!(
                "Grid rows {} must be divisible by region rows {}",
                grid_size.0,
                region_size.0
            );
        }
        if grid_size.1 % region_size.1 != 0 {
            anyhow::bail!(
                "Grid cols {} must be divisible by region cols {}",
                grid_size.1,
                region_size.1
            );
        }

        Ok(Self {
            grid_size,
            region_size,
            target_ratio: 100.0, // Default: 100x compute over communicate
            convergence_threshold: 0.01,
            max_iterations: 1000,
            boundary_temps: (100.0, 0.0, 0.0, 0.0), // Hot top, cold rest
        })
    }

    /// Get the number of actor regions in each dimension
    ///
    /// Returns: (actor_rows, actor_cols)
    pub fn actor_grid_dimensions(&self) -> (usize, usize) {
        (
            self.grid_size.0 / self.region_size.0,
            self.grid_size.1 / self.region_size.1,
        )
    }

    /// Get total number of actors
    pub fn actor_count(&self) -> usize {
        let (rows, cols) = self.actor_grid_dimensions();
        rows * cols
    }

    /// Get cells per actor
    pub fn cells_per_actor(&self) -> usize {
        self.region_size.0 * self.region_size.1
    }

    /// Compute FLOPs per iteration per actor
    ///
    /// Each cell: 4 additions + 1 division = 5 FLOPs
    pub fn flops_per_actor_per_iteration(&self) -> usize {
        self.cells_per_actor() * 5 // 5 FLOPs per cell (Jacobi stencil)
    }

    /// Get number of boundary exchanges per actor per iteration
    ///
    /// Each actor exchanges with up to 4 neighbors (N, S, E, W)
    /// Edge actors exchange with 2-3 neighbors
    /// Corner actors exchange with 2 neighbors
    pub fn boundary_exchanges_per_actor(&self) -> usize {
        4 // Maximum (interior actors)
    }
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            grid_size: (20, 20),      // 400 total cells
            region_size: (10, 10),    // 4 actors (2x2), 100 cells per actor
            target_ratio: 100.0,
            convergence_threshold: 0.01,
            max_iterations: 1000,
            boundary_temps: (100.0, 0.0, 0.0, 0.0),
        }
    }
}

/// Heat Diffusion Configuration (loaded from release.toml)
#[derive(Debug, Deserialize, Default)]
pub struct HeatDiffusionConfig {
    /// Grid dimensions
    #[serde(default = "default_grid_rows")]
    pub grid_rows: usize,
    #[serde(default = "default_grid_cols")]
    pub grid_cols: usize,
    /// Region size per actor
    #[serde(default = "default_region_rows")]
    pub region_rows: usize,
    #[serde(default = "default_region_cols")]
    pub region_cols: usize,
    /// Convergence threshold
    #[serde(default = "default_convergence_threshold")]
    pub convergence_threshold: f64,
    /// Maximum iterations
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,
    /// Target granularity ratio
    #[serde(default = "default_target_ratio")]
    pub target_ratio: f64,
    /// Boundary temperatures (top, bottom, left, right)
    #[serde(default = "default_boundary_top")]
    pub boundary_top: f64,
    #[serde(default = "default_boundary_bottom")]
    pub boundary_bottom: f64,
    #[serde(default = "default_boundary_left")]
    pub boundary_left: f64,
    #[serde(default = "default_boundary_right")]
    pub boundary_right: f64,
}

fn default_grid_rows() -> usize { 20 }
fn default_grid_cols() -> usize { 20 }
fn default_region_rows() -> usize { 10 }
fn default_region_cols() -> usize { 10 }
fn default_convergence_threshold() -> f64 { 0.01 }
fn default_max_iterations() -> usize { 1000 }
fn default_target_ratio() -> f64 { 100.0 }
fn default_boundary_top() -> f64 { 100.0 }
fn default_boundary_bottom() -> f64 { 0.0 }
fn default_boundary_left() -> f64 { 0.0 }
fn default_boundary_right() -> f64 { 0.0 }

impl HeatDiffusionConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Convert to GridConfig
    pub fn to_grid_config(&self) -> anyhow::Result<GridConfig> {
        GridConfig::new(
            (self.grid_rows, self.grid_cols),
            (self.region_rows, self.region_cols),
        ).map(|mut config| {
            config.convergence_threshold = self.convergence_threshold;
            config.max_iterations = self.max_iterations;
            config.target_ratio = self.target_ratio;
            config.boundary_temps = (self.boundary_top, self.boundary_bottom, self.boundary_left, self.boundary_right);
            config
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_config_validation() {
        // Valid configuration
        let config = GridConfig::new((20, 20), (10, 10)).unwrap();
        assert_eq!(config.actor_grid_dimensions(), (2, 2));
        assert_eq!(config.actor_count(), 4);
        assert_eq!(config.cells_per_actor(), 100);

        // Invalid: grid not divisible by region
        let result = GridConfig::new((20, 20), (7, 7));
        assert!(result.is_err());
    }

    #[test]
    fn test_different_granularities() {
        // Fine-grained (too many actors)
        let fine = GridConfig::new((100, 100), (10, 10)).unwrap();
        assert_eq!(fine.actor_count(), 100); // 10x10 actors
        assert_eq!(fine.cells_per_actor(), 100);

        // Coarse-grained (fewer actors)
        let coarse = GridConfig::new((100, 100), (25, 25)).unwrap();
        assert_eq!(coarse.actor_count(), 16); // 4x4 actors
        assert_eq!(coarse.cells_per_actor(), 625);

        // Coarse has 6.25x more computation per actor
        assert_eq!(
            coarse.cells_per_actor() / fine.cells_per_actor(),
            6
        );
    }

    #[test]
    fn test_flops_calculation() {
        let config = GridConfig::new((20, 20), (10, 10)).unwrap();

        // 100 cells per actor * 5 FLOPs per cell = 500 FLOPs
        assert_eq!(config.flops_per_actor_per_iteration(), 500);
    }
}
