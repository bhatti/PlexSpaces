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

//! Region Behavior for Heat Diffusion
//!
//! Each RegionBehavior manages an NxM region of cells in the heat diffusion grid.
//! This follows the **Granularity Principle**: actors manage regions, not individual cells.

use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;

use crate::region_actor::ActorPosition;

/// Region message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RegionMessage {
    /// Compute one Jacobi iteration
    ComputeIteration,
    /// Write boundary values to TupleSpace
    WriteBoundaries,
    /// Read boundary values from TupleSpace
    ReadBoundaries,
    /// Get maximum temperature change (for convergence check)
    GetMaxChange,
    /// Get current temperatures (for visualization)
    GetTemperatures,
}

/// Shared state for region (accessible from both behavior and coordinator)
#[derive(Debug, Clone)]
pub struct RegionState {
    pub temperatures: Vec<Vec<f64>>,
    pub max_change: f64,
    pub iteration: usize,
}

impl RegionState {
    pub fn new(rows: usize, cols: usize) -> Self {
        Self {
            temperatures: vec![vec![0.0; cols]; rows],
            max_change: 0.0,
            iteration: 0,
        }
    }
}

/// Region Behavior
///
/// Manages a region of cells and performs Jacobi iteration for heat diffusion.
pub struct RegionBehavior {
    position: ActorPosition,
    actor_grid_size: (usize, usize),
    region_size: (usize, usize),
    temperatures: Vec<Vec<f64>>,
    new_temperatures: Vec<Vec<f64>>,
    boundary_temps: (f64, f64, f64, f64),
    iteration: usize,
    max_change: f64,
    tuplespace: Arc<TupleSpace>,
    shared_state: Arc<tokio::sync::RwLock<RegionState>>,
}

impl RegionBehavior {
    pub fn new(
        position: ActorPosition,
        actor_grid_size: (usize, usize),
        region_size: (usize, usize),
        boundary_temps: (f64, f64, f64, f64),
        tuplespace: Arc<TupleSpace>,
    ) -> Self {
        let (rows, cols) = region_size;

        // Initialize with zeros (will be set to boundary temps if on edge)
        let mut temperatures = vec![vec![0.0; cols]; rows];
        let mut new_temperatures = vec![vec![0.0; cols]; rows];

        // Initialize boundary cells if this actor is on the grid edge
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

        let shared_state = Arc::new(tokio::sync::RwLock::new(RegionState::new(rows, cols)));

        Self {
            position,
            actor_grid_size,
            region_size,
            temperatures,
            new_temperatures,
            boundary_temps,
            iteration: 0,
            max_change: 0.0,
            tuplespace,
            shared_state: shared_state.clone(),
        }
    }

    /// Get shared state (for coordinator access)
    pub fn shared_state(&self) -> Arc<tokio::sync::RwLock<RegionState>> {
        self.shared_state.clone()
    }

    /// Perform one Jacobi iteration on this region
    async fn compute_iteration(&mut self) -> f64 {
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
        self.max_change = max_change;
        
        // Update shared state
        {
            let mut state = self.shared_state.write().await;
            state.temperatures = self.temperatures.clone();
            state.max_change = max_change;
            state.iteration = self.iteration;
        }
        
        max_change
    }

    /// Write boundary values to TupleSpace for neighbors to read
    async fn write_boundaries(&self) -> Result<(), Box<dyn std::error::Error>> {
        let (rows, cols) = self.region_size;
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
            self.tuplespace.write(tuple).await?;
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
            self.tuplespace.write(tuple).await?;
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
            self.tuplespace.write(tuple).await?;
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
            self.tuplespace.write(tuple).await?;
        }

        Ok(())
    }

    /// Read boundary values from neighbors via TupleSpace
    async fn read_boundaries(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let (rows, cols) = self.region_size;

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

            if let Some(tuple) = self.tuplespace.read(pattern).await? {
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

            if let Some(tuple) = self.tuplespace.read(pattern).await? {
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

            if let Some(tuple) = self.tuplespace.read(pattern).await? {
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

            if let Some(tuple) = self.tuplespace.read(pattern).await? {
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

    /// Get actor ID
    pub fn actor_id(&self) -> String {
        self.position.actor_id()
    }


    /// Increment iteration counter (called after boundary exchange)
    pub async fn increment_iteration(&mut self) {
        self.iteration += 1;
        
        // Update shared state
        let mut state = self.shared_state.write().await;
        state.iteration = self.iteration;
    }
}

#[async_trait::async_trait]
impl ActorTrait for RegionBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenEvent
    }

    async fn handle_message(
        &mut self,
        _context: &ActorContext,
        message: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        // Deserialize message
        let payload = message.payload();
        let region_msg: RegionMessage = serde_json::from_slice(payload)
            .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;

        match region_msg {
            RegionMessage::ComputeIteration => {
                self.compute_iteration().await;
            }
            RegionMessage::WriteBoundaries => {
                self.write_boundaries().await
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
            }
            RegionMessage::ReadBoundaries => {
                self.read_boundaries().await
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
                // Increment iteration after reading boundaries
                self.increment_iteration().await;
            }
            RegionMessage::GetMaxChange | RegionMessage::GetTemperatures => {
                // These are handled via shared state access
            }
        }

        Ok(())
    }
}

