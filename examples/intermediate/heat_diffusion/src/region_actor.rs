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

//! Region Actor for Heat Diffusion
//!
//! ## Purpose
//! Each RegionActor manages an NxM region of cells in the heat diffusion grid.
//! This follows the **Granularity Principle**: actors manage regions, not individual cells.
//!
//! ## Jacobi Iteration Algorithm
//! For each cell in the region:
//! ```text
//! new_temp[i][j] = (temp[i-1][j] + temp[i+1][j] +
//!                   temp[i][j-1] + temp[i][j+1]) / 4.0
//! ```
//!
//! ## Boundary Exchange
//! After each iteration, actors exchange edge values with neighbors via TupleSpace:
//! - North neighbor: send top row, receive from their bottom row
//! - South neighbor: send bottom row, receive from their top row
//! - East neighbor: send right column, receive from their left column
//! - West neighbor: send left column, receive from their right column
//!
//! ## Metrics Tracked
//! - `compute_duration_ms`: Time spent computing Jacobi iteration
//! - `coordinate_duration_ms`: Time spent exchanging boundaries + barrier
//! - `granularity_ratio`: compute_duration / coordinate_duration
//!
//! Goal: `granularity_ratio >= 100.0` (compute dominates)

use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField, TupleSpace};
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Position in the actor grid
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorPosition {
    /// Actor row in the actor grid (not cell row)
    pub row: usize,
    /// Actor column in the actor grid (not cell column)
    pub col: usize,
}

impl ActorPosition {
    pub fn new(row: usize, col: usize) -> Self {
        Self { row, col }
    }

    /// Get unique actor ID
    pub fn actor_id(&self) -> String {
        format!("region_{}_{}", self.row, self.col)
    }
}

/// Heat diffusion region actor state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionActor {
    /// Position in the actor grid
    position: ActorPosition,

    /// Actor grid dimensions (total actors in each dimension)
    actor_grid_size: (usize, usize),

    /// Region size (cells managed by this actor)
    region_size: (usize, usize),

    /// Temperature values for this region
    ///
    /// Dimensions: region_size.0 x region_size.1
    /// Includes boundary cells
    temperatures: Vec<Vec<f64>>,

    /// New temperature values (double buffering)
    new_temperatures: Vec<Vec<f64>>,

    /// Boundary temperatures (top, bottom, left, right)
    boundary_temps: (f64, f64, f64, f64),

    /// Current iteration number
    iteration: usize,

    /// Metrics
    total_compute_ms: f64,
    total_coordinate_ms: f64,
}

impl RegionActor {
    /// Create a new region actor
    pub fn new(
        position: ActorPosition,
        actor_grid_size: (usize, usize),
        region_size: (usize, usize),
        boundary_temps: (f64, f64, f64, f64),
    ) -> Self {
        let (rows, cols) = region_size;

        // Initialize with zeros (will be set to boundary temps if on edge)
        let mut temperatures = vec![vec![0.0; cols]; rows];
        let mut new_temperatures = vec![vec![0.0; cols]; rows];

        // Initialize boundary cells if this actor is on the grid edge
        // Must initialize BOTH buffers because of double buffering
        if position.row == 0 {
            // Top edge
            for j in 0..cols {
                temperatures[0][j] = boundary_temps.0;
                new_temperatures[0][j] = boundary_temps.0;
            }
        }
        if position.row == actor_grid_size.0 - 1 {
            // Bottom edge
            for j in 0..cols {
                temperatures[rows - 1][j] = boundary_temps.1;
                new_temperatures[rows - 1][j] = boundary_temps.1;
            }
        }
        if position.col == 0 {
            // Left edge
            for i in 0..rows {
                temperatures[i][0] = boundary_temps.2;
                new_temperatures[i][0] = boundary_temps.2;
            }
        }
        if position.col == actor_grid_size.1 - 1 {
            // Right edge
            for i in 0..rows {
                temperatures[i][cols - 1] = boundary_temps.3;
                new_temperatures[i][cols - 1] = boundary_temps.3;
            }
        }

        Self {
            position,
            actor_grid_size,
            region_size,
            temperatures,
            new_temperatures,
            boundary_temps,
            iteration: 0,
            total_compute_ms: 0.0,
            total_coordinate_ms: 0.0,
        }
    }

