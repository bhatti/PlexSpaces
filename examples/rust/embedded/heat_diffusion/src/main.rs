// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Heat Diffusion Example (2D Stencil with TupleSpace Coordination)
//
// Demonstrates parallel stencil computation using PlexSpaces:
// - TupleSpace for ghost cell exchange between neighbors
// - Barrier synchronization between iterations
// - Actual actors (not structs) using SDK patterns
// - CoordinationComputeTracker metrics
//
// Use Case: Thermal simulation, image processing, weather modeling
//
// Architecture:
// - GridRegionActor: Each actor manages a horizontal strip of the grid
// - TupleSpace: Ghost cell exchange (boundary values) between neighbors
// - Barrier: Synchronize iterations (all regions compute before next iteration)
// - Metrics: Track coordination vs. computation time

use plexspaces_sdk::{gen_server_actor, plexspaces_handlers, handler, json, spawn, GenServerRef, RequestContext};
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use plexspaces_node::{NodeBuilder, CoordinationComputeTracker, service_wrappers::TupleSpaceProviderWrapper};
use plexspaces_core::{TupleSpaceProvider, BehaviorError, ActorContext, Message};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::info;
use anyhow::Result;

// =============================================================================
// GridRegionActor - Manages a horizontal strip of the grid
// =============================================================================

/// Grid region actor for heat diffusion simulation
///
/// ## Purpose
/// Each actor manages a horizontal strip of the 2D grid and performs stencil computation.
/// Ghost cells (boundaries) are exchanged via TupleSpace for neighbor communication.
///
/// ## Architecture
/// - Actor receives compute requests with iteration number
/// - Writes boundary values to TupleSpace for neighbors
/// - Reads neighbor boundaries from TupleSpace
/// - Computes new values using 5-point stencil
/// - Returns max difference for convergence check
#[gen_server_actor]
struct GridRegionActor {
    /// Region ID (0 = top, 1 = bottom, etc.)
    region_id: usize,
    /// Current temperature values (1D strip)
    data: Vec<f64>,
    /// Grid width (number of columns)
    width: usize,
    /// Fixed boundary values (north for top region, south for bottom region)
    fixed_boundary: Vec<f64>,
}

impl GridRegionActor {
    /// Create a new grid region actor
    ///
    /// ## Arguments
    /// - `region_id`: Unique identifier for this region
    /// - `width`: Number of columns in the grid
    /// - `initial`: Initial temperature values
    /// - `fixed_boundary`: Fixed boundary values (north for top, south for bottom)
    fn new(region_id: usize, width: usize, initial: Vec<f64>, fixed_boundary: Vec<f64>) -> Self {
        Self {
            region_id,
            data: initial,
            width,
            fixed_boundary,
        }
    }

    /// Compute new values using 5-point stencil with ghost cells
    ///
    /// ## Stencil Pattern
    /// 5-point stencil averages four neighbors (north, south, east, west):
    /// ```
    ///     N
    ///   W C E
    ///     S
    /// ```
    /// New value = (W + E + N + S) / 4.0
    ///
    /// ## Arguments
    /// - `north`: Ghost cells from north neighbor (or fixed boundary for top region)
    /// - `south`: Ghost cells from south neighbor (or fixed boundary for bottom region)
    ///
    /// ## Returns
    /// Tuple of (new_data, max_diff) where:
    /// - `new_data`: Updated temperature values after one iteration
    /// - `max_diff`: Maximum absolute change across all cells (for convergence check)
    fn compute_with_boundaries(&self, north: &[f64], south: &[f64]) -> (Vec<f64>, f64) {
        let mut new_data = self.data.clone();
        let mut max_diff = 0.0f64;

        // Interior cells only (boundaries at indices 0 and len-1 are fixed)
        for i in 1..self.data.len() - 1 {
            let left = self.data[i - 1];
            let right = self.data[i + 1];
            let top = north[i];
            let bottom = south[i];
            // 5-point stencil: average of four neighbors
            let new_val = (left + right + top + bottom) / 4.0;
            max_diff = max_diff.max((new_val - self.data[i]).abs());
            new_data[i] = new_val;
        }

        (new_data, max_diff)
    }
}

