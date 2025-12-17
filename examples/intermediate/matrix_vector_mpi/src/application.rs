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

//! Matrix-Vector MPI Application
//!
//! ## Purpose
//! Implements the Matrix-Vector MPI example as a PlexSpaces Application,
//! demonstrating MPI-style collective operations in the Application/Release framework.
//!
//! ## Features
//! - Configurable matrix dimensions
//! - Configurable number of workers
//! - TupleSpace-based MPI collectives (scatter, broadcast, gather, reduce)
//! - Performance metrics collection

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker};
use plexspaces_tuplespace::TupleSpace;
use plexspaces_mailbox::Message;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{scatter_rows, broadcast_vector, gather_results, barrier_sync};
use crate::{WorkerActor, WorkerMessage, MatrixVectorConfig};

/// Matrix-Vector MPI Application
///
/// ## Design
/// - Creates TupleSpace for coordination
/// - Spawns worker tasks for parallel computation
/// - Uses MPI-style collective operations
/// - Tracks performance metrics
pub struct MatrixVectorApplication {
    /// Number of matrix rows
    num_rows: usize,
    /// Number of matrix columns
    num_cols: usize,
    /// Number of worker tasks
    num_workers: usize,
    /// TupleSpace for coordination (created during start)
    tuplespace: Arc<RwLock<Option<Arc<TupleSpace>>>>,
    /// Worker actor IDs (for cleanup)
    worker_actor_ids: Arc<RwLock<Vec<String>>>,
}

