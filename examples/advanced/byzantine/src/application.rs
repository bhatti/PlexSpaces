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

//! Byzantine Generals Application
//!
//! ## Purpose
//! Implements the Byzantine Generals example as a PlexSpaces Application,
//! demonstrating how to package a distributed consensus algorithm into
//! the Application/Release framework.
//!
//! ## Features
//! - Configurable number of generals
//! - Configurable number of Byzantine (faulty) generals
//! - TupleSpace-based coordination
//! - Supervision tree for fault tolerance
//! - Graceful startup and shutdown

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use plexspaces_node::CoordinationComputeTracker;
use plexspaces_persistence::{MemoryJournal, Journal};
use plexspaces_tuplespace::TupleSpace;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::byzantine::General;
use crate::config::ByzantineConfig;

/// Byzantine Generals Application
///
/// ## Design
/// - Uses NodeBuilder/ActorBuilder for actor creation
/// - Creates N generals (1 commander, N-1 lieutenants)
/// - F generals are Byzantine (faulty)
/// - Uses TupleSpace for coordination
/// - Implements Application trait for lifecycle management
/// - Tracks coordination vs compute metrics
pub struct ByzantineApplication {
    /// Configuration
    config: ByzantineConfig,
    /// Metrics tracker
    metrics_tracker: CoordinationComputeTracker,
}

impl ByzantineApplication {
    /// Create new Byzantine Application from config
    pub fn from_config(config: ByzantineConfig) -> Result<Self, String> {
        config.validate()?;
        
        Ok(Self {
            config,
            metrics_tracker: CoordinationComputeTracker::new("byzantine-generals".to_string()),
        })
    }

    /// Create new Byzantine Application
    ///
    /// ## Arguments
    /// * `general_count` - Total number of generals (minimum 4)
    /// * `byzantine_count` - Number of Byzantine generals (must be < general_count/3)
    pub fn new(general_count: usize, byzantine_count: usize) -> Self {
        let config = ByzantineConfig {
            general_count,
            fault_count: byzantine_count,
            ..Default::default()
        };
        
        Self {
            config,
            metrics_tracker: CoordinationComputeTracker::new("byzantine-generals".to_string()),
        }
    }
}

#[async_trait]
impl Application for ByzantineApplication {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        println!("🚀 Starting Byzantine Generals Application");
        println!("   Generals: {}", self.config.general_count);
        println!("   Byzantine: {}", self.config.fault_count);
        
        self.metrics_tracker.start_coordinate();

        // Create shared resources
        let journal = Arc::new(MemoryJournal::new());
        let tuplespace = Arc::new(TupleSpace::new());

        // Spawn generals using ApplicationNode::spawn_actor
        for i in 0..self.config.general_count {
            let general_id = format!("general-{}@{}", i, node.id());
            let is_commander = i == 0; // First general is commander
            let is_byzantine = i < self.config.fault_count;

            // Create general behavior
            let general = General::new(
                general_id.clone(),
                is_commander,
                is_byzantine,
                journal.clone() as Arc<dyn Journal>,
                tuplespace.clone(),
            ).await.map_err(|e| ApplicationError::StartupFailed(format!("Failed to create General: {}", e)))?;

            // Spawn actor using ApplicationNode trait
            let behavior = Box::new(general);
            node.spawn_actor(general_id.clone(), behavior, "byzantine".to_string())
                .await
                .map_err(|e| ApplicationError::StartupFailed(format!("Failed to spawn {}: {}", general_id, e)))?;

            println!("   ✅ Spawned {} (byzantine: {})", general_id, is_byzantine);
        }

        self.metrics_tracker.end_coordinate();
        
        // Report metrics (clone tracker to avoid move)
        let metrics = std::mem::replace(&mut self.metrics_tracker, CoordinationComputeTracker::new("byzantine-generals".to_string())).finalize();
        println!("📊 Metrics:");
        println!("   Coordination time: {:.2}ms", metrics.coordinate_duration_ms);
        println!("   Compute time: {:.2}ms", metrics.compute_duration_ms);
        println!("   Granularity ratio: {:.2}", metrics.granularity_ratio);
        
        println!("✅ Byzantine Generals Application started");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        println!("🛑 Stopping Byzantine Generals Application");
        println!("✅ Byzantine Generals Application stopped");
        Ok(())
    }

    fn name(&self) -> &str {
        "byzantine-generals"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Test: Create application with valid parameters
    #[test]
    fn test_create_application() {
        let app = ByzantineApplication::new(4, 1);
        assert_eq!(app.name(), "byzantine-generals");
        assert_eq!(app.config.general_count, 4);
        assert_eq!(app.config.fault_count, 1);
    }

    /// Test: Create from environment
    // TODO: Re-enable after migrating to ApplicationNode API
    #[test]
    #[cfg(feature = "disabled_tests")]
    fn test_create_from_env() {
        use plexspaces_proto::node::v1::{ApplicationConfig, ApplicationState};
        use std::collections::HashMap;

        let mut env = HashMap::new();
        env.insert("BYZANTINE_GENERAL_COUNT".to_string(), "6".to_string());
        env.insert("BYZANTINE_FAULT_COUNT".to_string(), "1".to_string());

        let state = ApplicationState {
            name: "byzantine-generals".to_string(),
            status: 0,
            start_timestamp_ms: 0,
            supervisor_pid: None,
            env,
        };

        let ctx = ApplicationContext::new(state);
        let app = ByzantineApplication::from_env(&ctx);

        assert_eq!(app.general_count, 6);
        assert_eq!(app.byzantine_count, 1);
    }

    /// Test: Application lifecycle (start/stop)
    // TODO: Re-enable after migrating to ApplicationNode API
    #[tokio::test]
    #[cfg(feature = "disabled_tests")]
    async fn test_application_lifecycle() {
        use plexspaces_proto::node::v1::{ApplicationConfig, ApplicationState};
        use std::collections::HashMap;

        let app = ByzantineApplication::new(4, 1);

        let state = ApplicationState {
            name: "byzantine-generals".to_string(),
            status: 0,
            start_timestamp_ms: 0,
            supervisor_pid: None,
            env: HashMap::new(),
        };

        let ctx = ApplicationContext::new(state);

        // Start
        app.start(&ctx)
            .await
            .expect("Start should succeed");

        // Verify supervisor was created
        {
            let sup_guard = app.supervisor.read().await;
            assert!(sup_guard.is_some());
        }

        // Stop
        app.stop(&ctx)
            .await
            .expect("Stop should succeed");

        // Verify supervisor was cleaned up
        {
            let sup_guard = app.supervisor.read().await;
            assert!(sup_guard.is_none());
        }
    }

    /// Test: Invalid configuration (too few generals)
    #[test]
    fn test_too_few_generals() {
        let config = ByzantineConfig {
            general_count: 3,
            fault_count: 0,
            ..Default::default()
        };
        assert!(ByzantineApplication::from_config(config).is_err());
    }

    /// Test: Invalid configuration (too many Byzantine)
    #[test]
    fn test_too_many_byzantine() {
        let config = ByzantineConfig {
            general_count: 6,
            fault_count: 2,  // 2 >= 6/3, should fail
            ..Default::default()
        };
        assert!(ByzantineApplication::from_config(config).is_err());
    }
}