    /// Perform one Jacobi iteration on this region
    ///
    /// ## Returns
    /// Maximum temperature change in this region (for convergence check)
    pub fn compute_iteration(&mut self) -> f64 {
        let start = Instant::now();

        let (rows, cols) = self.region_size;
        let mut max_change = 0.0;

        // Jacobi stencil: new_temp[i][j] = average of 4 neighbors
        for i in 1..rows - 1 {
            for j in 1..cols - 1 {
                let avg = (self.temperatures[i - 1][j]
                    + self.temperatures[i + 1][j]
                    + self.temperatures[i][j - 1]
                    + self.temperatures[i][j + 1])
                    / 4.0;

                self.new_temperatures[i][j] = avg;

                let change = (avg - self.temperatures[i][j]).abs();
                if change > max_change {
                    max_change = change;
                }
            }
        }

        // Swap buffers
        std::mem::swap(&mut self.temperatures, &mut self.new_temperatures);

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        self.total_compute_ms += elapsed;

        max_change
    }

    /// Write boundary values to TupleSpace for neighbors to read
    pub async fn write_boundaries(
        &self,
        tuplespace: &TupleSpace,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let (rows, cols) = self.region_size;
        self.write_boundaries_impl(tuplespace, rows, cols).await?;
        Ok(())
    }

    /// Read boundary values from neighbors via TupleSpace
    pub async fn read_boundaries(
        &mut self,
        tuplespace: &TupleSpace,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let start = Instant::now();
        let (rows, cols) = self.region_size;
        self.read_boundaries_impl(tuplespace, rows, cols).await?;

        let elapsed = start.elapsed().as_secs_f64() * 1000.0;
        self.total_coordinate_ms += elapsed;
        Ok(())
    }

    async fn write_boundaries_impl(
        &self,
        tuplespace: &TupleSpace,
        rows: usize,
        cols: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let actor_id = self.position.actor_id();

        // North boundary (top row) - if not on grid edge
        if self.position.row > 0 {
            let top_row: Vec<f64> = self.temperatures[0].clone();
            let tuple = Tuple::new(vec![
                TupleField::String("boundary".to_string()),
                TupleField::Integer(self.iteration as i64),
                TupleField::String(actor_id.clone()),
                TupleField::String("north".to_string()),
                TupleField::String(serde_json::to_string(&top_row)?),
            ]);
            tuplespace.write(tuple).await?;
        }

        // South boundary (bottom row)
        if self.position.row < self.actor_grid_size.0 - 1 {
            let bottom_row: Vec<f64> = self.temperatures[rows - 1].clone();
            let tuple = Tuple::new(vec![
                TupleField::String("boundary".to_string()),
                TupleField::Integer(self.iteration as i64),
                TupleField::String(actor_id.clone()),
                TupleField::String("south".to_string()),
                TupleField::String(serde_json::to_string(&bottom_row)?),
            ]);
            tuplespace.write(tuple).await?;
        }

        // East boundary (right column)
        if self.position.col < self.actor_grid_size.1 - 1 {
            let right_col: Vec<f64> = self
                .temperatures
                .iter()
                .map(|row| row[cols - 1])
                .collect();
            let tuple = Tuple::new(vec![
                TupleField::String("boundary".to_string()),
                TupleField::Integer(self.iteration as i64),
                TupleField::String(actor_id.clone()),
                TupleField::String("east".to_string()),
                TupleField::String(serde_json::to_string(&right_col)?),
            ]);
            tuplespace.write(tuple).await?;
        }

        // West boundary (left column)
        if self.position.col > 0 {
            let left_col: Vec<f64> = self
                .temperatures
                .iter()
                .map(|row| row[0])
                .collect();
            let tuple = Tuple::new(vec![
                TupleField::String("boundary".to_string()),
                TupleField::Integer(self.iteration as i64),
                TupleField::String(actor_id),
                TupleField::String("west".to_string()),
                TupleField::String(serde_json::to_string(&left_col)?),
            ]);
            tuplespace.write(tuple).await?;
        }

        Ok(())
    }

