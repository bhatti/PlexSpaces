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

//! End-to-End Tests with Detailed Metrics
//!
//! ## Purpose
//! Comprehensive E2E tests with detailed output showing:
//! - Computation vs Coordination metrics
//! - Granularity ratios
//! - Performance characteristics
//! - Convergence behavior

use heat_diffusion::config::GridConfig;
use heat_diffusion::coordinator::Coordinator;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;

/// Helper to print detailed metrics
fn print_detailed_metrics(config: &GridConfig, result: &heat_diffusion::coordinator::SimulationResult) {
    println!("\n========== E2E TEST RESULTS ==========");
    println!("Grid: {:?}, Region: {:?}", config.grid_size, config.region_size);
    println!("Actors: {} ({} rows × {} cols)",
        result.metrics.step_metrics.len(),
        config.actor_grid_dimensions().0,
        config.actor_grid_dimensions().1);
    println!("Cells per actor: {}", config.cells_per_actor());
    println!();

    println!("Convergence:");
    println!("  Converged: {}", result.converged);
    println!("  Iterations: {}", result.iterations);
    println!();

    println!("Performance Metrics:");
    let total_compute: f64 = result.metrics.compute_duration_ms as f64;
    let total_coord: f64 = result.metrics.coordinate_duration_ms as f64;
    let avg_ratio: f64 = result.metrics.granularity_ratio;

    println!("  Total Compute Time:   {:.2} ms", total_compute);
    println!("  Total Coordinate Time: {:.2} ms", total_coord);
    println!("  Avg Granularity Ratio: {:.4}", avg_ratio);
    println!("  Target Ratio:          {:.2}", config.target_ratio);

    if avg_ratio < config.target_ratio {
        println!("  ⚠️  Ratio < target (expected for small test grids)");
    } else {
        println!("  ✓ Ratio meets target!");
    }
    println!();

    println!("Per-Step Breakdown:");
    for (i, metric) in result.metrics.step_metrics.iter().enumerate() {
        println!("  Step {}: compute={}ms, coord={}ms, messages={}",
            i, metric.compute_ms, metric.coordinate_ms, metric.message_count);
    }
    println!("Overall: compute={}ms, coord={}ms, ratio={:.4}",
        result.metrics.compute_duration_ms, result.metrics.coordinate_duration_ms,
        result.metrics.granularity_ratio);
    println!("======================================\n");
}

#[tokio::test]
async fn e2e_small_grid_with_metrics() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=info")
        .try_init();

    println!("\n🔬 E2E Test: Small Grid (10×10)");

    let config = GridConfig::new((10, 10), (5, 5))
        .expect("Valid configuration");

    let node = Arc::new(NodeBuilder::new("test-node")
        .build()
        .await);
    let mut coordinator = Coordinator::new(config.clone());
    coordinator.initialize(node).await.expect("Should initialize");

    let result = coordinator.run().await.expect("Should succeed");

    assert!(result.converged, "Should converge");
    print_detailed_metrics(&config, &result);

    // Verify metrics make sense
    assert!(result.metrics.compute_duration_ms > 0, "Should have compute time");
    assert!(result.metrics.coordinate_duration_ms > 0, "Should have coordinate time");
}

#[tokio::test]
async fn e2e_medium_grid_with_metrics() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=info")
        .try_init();

    println!("\n🔬 E2E Test: Medium Grid (30×30)");

    let config = GridConfig::new((30, 30), (10, 10))
        .expect("Valid configuration");

    let node = Arc::new(NodeBuilder::new("test-node")
        .build()
        .await);
    let mut coordinator = Coordinator::new(config.clone());
    coordinator.initialize(node).await.expect("Should initialize");

    let result = coordinator.run().await.expect("Should succeed");

    assert!(result.converged, "Should converge");
    print_detailed_metrics(&config, &result);

    // Verify granularity calculations
    let avg_ratio: f64 = result.metrics.granularity_ratio;

    assert!(avg_ratio > 0.0, "Granularity ratio should be positive");
}

