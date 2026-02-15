// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Byzantine Generals Configuration
//
// Loaded via ConfigBootstrap from release.toml

use plexspaces_node::ConfigBootstrap;
use serde::Deserialize;

/// Byzantine Generals Configuration
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ByzantineConfig {
    /// Total number of generals (minimum 4)
    #[serde(default = "default_general_count")]
    pub general_count: usize,
    
    /// Number of Byzantine (faulty) generals (must be < general_count/3)
    #[serde(default = "default_fault_count")]
    pub fault_count: usize,
    
    /// Number of consensus rounds to run
    #[serde(default = "default_num_rounds")]
    pub num_rounds: usize,
}

fn default_general_count() -> usize { 20 }
fn default_fault_count() -> usize { 5 }
fn default_num_rounds() -> usize { 5 }

impl ByzantineConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.general_count < 4 {
            return Err(format!(
                "Need at least 4 generals, got {}",
                self.general_count
            ));
        }
        
        // Byzantine fault tolerance requires: n >= 3f + 1
        // So f < n/3
        if self.fault_count * 3 >= self.general_count {
            return Err(format!(
                "Byzantine count must be < n/3 for consensus (got {} faulty, {} total)",
                self.fault_count,
                self.general_count
            ));
        }
        
        Ok(())
    }
}
