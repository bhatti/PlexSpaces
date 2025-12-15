// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Configuration module for finance-risk example
//!
//! Uses ConfigBootstrap to load configuration from release.toml or environment variables.

use plexspaces_node::ConfigBootstrap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Finance Risk Assessment application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinanceRiskConfig {
    /// Worker pool sizes
    #[serde(default = "default_worker_pools")]
    pub worker_pools: WorkerPoolConfig,

    /// Backend configuration
    #[serde(default)]
    pub backend: BackendType,

    /// Redis URL (if backend is redis)
    #[serde(default)]
    pub redis_url: Option<String>,

    /// Node configuration (from release.toml)
    #[serde(default)]
    pub node: NodeConfig,
}

/// Worker pool configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkerPoolConfig {
    #[serde(default = "default_credit_pool_size")]
    pub credit_check: usize,
    #[serde(default = "default_bank_pool_size")]
    pub bank_analysis: usize,
    #[serde(default = "default_employment_pool_size")]
    pub employment: usize,
    #[serde(default = "default_risk_scoring_pool_size")]
    pub risk_scoring: usize,
    #[serde(default = "default_decision_engine_pool_size")]
    pub decision_engine: usize,
    #[serde(default = "default_document_pool_size")]
    pub document: usize,
    #[serde(default = "default_notification_pool_size")]
    pub notification: usize,
    #[serde(default = "default_audit_pool_size")]
    pub audit: usize,
}

/// Backend type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BackendType {
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "redis")]
    Redis,
    #[serde(rename = "sqlite")]
    Sqlite,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Memory
    }
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeConfig {
    #[serde(default)]
    pub node_id: String,
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    #[serde(default)]
    pub labels: HashMap<String, String>,
    #[serde(default)]
    pub capacity: ResourceCapacity,
}

/// Resource capacity configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceCapacity {
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: f64,
    #[serde(default = "default_memory_gb")]
    pub memory_gb: f64,
    #[serde(default)]
    pub gpu_count: u32,
}

// Default values
fn default_worker_pools() -> WorkerPoolConfig {
    WorkerPoolConfig {
        credit_check: 5,
        bank_analysis: 5,
        employment: 3,
        risk_scoring: 2,
        decision_engine: 1,
        document: 2,
        notification: 3,
        audit: 1,
    }
}

fn default_credit_pool_size() -> usize {
    5
}
fn default_bank_pool_size() -> usize {
    5
}
fn default_employment_pool_size() -> usize {
    3
}
fn default_risk_scoring_pool_size() -> usize {
    2
}
fn default_decision_engine_pool_size() -> usize {
    1
}
fn default_document_pool_size() -> usize {
    2
}
fn default_notification_pool_size() -> usize {
    3
}
fn default_audit_pool_size() -> usize {
    1
}
fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}
fn default_cpu_cores() -> f64 {
    4.0
}
fn default_memory_gb() -> f64 {
    8.0
}

impl FinanceRiskConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.worker_pools.credit_check == 0 {
            return Err("credit_check pool size must be > 0".to_string());
        }
        if self.worker_pools.bank_analysis == 0 {
            return Err("bank_analysis pool size must be > 0".to_string());
        }
        if self.worker_pools.employment == 0 {
            return Err("employment pool size must be > 0".to_string());
        }
        if self.worker_pools.risk_scoring == 0 {
            return Err("risk_scoring pool size must be > 0".to_string());
        }
        if self.worker_pools.decision_engine == 0 {
            return Err("decision_engine pool size must be > 0".to_string());
        }
        if self.backend == BackendType::Redis && self.redis_url.is_none() {
            return Err("redis_url is required when backend is redis".to_string());
        }
        Ok(())
    }
}

impl Default for FinanceRiskConfig {
    fn default() -> Self {
        Self {
            worker_pools: default_worker_pools(),
            backend: BackendType::Memory,
            redis_url: None,
            node: NodeConfig::default(),
        }
    }
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        default_worker_pools()
    }
}
