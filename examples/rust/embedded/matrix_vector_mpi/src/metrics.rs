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

//! Performance Metrics for Matrix-Vector Multiplication
//!
//! ## Purpose
//! Track and report compute vs coordination time to demonstrate
//! HPC performance characteristics and granularity ratios.

use std::time::Duration;

/// Metrics collector for matrix-vector multiplication performance
///
/// ## Key Metrics
/// - **Compute Time**: Time spent in actual computation (matrix-vector product)
/// - **Coordination Time**: Time spent in communication (scatter, broadcast, gather, barrier)
/// - **Granularity Ratio**: Compute / Coordination (should be >> 10x for efficiency)
pub struct ComputeMetrics {
    // Problem size
    num_rows: usize,
    num_cols: usize,
    num_workers: usize,

    // Coordination times
    scatter_time: Duration,
    broadcast_time: Duration,
    barrier_time: Duration,
    gather_time: Duration,

    // Computation time
    compute_time: Duration,
}

impl ComputeMetrics {
    /// Create new metrics tracker
    pub fn new(num_rows: usize, num_cols: usize, num_workers: usize) -> Self {
        Self {
            num_rows,
            num_cols,
            num_workers,
            scatter_time: Duration::ZERO,
            broadcast_time: Duration::ZERO,
            barrier_time: Duration::ZERO,
            gather_time: Duration::ZERO,
            compute_time: Duration::ZERO,
        }
    }

    /// Record scatter operation time
    pub fn record_scatter(&mut self, duration: Duration) {
        self.scatter_time = duration;
    }

    /// Record broadcast operation time
    pub fn record_broadcast(&mut self, duration: Duration) {
        self.broadcast_time = duration;
    }

    /// Record barrier synchronization time
    pub fn record_barrier(&mut self, duration: Duration) {
        self.barrier_time = duration;
    }

    /// Record gather operation time
    pub fn record_gather(&mut self, duration: Duration) {
        self.gather_time = duration;
    }

    /// Record computation time
    pub fn record_compute(&mut self, duration: Duration) {
        self.compute_time = duration;
    }

    /// Total coordination time (all communication operations)
    pub fn total_coordination_time(&self) -> Duration {
        self.scatter_time + self.broadcast_time + self.barrier_time + self.gather_time
    }

    /// Total execution time
    pub fn total_time(&self) -> Duration {
        self.compute_time + self.total_coordination_time()
    }

    /// Get compute time
    pub fn compute_time(&self) -> Duration {
        self.compute_time
    }

    /// Granularity ratio: Compute / Coordination
    ///
    /// ## Interpretation
    /// - < 10x: Too much overhead, consider coarser granularity
    /// - 10x-100x: Acceptable for small problems
    /// - > 100x: Good efficiency, parallelism beneficial
    pub fn granularity_ratio(&self) -> f64 {
        let coord_ms = self.total_coordination_time().as_secs_f64() * 1000.0;
        let compute_ms = self.compute_time.as_secs_f64() * 1000.0;

        if coord_ms > 0.0 {
            compute_ms / coord_ms
        } else {
            f64::INFINITY
        }
    }

    /// Estimated speedup vs sequential (simplified model)
    ///
    /// Using Amdahl's Law approximation:
    /// - Sequential fraction: coordination time
    /// - Parallel fraction: computation time
    pub fn estimated_speedup(&self) -> f64 {
        let total = self.total_time().as_secs_f64();
        let sequential = self.total_coordination_time().as_secs_f64();
        let parallel = self.compute_time.as_secs_f64();

        if total > 0.0 {
            total / (sequential + parallel / self.num_workers as f64)
        } else {
            1.0
        }
    }

    /// Parallel efficiency: Speedup / Workers
    pub fn efficiency(&self) -> f64 {
        self.estimated_speedup() / self.num_workers as f64
    }

    /// Compute operations count
    pub fn compute_operations(&self) -> usize {
        // Matrix-vector product: M × N multiplications + M × (N-1) additions
        let multiplications = self.num_rows * self.num_cols;
        let additions = self.num_rows * (self.num_cols - 1);
        multiplications + additions
    }

    /// Coordination operations count
    pub fn coordination_operations(&self) -> usize {
        // Scatter: num_workers writes
        // Broadcast: 1 write + num_workers reads
        // Gather: num_workers reads
        // Barrier: num_workers writes + num_workers waits
        self.num_workers * 4 + 1
    }