impl MatrixVectorApplication {
    /// Create new Matrix-Vector Application from config
    ///
    /// Loads configuration using ConfigBootstrap
    pub fn new() -> Self {
        let config: MatrixVectorConfig = ConfigBootstrap::load().unwrap_or_default();
        let num_rows = config.num_rows;
        let num_cols = config.num_cols;
        let num_workers = config.num_workers;

        assert!(num_rows > 0, "Matrix must have at least 1 row");
        assert!(num_cols > 0, "Matrix must have at least 1 column");
        assert!(num_workers > 0, "Need at least 1 worker");
        assert_eq!(
            num_rows % num_workers, 0,
            "Number of rows must be divisible by number of workers"
        );

        Self {
            num_rows,
            num_cols,
            num_workers,
            tuplespace: Arc::new(RwLock::new(None)),
            worker_actor_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create with explicit parameters (for testing)
    pub fn with_params(num_rows: usize, num_cols: usize, num_workers: usize) -> Self {
        assert!(num_rows > 0, "Matrix must have at least 1 row");
        assert!(num_cols > 0, "Matrix must have at least 1 column");
        assert!(num_workers > 0, "Need at least 1 worker");
        assert_eq!(
            num_rows % num_workers, 0,
            "Number of rows must be divisible by number of workers"
        );

        Self {
            num_rows,
            num_cols,
            num_workers,
            tuplespace: Arc::new(RwLock::new(None)),
            worker_actor_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Application for MatrixVectorApplication {
    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        tracing::info!("🚀 Starting Matrix-Vector MPI Application");
        tracing::info!("   Matrix: {}×{}", self.num_rows, self.num_cols);
        tracing::info!("   Workers: {}", self.num_workers);

        // Create TupleSpace
        let space = Arc::new(TupleSpace::new());
        {
            let mut ts_guard = self.tuplespace.write().await;
            *ts_guard = Some(space.clone());
        }

        // Create test data
        let matrix: Vec<Vec<f64>> = (0..self.num_rows)
            .map(|i| (0..self.num_cols).map(|j| (i * self.num_cols + j + 1) as f64).collect())
            .collect();
        let vector: Vec<f64> = (0..self.num_cols).map(|i| (i + 1) as f64).collect();

        tracing::info!("\n📊 Matrix A: {}×{}", self.num_rows, self.num_cols);
        tracing::info!("📊 Vector x: {}×1", self.num_cols);

        // Initialize metrics tracker
        let mut metrics_tracker = CoordinationComputeTracker::new("matrix-vector-mpi".to_string());

        // Create and spawn worker actors
        tracing::info!("\n=== Creating Worker Actors ===");
        let mut worker_actor_ids = Vec::new();
        for worker_id in 0..self.num_workers {
            let worker_actor = WorkerActor::new(space.clone(), worker_id, self.num_cols);
            let behavior = Box::new(worker_actor);
            let actor_id = format!("worker-{}@{}", worker_id, node.id());
            
            // Use ActorFactory directly
            use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
            use plexspaces_mailbox::{mailbox_config_default, Mailbox};
            use std::sync::Arc;
            
            // Create mailbox for actor
            let mut mailbox_config = mailbox_config_default();
            mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
            mailbox_config.ordering_strategy = plexspaces_mailbox::OrderingStrategy::OrderingFifo as i32;
            mailbox_config.durability_strategy = plexspaces_mailbox::DurabilityStrategy::DurabilityNone as i32;
            mailbox_config.capacity = 1000;
            mailbox_config.backpressure_strategy = plexspaces_mailbox::BackpressureStrategy::DropOldest as i32;
            let mailbox = Mailbox::new(mailbox_config, format!("{}:mailbox", actor_id))
                .await
                .map_err(|e| ApplicationError::StartupFailed(format!("Failed to create mailbox for worker {}: {}", worker_id, e)))?;
            
            // Create actor from behavior
            let actor = Actor::new(actor_id.clone(), behavior, mailbox, "matrix-vector-mpi".to_string(), None);
            
            // Spawn using ActorFactory
            let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
                .ok_or_else(|| ApplicationError::StartupFailed(format!("ActorFactory not found in ServiceLocator")))?;
            
            actor_factory.spawn_built_actor(Arc::new(actor), None, None, Some("matrix-vector-mpi".to_string())).await
                .map_err(|e| ApplicationError::StartupFailed(format!("Failed to spawn worker {}: {}", worker_id, e)))?;
            
            worker_actor_ids.push(actor_id);
            tracing::info!("  Created worker actor: worker-{}", worker_id);
        }
        {
            let mut ids_guard = self.worker_actor_ids.write().await;
            *ids_guard = worker_actor_ids.clone();
        }

        // Wait for actors to initialize
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Phase 1: Scatter rows
        tracing::info!("\n=== Phase 1: Scatter (Distribute Rows) ===");
        metrics_tracker.start_coordinate();
        scatter_rows(&space, &matrix, self.num_workers)
            .await
            .map_err(|e| ApplicationError::StartupFailed(e.to_string()))?;
        metrics_tracker.end_coordinate();

        // Phase 2: Broadcast vector
        tracing::info!("\n=== Phase 2: Broadcast (Send Vector to All) ===");
        metrics_tracker.start_coordinate();
        crate::broadcast_vector(&space, &vector)
            .await
            .map_err(|e| ApplicationError::StartupFailed(e.to_string()))?;
        metrics_tracker.end_coordinate();

        // Phase 3: Workers compute (send messages to actors)
        tracing::info!("\n=== Phase 3: Local Computation ===");
        metrics_tracker.start_compute();
        // Note: In a real application, we'd need ActorRef to send messages
        // For now, we'll use a simplified approach where workers read from TupleSpace
        // This is a limitation of the Application trait - it doesn't provide ActorRef
        // Workers will read from TupleSpace and compute automatically
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        metrics_tracker.end_compute();

        // Phase 4: Barrier
        tracing::info!("\n=== Phase 4: Barrier (Synchronize Workers) ===");
        metrics_tracker.start_coordinate();
        metrics_tracker.increment_barrier();
        barrier_sync(&space, self.num_workers)
            .await
            .map_err(|e| ApplicationError::StartupFailed(e.to_string()))?;
        metrics_tracker.end_coordinate();

        // Phase 5: Gather results
        tracing::info!("\n=== Phase 5: Gather (Collect Results) ===");
        metrics_tracker.start_coordinate();
        let _final_result = gather_results(&space, self.num_workers, self.num_rows)
            .await
            .map_err(|e| ApplicationError::StartupFailed(e.to_string()))?;
        metrics_tracker.end_coordinate();

        // Print metrics
        let metrics = metrics_tracker.finalize();
        tracing::info!("\n📈 Performance Metrics:");
        tracing::info!("  Compute time: {:.2}ms", metrics.compute_duration_ms);
        tracing::info!("  Coordination time: {:.2}ms", metrics.coordinate_duration_ms);
        tracing::info!("  Granularity ratio: {:.2}×", metrics.granularity_ratio);
        tracing::info!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);

        tracing::info!("\n✅ Matrix-Vector MPI Application started and computation complete");
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        tracing::info!("🛑 Stopping Matrix-Vector MPI Application");

        // Clear TupleSpace
        {
            let mut ts_guard = self.tuplespace.write().await;
            *ts_guard = None;
        }

        // Clear worker actor IDs
        {
            let mut ids_guard = self.worker_actor_ids.write().await;
            ids_guard.clear();
        }

        tracing::info!("✅ Matrix-Vector MPI Application stopped");
        Ok(())
    }

    fn name(&self) -> &str {
        "matrix-vector-mpi"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::node::v1::ApplicationState;
    use std::collections::HashMap;

    #[test]
    fn test_create_application() {
        let app = MatrixVectorApplication::with_params(8, 4, 2);
        assert_eq!(app.name(), "matrix-vector-mpi");
        assert_eq!(app.num_rows, 8);
        assert_eq!(app.num_cols, 4);
        assert_eq!(app.num_workers, 2);
    }


    #[test]
    #[should_panic(expected = "Matrix must have at least 1 row")]
    fn test_zero_rows() {
        MatrixVectorApplication::with_params(0, 4, 2);
    }

    #[test]
    #[should_panic(expected = "Number of rows must be divisible by number of workers")]
    fn test_indivisible_rows() {
        MatrixVectorApplication::with_params(7, 4, 2); // 7 is not divisible by 2
    }
}
