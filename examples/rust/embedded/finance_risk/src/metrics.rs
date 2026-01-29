// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Performance Metrics for Financial Risk Assessment
//!
//! Tracks coordination vs computation time and external API latencies.
//! Per CLAUDE.md Design Principle #6: Computation Cost >> Communication Cost
//!
//! Golden Rule: compute_time / coordinate_time >= 10x (minimum)

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Performance metrics for loan application workflow
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanMetrics {
    /// Application being processed
    pub application_id: String,

    /// Time spent on computation (data analysis, risk scoring, etc.)
    pub compute_duration_ms: u64,

    /// Time spent on coordination (message passing, waiting)
    pub coordinate_duration_ms: u64,

    /// Time spent waiting for external APIs
    pub external_api_duration_ms: u64,

    /// Number of messages sent
    pub message_count: u64,

    /// Number of external API calls
    pub api_call_count: u64,

    /// Total workflow duration
    pub total_duration_ms: u64,

    /// Granularity ratio (compute / coordinate)
    pub granularity_ratio: f64,

    /// Efficiency (compute / total)
    pub efficiency: f64,

    /// API efficiency ((compute + api) / total)
    pub api_efficiency: f64,

    /// Step-by-step breakdown
    pub step_metrics: Vec<StepMetric>,
}

/// Metrics for individual workflow step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepMetric {
    pub step_name: String,
    pub compute_ms: u64,
    pub coordinate_ms: u64,
    pub api_ms: u64,
    pub message_count: u64,
}

impl LoanMetrics {
    pub fn new(application_id: String) -> Self {
        Self {
            application_id,
            compute_duration_ms: 0,
            coordinate_duration_ms: 0,
            external_api_duration_ms: 0,
            message_count: 0,
            api_call_count: 0,
            total_duration_ms: 0,
            granularity_ratio: 0.0,
            efficiency: 0.0,
            api_efficiency: 0.0,
            step_metrics: Vec::new(),
        }
    }

    /// Add a step's metrics
    pub fn add_step(&mut self, step: StepMetric) {
        self.compute_duration_ms += step.compute_ms;
        self.coordinate_duration_ms += step.coordinate_ms;
        self.external_api_duration_ms += step.api_ms;
        self.message_count += step.message_count;
        if step.api_ms > 0 {
            self.api_call_count += 1;
        }
        self.step_metrics.push(step);
    }

    /// Finalize metrics and calculate derived values
    pub fn finalize(&mut self) {
        self.total_duration_ms =
            self.compute_duration_ms + self.coordinate_duration_ms + self.external_api_duration_ms;

        // Calculate granularity ratio (excluding external APIs)
        if self.coordinate_duration_ms > 0 {
            self.granularity_ratio =
                self.compute_duration_ms as f64 / self.coordinate_duration_ms as f64;
        } else {
            self.granularity_ratio = f64::INFINITY;
        }

        // Calculate efficiency (compute / total)
        if self.total_duration_ms > 0 {
            self.efficiency = self.compute_duration_ms as f64 / self.total_duration_ms as f64;
            self.api_efficiency = (self.compute_duration_ms + self.external_api_duration_ms) as f64
                / self.total_duration_ms as f64;
        } else {
            self.efficiency = 0.0;
            self.api_efficiency = 0.0;
        }
    }

    /// Print performance report
    pub fn print_report(&self) {
        println!("\n=== Financial Risk Assessment Performance Report ===");
        println!("Application: {}", self.application_id);
        println!();
        println!("Timing:");
        println!("  Compute Time:     {:>8} ms", self.compute_duration_ms);
        println!("  Coordinate Time:  {:>8} ms", self.coordinate_duration_ms);
        println!(
            "  External API Time:{:>8} ms",
            self.external_api_duration_ms
        );
        println!("  Total Time:       {:>8} ms", self.total_duration_ms);
        println!();
        println!("Communication:");
        println!("  Messages Sent:    {:>8}", self.message_count);
        println!("  API Calls:        {:>8}", self.api_call_count);
        println!();
        println!("Performance:");
        println!(
            "  Granularity Ratio: {:>7.2}× (compute/coordinate)",
            self.granularity_ratio
        );

        if self.granularity_ratio < 10.0 {
            println!("    ⚠️  WARNING: Ratio < 10×! Coordination overhead too high!");
            println!("    Consider:");
            println!("      - Batch API calls (reduce message frequency)");
            println!("      - Fewer worker pools (reduce coordination)");
            println!("      - Async API calls with futures (reduce waiting)");
        } else if self.granularity_ratio < 100.0 {
            println!("    ✓  Acceptable (>10×), but could be optimized");
        } else {
            println!("    ✅ Excellent (>100×)! Optimal granularity");
        }

        println!(
            "  Compute Efficiency:   {:>7.1}% (compute/total)",
            self.efficiency * 100.0
        );
        println!(
            "  API Efficiency:       {:>7.1}% ((compute+api)/total)",
            self.api_efficiency * 100.0
        );

        if self.api_efficiency < 0.8 {
            println!("    ⚠️  API efficiency < 80%! Too much coordination overhead");
        }

        println!();
        println!("Step Breakdown:");
        for step in &self.step_metrics {
            let step_total = step.compute_ms + step.coordinate_ms + step.api_ms;
            let step_ratio = if step.coordinate_ms > 0 {
                step.compute_ms as f64 / step.coordinate_ms as f64
            } else {
                f64::INFINITY
            };
            println!("  {:<25} compute: {:>5}ms, coord: {:>5}ms, api: {:>5}ms, total: {:>5}ms, ratio: {:>5.1}×",
                step.step_name, step.compute_ms, step.coordinate_ms, step.api_ms, step_total, step_ratio);
        }
        println!("===================================================\n");
    }
}

/// Timer for measuring computation time
pub struct ComputeTimer {
    start: Instant,
}

impl ComputeTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// Timer for measuring coordination time
pub struct CoordinateTimer {
    start: Instant,
}

impl CoordinateTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

/// Timer for measuring external API time
pub struct ApiTimer {
    start: Instant,
}

impl ApiTimer {
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    pub fn elapsed_ms(&self) -> u64 {
        self.start.elapsed().as_millis() as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_creation() {
        let metrics = LoanMetrics::new("APP001".to_string());
        assert_eq!(metrics.application_id, "APP001");
        assert_eq!(metrics.compute_duration_ms, 0);
    }

    #[test]
    fn test_add_step() {
        let mut metrics = LoanMetrics::new("APP001".to_string());
        metrics.add_step(StepMetric {
            step_name: "Credit Check".to_string(),
            compute_ms: 50,
            coordinate_ms: 5,
            api_ms: 200,
            message_count: 2,
        });

        assert_eq!(metrics.compute_duration_ms, 50);
        assert_eq!(metrics.coordinate_duration_ms, 5);
        assert_eq!(metrics.external_api_duration_ms, 200);
    }

    #[test]
    fn test_granularity_ratio() {
        let mut metrics = LoanMetrics::new("APP001".to_string());
        metrics.add_step(StepMetric {
            step_name: "Risk Scoring".to_string(),
            compute_ms: 100,
            coordinate_ms: 5,
            api_ms: 0,
            message_count: 3,
        });
        metrics.finalize();

        assert_eq!(metrics.granularity_ratio, 20.0);
    }
}
