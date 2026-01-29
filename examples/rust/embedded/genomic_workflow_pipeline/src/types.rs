// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Genomic Pipeline Types
//!
//! ## Purpose
//! Data structures for genomic analysis with metrics tracking.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// DNA sequence read
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceRead {
    pub id: String,
    pub sequence: String,
    pub quality: Vec<u8>,
}

/// Pipeline metrics tracking compute vs coordinate overhead
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineMetrics {
    /// Time spent on actual computation (QC, alignment, variant calling)
    pub compute_time_ms: u64,
    
    /// Time spent on coordination (message passing, workflow orchestration)
    pub coordinate_time_ms: u64,
    
    /// Total time
    pub total_time_ms: u64,
    
    /// Compute/coordinate ratio (target: >= 100x)
    pub granularity_ratio: f64,
    
    /// Efficiency percentage (compute/total)
    pub efficiency_percent: f64,
}

impl PipelineMetrics {
    pub fn new() -> Self {
        Self {
            compute_time_ms: 0,
            coordinate_time_ms: 0,
            total_time_ms: 0,
            granularity_ratio: 0.0,
            efficiency_percent: 0.0,
        }
    }
    
    pub fn record_compute(&mut self, duration: Duration) {
        self.compute_time_ms += duration.as_millis() as u64;
    }
    
    pub fn record_coordinate(&mut self, duration: Duration) {
        self.coordinate_time_ms += duration.as_millis() as u64;
    }
    
    pub fn calculate(&mut self) {
        self.total_time_ms = self.compute_time_ms + self.coordinate_time_ms;
        
        if self.coordinate_time_ms > 0 {
            self.granularity_ratio = self.compute_time_ms as f64 / self.coordinate_time_ms as f64;
        }
        
        if self.total_time_ms > 0 {
            self.efficiency_percent = (self.compute_time_ms as f64 / self.total_time_ms as f64) * 100.0;
        }
    }
    
    pub fn meets_target(&self, target_ratio: f64) -> bool {
        self.granularity_ratio >= target_ratio
    }
}

impl Default for PipelineMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// QC result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QCResult {
    pub passed: bool,
    pub avg_quality: f64,
}

/// Alignment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlignmentResult {
    pub chromosome: String,
    pub position: usize,
    pub score: i32,
}

/// Variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Variant {
    pub chromosome: String,
    pub position: usize,
    pub reference: String,
    pub alternate: String,
}

/// Pipeline result with metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineResult {
    pub total_reads: usize,
    pub qc_passed: usize,
    pub variants_called: usize,
    pub metrics: PipelineMetrics,
}