#[plexspaces_handlers(gen_server)]
impl GridRegionActor {
    /// Handle compute request for an iteration
    ///
    /// ## Request Format
    /// ```json
    /// {
    ///   "iteration": 1,
    ///   "tuplespace": "reference to TupleSpace"
    /// }
    /// ```
    ///
    /// ## Response Format
    /// ```json
    /// {
    ///   "max_diff": 12.5,
    ///   "converged": false
    /// }
    /// ```
    #[handler("compute")]
    async fn handle_compute(
        &mut self,
        ctx: &ActorContext,
        msg: &Message,
    ) -> Result<serde_json::Value, BehaviorError> {
        let request: serde_json::Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid compute request: {}", e)))?;

        let iteration = request["iteration"]
            .as_u64()
            .ok_or_else(|| BehaviorError::ProcessingError("Missing iteration".to_string()))? as usize;

        // Get TupleSpace from ActorContext
        let tuplespace_provider = ctx.get_tuplespace().await
            .ok_or_else(|| BehaviorError::ProcessingError("TupleSpace not available".to_string()))?;

        // Write phase: publish boundary to TupleSpace
        let boundary_data = serde_json::to_vec(&self.data)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))?;

        let south_tuple = Tuple::new(vec![
            TupleField::String("boundary".to_string()),
            TupleField::Integer(iteration as i64),
            TupleField::Integer(self.region_id as i64),
            TupleField::String("south".to_string()),
            TupleField::Binary(boundary_data.clone()),
        ]);

        tuplespace_provider.write(south_tuple).await
            .map_err(|e| BehaviorError::ProcessingError(format!("TupleSpace write error: {}", e)))?;

        let north_tuple = Tuple::new(vec![
            TupleField::String("boundary".to_string()),
            TupleField::Integer(iteration as i64),
            TupleField::Integer(self.region_id as i64),
            TupleField::String("north".to_string()),
            TupleField::Binary(boundary_data),
        ]);

        tuplespace_provider.write(north_tuple).await
            .map_err(|e| BehaviorError::ProcessingError(format!("TupleSpace write error: {}", e)))?;

        // Read phase: get neighbor boundaries from TupleSpace
        // Neighbor matching logic:
        // - North neighbor: region_id - 1 (top region has no north neighbor)
        // - South neighbor: region_id + 1 (bottom region has no south neighbor)
        // Edge regions fall back to fixed boundaries if neighbor doesn't exist
        let north_neighbor_id = if self.region_id > 0 { self.region_id - 1 } else { usize::MAX };
        let south_neighbor_id = self.region_id + 1;

        // Read neighbor boundaries (only if neighbors exist)
        let north_tuples = if north_neighbor_id != usize::MAX {
            let north_pattern = Pattern::new(vec![
                PatternField::Exact(TupleField::String("boundary".to_string())),
                PatternField::Exact(TupleField::Integer(iteration as i64)),
                PatternField::Exact(TupleField::Integer(north_neighbor_id as i64)),
                PatternField::Exact(TupleField::String("south".to_string())), // Neighbor's south = our north
                PatternField::Wildcard,
            ]);
            tuplespace_provider.read(&north_pattern).await
                .map_err(|e| BehaviorError::ProcessingError(format!("TupleSpace read error: {}", e)))?
        } else {
            Vec::new() // Top region - no north neighbor
        };

        // Read south neighbor boundary
        // If south_neighbor_id doesn't exist (bottom region), read will return empty and we use fixed boundary
        let south_pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("boundary".to_string())),
            PatternField::Exact(TupleField::Integer(iteration as i64)),
            PatternField::Exact(TupleField::Integer(south_neighbor_id as i64)),
            PatternField::Exact(TupleField::String("north".to_string())), // Neighbor's north = our south
            PatternField::Wildcard,
        ]);
        let south_tuples = tuplespace_provider.read(&south_pattern).await
            .map_err(|e| BehaviorError::ProcessingError(format!("TupleSpace read error: {}", e)))?;

        // Extract neighbor data from tuples (fallback to fixed boundary if no neighbor found)
        // Tuple structure: ("boundary", iteration, region_id, edge, data)
        // Field index 4 contains serialized Vec<f64> boundary data
        let north_ghost = if let Some(tuple) = north_tuples.first() {
            if let Some(TupleField::Binary(data)) = tuple.fields().get(4) {
                serde_json::from_slice::<Vec<f64>>(data)
                    .unwrap_or_else(|_| self.fixed_boundary.clone())
            } else {
                self.fixed_boundary.clone()
            }
        } else {
            // No north neighbor found (top region) - use fixed boundary
            self.fixed_boundary.clone()
        };

        let south_ghost = if let Some(tuple) = south_tuples.first() {
            if let Some(TupleField::Binary(data)) = tuple.fields().get(4) {
                serde_json::from_slice::<Vec<f64>>(data)
                    .unwrap_or_else(|_| self.fixed_boundary.clone())
            } else {
                self.fixed_boundary.clone()
            }
        } else {
            // No south neighbor found (bottom region or neighbor not ready) - use fixed boundary
            self.fixed_boundary.clone()
        };

        // Compute phase: update values using 5-point stencil
        // This is the actual computation work (coordination overhead excluded)
        let compute_start = Instant::now();
        let (new_data, max_diff) = self.compute_with_boundaries(&north_ghost, &south_ghost);
        self.data = new_data;
        let compute_time = compute_start.elapsed();

        // Barrier synchronization: write barrier tuple after computation completes
        // Barrier pattern: ("barrier", iteration, region_id)
        // Coordinator waits for all regions to write barrier tuples before next iteration
        let barrier_tuple = Tuple::new(vec![
            TupleField::String("barrier".to_string()),
            TupleField::Integer(iteration as i64),
            TupleField::Integer(self.region_id as i64),
        ]);
        tuplespace_provider.write(barrier_tuple.clone()).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Barrier write error: {}", e)))?;
        
        tracing::debug!("Region {} wrote barrier tuple for iteration {}", self.region_id, iteration);

        // Return result with convergence status and computation time
        // Coordinator uses max_diff to check convergence across all regions
        Ok(json!({
            "max_diff": max_diff,
            "converged": max_diff < 0.5, // Tolerance threshold
            "compute_time_ms": compute_time.as_millis() as u64,
        }))
    }
}

