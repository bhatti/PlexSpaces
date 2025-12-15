// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Genomics Pipeline Application
//!
//! Erlang/OTP-style application implementation for genomics pipeline.
//! Manages worker actors for quality control, alignment, and variant calling.
//!
//! ## Durability Integration
//! Chromosome workers are configured with durable execution:
//! - SQLite journal for event sourcing
//! - Periodic checkpoints for fast recovery (every 10 variants)
//! - Side effect caching for exactly-once semantics

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationNode, ApplicationError, HealthStatus};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

// Import worker actors
use crate::workers::{
    QCWorker,
    AlignmentWorker,
    ChromosomeWorker,
    AnnotationWorker,
    ReportWorker,
};

// Import configuration
use crate::config::GenomicsPipelineConfig;

// Import supervision infrastructure
use plexspaces_supervisor::{
    Supervisor,
    SupervisedChild,
    ActorSpec,
    SupervisionStrategy,
    RestartPolicy,
    ChildType,
};
use plexspaces_mailbox::{Mailbox, mailbox_config_default, StorageStrategy, OrderingStrategy, DurabilityStrategy, BackpressureStrategy};

/// Genomics Pipeline Application
///
/// ## Purpose
/// Manages the lifecycle of genomics pipeline workers:
/// - Quality control workers
/// - Alignment workers
/// - Variant calling workers (chromosome workers)
/// - Annotation workers
/// - Report workers
///
/// ## Configuration
/// Workers are spawned based on configuration loaded via ConfigBootstrap:
/// - Configuration from `release.toml` or environment variables
/// - Default pool sizes if not specified
pub struct GenomicsPipelineApplication {
    /// Application configuration
    config: GenomicsPipelineConfig,
    /// Supervisors for each worker pool (for fault tolerance)
    supervisors: Arc<RwLock<Vec<Arc<RwLock<Supervisor>>>>>,
}

