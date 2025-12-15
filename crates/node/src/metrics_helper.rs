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

//! Coordination vs Compute Metrics Helper
//!
//! ## Purpose
//! Tracks coordination overhead vs actual computation time for HPC workloads.
//! Per CLAUDE.md Design Principle #6: Computation Cost >> Communication Cost
//!
//! ## Golden Rule
//! compute_time / coordinate_time >= 10x (minimum), ideally >= 100x
//!
//! ## Usage
//! ```rust
//! let mut metrics = CoordinationComputeTracker::new("workflow-123");
//!
//! // Track computation
//! metrics.start_compute();
//! // ... do actual work ...
//! metrics.end_compute();
//!
//! // Track coordination
//! metrics.start_coordinate();
//! // ... send messages, wait for barriers ...
//! metrics.end_coordinate();
//!
//! // Get final metrics
//! let report = metrics.finalize();
//! println!("Granularity ratio: {:.2}", report.granularity_ratio);
//! ```

use std::time::{Duration, Instant};
use plexspaces_proto::metrics::v1::{CoordinationComputeMetrics, StepMetrics};

/// Tracker for coordination vs compute metrics
pub struct CoordinationComputeTracker {
    workflow_id: String,
    compute_start: Option<Instant>,
    coordinate_start: Option<Instant>,
    compute_duration: Duration,
    coordinate_duration: Duration,
    message_count: u64,
    barrier_count: u64,
    step_metrics: Vec<StepMetrics>,
    current_step: Option<String>,
    current_step_compute_start: Option<Instant>,
    current_step_coordinate_start: Option<Instant>,
    current_step_compute_duration: Duration,
    current_step_coordinate_duration: Duration,
    current_step_message_count: u64,
}

impl CoordinationComputeTracker {
    /// Create a new metrics tracker
    pub fn new(workflow_id: String) -> Self {
        Self {
            workflow_id,
            compute_start: None,
            coordinate_start: None,
            compute_duration: Duration::ZERO,
            coordinate_duration: Duration::ZERO,
            message_count: 0,
            barrier_count: 0,
            step_metrics: Vec::new(),
            current_step: None,
            current_step_compute_start: None,
            current_step_coordinate_start: None,
            current_step_compute_duration: Duration::ZERO,
            current_step_coordinate_duration: Duration::ZERO,
            current_step_message_count: 0,
        }
    }

    /// Start tracking computation time
    pub fn start_compute(&mut self) {
        self.compute_start = Some(Instant::now());
        if self.current_step.is_some() {
            self.current_step_compute_start = Some(Instant::now());
        }
    }

    /// End computation time tracking
    pub fn end_compute(&mut self) {
        if let Some(start) = self.compute_start.take() {
            let elapsed = start.elapsed();
            self.compute_duration += elapsed;
            
            if let Some(step_start) = self.current_step_compute_start.take() {
                let step_elapsed = step_start.elapsed();
                self.current_step_compute_duration += step_elapsed;
            }
        }
    }

    /// Start tracking coordination time
    pub fn start_coordinate(&mut self) {
        self.coordinate_start = Some(Instant::now());
        if let Some(_step) = &self.current_step {
            self.current_step_coordinate_start = Some(Instant::now());
        }
    }

    /// End coordination time tracking
    pub fn end_coordinate(&mut self) {
        if let Some(start) = self.coordinate_start.take() {
            let elapsed = start.elapsed();
            self.coordinate_duration += elapsed;
            
            if let Some(step_start) = self.current_step_coordinate_start.take() {
                let step_elapsed = step_start.elapsed();
                self.current_step_coordinate_duration += step_elapsed;
            }
        }
    }

    /// Increment message count
    pub fn increment_message(&mut self) {
        self.message_count += 1;
        self.current_step_message_count += 1;
    }

    /// Increment barrier count
    pub fn increment_barrier(&mut self) {
        self.barrier_count += 1;
    }

    /// Start tracking a new step
    pub fn start_step(&mut self, step_name: String) {
        // Finalize previous step if any
        if let Some(prev_step) = self.current_step.take() {
            self.finalize_step(prev_step);
        }
        
        self.current_step = Some(step_name);
        self.current_step_compute_duration = Duration::ZERO;
        self.current_step_coordinate_duration = Duration::ZERO;
        self.current_step_message_count = 0;
    }

