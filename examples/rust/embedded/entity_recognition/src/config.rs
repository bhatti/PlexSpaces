// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Configuration for entity recognition example.
//!
//! Uses ConfigBootstrap for Erlang/OTP-style configuration loading.

use plexspaces_node::ConfigBootstrap;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Backend type for channels and state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackendType {
    /// In-memory (single-node, testing)
    #[serde(rename = "memory")]
    Memory,
    /// Redis (multi-node, production)
    #[serde(rename = "redis")]
    Redis,
}

impl Default for BackendType {
    fn default() -> Self {
        BackendType::Memory
    }
}

impl std::str::FromStr for BackendType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "memory" => Ok(BackendType::Memory),
            "redis" => Ok(BackendType::Redis),
            _ => Err(format!("Unknown backend type: {}", s)),
        }
    }
}

/// Entity recognition application configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRecognitionConfig {
    /// Number of loader actors (CPU-intensive)
    #[serde(default = "default_loader_count")]
    pub loader_count: u32,
    
    /// Number of processor actors (GPU-intensive)
    #[serde(default = "default_processor_count")]
    pub processor_count: u32,
    
    /// Number of aggregator actors (CPU-intensive)
    #[serde(default = "default_aggregator_count")]
    pub aggregator_count: u32,
    
    /// Backend type: "memory" or "redis"
    #[serde(default = "default_backend")]
    pub backend: String,
    
    /// Redis URL (if backend is redis)
    #[serde(default)]
    pub redis_url: Option<String>,
    
    /// Node configuration
    #[serde(default)]
    pub node: NodeConfig,
    
    /// Resource capacity
    #[serde(default)]
    pub capacity: ResourceConfig,
}

fn default_loader_count() -> u32 {
    2
}

fn default_processor_count() -> u32 {
    1
}

fn default_aggregator_count() -> u32 {
    1
}

fn default_backend() -> String {
    "memory".to_string()
}

impl Default for EntityRecognitionConfig {
    fn default() -> Self {
        Self {
            loader_count: default_loader_count(),
            processor_count: default_processor_count(),
            aggregator_count: default_aggregator_count(),
            backend: default_backend(),
            redis_url: None,
            node: NodeConfig::default(),
            capacity: ResourceConfig::default(),
        }
    }
}

impl EntityRecognitionConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Get backend type
    pub fn backend_type(&self) -> BackendType {
        self.backend.parse().unwrap_or(BackendType::Memory)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.loader_count == 0 {
            return Err("loader_count must be > 0".to_string());
        }
        if self.processor_count == 0 {
            return Err("processor_count must be > 0".to_string());
        }
        if self.aggregator_count == 0 {
            return Err("aggregator_count must be > 0".to_string());
        }
        if self.backend_type() == BackendType::Redis && self.redis_url.is_none() {
            return Err("redis_url must be set when backend is redis".to_string());
        }
        Ok(())
    }
}

/// Node configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    /// Node ID (auto-generated if empty)
    #[serde(default)]
    pub node_id: String,
    
    /// Listen address (gRPC)
    #[serde(default = "default_listen_addr")]
    pub listen_addr: String,
    
    /// Node labels (comma-separated key=value pairs)
    #[serde(default)]
    pub labels: String,
}

fn default_listen_addr() -> String {
    "0.0.0.0:9000".to_string()
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            listen_addr: default_listen_addr(),
            labels: String::new(),
        }
    }
}

impl NodeConfig {
    /// Parse labels string into HashMap
    pub fn parse_labels(&self) -> HashMap<String, String> {
        let mut labels = HashMap::new();
        if !self.labels.is_empty() {
            for pair in self.labels.split(',') {
                let parts: Vec<&str> = pair.split('=').collect();
                if parts.len() == 2 {
                    labels.insert(parts[0].trim().to_string(), parts[1].trim().to_string());
                }
            }
        }
        labels
    }
}

/// Resource configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceConfig {
    /// CPU cores
    #[serde(default = "default_cpu_cores")]
    pub cpu_cores: f64,
    
    /// Memory in bytes
    #[serde(default = "default_memory_bytes")]
    pub memory_bytes: u64,
    
    /// Disk in bytes
    #[serde(default = "default_disk_bytes")]
    pub disk_bytes: u64,
    
    /// GPU count
    #[serde(default)]
    pub gpu_count: u32,
    
    /// GPU type
    #[serde(default)]
    pub gpu_type: String,
}

fn default_cpu_cores() -> f64 {
    4.0
}

fn default_memory_bytes() -> u64 {
    8 * 1024 * 1024 * 1024 // 8GB
}

fn default_disk_bytes() -> u64 {
    100 * 1024 * 1024 * 1024 // 100GB
}

impl Default for ResourceConfig {
    fn default() -> Self {
        Self {
            cpu_cores: default_cpu_cores(),
            memory_bytes: default_memory_bytes(),
            disk_bytes: default_disk_bytes(),
            gpu_count: 0,
            gpu_type: String::new(),
        }
    }
}

// Legacy types for backward compatibility
pub use EntityRecognitionConfig as AppConfig;

