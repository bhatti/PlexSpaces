// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Configuration module for genomics-pipeline example
//!
//! Uses ConfigBootstrap to load configuration from release.toml or environment variables.

use plexspaces_node::ConfigBootstrap;
use serde::{Deserialize, Serialize};

/// Genomics Pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenomicsPipelineConfig {
    /// Worker pool configuration
    #[serde(default)]
    pub worker_pools: WorkerPoolConfig,

    /// Node configuration
    #[serde(default)]
    pub node: NodeConfig,

    /// Target granularity ratio (compute/coordinate)
    #[serde(default = "default_target_granularity_ratio")]
    pub target_granularity_ratio: f64,
}

/// Worker pool sizes
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPoolConfig {
    /// Number of QC workers
    #[serde(default = "default_qc_pool_size")]
    pub qc: usize,

    /// Number of alignment workers
    #[serde(default = "default_alignment_pool_size")]
    pub alignment: usize,

    /// Number of chromosome workers (one per chromosome: chr1-22, X, Y = 24)
    #[serde(default = "default_chromosome_pool_size")]
    pub chromosome: usize,

    /// Number of annotation workers
    #[serde(default = "default_annotation_pool_size")]
    pub annotation: usize,

    /// Number of report workers
    #[serde(default = "default_report_pool_size")]
    pub report: usize,
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    #[serde(default)]
    pub id: String,

    #[serde(default = "default_listen_addr")]
    pub listen_address: String,
}

fn default_qc_pool_size() -> usize {
    2
}

fn default_alignment_pool_size() -> usize {
    2
}

fn default_chromosome_pool_size() -> usize {
    24 // One per chromosome (chr1-22, X, Y)
}

fn default_annotation_pool_size() -> usize {
    1
}

fn default_report_pool_size() -> usize {
    1
}

fn default_target_granularity_ratio() -> f64 {
    100.0
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        Self {
            qc: default_qc_pool_size(),
            alignment: default_alignment_pool_size(),
            chromosome: default_chromosome_pool_size(),
            annotation: default_annotation_pool_size(),
            report: default_report_pool_size(),
        }
    }
}

impl Default for GenomicsPipelineConfig {
    fn default() -> Self {
        Self {
            worker_pools: WorkerPoolConfig::default(),
            node: NodeConfig::default(),
            target_granularity_ratio: default_target_granularity_ratio(),
        }
    }
}

impl GenomicsPipelineConfig {
    /// Load configuration using ConfigBootstrap
    /// 
    /// Configuration is loaded from:
    /// 1. Environment variables (highest precedence)
    /// 2. release.toml file
    /// 3. Default values (lowest precedence)
    pub fn load() -> Result<Self, anyhow::Error> {
        ConfigBootstrap::load()
            .map_err(|e| anyhow::anyhow!("Failed to load config: {}", e))
    }
}

