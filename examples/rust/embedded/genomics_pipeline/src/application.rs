// SPDX-License-Identifier: AGPL-3.0-or-later
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
use plexspaces_application::{Application, ApplicationNode, ApplicationError, HealthStatus};
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
use plexspaces_actor::supervisor::{
    Supervisor,
    SupervisedChild,
    SupervisionStrategy,
};
use plexspaces_actor::{ChildSpec, child_spec::{RestartStrategy, ShutdownSpec, StartedChild}};
use plexspaces_core::{ActorId, ActorRef as CoreActorRef};
use plexspaces_mailbox::{Mailbox, mailbox_config_default, OrderingStrategy, BackpressureStrategy};

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

/// Stable supervisor label (opaque string) — not an [`ActorId`], must not match any child instance name.
fn supervisor_label(pool: &str, node_id: &str) -> String {
    let safe = node_id.replace([':', '/', '.'], "-");
    format!("{pool}-supervisor-{safe}")
}

/// Helper to create a ChildSpec from a sync factory
fn create_child_spec(
    child_actor_id: ActorId,
    factory: Arc<dyn Fn() -> Result<plexspaces_actor::Actor, plexspaces_core::ActorError> + Send + Sync>,
    restart: RestartStrategy,
    shutdown_timeout_ms: Option<u64>,
) -> ChildSpec {
    let actor_ref = CoreActorRef::new(child_actor_id.clone()).expect("Failed to create actor ref");

    ChildSpec::worker_sync(
        child_actor_id,
        factory,
        actor_ref,
    )
    .with_restart(restart)
    .with_shutdown(match shutdown_timeout_ms {
        Some(ms) => ShutdownSpec::Timeout(std::time::Duration::from_millis(ms)),
        None => ShutdownSpec::Infinity,
    })
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

        // Create ServiceLocator for supervisors (use node's or fallback to stub)
        let service_locator: Arc<dyn plexspaces_core::ServiceLocator> = node.service_locator()
            .unwrap_or_else(|| Arc::new(plexspaces_actor::TestServiceLocatorStub::new()));

        // Create supervisor for QC pool
        info!("Creating QC supervisor");
        let (qc_supervisor, _) = Supervisor::new(
            supervisor_label("qc", &node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
            service_locator.clone(),
        );

        for i in 0..self.config.worker_pools.qc {
            let child_actor_id = ActorId::new(format!("qc-{i}"), "qc_worker", "genomics", &node_id)
                .map_err(|e| {
                    ApplicationError::ActorSpawnFailed(format!("qc-{i}"), e.to_string())
                })?;
            let child_actor_id_for_factory = child_actor_id.clone();
            let node_id_for_factory = node_id.clone();
            let spec = create_child_spec(
                child_actor_id.clone(),
                Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(Mailbox::new(
                            config,
                            format!("mailbox-{}", child_actor_id_for_factory.name()),
                        ))
                    })
                    .expect("Failed to create mailbox");
                    Ok(plexspaces_actor::Actor::new(
                        child_actor_id_for_factory.clone(),
                        Box::new(QCWorker::new(child_actor_id_for_factory.name().to_string())),
                        mailbox,
                        "default".to_string(), // tenant_id
                        "genomics".to_string(), // namespace
                        Some(node_id_for_factory.clone()), // node_id
                    ))
                }),
                RestartStrategy::Permanent,
                Some(5000),
            );
            qc_supervisor.add_child(spec).await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(child_actor_id.to_string(), format!("{:?}", e))
            })?;
            info!("Added QC actor: {}", child_actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(qc_supervisor)));

        // Create supervisor for alignment pool
        info!("Creating alignment supervisor");
        let (alignment_supervisor, _) = Supervisor::new(
            supervisor_label("alignment", &node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
            service_locator.clone(),
        );

        for i in 0..self.config.worker_pools.alignment {
            let child_actor_id =
                ActorId::new(format!("alignment-{i}"), "alignment_worker", "genomics", &node_id)
                    .map_err(|e| {
                        ApplicationError::ActorSpawnFailed(format!("alignment-{i}"), e.to_string())
                    })?;
            let child_actor_id_for_factory = child_actor_id.clone();
            let node_id_for_factory = node_id.clone();
            let spec = create_child_spec(
                child_actor_id.clone(),
                Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(Mailbox::new(
                            config,
                            format!("mailbox-{}", child_actor_id_for_factory.name()),
                        ))
                    })
                    .expect("Failed to create mailbox");
                    Ok(plexspaces_actor::Actor::new(
                        child_actor_id_for_factory.clone(),
                        Box::new(AlignmentWorker::new(
                            child_actor_id_for_factory.name().to_string(),
                        )),
                        mailbox,
                        "default".to_string(), // tenant_id
                        "genomics".to_string(), // namespace
                        Some(node_id_for_factory.clone()), // node_id
                    ))
                }),
                RestartStrategy::Permanent,
                Some(5000),
            );
            alignment_supervisor.add_child(spec).await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(child_actor_id.to_string(), format!("{:?}", e))
            })?;
            info!("Added alignment actor: {}", child_actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(alignment_supervisor)));

        // Create supervisor for variant calling pool
        info!("Creating variant calling supervisor");
        let (variant_supervisor, _) = Supervisor::new(
            supervisor_label("variant", &node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
            service_locator.clone(),
        );

        for i in 0..self.config.worker_pools.chromosome {
            let child_actor_id =
                ActorId::new(format!("variant-{i}"), "chromosome_worker", "genomics", &node_id)
                    .map_err(|e| {
                        ApplicationError::ActorSpawnFailed(format!("variant-{i}"), e.to_string())
                    })?;
            let chromosome = format!("chr{}", i + 1);
            let child_actor_id_for_factory = child_actor_id.clone();
            let chromosome_for_factory = chromosome.clone();
            let node_id_for_factory = node_id.clone();
            let spec = create_child_spec(
                child_actor_id.clone(),
                Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(Mailbox::new(
                            config,
                            format!("mailbox-{}", child_actor_id_for_factory.name()),
                        ))
                    })
                    .expect("Failed to create mailbox");
                    Ok(plexspaces_actor::Actor::new(
                        child_actor_id_for_factory.clone(),
                        Box::new(ChromosomeWorker::new(
                            child_actor_id_for_factory.name().to_string(),
                            chromosome_for_factory.clone(),
                        )),
                        mailbox,
                        "default".to_string(), // tenant_id
                        "genomics".to_string(), // namespace
                        Some(node_id_for_factory.clone()), // node_id
                    ))
                }),
                RestartStrategy::Permanent,
                Some(5000),
            );
            variant_supervisor.add_child(spec).await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(child_actor_id.to_string(), format!("{:?}", e))
            })?;
            info!(
                "Added variant calling actor: {} for {}",
                child_actor_id, chromosome
            );
        }
        supervisors.push(Arc::new(RwLock::new(variant_supervisor)));

        // Create supervisor for annotation pool
        info!("Creating annotation supervisor");
        let (annotation_supervisor, _) = Supervisor::new(
            supervisor_label("annotation", &node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
            service_locator.clone(),
        );

        for i in 0..self.config.worker_pools.annotation {
            let child_actor_id =
                ActorId::new(format!("annotation-{i}"), "annotation_worker", "genomics", &node_id)
                    .map_err(|e| {
                        ApplicationError::ActorSpawnFailed(format!("annotation-{i}"), e.to_string())
                    })?;
            let child_actor_id_for_factory = child_actor_id.clone();
            let node_id_for_factory = node_id.clone();
            let spec = create_child_spec(
                child_actor_id.clone(),
                Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(Mailbox::new(
                            config,
                            format!("mailbox-{}", child_actor_id_for_factory.name()),
                        ))
                    })
                    .expect("Failed to create mailbox");
                    Ok(plexspaces_actor::Actor::new(
                        child_actor_id_for_factory.clone(),
                        Box::new(AnnotationWorker::new(
                            child_actor_id_for_factory.name().to_string(),
                        )),
                        mailbox,
                        "default".to_string(), // tenant_id
                        "genomics".to_string(), // namespace
                        Some(node_id_for_factory.clone()), // node_id
                    ))
                }),
                RestartStrategy::Permanent,
                Some(5000),
            );
            annotation_supervisor.add_child(spec).await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(child_actor_id.to_string(), format!("{:?}", e))
            })?;
            info!("Added annotation actor: {}", child_actor_id);
        }
        supervisors.push(Arc::new(RwLock::new(annotation_supervisor)));

        // Create supervisor for report pool
        info!("Creating report supervisor");
        let (report_supervisor, _) = Supervisor::new(
            supervisor_label("report", &node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
            service_locator.clone(),
        );

        for i in 0..self.config.worker_pools.report {
            let child_actor_id =
                ActorId::new(format!("report-{i}"), "report_worker", "genomics", &node_id).map_err(
                    |e| ApplicationError::ActorSpawnFailed(format!("report-{i}"), e.to_string()),
                )?;
            let child_actor_id_for_factory = child_actor_id.clone();
            let node_id_for_factory = node_id.clone();
            let spec = create_child_spec(
                child_actor_id.clone(),
                Arc::new(move || {
                    let mut config = mailbox_config_default();
                    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
                    config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
                    config.capacity = 1000;
                    let mailbox = tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(Mailbox::new(
                            config,
                            format!("mailbox-{}", child_actor_id_for_factory.name()),
                        ))
                    })
                    .expect("Failed to create mailbox");
                    Ok(plexspaces_actor::Actor::new(
                        child_actor_id_for_factory.clone(),
                        Box::new(ReportWorker::new(
                            child_actor_id_for_factory.name().to_string(),
                        )),
                        mailbox,
                        "default".to_string(), // tenant_id
                        "genomics".to_string(), // namespace
                        Some(node_id_for_factory.clone()), // node_id
                    ))
                }),
                RestartStrategy::Permanent,
                Some(5000),
            );
            report_supervisor.add_child(spec).await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(child_actor_id.to_string(), format!("{:?}", e))
            })?;
            info!("Added report actor: {}", child_actor_id);
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
    impl plexspaces_application::ApplicationNode for MockNode {
        fn id(&self) -> &str {
            &self.id
        }

        fn listen_addr(&self) -> &str {
            &self.addr
        }
    }

    #[tokio::test]
    async fn test_create_application() {
        let app = GenomicsPipelineApplication::new();
        assert_eq!(app.name(), "genomics-pipeline");
    }

    #[tokio::test(flavor = "multi_thread")]
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

    #[tokio::test(flavor = "multi_thread")]
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
