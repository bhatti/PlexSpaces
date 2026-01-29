// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Configuration module for genomic-workflow-pipeline example
//!
//! Uses ConfigBootstrap to load configuration from release.toml or environment variables.

use plexspaces_node::ConfigBootstrap;
use serde::{Deserialize, Serialize};

/// Genomic Pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenomicPipelineConfig {
    /// Target granularity ratio (compute/coordinate)
    #[serde(default = "default_target_granularity_ratio")]
    pub target_granularity_ratio: f64,

    /// Node configuration
    #[serde(default)]
    pub node: NodeConfig,

    /// Workflow recovery configuration
    #[serde(default)]
    pub recovery: RecoveryConfig,
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    #[serde(default)]
    pub id: String,
    #[serde(default = "default_listen_addr")]
    pub listen_address: String,
}

/// Workflow recovery configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryConfig {
    /// Enable auto-recovery on startup
    #[serde(default = "default_true")]
    pub auto_recover: bool,

    /// Stale workflow threshold (seconds since last heartbeat)
    #[serde(default = "default_stale_threshold_secs")]
    pub stale_threshold_secs: u64,

    /// Heartbeat interval (seconds)
    #[serde(default = "default_heartbeat_interval_secs")]
    pub heartbeat_interval_secs: u64,
}

impl Default for RecoveryConfig {
    fn default() -> Self {
        Self {
            auto_recover: true,
            stale_threshold_secs: 300, // 5 minutes
            heartbeat_interval_secs: 60, // 1 minute
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_stale_threshold_secs() -> u64 {
    300 // 5 minutes
}

fn default_heartbeat_interval_secs() -> u64 {
    60 // 1 minute
}

fn default_target_granularity_ratio() -> f64 {
    100.0
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

impl Default for GenomicPipelineConfig {
    fn default() -> Self {
        Self {
            target_granularity_ratio: default_target_granularity_ratio(),
            node: NodeConfig::default(),
            recovery: RecoveryConfig::default(),
        }
    }
}

impl GenomicPipelineConfig {
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

