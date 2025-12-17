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

//! Coordinator for Heat Diffusion Simulation
//!
//! ## Purpose
//! Manages the overall simulation:
//! - Creates region actors using NodeBuilder/ActorBuilder
//! - Coordinates iterations via messages
//! - Checks convergence
//! - Collects metrics using CoordinationComputeTracker
//! - Generates visualization

use crate::config::GridConfig;
use crate::region_actor::ActorPosition;
use crate::region_behavior::{RegionBehavior, RegionMessage, RegionState};
use plexspaces_node::{NodeBuilder, CoordinationComputeTracker};
use plexspaces_actor::ActorBuilder;
use plexspaces_core::ActorRef;
use plexspaces_tuplespace::TupleSpace;
use plexspaces_mailbox::Message;
use std::sync::Arc;

/// Simulation coordinator
pub struct Coordinator {
    config: GridConfig,
    tuplespace: Arc<TupleSpace>,
    actor_refs: Vec<ActorRef>,
    shared_states: Vec<Arc<tokio::sync::RwLock<RegionState>>>,
    metrics_tracker: CoordinationComputeTracker,
    node: Option<Arc<plexspaces_node::Node>>,
}

impl Coordinator {
    /// Create a new coordinator with default in-memory TupleSpace
    pub fn new(config: GridConfig) -> Self {
        let tuplespace = Arc::new(TupleSpace::new());
        Self::new_with_tuplespace(config, tuplespace)
    }

    /// Create a new coordinator with custom TupleSpace backend
    ///
    /// This allows using distributed backends like Redis or SQLite for
    /// multi-process coordination.
    pub fn new_with_tuplespace(config: GridConfig, tuplespace: Arc<TupleSpace>) -> Self {
        Self {
            config,
            tuplespace,
            actor_refs: Vec::new(),
            shared_states: Vec::new(),
            metrics_tracker: CoordinationComputeTracker::new("heat-diffusion".to_string()),
            node: None,
        }
    }

    /// Initialize all region actors using NodeBuilder/ActorBuilder
    pub async fn initialize(&mut self, node: Arc<plexspaces_node::Node>) -> anyhow::Result<()> {
        self.node = Some(node.clone());
        let (actor_rows, actor_cols) = self.config.actor_grid_dimensions();

        for row in 0..actor_rows {
            for col in 0..actor_cols {
                let position = ActorPosition::new(row, col);
                
                // Create behavior
                let behavior = RegionBehavior::new(
                    position,
                    (actor_rows, actor_cols),
                    self.config.region_size,
                    self.config.boundary_temps,
                    self.tuplespace.clone(),
                );
                
                // Store shared state for coordinator access
                let shared_state = behavior.shared_state();
                self.shared_states.push(shared_state.clone());
                
                // Create actor with behavior
                let behavior_box = Box::new(behavior);
                
                let actor = ActorBuilder::new(behavior_box)
                    .with_name(position.actor_id())
                    .build()
                    .await;
                
                // Use ActorFactory to spawn actor
                use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
                use std::sync::Arc;
                let actor_factory: Arc<ActorFactoryImpl> = node.as_ref().service_locator().get_service().await
                    .ok_or_else(|| format!("ActorFactory not found"))?;
                let actor_id = actor.id().clone();
                let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
                    .map_err(|e| format!("Failed to spawn actor: {}", e))?;
                let actor_ref = plexspaces_core::ActorRef::new(actor_id)
                    .map_err(|e| format!("Failed to create ActorRef: {}", e))?;
                self.actor_refs.push(actor_ref);
            }
        }

        tracing::info!(
            "Initialized {} actors ({} rows × {} cols)",
            self.actor_refs.len(),
            actor_rows,
            actor_cols
        );
        tracing::info!(
            "Each actor manages {} cells ({} rows × {} cols)",
            self.config.cells_per_actor(),
            self.config.region_size.0,
            self.config.region_size.1
        );

        // Wait for actors to initialize
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        Ok(())
    }

