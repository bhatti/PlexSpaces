// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Finance Risk Assessment Application
//!
//! Erlang/OTP-style application implementation for finance risk assessment.
//! Manages worker actors for credit checks, bank analysis, employment verification,
//! risk scoring, and post-decision services.

use async_trait::async_trait;
use plexspaces_core::application::{Application, ApplicationError, ApplicationNode, HealthStatus};
use plexspaces_core::ServiceLocator;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};

use plexspaces_supervisor::{
    ActorSpec, ChildType, RestartPolicy, SupervisedChild, SupervisionStrategy, Supervisor,
};
// TODO: Refactor actors to use new API
// use crate::actors::{
//     credit_risk_actor,
//     market_risk_actor,
//     operational_risk_actor,
//     liquidity_risk_actor,
//     compliance_risk_actor,
//     fraud_detection_actor,
//     portfolio_optimization_actor,
//     scenario_analysis_actor,
// };
use plexspaces_core::ActorError;
use plexspaces_core::ActorRef;
use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};

// Import durability infrastructure
use crate::storage_config::StorageConfig;
use plexspaces_journaling::SqliteJournalStorage;

/// Finance Risk Assessment Application
///
/// ## Purpose
/// Manages the lifecycle of finance risk assessment workers:
/// - Data collection workers (credit, bank, employment)
/// - Risk scoring workers
/// - Decision engine workers
/// - Post-decision service workers
///
/// ## Configuration
/// Workers are spawned based on environment variables:
/// - `CREDIT_POOL_SIZE`: Number of credit check workers (default: 5)
/// - `BANK_POOL_SIZE`: Number of bank analysis workers (default: 5)
/// - `EMPLOYMENT_POOL_SIZE`: Number of employment verification workers (default: 3)
/// - `RISK_SCORING_POOL_SIZE`: Number of risk scoring workers (default: 2)
/// - `DECISION_ENGINE_POOL_SIZE`: Number of decision engine workers (default: 1)
/// - `DOCUMENT_POOL_SIZE`: Number of document generation workers (default: 2)
/// - `NOTIFICATION_POOL_SIZE`: Number of notification workers (default: 3)
/// - `AUDIT_POOL_SIZE`: Number of audit logging workers (default: 1)
pub struct FinanceRiskApplication {
    /// Worker configuration
    config: WorkerConfig,
    /// Storage configuration for durability (None = no persistence)
    storage_config: Option<StorageConfig>,
    /// Shared SQLite storage (created once, cloned for each actor)
    storage: Arc<RwLock<Option<SqliteJournalStorage>>>,
    /// Node context for spawning and stopping actors
    node_context: Option<Arc<dyn ApplicationNode>>,
    /// Supervisors for each worker pool (for fault tolerance)
    supervisors: Arc<RwLock<Vec<Arc<RwLock<Supervisor>>>>>,
}

/// Worker pool configuration
#[derive(Debug, Clone)]
pub struct WorkerConfig {
    // Data collection pools
    credit_pool_size: usize,
    bank_pool_size: usize,
    employment_pool_size: usize,

    // Risk assessment pools
    risk_scoring_pool_size: usize,
    decision_engine_pool_size: usize,

    // Post-decision pools
    document_pool_size: usize,
    notification_pool_size: usize,
    audit_pool_size: usize,
}