// =============================================================================
// Main - Demonstrates TupleSpace coordination for stencil computation
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("heat_diffusion=debug,plexspaces=warn"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║       Heat Diffusion with TupleSpace Coordination              ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    info!("TupleSpace: ghost cell exchange, barrier synchronization");
    info!("SDK: gen_server_actor, spawn(), GenServerRef");
    println!();

    // Configuration: Use non-trivial data sizes (run for several seconds to show real metrics)
    let width = 1000; // 1000 columns per region (increased for substantial computation)
    let num_regions = 8; // 8 horizontal strips (increased parallelism)
    let max_iterations = 100;
    let tolerance = 0.5;

    info!("Configuration:");
    info!("  Grid width: {} columns", width);
    info!("  Regions: {} horizontal strips", num_regions);
    info!("  Max iterations: {}", max_iterations);
    info!("  Tolerance: {}", tolerance);
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("heat-diffusion".to_string());
    let total_start = Instant::now();

    // RequestContext: explicit tenant/namespace for multi-tenancy
    let tenant_id = "heat-sim";
    let namespace = "diffusion";
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Setup: Node and TupleSpace
    let node = NodeBuilder::new("heat-node".to_string())
        .build_started()
        .await;
    let service_locator = node.service_locator();

    // Create TupleSpace and register with ServiceLocator so actors can access it
    let tuplespace = Arc::new(TupleSpace::with_tenant_namespace(tenant_id, namespace));
    let tuplespace_provider: Arc<dyn TupleSpaceProvider> = Arc::new(TupleSpaceProviderWrapper::new(tuplespace.clone()));
    service_locator.register_tuplespace_provider(tuplespace_provider).await;

    // =========================================================================
    // Step 1: Spawn region actors
    // =========================================================================
    info!("Step 1: Spawn {} region actors", num_regions);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let mut region_refs: Vec<GenServerRef> = Vec::new();

    for region_id in 0..num_regions {
        // Initial temperature: linear gradient from cold (top) to hot (bottom)
        let initial_temp = (region_id as f64) * (100.0 / (num_regions - 1) as f64);
        let initial_data = vec![initial_temp; width];

        // Fixed boundaries: cold at top (region 0), hot at bottom (last region)
        let fixed_boundary = if region_id == 0 {
            vec![0.0; width] // Cold top
        } else if region_id == num_regions - 1 {
            vec![100.0; width] // Hot bottom
        } else {
            vec![initial_temp; width] // Interior regions use initial temp
        };

        let actor = GridRegionActor::new(region_id, width, initial_data, fixed_boundary);
        let actor_name = format!("region-{}", region_id);

        let actor_ref = spawn(&ctx, service_locator.clone(), actor_name, namespace, actor).await
            .map_err(|e| anyhow::anyhow!("Failed to spawn region {}: {}", region_id, e))?;

        let region_ref = GenServerRef::new(actor_ref);
        region_refs.push(region_ref);

        if region_id < 3 || region_id == num_regions - 1 {
            info!("  Spawned region-{} (initial temp: {:.1}°C)", region_id, initial_temp);
        }
    }
    println!();

    // =========================================================================
    // Step 2: Run diffusion iterations with TupleSpace coordination
    // =========================================================================
    info!("Step 2: Run diffusion with TupleSpace ghost cell exchange");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let mut converged = false;
    let mut final_iteration = 0;
    let mut iteration_metrics: Vec<(usize, Duration, Duration, Duration, f64)> = Vec::new();

    for iteration in 1..=max_iterations {
        let iteration_start = Instant::now();

        // Computation phase: send requests and wait for actor responses
        // Actors perform stencil computation and write barrier tuples after compute
        let request = json!({
            "iteration": iteration,
        });
        let mut max_diff: f64 = 0.0;
        
        metrics_tracker.start_compute();
        let compute_phase_start = Instant::now();
        for region_ref in &region_refs {
            let result: serde_json::Value = region_ref.call("compute", &request).await
                .map_err(|e| anyhow::anyhow!("Region compute failed: {}", e))?;
            
            let diff = result["max_diff"].as_f64().unwrap_or(0.0);
            max_diff = max_diff.max(diff);
            if result["converged"].as_bool().unwrap_or(false) {
                converged = true;
            }
            
            metrics_tracker.increment_message();
        }
        let compute_phase_time = compute_phase_start.elapsed();
        metrics_tracker.end_compute();

        // Coordination phase: verify barrier synchronization
        // All actors have written barrier tuples during compute phase
        // Since we await all actor responses above, barrier tuples should already exist
        // This verification measures coordination overhead (tuple space operations)
        metrics_tracker.start_coordinate();
        let coordinate_phase_start = Instant::now();
        let barrier_pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("barrier".to_string())),
            PatternField::Exact(TupleField::Integer(iteration as i64)),
            PatternField::Wildcard, // Any region ID
        ]);
        
        // Verify all barrier tuples exist (actors write them during compute)
        // Since we await all actor responses sequentially, tuples should be written by now
        // This is a quick verification to measure coordination overhead
        let count = tuplespace.count(barrier_pattern.clone()).await
            .unwrap_or(0);
        tracing::debug!("Iteration {}: Found {}/{} barrier tuples", iteration, count, num_regions);
        if count < num_regions {
            return Err(anyhow::anyhow!("Barrier verification failed: only {}/{} barrier tuples found", count, num_regions));
        }
        
        let coordinate_phase_time = coordinate_phase_start.elapsed();
        metrics_tracker.increment_barrier();
        metrics_tracker.end_coordinate();

        let iteration_time = iteration_start.elapsed();
        
        // Store iteration metrics for detailed reporting
        iteration_metrics.push((
            iteration,
            compute_phase_time,
            coordinate_phase_time,
            iteration_time,
            max_diff,
        ));

        if iteration <= 5 || iteration % 20 == 0 {
            let compute_pct = (compute_phase_time.as_secs_f64() / iteration_time.as_secs_f64()) * 100.0;
            let coord_pct = (coordinate_phase_time.as_secs_f64() / iteration_time.as_secs_f64()) * 100.0;
            info!("  Iteration {}: max_diff={:.4}, total={:.2}ms (compute={:.2}ms/{:.1}%, coord={:.2}ms/{:.1}%)", 
                  iteration, max_diff, 
                  iteration_time.as_secs_f64() * 1000.0,
                  compute_phase_time.as_secs_f64() * 1000.0, compute_pct,
                  coordinate_phase_time.as_secs_f64() * 1000.0, coord_pct);
        }

        if converged || max_diff < tolerance {
            final_iteration = iteration;
            if converged {
                info!("  Converged at iteration {} (diff {:.4} < {:.2})", iteration, max_diff, tolerance);
            }
            break;
        }
    }
    println!();

    // =========================================================================
    // Step 3: Benchmarks (eprintln! to stderr so visible when stdout is piped/buffered)
    // =========================================================================
    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();

    let coordinate_time = Duration::from_millis(metrics.coordinate_duration_ms);
    let compute_time = Duration::from_millis(metrics.compute_duration_ms);
    let total_data_points = width * num_regions * final_iteration;
    let total_time_s = total_time.as_secs_f64();
    let total_time_ms = total_time_s * 1000.0;
    let coord_ms = coordinate_time.as_secs_f64() * 1000.0;
    let comp_ms = compute_time.as_secs_f64() * 1000.0;
    let coord_pct = if total_time_s > 0.0 { (coordinate_time.as_secs_f64() / total_time_s) * 100.0 } else { 0.0 };
    let comp_pct = if total_time_s > 0.0 { (compute_time.as_secs_f64() / total_time_s) * 100.0 } else { 0.0 };

    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!("  BENCHMARKS (compute vs coord, latency, data size)");
    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!();
    eprintln!("  Data size:");
    eprintln!("    Grid:           {} columns × {} regions = {} points", width, num_regions, width * num_regions);
    eprintln!("    Iterations:    {}", final_iteration);
    eprintln!("    Points/iter:   {}", width * num_regions);
    eprintln!("    Total points:  {} (stencil updates)", total_data_points);
    eprintln!();
    eprintln!("  Execution:");
    eprintln!("    Wall time:     {:.2} ms  ({:.2} s)", total_time_ms, total_time_s);
    eprintln!("    Compute time:  {:.2} ms  ({:.1}%)", comp_ms, comp_pct);
    eprintln!("    Coord time:    {:.2} ms  ({:.1}%)", coord_ms, coord_pct);
    eprintln!("    Efficiency:    {:.1}% (compute/total)", metrics.efficiency * 100.0);
    eprintln!();
    eprintln!("  Latency & throughput:");
    eprintln!("    Messages:      {}", metrics.message_count);
    eprintln!("    Barriers:      {}", metrics.barrier_count);
    if metrics.message_count > 0 {
        let avg_latency_ms = comp_ms / metrics.message_count as f64;
        let throughput = metrics.message_count as f64 / total_time_s;
        eprintln!("    Avg latency:   {:.2} ms/msg", avg_latency_ms);
        eprintln!("    Throughput:    {:.1} msg/s", throughput);
    }
    eprintln!();

    if !iteration_metrics.is_empty() {
        let avg_iter_time: f64 = iteration_metrics.iter()
            .map(|(_, _, _, it, _)| it.as_secs_f64() * 1000.0)
            .sum::<f64>() / iteration_metrics.len() as f64;
        let avg_compute_time: f64 = iteration_metrics.iter()
            .map(|(_, ct, _, _, _)| ct.as_secs_f64() * 1000.0)
            .sum::<f64>() / iteration_metrics.len() as f64;
        let avg_coord_time: f64 = iteration_metrics.iter()
            .map(|(_, _, cot, _, _)| cot.as_secs_f64() * 1000.0)
            .sum::<f64>() / iteration_metrics.len() as f64;
        eprintln!("  Per-iteration (avg):  total={:.2} ms  compute={:.2} ms  coord={:.2} ms", avg_iter_time, avg_compute_time, avg_coord_time);
        eprintln!();
    }

    eprintln!("  Granularity (compute/coord):  {:.2}x", metrics.granularity_ratio);
    if coordinate_time.as_secs_f64() > 0.0 {
        if metrics.granularity_ratio >= 100.0 {
            eprintln!("    (excellent: coordination negligible)");
        } else if metrics.granularity_ratio >= 10.0 {
            eprintln!("    (good: low coordination overhead)");
        } else if metrics.granularity_ratio >= 1.0 {
            eprintln!("    (moderate: coordination noticeable)");
        } else {
            eprintln!("    (poor: coordination dominates)");
        }
    }
    eprintln!();
    eprintln!("  Errors: 0");
    eprintln!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    eprintln!("  Heat Diffusion Example Complete");
    eprintln!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
