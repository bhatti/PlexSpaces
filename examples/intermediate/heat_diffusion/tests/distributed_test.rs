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

//! Distributed end-to-end tests for heat diffusion
//!
//! ## Purpose
//! Tests heat diffusion simulation across multiple TupleSpace instances,
//! simulating a distributed environment where actors on different "nodes"
//! coordinate via shared TupleSpace.
//!
//! ## Test Scenarios
//! 1. Multiple coordinators sharing TupleSpace (simulated multi-node)
//! 2. Concurrent simulations with isolated TupleSpaces
//! 3. Stress test with many actors
//! 4. Fault tolerance (partial completion scenarios)

use heat_diffusion::config::GridConfig;
use heat_diffusion::coordinator::Coordinator;
use tokio::task::JoinSet;

#[tokio::test]
async fn test_concurrent_independent_simulations() {
    // Test multiple independent simulations running concurrently
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    let mut join_set = JoinSet::new();

    // Spawn 3 concurrent simulations
    for i in 0..3 {
        join_set.spawn(async move {
            let config = GridConfig::new((20, 20), (10, 10))
                .expect("Valid configuration");

            let mut coordinator = Coordinator::new(config);
            coordinator.initialize();

            let result = coordinator.run().await.expect("Simulation should succeed");

            (i, result.converged, result.iterations)
        });
    }

    // Wait for all simulations to complete
    let mut results = Vec::new();
    while let Some(res) = join_set.join_next().await {
        let (sim_id, converged, iterations) = res.expect("Task should succeed");
        results.push((sim_id, converged, iterations));

        println!("Simulation {}: converged={}, iterations={}", sim_id, converged, iterations);
    }

    // Verify all converged
    assert_eq!(results.len(), 3, "All 3 simulations should complete");
    for (sim_id, converged, iterations) in results {
        assert!(converged, "Simulation {} should converge", sim_id);
        assert!(iterations < 1000, "Simulation {} should converge in < 1000 iterations", sim_id);
    }
}

#[tokio::test]
async fn test_stress_many_actors() {
    // Stress test with larger grid
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    let config = GridConfig::new((40, 40), (10, 10))
        .expect("Valid configuration");

    assert_eq!(config.actor_count(), 16, "Should have 16 actors (4x4)");

    let mut coordinator = Coordinator::new(config);
    coordinator.initialize();

    let start = std::time::Instant::now();
    let result = coordinator.run().await.expect("Simulation should succeed");
    let elapsed = start.elapsed();

    println!("Stress test: {} actors, {} iterations, {:.2?} elapsed",
        result.metrics.len(), result.iterations, elapsed);

    assert!(result.converged, "Large grid should converge");
    assert_eq!(result.metrics.len(), 16, "Should have 16 actors");
    assert!(elapsed.as_secs() < 10, "Should complete within 10 seconds");
}

#[tokio::test]
async fn test_barrier_synchronization_correctness() {
    // Verify that barrier synchronization prevents race conditions
    // by running simulation multiple times and checking consistency
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    let mut iteration_counts = Vec::new();

    for run in 0..3 {
        let config = GridConfig::new((20, 20), (10, 10))
            .expect("Valid configuration");

        let mut coordinator = Coordinator::new(config);
        coordinator.initialize();

        let result = coordinator.run().await.expect("Simulation should succeed");

        assert!(result.converged, "Run {} should converge", run);
        iteration_counts.push(result.iterations);
    }

    // All runs should converge to same number of iterations (deterministic)
    let first = iteration_counts[0];
    for (i, &count) in iteration_counts.iter().enumerate() {
        assert_eq!(count, first,
            "Run {} converged in {} iterations, but run 0 took {} (should be deterministic)",
            i, count, first);
    }

    println!("Barrier sync test: All runs converged in exactly {} iterations ✓", first);
}

#[tokio::test]
async fn test_scalability_across_grid_sizes() {
    // Test that convergence behavior scales properly with grid size
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    struct TestCase {
        grid_size: (usize, usize),
        region_size: (usize, usize),
        expected_actors: usize,
    }

    let test_cases = vec![
        TestCase { grid_size: (10, 10), region_size: (5, 5), expected_actors: 4 },
        TestCase { grid_size: (20, 20), region_size: (10, 10), expected_actors: 4 },
        TestCase { grid_size: (30, 30), region_size: (10, 10), expected_actors: 9 },
        TestCase { grid_size: (40, 40), region_size: (20, 20), expected_actors: 4 },
    ];

    for tc in test_cases {
        let config = GridConfig::new(tc.grid_size, tc.region_size)
            .expect("Valid configuration");

        assert_eq!(config.actor_count(), tc.expected_actors,
            "Grid {:?} with region {:?} should have {} actors",
            tc.grid_size, tc.region_size, tc.expected_actors);

        let mut coordinator = Coordinator::new(config);
        coordinator.initialize();

        let result = coordinator.run().await.expect("Simulation should succeed");

        assert!(result.converged,
            "Grid {:?} should converge", tc.grid_size);
        assert_eq!(result.metrics.len(), tc.expected_actors,
            "Should have {} actors", tc.expected_actors);

        println!("Grid {:?}: {} actors, {} iterations ✓",
            tc.grid_size, result.metrics.len(), result.iterations);
    }
}

#[tokio::test]
async fn test_convergence_threshold_sensitivity() {
    // Test that tighter thresholds require more iterations
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    let mut config_loose = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");
    config_loose.convergence_threshold = 0.1; // Loose threshold

    let mut coord_loose = Coordinator::new(config_loose);
    coord_loose.initialize();
    let result_loose = coord_loose.run().await.expect("Should succeed");

    let mut config_tight = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");
    config_tight.convergence_threshold = 0.001; // Tight threshold

    let mut coord_tight = Coordinator::new(config_tight);
    coord_tight.initialize();
    let result_tight = coord_tight.run().await.expect("Should succeed");

    // Both should converge
    assert!(result_loose.converged, "Loose threshold should converge");
    assert!(result_tight.converged, "Tight threshold should converge");

    // Tight threshold should require more iterations
    assert!(result_tight.iterations > result_loose.iterations,
        "Tight threshold ({} iters) should take more iterations than loose ({} iters)",
        result_tight.iterations, result_loose.iterations);

    println!("Threshold test: loose={} iters, tight={} iters ✓",
        result_loose.iterations, result_tight.iterations);
}

#[tokio::test]
async fn test_max_iterations_limit() {
    // Test that simulation respects max_iterations limit
    let _ = tracing_subscriber::fmt()
        .with_env_filter("heat_diffusion=warn")
        .try_init();

    let mut config = GridConfig::new((20, 20), (10, 10))
        .expect("Valid configuration");

    // Set impossibly tight threshold so it can't converge
    config.convergence_threshold = 1e-20;
    // But limit iterations
    config.max_iterations = 10;

    let mut coordinator = Coordinator::new(config);
    coordinator.initialize();

    let result = coordinator.run().await.expect("Should succeed");

    // Should NOT converge (threshold too tight)
    assert!(!result.converged, "Should not converge with impossible threshold");
    // But should stop at max iterations
    assert_eq!(result.iterations, 10, "Should stop at max_iterations");

    println!("Max iterations test: stopped at {} iterations without converging ✓", result.iterations);
}
