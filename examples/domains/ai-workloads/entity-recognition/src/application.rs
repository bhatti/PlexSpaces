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

        // Use ActorFactory directly
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl, Actor};
        use plexspaces_mailbox::{mailbox_config_default, Mailbox};
        
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| ApplicationError::StartupFailed("ActorFactory not found in ServiceLocator".to_string()))?;
        
        // Create mailbox config
        let mut mailbox_config = mailbox_config_default();
        mailbox_config.storage_strategy = plexspaces_mailbox::StorageStrategy::Memory as i32;
        mailbox_config.ordering_strategy = plexspaces_mailbox::OrderingStrategy::OrderingFifo as i32;
        mailbox_config.durability_strategy = plexspaces_mailbox::DurabilityStrategy::DurabilityNone as i32;
        mailbox_config.capacity = 1000;
        mailbox_config.backpressure_strategy = plexspaces_mailbox::BackpressureStrategy::DropOldest as i32;
        
        // Spawn loader actors (CPU-intensive)
        for i in 0..self.config.loader_count {
            let actor_id = format!("loader-{}@{}", i, node.id());
            let behavior = Box::new(LoaderBehavior::new(vec![]));
            
            let mailbox = Mailbox::new(mailbox_config.clone(), format!("{}:mailbox", actor_id))
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Failed to create mailbox: {}", e)))?;
            
            let actor = Actor::new(actor_id.clone(), behavior, mailbox, "entity-recognition".to_string(), None);
            
            let ctx = plexspaces_core::RequestContext::internal();
            actor_factory.spawn_actor(
                &ctx,
                &actor_id,
                "entity-recognition", // actor_type
                vec![], // initial_state
                None, // config
                std::collections::HashMap::new(), // labels
                vec![], // facets
            ).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{}", e)))?;
            
            self.actor_refs.write().await.push(actor_id.clone());
            println!("   ✅ Spawned {}", actor_id);
        }

        // Spawn processor actors (GPU-intensive)
        for i in 0..self.config.processor_count {
            let actor_id = format!("processor-{}@{}", i, node.id());
            let behavior = Box::new(ProcessorBehavior::new());
            
            let mailbox = Mailbox::new(mailbox_config.clone(), format!("{}:mailbox", actor_id))
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Failed to create mailbox: {}", e)))?;
            
            let actor = Actor::new(actor_id.clone(), behavior, mailbox, "entity-recognition".to_string(), None);
            
            let ctx = plexspaces_core::RequestContext::internal();
            actor_factory.spawn_actor(
                &ctx,
                &actor_id,
                "entity-recognition", // actor_type
                vec![], // initial_state
                None, // config
                std::collections::HashMap::new(), // labels
                vec![], // facets
            ).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{}", e)))?;
            
            self.actor_refs.write().await.push(actor_id.clone());
            println!("   ✅ Spawned {}", actor_id);
        }

        // Spawn aggregator actors (CPU-intensive)
        for i in 0..self.config.aggregator_count {
            let actor_id = format!("aggregator-{}@{}", i, node.id());
            let behavior = Box::new(AggregatorBehavior::new(0)); // Expected count set later
            
            let mailbox = Mailbox::new(mailbox_config.clone(), format!("{}:mailbox", actor_id))
                .await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("Failed to create mailbox: {}", e)))?;
            
            let actor = Actor::new(actor_id.clone(), behavior, mailbox, "entity-recognition".to_string(), None);
            
            let ctx = plexspaces_core::RequestContext::internal();
            actor_factory.spawn_actor(
                &ctx,
                &actor_id,
                "entity-recognition", // actor_type
                vec![], // initial_state
                None, // config
                std::collections::HashMap::new(), // labels
                vec![], // facets
            ).await
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

