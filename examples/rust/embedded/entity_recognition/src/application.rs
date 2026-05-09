// SPDX-License-Identifier: AGPL-3.0-or-later
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
//! - Uses SDK spawn helpers (not actor-factory directly)
//! - Resource-aware scheduling via actor groups
//! - Tracks coordination vs compute metrics

use async_trait::async_trait;
use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_node::CoordinationComputeTracker;
use plexspaces_sdk::spawn_with_facets;
use plexspaces_actor::{RequestContext, RequestContextExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

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
/// - Uses SDK spawn helpers so core lifecycle logic stays in the framework
/// - Resource-aware scheduling via actor groups
/// - Tracks coordination vs compute metrics
pub struct EntityRecognitionApplication {
    /// Application configuration
    config: EntityRecognitionConfig,
    /// Metrics tracker
    metrics_tracker: CoordinationComputeTracker,
    /// Actor IDs for cleanup
    actor_ids: Arc<RwLock<Vec<String>>>,
}

impl EntityRecognitionApplication {
    /// Create new Entity Recognition Application
    pub fn new() -> Self {
        let config = EntityRecognitionConfig::load();
        Self {
            config,
            metrics_tracker: CoordinationComputeTracker::new("entity-recognition".to_string()),
            actor_ids: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get configuration
    pub fn config(&self) -> &EntityRecognitionConfig {
        &self.config
    }
}

#[async_trait]
impl Application for EntityRecognitionApplication {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn name(&self) -> &str {
        "entity-recognition"
    }

    fn version(&self) -> &str {
        "0.1.0"
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        let service_locator = node
            .service_locator()
            .ok_or_else(|| ApplicationError::StartupFailed(
                "ServiceLocator not available from ApplicationNode".to_string(),
            ))?;

        let ctx = RequestContext::new_without_auth(
            "entity-recognition".to_string(),
            "entity-recognition".to_string(),
        );

        self.metrics_tracker.start_coordinate();

        // Spawn loader actors (CPU-intensive) via SDK helper
        for i in 0..self.config.loader_count {
            let actor_name = format!("loader-{}", i);
            let actor_ref = spawn_with_facets(
                &ctx,
                service_locator.clone(),
                actor_name.clone(),
                "entity-recognition",
                LoaderBehavior::new(vec![]),
                vec![],
            )
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_name.clone(), e.to_string()))?;
            let actor_id = actor_ref.id().to_string();
            self.actor_ids.write().await.push(actor_id.clone());
            info!(actor_id = %actor_id, "spawned loader actor");
        }

        // Spawn processor actors (GPU-intensive) via SDK helper
        for i in 0..self.config.processor_count {
            let actor_name = format!("processor-{}", i);
            let actor_ref = spawn_with_facets(
                &ctx,
                service_locator.clone(),
                actor_name.clone(),
                "entity-recognition",
                ProcessorBehavior::new(),
                vec![],
            )
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_name.clone(), e.to_string()))?;
            let actor_id = actor_ref.id().to_string();
            self.actor_ids.write().await.push(actor_id.clone());
            info!(actor_id = %actor_id, "spawned processor actor");
        }

        // Spawn aggregator actors (CPU-intensive) via SDK helper
        for i in 0..self.config.aggregator_count {
            let actor_name = format!("aggregator-{}", i);
            let actor_ref = spawn_with_facets(
                &ctx,
                service_locator.clone(),
                actor_name.clone(),
                "entity-recognition",
                AggregatorBehavior::new(0),
                vec![],
            )
            .await
            .map_err(|e| ApplicationError::ActorSpawnFailed(actor_name.clone(), e.to_string()))?;
            let actor_id = actor_ref.id().to_string();
            self.actor_ids.write().await.push(actor_id.clone());
            info!(actor_id = %actor_id, "spawned aggregator actor");
        }

        self.metrics_tracker.end_coordinate();

        let metrics = std::mem::replace(
            &mut self.metrics_tracker,
            CoordinationComputeTracker::new("entity-recognition".to_string()),
        )
        .finalize();

        info!(
            coordinate_ms = metrics.coordinate_duration_ms,
            compute_ms = metrics.compute_duration_ms,
            ratio = metrics.granularity_ratio,
            "entity recognition startup metrics"
        );

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        let ids = self.actor_ids.read().await.clone();
        for actor_id in &ids {
            info!(actor_id = %actor_id, "stopping actor");
        }
        self.actor_ids.write().await.clear();
        Ok(())
    }
}
