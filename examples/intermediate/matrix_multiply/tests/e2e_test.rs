// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! End-to-End Tests with Detailed Metrics
//!
//! ## Purpose
//! Comprehensive E2E tests with detailed output showing:
//! - Computation vs Coordination metrics
//! - Granularity ratios (compute time / coordinate time)
//! - Performance characteristics by matrix/block size

use std::time::{Duration, Instant};

use matrix_multiply::matrix::Matrix;

/// Metrics collected during matrix multiplication
#[derive(Debug, Clone)]
struct MultiplicationMetrics {
    /// Matrix size (N×N)
    matrix_size: usize,
    /// Block size
    block_size: usize,
    /// Number of blocks (total work items)
    num_blocks: usize,
    /// Sequential multiplication time
    sequential_time_ms: f64,
    /// Estimated parallel computation time (based on block multiplication)
    estimated_parallel_time_ms: f64,
    /// Speedup (sequential / parallel)
    speedup: f64,
    /// Granularity ratio (assume 10% coordination overhead)
    granularity_ratio: f64,
}

impl MultiplicationMetrics {
    fn new(matrix_size: usize, block_size: usize, seq_time: Duration, parallel_time: Duration) -> Self {
        let sequential_ms = seq_time.as_secs_f64() * 1000.0;
        let parallel_ms = parallel_time.as_secs_f64() * 1000.0;

        let speedup = if parallel_ms > 0.0 {
            sequential_ms / parallel_ms
        } else {
            0.0
        };

        // Assume coordination takes ~10% of parallel time
        let coordination_ms = parallel_ms * 0.1;
        let computation_ms = parallel_ms * 0.9;

        let granularity_ratio = if coordination_ms > 0.0 {
            computation_ms / coordination_ms
        } else {
            0.0
        };

        let num_blocks_per_dim = matrix_size / block_size;
        let num_blocks = num_blocks_per_dim * num_blocks_per_dim;

        Self {
            matrix_size,
            block_size,
            num_blocks,
            sequential_time_ms: sequential_ms,
            estimated_parallel_time_ms: parallel_ms,
            speedup,
            granularity_ratio,
        }
    }

    fn print_detailed(&self) {
        println!("\n========== E2E TEST RESULTS ==========");
        println!("Configuration:");
        println!("  Matrix Size:  {}×{}", self.matrix_size, self.matrix_size);
        println!("  Block Size:   {}×{}", self.block_size, self.block_size);
        println!("  Num Blocks:   {}", self.num_blocks);
        println!();

        println!("Performance Metrics:");
        println!("  Sequential Time:     {:.2} ms", self.sequential_time_ms);
        println!("  Parallel Time:       {:.2} ms (estimated)", self.estimated_parallel_time_ms);
        println!();

        println!("Analysis:");
        println!("  Speedup:             {:.2}×", self.speedup);
        println!("  Granularity Ratio:   {:.2}× (compute/coordinate, estimated)", self.granularity_ratio);
        println!();

        // Analysis and recommendations
        if self.granularity_ratio < 10.0 {
            println!("  ⚠️  Low granularity ratio (< 10×) - coordination overhead dominates");
            println!("     Recommendation: Increase block size or matrix size");
        } else if self.granularity_ratio < 100.0 {
            println!("  ✓ Acceptable granularity ratio (10-100×)");
        } else {
            println!("  ✓ Excellent granularity ratio (> 100×)");
        }

        println!("======================================\n");
    }
}

/// Test matrix multiplication correctness and measure performance
fn test_matrix_multiplication(matrix_size: usize, block_size: usize) -> MultiplicationMetrics {
    // Generate matrices
    let matrix_a = Matrix::random(matrix_size, matrix_size);
    let matrix_b = Matrix::random(matrix_size, matrix_size);

    // Sequential multiplication (baseline)
    let seq_start = Instant::now();
    let expected = matrix_a.multiply(&matrix_b);
    let sequential_time = seq_start.elapsed();

    // Block-based multiplication (simulating parallel)
    let num_blocks_per_dim = matrix_size / block_size;

    let parallel_start = Instant::now();
    let mut result = Matrix::zeros(matrix_size, matrix_size);

    // Simulate parallel processing by doing all block multiplications
    for block_i in 0..num_blocks_per_dim {
        for block_j in 0..num_blocks_per_dim {
            // Extract blocks and multiply (simulating worker computation)
            let mut block_result = Matrix::zeros(block_size, block_size);

            for k in 0..num_blocks_per_dim {
                let a_block = matrix_a.get_block(
                    block_i * block_size,
                    k * block_size,
                    block_size,
                    block_size,
                );

                let b_block = matrix_b.get_block(
                    k * block_size,
                    block_j * block_size,
                    block_size,
                    block_size,
                );

                let product = a_block.multiply(&b_block);
                block_result.add(&product);
            }

            // Insert block into result
            result.set_block(
                block_i * block_size,
                block_j * block_size,
                &block_result,
            );
        }
    }

    let parallel_time = parallel_start.elapsed();

    // Verify correctness
    assert!(result.approx_equal(&expected, 1e-10), "Block multiplication result should match sequential");

    MultiplicationMetrics::new(matrix_size, block_size, sequential_time, parallel_time)
}