    /// Barrier synchronization - ensure all actors complete iteration
    async fn barrier_sync(&self, iteration: usize) -> Result<(), Box<dyn std::error::Error>> {
        let barrier_name = format!("iteration_{}", iteration);

        // All actors write completion tuples
        for actor_ref in &self.actor_refs {
            let tuple = plexspaces_tuplespace::Tuple::new(vec![
                plexspaces_tuplespace::TupleField::String("barrier".to_string()),
                plexspaces_tuplespace::TupleField::String(barrier_name.clone()),
                plexspaces_tuplespace::TupleField::String(actor_ref.id().to_string()),
            ]);
            self.tuplespace.write(tuple).await?;
        }

        // Wait for all actors to reach barrier
        let pattern = plexspaces_tuplespace::Pattern::new(vec![
            plexspaces_tuplespace::PatternField::Exact(
                plexspaces_tuplespace::TupleField::String("barrier".to_string())
            ),
            plexspaces_tuplespace::PatternField::Exact(
                plexspaces_tuplespace::TupleField::String(barrier_name.clone())
            ),
            plexspaces_tuplespace::PatternField::Wildcard,
        ]);

        // Count barrier tuples until we have all actors
        let actor_count = self.actor_refs.len();
        while self.tuplespace.count(pattern.clone()).await? < actor_count {
            tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        }

        // Clean up barrier tuples for next iteration
        let _ = self.tuplespace.take_all(pattern).await;

        Ok(())
    }

    /// Run simulation until convergence
    pub async fn run(&mut self) -> Result<SimulationResult, Box<dyn std::error::Error>> {
        let mut iteration = 0;
        let mut converged = false;

        tracing::info!("Starting heat diffusion simulation");
        tracing::info!("Target granularity ratio: {:.1}", self.config.target_ratio);

        while iteration < self.config.max_iterations && !converged {
            // Step 1: Compute iteration for all actors
            self.metrics_tracker.start_compute();
            let mut max_change = 0.0;
            
            for actor_ref in &self.actor_refs {
                let msg = RegionMessage::ComputeIteration;
                let payload = serde_json::to_vec(&msg)?;
                let message = Message::new(payload)
                    .with_message_type("ComputeIteration".to_string());
                
                // Use ActorRef::tell() directly - no need to route through Node
                actor_ref.tell(message).await?;
            }
            
            // Wait for computation to complete
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            
            // Get max_change from shared states
            for state in &self.shared_states {
                let guard = state.read().await;
                if guard.max_change > max_change {
                    max_change = guard.max_change;
                }
            }
            self.metrics_tracker.end_compute();

            // Step 2: Exchange boundaries - WRITE phase
            self.metrics_tracker.start_coordinate();
            for actor_ref in &self.actor_refs {
                let msg = RegionMessage::WriteBoundaries;
                let payload = serde_json::to_vec(&msg)?;
                let message = Message::new(payload)
                    .with_message_type("WriteBoundaries".to_string());
                
                // Use ActorRef::tell() directly - no need to route through Node
                actor_ref.tell(message).await?;
            }
            
            // Wait for all writes to complete
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;

            // Step 3: Exchange boundaries - READ phase
            for actor_ref in &self.actor_refs {
                let msg = RegionMessage::ReadBoundaries;
                let payload = serde_json::to_vec(&msg)?;
                let message = Message::new(payload)
                    .with_message_type("ReadBoundaries".to_string());
                
                // Use ActorRef::tell() directly - no need to route through Node
                actor_ref.tell(message).await?;
            }
            
            // Wait for all reads to complete
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            self.metrics_tracker.end_coordinate();

            // Step 4: Barrier - ensure all actors complete before checking convergence
            self.metrics_tracker.start_coordinate();
            self.barrier_sync(iteration).await?;
            self.metrics_tracker.end_coordinate();

            // Step 5: Update iteration counter for all actors
            // (handled internally by RegionBehavior when processing ReadBoundaries message)

            iteration += 1;

            // Step 6: Check convergence
            if max_change < self.config.convergence_threshold {
                converged = true;
                tracing::info!(
                    "Converged at iteration {} (max change: {:.6})",
                    iteration,
                    max_change
                );
            } else if iteration % 10 == 0 {
                tracing::info!(
                    "Iteration {}: max change = {:.6}",
                    iteration,
                    max_change
                );
            }
        }

        // Collect metrics from CoordinationComputeTracker
        let metrics = std::mem::replace(&mut self.metrics_tracker, CoordinationComputeTracker::new("heat-diffusion".to_string())).finalize();

        let result = SimulationResult {
            iterations: iteration,
            converged,
            metrics,
        };

        self.report_results(&result);

        Ok(result)
    }