impl GenomicsPipelineApplication {
    /// Create a new genomics pipeline application with default config
    pub fn new() -> Self {
        Self {
            config: GenomicsPipelineConfig::default(),
            supervisors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new genomics pipeline application with custom config
    pub fn with_config(config: GenomicsPipelineConfig) -> Self {
        Self {
            config,
            supervisors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get application configuration
    pub fn config(&self) -> &GenomicsPipelineConfig {
        &self.config
    }
}

impl Default for GenomicsPipelineApplication {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Application for GenomicsPipelineApplication {
    fn name(&self) -> &str {
        "genomics-pipeline"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        info!("Starting Genomics Pipeline Application");
        info!("Node ID: {}", node.id());
        info!("Node address: {}", node.listen_addr());
        info!("Worker configuration:");
        info!("  QC workers: {}", self.config.worker_pools.qc);
        info!("  Alignment workers: {}", self.config.worker_pools.alignment);
        info!("  Chromosome workers: {}", self.config.worker_pools.chromosome);
        info!("  Annotation workers: {}", self.config.worker_pools.annotation);
        info!("  Report workers: {}", self.config.worker_pools.report);

        let mut supervisors = self.supervisors.write().await;
        let node_id = node.id().to_string();

        // Create supervisor for QC pool
        info!("Creating QC supervisor");
        let (qc_supervisor, _) = Supervisor::new(
            format!("qc-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );

        for i in 0..self.config.worker_pools.qc {
            let actor_id = format!("qc-{}@{}", i, node_id);
            let actor_id_for_factory = actor_id.clone();
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.storage_strategy = StorageStrategy::Memory as i32;
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = Mailbox::new(config);
                    Ok(plexspaces_actor::Actor::new(
                        actor_id_for_factory.clone(),
                        Box::new(QCWorker::new(actor_id_for_factory.clone())),
                        mailbox,
                        "genomics".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            qc_supervisor.add_child(spec).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{:?}", e)))?;
            info!("Added QC actor: {}", actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(qc_supervisor)));

        // Create supervisor for alignment pool
        info!("Creating alignment supervisor");
        let (alignment_supervisor, _) = Supervisor::new(
            format!("alignment-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );

        for i in 0..self.config.worker_pools.alignment {
            let actor_id = format!("alignment-{}@{}", i, node_id);
            let actor_id_for_factory = actor_id.clone();
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.storage_strategy = StorageStrategy::Memory as i32;
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = Mailbox::new(config);
                    Ok(plexspaces_actor::Actor::new(
                        actor_id_for_factory.clone(),
                        Box::new(AlignmentWorker::new(actor_id_for_factory.clone())),
                        mailbox,
                        "genomics".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            alignment_supervisor.add_child(spec).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{:?}", e)))?;
            info!("Added alignment actor: {}", actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(alignment_supervisor)));

        // Create supervisor for variant calling pool
        info!("Creating variant calling supervisor");
        let (variant_supervisor, _) = Supervisor::new(
            format!("variant-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );

        for i in 0..self.config.worker_pools.chromosome {
            let actor_id = format!("variant-{}@{}", i, node_id);
            let chromosome = format!("chr{}", i + 1);
            let actor_id_for_factory = actor_id.clone();
            let chromosome_for_factory = chromosome.clone();
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.storage_strategy = StorageStrategy::Memory as i32;
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = Mailbox::new(config);
                    Ok(plexspaces_actor::Actor::new(
                        actor_id_for_factory.clone(),
                        Box::new(ChromosomeWorker::new(actor_id_for_factory.clone(), chromosome_for_factory.clone())),
                        mailbox,
                        "genomics".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            variant_supervisor.add_child(spec).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{:?}", e)))?;
            info!("Added variant calling actor: {} for {}", actor_id, chromosome);
        }
        supervisors.push(Arc::new(RwLock::new(variant_supervisor)));

        // Create supervisor for annotation pool
        info!("Creating annotation supervisor");
        let (annotation_supervisor, _) = Supervisor::new(
            format!("annotation-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );

        for i in 0..self.config.worker_pools.annotation {
            let actor_id = format!("annotation-{}@{}", i, node_id);
            let actor_id_for_factory = actor_id.clone();
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.storage_strategy = StorageStrategy::Memory as i32;
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = Mailbox::new(config);
                    Ok(plexspaces_actor::Actor::new(
                        actor_id_for_factory.clone(),
                        Box::new(AnnotationWorker::new(actor_id_for_factory.clone())),
                        mailbox,
                        "genomics".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            annotation_supervisor.add_child(spec).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{:?}", e)))?;
            info!("Added annotation actor: {}", actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(annotation_supervisor)));

        // Create supervisor for report pool
        info!("Creating report supervisor");
        let (report_supervisor, _) = Supervisor::new(
            format!("report-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );

        for i in 0..self.config.worker_pools.report {
            let actor_id = format!("report-{}@{}", i, node_id);
            let actor_id_for_factory = actor_id.clone();
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.storage_strategy = StorageStrategy::Memory as i32;
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = Mailbox::new(config);
                    Ok(plexspaces_actor::Actor::new(
                        actor_id_for_factory.clone(),
                        Box::new(ReportWorker::new(actor_id_for_factory.clone())),
                        mailbox,
                        "genomics".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            report_supervisor.add_child(spec).await
                .map_err(|e| ApplicationError::ActorSpawnFailed(actor_id.clone(), format!("{:?}", e)))?;
            info!("Added report actor: {}", actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(report_supervisor)));

        let total_actors: usize = self.config.worker_pools.qc +
                                  self.config.worker_pools.alignment +
                                  self.config.worker_pools.chromosome +
                                  self.config.worker_pools.annotation +
                                  self.config.worker_pools.report;

        info!("Genomics Pipeline Application started with {} supervisors managing {} actors",
              supervisors.len(), total_actors);

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        info!("Stopping Genomics Pipeline Application");

        let supervisors = self.supervisors.read().await;
        info!("Stopping {} supervisors", supervisors.len());

        // Stop all supervisors (which will stop their managed actors)
        for supervisor_lock in supervisors.iter() {
            let mut supervisor = supervisor_lock.write().await;
            info!("Stopping supervisor: {}", supervisor.id());
            supervisor.stop(Some(std::time::Duration::from_secs(10))).await
                .map_err(|e| ApplicationError::ShutdownFailed(format!("{:?}", e)))?;
        }

        info!("Genomics Pipeline Application stopped");

        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // TODO: Check if workers are healthy
        // For now, always return healthy
        HealthStatus::HealthStatusHealthy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockNode {
        id: String,
        addr: String,
    }

    #[async_trait]
    impl plexspaces_core::application::ApplicationNode for MockNode {
        fn id(&self) -> &str {
            &self.id
        }

        fn listen_addr(&self) -> &str {
            &self.addr
        }

        async fn spawn_actor(
            &self,
            actor_id: String,
            _behavior: Box<dyn plexspaces_core::ActorBehavior>,
            _namespace: String,
        ) -> Result<String, ApplicationError> {
            // Mock implementation for tests
            Ok(actor_id)
        }

        async fn stop_actor(
            &self,
            _actor_id: &str,
        ) -> Result<(), ApplicationError> {
            // Mock implementation for tests
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_create_application() {
        let app = GenomicsPipelineApplication::new();
        assert_eq!(app.name(), "genomics-pipeline");
    }

    #[tokio::test]
    async fn test_start_application() {
        let mut app = GenomicsPipelineApplication::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        }) as Arc<dyn ApplicationNode>;

        let result = app.start(node).await;
        assert!(result.is_ok());

        // Verify supervisors were created (one per pool = 5 supervisors)
        let supervisors = app.supervisors.read().await;
        assert_eq!(supervisors.len(), 5);
    }

    #[tokio::test]
    async fn test_stop_application() {
        let mut app = GenomicsPipelineApplication::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        }) as Arc<dyn ApplicationNode>;

        app.start(node).await.unwrap();
        let result = app.stop().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = GenomicsPipelineApplication::new();
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

}