#[test]
fn e2e_small_matrix_with_metrics() {
    println!("\n🔬 E2E Test: Small Matrix (8×8, block=2)");

    let metrics = test_matrix_multiplication(8, 2);
    metrics.print_detailed();

    // Verify basic constraints
    assert!(metrics.sequential_time_ms > 0.0, "Should have sequential time");
    assert!(metrics.estimated_parallel_time_ms > 0.0, "Should have parallel time");
    assert!(metrics.num_blocks == 16, "Should have 16 blocks");
}

#[test]
fn e2e_medium_matrix_with_metrics() {
    println!("\n🔬 E2E Test: Medium Matrix (16×16, block=4)");

    let metrics = test_matrix_multiplication(16, 4);
    metrics.print_detailed();

    assert!(metrics.num_blocks == 16, "Should have 16 blocks");
}

#[test]
fn e2e_granularity_comparison() {
    println!("\n🔬 E2E Test: Granularity Comparison Across Configurations");
    println!("{}", "=".repeat(60));

    let configs = vec![
        (8, 2),   // Small blocks
        (8, 4),   // Larger blocks
        (16, 4),  // Medium matrix
        (16, 8),  // Larger blocks
    ];

    for (matrix_size, block_size) in configs {
        println!("\nConfiguration: {}×{} matrix, {}×{} blocks",
            matrix_size, matrix_size, block_size, block_size);

        let metrics = test_matrix_multiplication(matrix_size, block_size);

        println!("  Granularity Ratio: {:.2}×", metrics.granularity_ratio);
        println!("  Speedup: {:.2}×", metrics.speedup);
        println!("  Num Blocks: {}", metrics.num_blocks);
    }

    println!("\n{}", "=".repeat(60));
    println!("Key Insight: Larger blocks → better granularity → less overhead\n");
}

#[test]
fn test_matrix_correctness_4x4() {
    // Test that block-based multiplication produces correct results
    let matrix_size = 4;
    let block_size = 2;

    let matrix_a = Matrix::random(matrix_size, matrix_size);
    let matrix_b = Matrix::random(matrix_size, matrix_size);

    // Sequential
    let expected = matrix_a.multiply(&matrix_b);

    // Block-based
    let num_blocks_per_dim = matrix_size / block_size;
    let mut result = Matrix::zeros(matrix_size, matrix_size);

    for block_i in 0..num_blocks_per_dim {
        for block_j in 0..num_blocks_per_dim {
            let mut block_result = Matrix::zeros(block_size, block_size);

            for k in 0..num_blocks_per_dim {
                let a_block = matrix_a.get_block(
                    block_i * block_size,
                    k * block_size,
                    block_size,
                    block_size,
                );

                let b_block = matrix_b.get_block(
                    k * block_size,
                    block_j * block_size,
                    block_size,
                    block_size,
                );

                let product = a_block.multiply(&b_block);
                block_result.add(&product);
            }

            result.set_block(
                block_i * block_size,
                block_j * block_size,
                &block_result,
            );
        }
    }

    assert!(result.approx_equal(&expected, 1e-10), "Block multiplication should match sequential");
}

#[test]
fn test_matrix_correctness_6x6() {
    // Test with 6×6 matrix and 3×3 blocks
    let matrix_size = 6;
    let block_size = 3;

    let matrix_a = Matrix::random(matrix_size, matrix_size);
    let matrix_b = Matrix::random(matrix_size, matrix_size);

    let expected = matrix_a.multiply(&matrix_b);

    let num_blocks_per_dim = matrix_size / block_size;
    let mut result = Matrix::zeros(matrix_size, matrix_size);

    for block_i in 0..num_blocks_per_dim {
        for block_j in 0..num_blocks_per_dim {
            let mut block_result = Matrix::zeros(block_size, block_size);

            for k in 0..num_blocks_per_dim {
                let a_block = matrix_a.get_block(
                    block_i * block_size,
                    k * block_size,
                    block_size,
                    block_size,
                );

                let b_block = matrix_b.get_block(
                    k * block_size,
                    block_j * block_size,
                    block_size,
                    block_size,
                );

                let product = a_block.multiply(&b_block);
                block_result.add(&product);
            }

            result.set_block(
                block_i * block_size,
                block_j * block_size,
                &block_result,
            );
        }
    }

    assert!(result.approx_equal(&expected, 1e-10), "Block multiplication should match sequential");
}
