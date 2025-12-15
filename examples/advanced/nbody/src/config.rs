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

//! N-Body Simulation Configuration

use plexspaces_node::ConfigBootstrap;
use serde::{Deserialize, Serialize};

/// N-Body simulation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NBodyConfig {
    /// Number of bodies to simulate
    #[serde(default = "default_body_count")]
    pub body_count: usize,

    /// Number of simulation steps
    #[serde(default = "default_steps")]
    pub steps: u32,

    /// Time step (seconds)
    #[serde(default = "default_dt")]
    pub dt: f64,

    /// Use 3-body system (Sun, Earth, Moon) if true, otherwise generate random bodies
    #[serde(default = "default_true")]
    pub use_3body_system: bool,
}

fn default_body_count() -> usize {
    3
}

fn default_steps() -> u32 {
    10
}

fn default_dt() -> f64 {
    1.0
}

fn default_true() -> bool {
    true
}

impl Default for NBodyConfig {
    fn default() -> Self {
        Self {
            body_count: default_body_count(),
            steps: default_steps(),
            dt: default_dt(),
            use_3body_system: default_true(),
        }
    }
}

impl NBodyConfig {
    /// Load configuration using ConfigBootstrap
    pub fn load() -> Self {
        ConfigBootstrap::load().unwrap_or_default()
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<(), String> {
        if self.body_count == 0 {
            return Err("body_count must be > 0".to_string());
        }
        if self.steps == 0 {
            return Err("steps must be > 0".to_string());
        }
        if self.dt <= 0.0 {
            return Err("dt must be > 0.0".to_string());
        }
        Ok(())
    }
}

