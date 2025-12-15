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

//! Configuration for Byzantine Generals Example

use serde::{Deserialize, Serialize};
use plexspaces_node::ConfigBootstrap;

/// Byzantine Generals Configuration (loaded from release.toml)
#[derive(Debug, Deserialize, Default, Clone)]
pub struct ByzantineConfig {
    /// Total number of generals (minimum 4)
    #[serde(default = "default_general_count")]
    pub general_count: usize,
    /// Number of Byzantine (faulty) generals (must be < general_count/3)
    #[serde(default = "default_fault_count")]
    pub fault_count: usize,
    /// TupleSpace backend type
    #[serde(default = "default_tuplespace_backend")]
    pub tuplespace_backend: String,
    /// Redis URL (if using redis backend)
    #[serde(default)]
    pub redis_url: Option<String>,
    /// PostgreSQL URL (if using postgres backend)
    #[serde(default)]
    pub postgres_url: Option<String>,
}

fn default_general_count() -> usize { 4 }
fn default_fault_count() -> usize { 1 }
fn default_tuplespace_backend() -> String { "memory".to_string() }

impl ByzantineConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.general_count < 4 {
            return Err(format!("Need at least 4 generals, got {}", self.general_count));
        }
        if self.fault_count * 3 >= self.general_count {
            return Err(format!(
                "Byzantine count must be < N/3 for consensus (got {} Byzantine, {} total)",
                self.fault_count,
                self.general_count
            ));
        }
        Ok(())
    }
}