    /// Report simulation results
    fn report_results(&self, result: &SimulationResult) {
        tracing::info!("\n========== Simulation Results ==========");
        tracing::info!("Iterations: {}", result.iterations);
        tracing::info!("Converged: {}", result.converged);

        tracing::info!("\n--- Performance Metrics ---");
        tracing::info!("Compute time: {:.2} ms", result.metrics.compute_duration_ms);
        tracing::info!("Coordinate time: {:.2} ms", result.metrics.coordinate_duration_ms);
        tracing::info!("Granularity ratio: {:.2}", result.metrics.granularity_ratio);
        tracing::info!("Target ratio: {:.2}", self.config.target_ratio);
        tracing::info!("Efficiency: {:.2}%", result.metrics.efficiency * 100.0);

        if result.metrics.granularity_ratio < self.config.target_ratio {
            tracing::warn!(
                "⚠️  Granularity ratio {:.2} < target {:.2}",
                result.metrics.granularity_ratio,
                self.config.target_ratio
            );
            tracing::warn!("Consider increasing region size for better performance");
        } else {
            tracing::info!(
                "✅ Granularity ratio {:.2} >= target {:.2}",
                result.metrics.granularity_ratio,
                self.config.target_ratio
            );
        }

        tracing::info!("========================================\n");
    }

    /// Generate ASCII heat map visualization
    pub async fn visualize(&self) -> String {
        let (actor_rows, actor_cols) = self.config.actor_grid_dimensions();
        let (region_rows, region_cols) = self.config.region_size;

        let mut output = String::new();
        output.push_str("\n========== Heat Map ==========\n");

        for actor_row in 0..actor_rows {
            for cell_row in 0..region_rows {
                for actor_col in 0..actor_cols {
                    let actor_idx = actor_row * actor_cols + actor_col;
                    let state = &self.shared_states[actor_idx];
                    let guard = state.read().await;
                    let temps = &guard.temperatures;

                    for cell_col in 0..region_cols {
                        let temp = temps[cell_row][cell_col];
                        let symbol = temp_to_symbol(temp);
                        output.push(symbol);
                        output.push(' ');
                    }
                    output.push_str("  "); // Separator between actors
                }
                output.push('\n');
            }
            output.push('\n'); // Separator between actor rows
        }

        output.push_str("Legend: ░ = cold, ▒ = warm, ▓ = hot, █ = very hot\n");
        output.push_str("==============================\n");

        output
    }
}

/// Convert temperature to ASCII symbol
fn temp_to_symbol(temp: f64) -> char {
    if temp < 25.0 {
        '░' // Cold
    } else if temp < 50.0 {
        '▒' // Warm
    } else if temp < 75.0 {
        '▓' // Hot
    } else {
        '█' // Very hot
    }
}

/// Simulation result
#[derive(Debug, Clone)]
pub struct SimulationResult {
    pub iterations: usize,
    pub converged: bool,
    pub metrics: plexspaces_proto::metrics::v1::CoordinationComputeMetrics,
}