impl WorkerConfig {
    /// Load configuration from environment variables
    fn from_env() -> Self {
        Self {
            // Data collection (Node 2)
            credit_pool_size: std::env::var("CREDIT_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            bank_pool_size: std::env::var("BANK_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5),
            employment_pool_size: std::env::var("EMPLOYMENT_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),

            // Risk assessment (Node 3)
            risk_scoring_pool_size: std::env::var("RISK_SCORING_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            decision_engine_pool_size: std::env::var("DECISION_ENGINE_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),

            // Post-decision services (Node 4)
            document_pool_size: std::env::var("DOCUMENT_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            notification_pool_size: std::env::var("NOTIFICATION_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
            audit_pool_size: std::env::var("AUDIT_POOL_SIZE")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
        }
    }
}

impl FinanceRiskApplication {
    /// Create a new finance risk assessment application without durability
    pub fn new() -> Self {
        Self {
            config: WorkerConfig::from_env(),
            storage_config: None,
            storage: Arc::new(RwLock::new(None)),
            node_context: None,
            supervisors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new finance risk assessment application with SQLite-based durability
    pub fn with_storage(storage_config: StorageConfig) -> Self {
        Self {
            config: WorkerConfig::from_env(),
            storage_config: Some(storage_config),
            storage: Arc::new(RwLock::new(None)),
            node_context: None,
            supervisors: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Get worker configuration
    pub fn config(&self) -> &WorkerConfig {
        &self.config
    }
}

impl Default for FinanceRiskApplication {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Application for FinanceRiskApplication {
    fn name(&self) -> &str {
        "finance-risk"
    }

    fn version(&self) -> &str {
        env!("CARGO_PKG_VERSION")
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        info!("Starting Finance Risk Assessment Application");
        info!("Node ID: {}", node.id());
        info!("Node address: {}", node.listen_addr());
        info!("Worker configuration:");
        info!("  Data Collection:");
        info!("    Credit check actors: {}", self.config.credit_pool_size);
        info!("    Bank analysis actors: {}", self.config.bank_pool_size);
        info!(
            "    Employment verification actors: {}",
            self.config.employment_pool_size
        );
        info!("  Risk Assessment:");
        info!(
            "    Risk scoring actors: {}",
            self.config.risk_scoring_pool_size
        );
        info!(
            "    Decision engine actors: {}",
            self.config.decision_engine_pool_size
        );
        info!("  Post-Decision Services:");
        info!(
            "    Document generation actors: {}",
            self.config.document_pool_size
        );
        info!(
            "    Notification actors: {}",
            self.config.notification_pool_size
        );
        info!("    Audit logging actors: {}", self.config.audit_pool_size);

        self.node_context = Some(node.clone());

        // Initialize storage if durability is enabled
        if let Some(ref storage_config) = self.storage_config {
            info!("Durability enabled:");
            info!("  Database: {}", storage_config.database_path().display());
            info!(
                "  Checkpoint interval: {}",
                storage_config.checkpoint_interval
            );
            info!("  Compression: {}", storage_config.enable_compression);
            info!(
                "  Replay on activation: {}",
                storage_config.replay_on_activation
            );
            info!(
                "  Cache side effects: {}",
                storage_config.cache_side_effects
            );

            // Initialize storage directory
            storage_config.initialize().await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(
                    "storage-init".to_string(),
                    format!("Failed to initialize storage: {:?}", e),
                )
            })?;

            // Create SQLite journal storage
            let storage = storage_config.create_storage().await.map_err(|e| {
                ApplicationError::ActorSpawnFailed(
                    "storage-create".to_string(),
                    format!("Failed to create storage: {:?}", e),
                )
            })?;

            // Store for use by actors
            *self.storage.write().await = Some(storage);

            info!("Storage initialized successfully");
        } else {
            info!("Durability disabled - actors will not persist state");
        }

        let mut supervisors = self.supervisors.write().await;
        let node_id = node.id().to_string();

        // Note: This Application trait implementation uses the old supervisor pattern.
        // The simplified example in main.rs uses NodeBuilder/ActorBuilder directly.
        // This Application implementation is kept for reference but is not used in the simplified example.
        /*
        // Create supervisor for credit risk pool
        let (mut credit_supervisor, _) = Supervisor::new(
            format!("credit-risk-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.credit_pool_size {
            let actor_id = format!("credit-risk-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(credit_risk_actor::CreditRiskActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            credit_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        credit_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(credit_supervisor)));

        // ... (create and start other supervisors)

        // Create supervisor for market risk pool
        let (mut market_supervisor, _) = Supervisor::new(
            format!("market-risk-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.bank_pool_size {
            let actor_id = format!("market-risk-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(market_risk_actor::MarketRiskActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            market_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        market_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(market_supervisor)));

        // Create supervisor for operational risk pool
        let (mut operational_supervisor, _) = Supervisor::new(
            format!("operational-risk-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.employment_pool_size {
            let actor_id = format!("operational-risk-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(operational_risk_actor::OperationalRiskActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            operational_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        operational_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(operational_supervisor)));

        // Create supervisor for liquidity risk pool
        let (mut liquidity_supervisor, _) = Supervisor::new(
            format!("liquidity-risk-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.risk_scoring_pool_size {
            let actor_id = format!("liquidity-risk-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(liquidity_risk_actor::LiquidityRiskActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            liquidity_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        liquidity_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(liquidity_supervisor)));

        // Create supervisor for compliance risk pool
        let (mut compliance_supervisor, _) = Supervisor::new(
            format!("compliance-risk-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.decision_engine_pool_size {
            let actor_id = format!("compliance-risk-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(compliance_risk_actor::ComplianceRiskActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            compliance_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        compliance_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(compliance_supervisor)));

        // Create supervisor for fraud detection pool
        let (mut fraud_supervisor, _) = Supervisor::new(
            format!("fraud-detection-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.document_pool_size {
            let actor_id = format!("fraud-detection-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(fraud_detection_actor::FraudDetectionActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            fraud_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        fraud_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(fraud_supervisor)));

        // Create supervisor for portfolio optimization pool
        let (mut portfolio_supervisor, _) = Supervisor::new(
            format!("portfolio-optimization-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.notification_pool_size {
            let actor_id = format!("portfolio-optimization-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(portfolio_optimization_actor::PortfolioOptimizationActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            portfolio_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        portfolio_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(portfolio_supervisor)));

        // Create supervisor for scenario analysis pool
        let (mut scenario_supervisor, _) = Supervisor::new(
            format!("scenario-analysis-supervisor@{}", node_id),
            SupervisionStrategy::OneForOne { max_restarts: 3, within_seconds: 5 },
        );
        for i in 0..self.config.audit_pool_size {
            let actor_id = format!("scenario-analysis-{}@{}", i, node_id);
            let spec = ActorSpec {
                id: actor_id.clone(),
                factory: Arc::new(move || {
                    let mailbox = Mailbox::new(Default::default());
                    Ok(plexspaces_actor::Actor::new(
                        actor_id.clone(),
                        Box::new(scenario_analysis_actor::ScenarioAnalysisActorBehavior),
                        mailbox,
                        "finance".to_string(),
                    ))
                }),
                restart: RestartPolicy::Permanent,
                child_type: ChildType::Worker,
                shutdown_timeout_ms: Some(5000),
            };
            scenario_supervisor.add_child(spec).await.map_err(|e| ApplicationError::ActorSpawnFailed(actor_id, format!("{:?}", e)))?;
        }
        scenario_supervisor.start().await.map_err(|e| ApplicationError::Internal(format!("Failed to start supervisor: {:?}", e)))?;
        supervisors.push(Arc::new(RwLock::new(scenario_supervisor)));
        */

        info!(
            "Finance Risk Assessment Application started with {} supervisors",
            supervisors.len()
        );

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        info!("Stopping Finance Risk Assessment Application");

        let supervisors = self.supervisors.read().await;
        info!("Stopping {} supervisors", supervisors.len());

        // Stop all supervisors (which will stop their managed actors)
        for supervisor_lock in supervisors.iter() {
            let mut supervisor = supervisor_lock.write().await;
            info!("Stopping supervisor: {}", supervisor.id());
            supervisor
                .stop(Some(std::time::Duration::from_secs(10)))
                .await
                .map_err(|e| ApplicationError::ShutdownFailed(format!("{:?}", e)))?;
        }

        info!("Finance Risk Assessment Application stopped");

        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        // Note: In production, this would check worker health via ObjectRegistry
        // For this simplified example, always return healthy
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
    impl plexspaces_core::application::ApplicationNode for MockNode {
        fn id(&self) -> &str {
            &self.id
        }

        fn listen_addr(&self) -> &str {
            &self.addr
        }

        // Note: spawn_actor and stop_actor are not part of ApplicationNode trait
        // These methods were removed - use ActorFactory instead
    }

    #[tokio::test]
    async fn test_create_application() {
        let app = FinanceRiskApplication::new();
        assert_eq!(app.name(), "finance-risk");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_start_application() {
        let mut app = FinanceRiskApplication::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        let result = app.start(node.clone()).await;
        assert!(result.is_ok());

        // Note: Simplified version doesn't create supervisors in start()
        // Supervisors are created via NodeBuilder/ActorBuilder in main.rs
        let supervisors = app.supervisors.read().await;
        assert_eq!(supervisors.len(), 0);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_stop_application() {
        let mut app = FinanceRiskApplication::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        app.start(node.clone()).await.unwrap();
        let result = app.stop().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_health_check() {
        let app = FinanceRiskApplication::new();
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_worker_config_from_env() {
        // Set env vars
        std::env::set_var("CREDIT_POOL_SIZE", "10");
        std::env::set_var("BANK_POOL_SIZE", "8");
        std::env::set_var("EMPLOYMENT_POOL_SIZE", "6");
        std::env::set_var("RISK_SCORING_POOL_SIZE", "4");
        std::env::set_var("DECISION_ENGINE_POOL_SIZE", "2");
        std::env::set_var("DOCUMENT_POOL_SIZE", "3");
        std::env::set_var("NOTIFICATION_POOL_SIZE", "5");
        std::env::set_var("AUDIT_POOL_SIZE", "2");

        let config = WorkerConfig::from_env();
        assert_eq!(config.credit_pool_size, 10);
        assert_eq!(config.bank_pool_size, 8);
        assert_eq!(config.employment_pool_size, 6);
        assert_eq!(config.risk_scoring_pool_size, 4);
        assert_eq!(config.decision_engine_pool_size, 2);
        assert_eq!(config.document_pool_size, 3);
        assert_eq!(config.notification_pool_size, 5);
        assert_eq!(config.audit_pool_size, 2);

        // Clean up
        std::env::remove_var("CREDIT_POOL_SIZE");
        std::env::remove_var("BANK_POOL_SIZE");
        std::env::remove_var("EMPLOYMENT_POOL_SIZE");
        std::env::remove_var("RISK_SCORING_POOL_SIZE");
        std::env::remove_var("DECISION_ENGINE_POOL_SIZE");
        std::env::remove_var("DOCUMENT_POOL_SIZE");
        std::env::remove_var("NOTIFICATION_POOL_SIZE");
        std::env::remove_var("AUDIT_POOL_SIZE");
    }
}