    async fn read_boundaries_impl(
        &mut self,
        tuplespace: &TupleSpace,
        rows: usize,
        cols: usize,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // Read from North neighbor (their south boundary)
        if self.position.row > 0 {
            let neighbor_id = ActorPosition::new(self.position.row - 1, self.position.col).actor_id();
            let pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("boundary".to_string())),
                PatternField::Exact(TupleField::Integer(self.iteration as i64)),
                PatternField::Exact(TupleField::String(neighbor_id)),
                PatternField::Exact(TupleField::String("south".to_string())),
                PatternField::Wildcard,
            ]);

            if let Some(tuple) = tuplespace.read(pattern).await? {
                if let TupleField::String(json) = &tuple.fields()[4] {
                    let values: Vec<f64> = serde_json::from_str(json)?;
                    // Update ghost cells (top row) in BOTH buffers
                    for (j, &val) in values.iter().enumerate() {
                        self.temperatures[0][j] = val;
                        self.new_temperatures[0][j] = val;
                    }
                }
            }
        }

        // Read from South neighbor (their north boundary)
        if self.position.row < self.actor_grid_size.0 - 1 {
            let neighbor_id = ActorPosition::new(self.position.row + 1, self.position.col).actor_id();
            let pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("boundary".to_string())),
                PatternField::Exact(TupleField::Integer(self.iteration as i64)),
                PatternField::Exact(TupleField::String(neighbor_id)),
                PatternField::Exact(TupleField::String("north".to_string())),
                PatternField::Wildcard,
            ]);

            if let Some(tuple) = tuplespace.read(pattern).await? {
                if let TupleField::String(json) = &tuple.fields()[4] {
                    let values: Vec<f64> = serde_json::from_str(json)?;
                    // Update ghost cells (bottom row) in BOTH buffers
                    for (j, &val) in values.iter().enumerate() {
                        self.temperatures[rows - 1][j] = val;
                        self.new_temperatures[rows - 1][j] = val;
                    }
                }
            }
        }

        // Read from East neighbor (their west boundary)
        if self.position.col < self.actor_grid_size.1 - 1 {
            let neighbor_id = ActorPosition::new(self.position.row, self.position.col + 1).actor_id();
            let pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("boundary".to_string())),
                PatternField::Exact(TupleField::Integer(self.iteration as i64)),
                PatternField::Exact(TupleField::String(neighbor_id)),
                PatternField::Exact(TupleField::String("west".to_string())),
                PatternField::Wildcard,
            ]);

            if let Some(tuple) = tuplespace.read(pattern).await? {
                if let TupleField::String(json) = &tuple.fields()[4] {
                    let values: Vec<f64> = serde_json::from_str(json)?;
                    // Update ghost cells (right column) in BOTH buffers
                    for (i, &val) in values.iter().enumerate() {
                        self.temperatures[i][cols - 1] = val;
                        self.new_temperatures[i][cols - 1] = val;
                    }
                }
            }
        }

        // Read from West neighbor (their east boundary)
        if self.position.col > 0 {
            let neighbor_id = ActorPosition::new(self.position.row, self.position.col - 1).actor_id();
            let pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("boundary".to_string())),
                PatternField::Exact(TupleField::Integer(self.iteration as i64)),
                PatternField::Exact(TupleField::String(neighbor_id)),
                PatternField::Exact(TupleField::String("east".to_string())),
                PatternField::Wildcard,
            ]);

            if let Some(tuple) = tuplespace.read(pattern).await? {
                if let TupleField::String(json) = &tuple.fields()[4] {
                    let values: Vec<f64> = serde_json::from_str(json)?;
                    // Update ghost cells (left column) in BOTH buffers
                    for (i, &val) in values.iter().enumerate() {
                        self.temperatures[i][0] = val;
                        self.new_temperatures[i][0] = val;
                    }
                }
            }
        }

        Ok(())
    }

    /// Increment iteration counter (called after boundary exchange)
    pub fn increment_iteration(&mut self) {
        self.iteration += 1;
    }

    /// Get actor ID for barrier coordination
    pub fn actor_id(&self) -> String {
        self.position.actor_id()
    }

    /// Get current granularity ratio
    pub fn granularity_ratio(&self) -> f64 {
        if self.total_coordinate_ms > 0.0 {
            self.total_compute_ms / self.total_coordinate_ms
        } else {
            0.0
        }
    }

    /// Get actor metrics
    pub fn metrics(&self) -> ActorMetrics {
        ActorMetrics {
            actor_id: self.position.actor_id(),
            total_compute_ms: self.total_compute_ms,
            total_coordinate_ms: self.total_coordinate_ms,
            granularity_ratio: self.granularity_ratio(),
            iterations: self.iteration,
        }
    }

    /// Get current temperatures (for visualization)
    pub fn temperatures(&self) -> &Vec<Vec<f64>> {
        &self.temperatures
    }
}

/// Actor performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorMetrics {
    pub actor_id: String,
    pub total_compute_ms: f64,
    pub total_coordinate_ms: f64,
    pub granularity_ratio: f64,
    pub iterations: usize,
}
