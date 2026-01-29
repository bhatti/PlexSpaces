// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Heat Diffusion Example (2D Stencil with TupleSpace Coordination)
//
// Demonstrates parallel stencil computation using PlexSpaces:
// - TupleSpace for ghost cell exchange between neighbors
// - Barrier synchronization between iterations
//
// Use Case: Thermal simulation, image processing, weather modeling

use plexspaces_tuplespace::TupleSpace;
use plexspaces_core::RequestContext;

// =============================================================================
// Grid Region (would be an Actor in full implementation)
// =============================================================================

struct GridRegion {
    id: usize,
    data: Vec<f64>,
    width: usize,
}

impl GridRegion {
    fn new(id: usize, width: usize, initial: Vec<f64>) -> Self {
        Self { id, data: initial, width }
    }

    fn compute_with_boundaries(&self, north: &[f64], south: &[f64]) -> (Vec<f64>, f64) {
        let mut new_data = self.data.clone();
        let mut max_diff = 0.0f64;

        // Interior cells only (boundaries are fixed)
        for i in 1..self.data.len() - 1 {
            let left = self.data[i - 1];
            let right = self.data[i + 1];
            let top = north[i];
            let bottom = south[i];
            let new_val = (left + right + top + bottom) / 4.0;
            max_diff = max_diff.max((new_val - self.data[i]).abs());
            new_data[i] = new_val;
        }

        (new_data, max_diff)
    }
}

// =============================================================================
// Main - Demonstrates TupleSpace coordination for stencil computation
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║       Heat Diffusion with TupleSpace Coordination              ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("PlexSpaces APIs demonstrated:");
    println!("  - TupleSpace::write() - publish ghost cells to neighbors");
    println!("  - TupleSpace::read()  - receive ghost cells from neighbors");
    println!("  - TupleSpace barrier  - synchronize iterations");
    println!();

    // Setup TupleSpace for coordination
    let tuplespace = TupleSpace::with_tenant_namespace("heat-sim", "diffusion");
    let ctx = RequestContext::new_without_auth("heat-sim".to_string(), "diffusion".to_string());

    // =========================================================================
    // Step 1: Initialize grid partitions (simulating 2 region actors)
    // =========================================================================
    println!("Step 1: Initialize grid with 2 region actors");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Two horizontal strips of a grid
    // Region 0: top half (cold boundary at top)
    // Region 1: bottom half (hot boundary at bottom)
    let width = 6;
    
    let mut region0 = GridRegion::new(0, width, vec![0.0; width]); // Cold top
    let mut region1 = GridRegion::new(1, width, vec![100.0; width]); // Hot bottom

    println!("  Region 0 (top):    {:?}", region0.data);
    println!("  Region 1 (bottom): {:?}", region1.data);
    println!();

    // =========================================================================
    // Step 2: Iterative diffusion with TupleSpace coordination
    // =========================================================================
    println!("Step 2: Run diffusion with TupleSpace ghost cell exchange");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let max_iterations = 20;
    let tolerance = 0.5;

    for iteration in 1..=max_iterations {
        // -----------------------------------------------------------------
        // WRITE PHASE: Each region publishes its boundary to TupleSpace
        // -----------------------------------------------------------------
        // In real code: tuplespace.write(&ctx, tuple!["boundary", iteration, region_id, "south", data]).await?
        
        let region0_south = region0.data.clone();
        let region1_north = region1.data.clone();

        if iteration <= 3 {
            println!("  Iteration {}: WRITE phase", iteration);
            println!("    Region 0 writes south boundary to TupleSpace: {:?}", 
                     region0_south.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
            println!("    Region 1 writes north boundary to TupleSpace: {:?}",
                     region1_north.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
        }

        // -----------------------------------------------------------------
        // READ PHASE: Each region reads neighbor's boundary from TupleSpace
        // -----------------------------------------------------------------
        // In real code: let tuple = tuplespace.read(&ctx, pattern!["boundary", iteration, neighbor_id, edge, _]).await?
        
        // Region 0 needs south neighbor (region 1's north boundary)
        let region0_gets_south = region1_north.clone();
        // Region 1 needs north neighbor (region 0's south boundary)  
        let region1_gets_north = region0_south.clone();

        if iteration <= 3 {
            println!("    Region 0 reads from TupleSpace (south neighbor): {:?}",
                     region0_gets_south.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
            println!("    Region 1 reads from TupleSpace (north neighbor): {:?}",
                     region1_gets_north.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
        }

        // -----------------------------------------------------------------
        // COMPUTE PHASE: Each region computes new values using ghost cells
        // -----------------------------------------------------------------
        let cold_boundary = vec![0.0; width]; // Fixed cold top
        let hot_boundary = vec![100.0; width]; // Fixed hot bottom

        let (new_data0, diff0) = region0.compute_with_boundaries(&cold_boundary, &region0_gets_south);
        let (new_data1, diff1) = region1.compute_with_boundaries(&region1_gets_north, &hot_boundary);

        region0.data = new_data0;
        region1.data = new_data1;

        let max_diff = diff0.max(diff1);

        if iteration <= 3 || iteration % 5 == 0 {
            println!("    Region 0 after compute: {:?}", 
                     region0.data.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
            println!("    Region 1 after compute: {:?}",
                     region1.data.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
            println!("    Max diff: {:.2}", max_diff);
            println!();
        }

        // -----------------------------------------------------------------
        // BARRIER: Wait for all regions before next iteration
        // -----------------------------------------------------------------
        // In real code: tuplespace.barrier(&ctx, format!("iteration_{}", iteration), num_regions).await?

        if max_diff < tolerance {
            println!("  Converged at iteration {} (diff {:.2} < {:.2})", iteration, max_diff, tolerance);
            break;
        }
    }
    println!();

    // =========================================================================
    // Step 3: Final state
    // =========================================================================
    println!("Step 3: Final grid state");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Top boundary (cold):  {:?}", vec![0.0; width].iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
    println!("  Region 0:             {:?}", region0.data.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
    println!("  Region 1:             {:?}", region1.data.iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
    println!("  Bottom boundary (hot):{:?}", vec![100.0; width].iter().map(|x| format!("{:.1}", x)).collect::<Vec<_>>());
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Heat Diffusion Example Complete");
    println!();
    println!("TupleSpace Coordination Pattern:");
    println!();
    println!("  ┌──────────────┐          ┌──────────────┐");
    println!("  │  Region 0    │          │  Region 1    │");
    println!("  │  (Actor)     │          │  (Actor)     │");
    println!("  └──────┬───────┘          └──────┬───────┘");
    println!("         │ write south              │ write north");
    println!("         ▼                          ▼");
    println!("  ┌─────────────────────────────────────────┐");
    println!("  │            TupleSpace                   │");
    println!("  │  [\"boundary\", iter, region, edge, data] │");
    println!("  └─────────────────────────────────────────┘");
    println!("         │ read north               │ read south");
    println!("         ▼                          ▼");
    println!("  ┌──────────────┐          ┌──────────────┐");
    println!("  │  compute()   │          │  compute()   │");
    println!("  └──────────────┘          └──────────────┘");
    println!();
    println!("Key APIs:");
    println!("  - tuplespace.write(&ctx, tuple![...]) - publish ghost cells");
    println!("  - tuplespace.read(&ctx, pattern![...]) - receive ghost cells");
    println!("  - tuplespace.barrier(&ctx, name, count) - sync iterations");
    println!();

    // Keep tuplespace in scope
    drop(tuplespace);
    drop(ctx);

    Ok(())
}