#[tokio::test]
async fn e2e_large_grid_stress_test() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    println!("\n🔬 E2E Test: Large Grid Stress Test (40×40, 16 actors)");

    let config = GridConfig::new((40, 40), (10, 10))
        .expect("Valid configuration");

    let node = Arc::new(NodeBuilder::new("test-node")
        .build()
        .await);
    let mut coordinator = Coordinator::new(config.clone());
    coordinator.initialize(node).await.expect("Should initialize");

    let start = std::time::Instant::now();
    let result = coordinator.run().await.expect("Should succeed");
    let elapsed = start.elapsed();

    assert!(result.converged, "Should converge");
    print_detailed_metrics(&config, &result);

    println!("Wall-clock time: {:.2?}", elapsed);
    println!("Performance: {:.0} iterations/sec", result.iterations as f64 / elapsed.as_secs_f64());

    // Should complete in reasonable time
    assert!(elapsed.as_secs() < 10, "Should complete within 10 seconds");
}

#[tokio::test]
async fn e2e_granularity_comparison() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    println!("\n🔬 E2E Test: Granularity Comparison");

    // Fine-grained: 16 actors (smaller regions)
    let config_fine = GridConfig::new((40, 40), (10, 10))
        .expect("Valid configuration");

    let node_fine = Arc::new(NodeBuilder::new("test-node-fine")
        .build()
        .await);
    let mut coord_fine = Coordinator::new(config_fine.clone());
    coord_fine.initialize(node_fine).await.expect("Should initialize");
    let result_fine = coord_fine.run().await.expect("Should succeed");

    // Coarse-grained: 4 actors (larger regions)
    let config_coarse = GridConfig::new((40, 40), (20, 20))
        .expect("Valid configuration");

    let node_coarse = Arc::new(NodeBuilder::new("test-node-coarse")
        .build()
        .await);
    let mut coord_coarse = Coordinator::new(config_coarse.clone());
    coord_coarse.initialize(node_coarse).await.expect("Should initialize");
    let result_coarse = coord_coarse.run().await.expect("Should succeed");

    println!("\n📊 Granularity Comparison:");
    println!("\nFine-grained (16 actors, 10×10 regions):");
    print_detailed_metrics(&config_fine, &result_fine);

    println!("\nCoarse-grained (4 actors, 20×20 regions):");
    print_detailed_metrics(&config_coarse, &result_coarse);

    // Coarse should have better granularity ratio
    let fine_ratio: f64 = result_fine.metrics.granularity_ratio;
    let coarse_ratio: f64 = result_coarse.metrics.granularity_ratio;

    println!("📈 Analysis:");
    println!("  Fine-grained ratio:   {:.4}", fine_ratio);
    println!("  Coarse-grained ratio: {:.4}", coarse_ratio);

    if coarse_ratio > fine_ratio {
        println!("  ✓ Coarse-grained has better ratio (as expected)");
    } else {
        println!("  ⚠️  Fine-grained has better ratio (unexpected, may be due to overhead)");
    }
}

#[tokio::test]
async fn e2e_convergence_behavior() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    println!("\n🔬 E2E Test: Convergence Behavior");

    let mut config = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");

    // Loose threshold
    config.convergence_threshold = 0.1;
    let node_loose = Arc::new(NodeBuilder::new("test-node-loose")
        .build()
        .await);
    let mut coord_loose = Coordinator::new(config.clone());
    coord_loose.initialize(node_loose).await.expect("Should initialize");
    let result_loose = coord_loose.run().await.expect("Should succeed");

    // Tight threshold
    config.convergence_threshold = 0.001;
    let node_tight = Arc::new(NodeBuilder::new("test-node-tight")
        .build()
        .await);
    let mut coord_tight = Coordinator::new(config.clone());
    coord_tight.initialize(node_tight).await.expect("Should initialize");
    let result_tight = coord_tight.run().await.expect("Should succeed");

    println!("\n📊 Convergence Threshold Analysis:");
    println!("\nLoose threshold (0.1):");
    println!("  Iterations: {}", result_loose.iterations);
    println!("  Converged: {}", result_loose.converged);

    println!("\nTight threshold (0.001):");
    println!("  Iterations: {}", result_tight.iterations);
    println!("  Converged: {}", result_tight.converged);

    println!("\n📈 Analysis:");
    println!("  Iteration difference: {}", result_tight.iterations - result_loose.iterations);

    assert!(result_tight.iterations > result_loose.iterations,
        "Tighter threshold should require more iterations");

    println!("  ✓ Tighter threshold requires more iterations (as expected)");
}