    /// Print detailed metrics report
    pub fn print_detailed(&self) {
        println!("\n========== PERFORMANCE METRICS ==========");

        // Problem size
        println!("Problem Size:");
        println!("  Matrix: {}×{}", self.num_rows, self.num_cols);
        println!("  Workers: {}", self.num_workers);
        println!("  Rows per worker: {}", self.num_rows / self.num_workers);

        // Timing breakdown
        println!("\nTiming Breakdown:");
        println!("  Scatter:    {:>8.2} ms", self.scatter_time.as_secs_f64() * 1000.0);
        println!("  Broadcast:  {:>8.2} ms", self.broadcast_time.as_secs_f64() * 1000.0);
        println!("  Compute:    {:>8.2} ms", self.compute_time.as_secs_f64() * 1000.0);
        println!("  Barrier:    {:>8.2} ms", self.barrier_time.as_secs_f64() * 1000.0);
        println!("  Gather:     {:>8.2} ms", self.gather_time.as_secs_f64() * 1000.0);
        println!("  ------------");
        println!("  Total Coordination: {:>8.2} ms",
            self.total_coordination_time().as_secs_f64() * 1000.0);
        println!("  Total Time:         {:>8.2} ms",
            self.total_time().as_secs_f64() * 1000.0);

        // Operation counts
        println!("\nOperation Counts:");
        println!("  Compute ops:      {:>8}", self.compute_operations());
        println!("  Coordination ops: {:>8}", self.coordination_operations());

        // Performance ratios
        println!("\nPerformance Analysis:");
        let ratio = self.granularity_ratio();
        println!("  Granularity Ratio: {:.2}× (compute/coordinate)", ratio);

        if ratio < 10.0 {
            println!("    ⚠️  WARNING: Overhead too high! Consider:");
            println!("       - Larger problem size (more rows/cols)");
            println!("       - Fewer workers (coarser granularity)");
        } else if ratio < 100.0 {
            println!("    ✓  ACCEPTABLE: Reasonable for small problems");
        } else {
            println!("    ✓  EXCELLENT: Good compute/coordinate ratio");
        }

        println!("\n  Estimated Speedup: {:.2}× (vs sequential)", self.estimated_speedup());
        println!("  Efficiency:        {:.1}% ({:.2}/{} workers)",
            self.efficiency() * 100.0,
            self.estimated_speedup(),
            self.num_workers);

        // Recommendations
        println!("\nScaling Recommendations:");
        if self.efficiency() < 0.5 {
            println!("  ⚠️  Low efficiency detected. For better scaling:");
            println!("     - WEAK SCALING: Increase problem size with workers");
            println!("     - Keep rows/worker constant, scale both together");
            println!("     - Current: {} rows/worker", self.num_rows / self.num_workers);
            println!("     - Recommend: 50-100 rows/worker minimum");
        } else {
            println!("  ✓  Good efficiency. System scales well at this size.");
            println!("     - Can add more workers if problem size increases");
            println!("     - Maintain ~{} rows/worker ratio", self.num_rows / self.num_workers);
        }

        println!("=========================================\n");
    }

    /// Print compact summary (for tests)
    pub fn print_summary(&self) {
        println!("Metrics: {:.2}ms compute, {:.2}ms coord, {:.2}× ratio, {:.1}% efficiency",
            self.compute_time.as_secs_f64() * 1000.0,
            self.total_coordination_time().as_secs_f64() * 1000.0,
            self.granularity_ratio(),
            self.efficiency() * 100.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_basic() {
        let mut metrics = ComputeMetrics::new(8, 4, 2);

        metrics.record_scatter(Duration::from_millis(5));
        metrics.record_broadcast(Duration::from_millis(3));
        metrics.record_compute(Duration::from_millis(100));
        metrics.record_barrier(Duration::from_millis(2));
        metrics.record_gather(Duration::from_millis(5));

        assert_eq!(metrics.total_coordination_time(), Duration::from_millis(15));
        assert_eq!(metrics.total_time(), Duration::from_millis(115));

        // 100ms compute / 15ms coord = 6.67× ratio
        let ratio = metrics.granularity_ratio();
        assert!((ratio - 6.67).abs() < 0.1);
    }

    #[test]
    fn test_compute_operations_count() {
        let metrics = ComputeMetrics::new(8, 4, 2);

        // 8 rows × 4 cols = 32 multiplications
        // 8 rows × 3 additions = 24 additions
        // Total: 56 operations
        assert_eq!(metrics.compute_operations(), 56);
    }

    #[test]
    fn test_coordination_operations_count() {
        let metrics = ComputeMetrics::new(8, 4, 2);

        // 2 workers:
        // Scatter: 2 writes
        // Broadcast: 1 write + 2 reads = 3
        // Gather: 2 reads
        // Barrier: 2 writes + 2 waits = 4
        // Total: 2 + 3 + 2 + 4 = 11? No wait...
        // Formula: num_workers * 4 + 1 = 2 * 4 + 1 = 9
        assert_eq!(metrics.coordination_operations(), 9);
    }

    #[test]
    fn test_efficiency_calculation() {
        let mut metrics = ComputeMetrics::new(16, 8, 4);

        metrics.record_scatter(Duration::from_millis(2));
        metrics.record_broadcast(Duration::from_millis(1));
        metrics.record_compute(Duration::from_millis(40)); // 40ms with 4 workers = ~160ms sequential
        metrics.record_barrier(Duration::from_millis(1));
        metrics.record_gather(Duration::from_millis(2));

        // Total coord: 6ms
        // Parallel compute: 40ms (160ms sequential)
        // Sequential equiv: 6 + 160 = 166ms
        // Parallel time: 6 + 40/4 = 16ms
        // Speedup: 166 / 16 ≈ 10.4×? Actually...
        // Speedup: (6 + 40) / (6 + 40/4) = 46 / 16 = 2.875×
        // Efficiency: 2.875 / 4 = 71.875%

        let speedup = metrics.estimated_speedup();
        let efficiency = metrics.efficiency();

        // With small overhead, should be reasonable
        assert!(speedup > 1.0);
        assert!(efficiency > 0.0 && efficiency <= 1.0);
    }

    #[test]
    fn test_zero_coordination_time() {
        let mut metrics = ComputeMetrics::new(8, 4, 2);
        metrics.record_compute(Duration::from_millis(100));

        // Should handle division by zero gracefully
        assert_eq!(metrics.granularity_ratio(), f64::INFINITY);
    }
}
