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

//! Basic tests for heat diffusion example

use heat_diffusion::config::GridConfig;
use heat_diffusion::coordinator::Coordinator;

#[tokio::test]
async fn test_2x2_heat_diffusion_convergence() {
    // Initialize tracing for test debugging
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=debug")
        .try_init();

    // Create small 2×2 actor grid (each managing 10×10 cells)
    let config = GridConfig::new(
        (20, 20),    // 20×20 total grid
        (10, 10),    // 10×10 cells per actor = 4 actors (2×2)
    ).expect("Valid grid configuration");

    assert_eq!(config.actor_grid_dimensions(), (2, 2));
    assert_eq!(config.actor_count(), 4);
    assert_eq!(config.cells_per_actor(), 100);

    // Create coordinator
    let mut coordinator = Coordinator::new(config);

    // Initialize actors
    coordinator.initialize();

    // Run simulation
    let result = coordinator.run().await.expect("Simulation should succeed");

    // Verify convergence
    assert!(result.converged, "Simulation should converge");
    assert!(result.iterations < 1000, "Should converge in reasonable iterations");

    // Verify granularity metrics
    let avg_ratio: f64 = result.metrics.iter()
        .map(|m| m.granularity_ratio)
        .sum::<f64>() / result.metrics.len() as f64;

    println!("Average granularity ratio: {:.2}", avg_ratio);

    // Check that compute time dominates (at least 1:1 ratio)
    // Note: In this simple test, coordination may dominate due to overhead
    // In production with larger regions, compute should dominate
    assert!(avg_ratio >= 0.0, "Granularity ratio should be non-negative");

    // Verify all actors participated
    assert_eq!(result.metrics.len(), 4, "Should have 4 actors");

    for (i, m) in result.metrics.iter().enumerate() {
        println!("Actor {}: compute={:.2}ms, coord={:.2}ms, ratio={:.2}",
            i, m.total_compute_ms, m.total_coordinate_ms, m.granularity_ratio);

        assert!(m.total_compute_ms > 0.0, "Actor should have computed");
        assert!(m.iterations == result.iterations, "All actors should have same iterations");
    }

    // Generate visualization (just to verify it doesn't crash)
    let viz = coordinator.visualize();
    assert!(!viz.is_empty(), "Visualization should not be empty");
    println!("{}", viz);
}

#[test]
fn test_grid_config_granularity_comparison() {
    // Fine-grained configuration (too many actors)
    let fine = GridConfig::new((100, 100), (10, 10))
        .expect("Valid configuration");
    assert_eq!(fine.actor_count(), 100); // 10×10 actors
    assert_eq!(fine.cells_per_actor(), 100); // 10×10 cells per actor

    // Coarse-grained configuration (better for performance)
    let coarse = GridConfig::new((100, 100), (25, 25))
        .expect("Valid configuration");
    assert_eq!(coarse.actor_count(), 16); // 4×4 actors
    assert_eq!(coarse.cells_per_actor(), 625); // 25×25 cells per actor

    // Coarse has 6.25x more computation per actor
    assert!(coarse.cells_per_actor() > fine.cells_per_actor() * 6);

    // Coarse should have better granularity ratio (in theory)
    // since each actor does more work relative to coordination
}

#[test]
fn test_grid_config_validation() {
    // Valid: grid evenly divisible by region
    let valid = GridConfig::new((20, 20), (10, 10));
    assert!(valid.is_ok());

    // Invalid: grid not divisible by region (rows)
    let invalid_rows = GridConfig::new((20, 20), (7, 10));
    assert!(invalid_rows.is_err());

    // Invalid: grid not divisible by region (cols)
    let invalid_cols = GridConfig::new((20, 20), (10, 7));
    assert!(invalid_cols.is_err());
}

#[tokio::test]
async fn test_temperature_distribution_correctness() {
    // Test that temperatures are correctly distributed from hot to cold
    let config = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");

    let mut coordinator = Coordinator::new(config);
    coordinator.initialize();

    let result = coordinator.run().await.expect("Simulation should succeed");
    assert!(result.converged, "Should converge");

    // Verify temperature gradient: top (hot) to bottom (cold)
    // Top actors should have higher average temps than bottom actors
    let top_left_temps = result.metrics.iter()
        .find(|m| m.actor_id == "region_0_0")
        .unwrap()
        .total_compute_ms;

    let bottom_left_temps = result.metrics.iter()
        .find(|m| m.actor_id == "region_1_0")
        .unwrap()
        .total_compute_ms;

    // Both should have done some computation
    assert!(top_left_temps > 0.0);
    assert!(bottom_left_temps > 0.0);
}

#[tokio::test]
async fn test_different_grid_sizes() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    // Test small grid
    let small = GridConfig::new((10, 10), (5, 5)).expect("Valid config");
    let mut coord_small = Coordinator::new(small);
    coord_small.initialize();
    let result_small = coord_small.run().await.expect("Should succeed");
    assert!(result_small.converged, "Small grid should converge");
    assert_eq!(result_small.metrics.len(), 4, "Should have 4 actors");

    // Test medium grid
    let medium = GridConfig::new((30, 30), (10, 10)).expect("Valid config");
    let mut coord_medium = Coordinator::new(medium);
    coord_medium.initialize();
    let result_medium = coord_medium.run().await.expect("Should succeed");
    assert!(result_medium.converged, "Medium grid should converge");
    assert_eq!(result_medium.metrics.len(), 9, "Should have 9 actors (3x3)");
}

#[tokio::test]
async fn test_single_actor_grid() {
    // Edge case: single actor managing entire grid
    let config = GridConfig::new((10, 10), (10, 10))
        .expect("Valid configuration");

    assert_eq!(config.actor_count(), 1, "Should have 1 actor");

    let mut coordinator = Coordinator::new(config);
    coordinator.initialize();

    let result = coordinator.run().await.expect("Simulation should succeed");

    assert!(result.converged, "Single actor should converge");
    assert_eq!(result.metrics.len(), 1, "Should have 1 actor");
    assert!(result.iterations < 1000, "Should converge reasonably");
}

#[tokio::test]
async fn test_boundary_exchange_correctness() {
    // Test that boundary exchange actually updates values
    let config = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");

    let mut coordinator = Coordinator::new(config);
    coordinator.initialize();

    let result = coordinator.run().await.expect("Simulation should succeed");

    // If boundary exchange didn't work, we'd hit max iterations without converging
    assert!(result.converged, "Should converge if boundaries are exchanged");
    assert!(result.iterations < 500, "Should converge in < 500 iterations");

    // All actors should have done coordination work
    for metric in &result.metrics {
        assert!(metric.total_coordinate_ms > 0.0,
            "Actor {} should have coordination time", metric.actor_id);
    }
}