    /// Finalize current step
    fn finalize_step(&mut self, step_name: String) {
        let step_metrics = StepMetrics {
            step_name,
            compute_ms: self.current_step_compute_duration.as_millis() as u64,
            coordinate_ms: self.current_step_coordinate_duration.as_millis() as u64,
            message_count: self.current_step_message_count,
        };
        self.step_metrics.push(step_metrics);
        
        self.current_step_compute_duration = Duration::ZERO;
        self.current_step_coordinate_duration = Duration::ZERO;
        self.current_step_message_count = 0;
    }

    /// Finalize metrics and return report
    pub fn finalize(mut self) -> CoordinationComputeMetrics {
        // Finalize any remaining step
        if let Some(step) = self.current_step.take() {
            self.finalize_step(step);
        }
        
        // Ensure all timers are stopped
        if self.compute_start.is_some() {
            self.end_compute();
        }
        if self.coordinate_start.is_some() {
            self.end_coordinate();
        }
        
        let total_duration = self.compute_duration + self.coordinate_duration;
        let compute_ms = self.compute_duration.as_millis() as u64;
        let coordinate_ms = self.coordinate_duration.as_millis() as u64;
        let total_ms = total_duration.as_millis() as u64;
        
        // Calculate granularity ratio
        let granularity_ratio = if coordinate_ms > 0 {
            compute_ms as f64 / coordinate_ms as f64
        } else if compute_ms > 0 {
            f64::INFINITY
        } else {
            0.0
        };
        
        // Calculate efficiency (compute / total)
        let efficiency = if total_ms > 0 {
            compute_ms as f64 / total_ms as f64
        } else {
            0.0
        };
        
        CoordinationComputeMetrics {
            workflow_id: self.workflow_id,
            compute_duration_ms: compute_ms,
            coordinate_duration_ms: coordinate_ms,
            message_count: self.message_count,
            barrier_count: self.barrier_count,
            total_duration_ms: total_ms,
            granularity_ratio,
            efficiency,
            step_metrics: self.step_metrics,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_basic_tracking() {
        let mut tracker = CoordinationComputeTracker::new("test-workflow".to_string());
        
        // Track some computation - use longer sleep times to avoid timing precision issues
        tracker.start_compute();
        std::thread::sleep(Duration::from_millis(50)); // Increased from 10ms for better reliability
        tracker.end_compute();
        
        // Track some coordination - use shorter sleep time
        tracker.start_coordinate();
        std::thread::sleep(Duration::from_millis(5)); // Increased from 1ms for better reliability
        tracker.end_coordinate();
        
        let metrics = tracker.finalize();
        
        // Verify durations are at least the sleep times (allowing for some overhead)
        assert!(metrics.compute_duration_ms >= 45, "compute_duration_ms was {}, expected >= 45", metrics.compute_duration_ms);
        assert!(metrics.coordinate_duration_ms >= 4, "coordinate_duration_ms was {}, expected >= 4", metrics.coordinate_duration_ms);
        
        // Granularity ratio should be compute/coordinate = 50/5 = 10.0
        // Allow tolerance for timing variations (minimum 4.0 to account for system load and timing precision)
        // This makes the test more robust when run with other tests under load or with coverage instrumentation
        assert!(
            metrics.granularity_ratio >= 4.0,
            "granularity_ratio was {}, expected >= 4.0 (compute_ms={}, coordinate_ms={})",
            metrics.granularity_ratio,
            metrics.compute_duration_ms,
            metrics.coordinate_duration_ms
        );
    }

    #[test]
    fn test_step_tracking() {
        let mut tracker = CoordinationComputeTracker::new("test-workflow".to_string());
        
        tracker.start_step("step1".to_string());
        tracker.start_compute();
        std::thread::sleep(Duration::from_millis(5));
        tracker.end_compute();
        tracker.increment_message();
        
        tracker.start_step("step2".to_string());
        tracker.start_compute();
        std::thread::sleep(Duration::from_millis(5));
        tracker.end_compute();
        
        let metrics = tracker.finalize();
        
        assert_eq!(metrics.step_metrics.len(), 2);
        assert_eq!(metrics.step_metrics[0].step_name, "step1");
        assert_eq!(metrics.step_metrics[0].message_count, 1);
        assert_eq!(metrics.step_metrics[1].step_name, "step2");
        assert_eq!(metrics.step_metrics[1].message_count, 0);
    }
}

