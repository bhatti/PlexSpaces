// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Entity Recognition Application
//!
//! Implements the `Application` trait for entity recognition workflow,
//! demonstrating resource-aware scheduling with AI workloads.
//!
//! ## Purpose
//! Manages the lifecycle of entity recognition actors:
//! - Loader actors (CPU-intensive)
//! - Processor actors (GPU-intensive)
//! - Aggregator actors (CPU-intensive)
//!
//! ## Design
//! - Uses NodeBuilder/ActorBuilder for actor creation
//! - Resource-aware scheduling via actor groups
//! - Tracks coordination vs compute metrics

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use plexspaces_node::CoordinationComputeTracker;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::EntityRecognitionConfig;
use crate::loader::LoaderBehavior;
use crate::processor::ProcessorBehavior;
use crate::aggregator::AggregatorBehavior;

/// Entity Recognition Application
///
/// ## Purpose
/// Manages the lifecycle of entity recognition actors:
/// - Loader actors (CPU-intensive)
/// - Processor actors (GPU-intensive)
/// - Aggregator actors (CPU-intensive)
///
/// ## Design
/// - Uses NodeBuilder/ActorBuilder for actor creation
/// - Resource-aware scheduling via actor groups
/// - Tracks coordination vs compute metrics
pub struct EntityRecognitionApplication {
    /// Application configuration
    config: EntityRecognitionConfig,
    /// Metrics tracker
    metrics_tracker: CoordinationComputeTracker,
    /// Actor references (for cleanup)
    actor_refs: Arc<RwLock<Vec<String>>>,
}

impl EntityRecognitionApplication {
    /// Create new Entity Recognition Application
    pub fn new() -> Self {
        let config = EntityRecognitionConfig::load();
        Self {
            config,
            metrics_tracker: CoordinationComputeTracker::new("entity-recognition".to_string()),
            actor_refs: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &EntityRecognitionConfig {
        &self.config
    }
}

#[async_trait]
impl Application for EntityRecognitionApplication {
    fn name(&self) -> &str {
        "entity-recognition"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        println!("🚀 Starting Entity Recognition Application");
        println!("   Loaders: {}", self.config.loader_count);
        println!("   Processors: {}", self.config.processor_count);
        println!("   Aggregators: {}", self.config.aggregator_count);
        
        self.metrics_tracker.start_coordinate();

        // Spawn loader actors (CPU-intensive)
        for i in 0..self.config.loader_count {
            let actor_id = format!("loader-{}@{}", i, node.id());
            let behavior = Box::new(LoaderBehavior::new(vec![]));
            node.spawn_actor(actor_id.clone(), behavior, "entity-recognition".to_string())
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{}", e)))?;
            
            self.actor_refs.write().await.push(actor_id.clone());
            println!("   ✅ Spawned {}", actor_id);
        }

        // Spawn processor actors (GPU-intensive)
        for i in 0..self.config.processor_count {
            let actor_id = format!("processor-{}@{}", i, node.id());
            let behavior = Box::new(ProcessorBehavior::new());
            node.spawn_actor(actor_id.clone(), behavior, "entity-recognition".to_string())
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{}", e)))?;
            
            self.actor_refs.write().await.push(actor_id.clone());
            println!("   ✅ Spawned {}", actor_id);
        }

        // Spawn aggregator actors (CPU-intensive)
        for i in 0..self.config.aggregator_count {
            let actor_id = format!("aggregator-{}@{}", i, node.id());
            let behavior = Box::new(AggregatorBehavior::new(0)); // Expected count set later
            node.spawn_actor(actor_id.clone(), behavior, "entity-recognition".to_string())
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{}", e)))?;
            
            self.actor_refs.write().await.push(actor_id.clone());
            println!("   ✅ Spawned {}", actor_id);
        }

        self.metrics_tracker.end_coordinate();
        
        // Report metrics
        let metrics = std::mem::replace(
            &mut self.metrics_tracker,
            CoordinationComputeTracker::new("entity-recognition".to_string())
        ).finalize();
        
        println!("📊 Metrics:");
        println!("   Coordination time: {:.2}ms", metrics.coordinate_duration_ms);
        println!("   Compute time: {:.2}ms", metrics.compute_duration_ms);
        println!("   Granularity ratio: {:.2}", metrics.granularity_ratio);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        println!("🛑 Stopping Entity Recognition Application");
        
        // Stop all actors
        let actor_ids = self.actor_refs.read().await.clone();
        for actor_id in actor_ids {
            // Note: ApplicationNode doesn't have stop_actor, so we'll just log
            println!("   ⏹️  Stopping {}", actor_id);
        }
        
        self.actor_refs.write().await.clear();
        
        Ok(())
    }
}

