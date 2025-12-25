// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Financial Risk Assessment Example
//!
//! Demonstrates a loan application workflow with:
//! - Multi-step orchestration (data collection, risk scoring, decision)
//! - Worker pools for parallel processing
//! - ConfigBootstrap for configuration
//! - CoordinationComputeTracker for metrics
//! - NodeBuilder/ActorBuilder for simplified setup

use anyhow;
use finance_risk::{
    coordinator::{CoordinatorMessage, LoanCoordinator},
    models::LoanApplication,
    workers::{
        AuditWorker, BankAnalysisWorker, CreditCheckWorker, DecisionEngineWorker, DocumentWorker,
        EmploymentWorker, NotificationWorker, RiskScoringWorker,
    },
    FinanceRiskConfig,
};
use plexspaces_mailbox::{mailbox_config_default, Message};
use plexspaces_node::CoordinationComputeTracker;
use plexspaces_node::{ConfigBootstrap, NodeBuilder};
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Financial Risk Assessment Example                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration using ConfigBootstrap
    let config: FinanceRiskConfig = ConfigBootstrap::load().unwrap_or_default();
    config
        .validate()
        .map_err(|e| anyhow::anyhow!("Config validation failed: {}", e))?;

    info!("📋 Configuration:");
    info!("   Worker Pools:");
    info!("     Credit Check: {}", config.worker_pools.credit_check);
    info!("     Bank Analysis: {}", config.worker_pools.bank_analysis);
    info!("     Employment: {}", config.worker_pools.employment);
    info!("     Risk Scoring: {}", config.worker_pools.risk_scoring);
    info!(
        "     Decision Engine: {}",
        config.worker_pools.decision_engine
    );
    info!("     Document: {}", config.worker_pools.document);
    info!("     Notification: {}", config.worker_pools.notification);
    info!("     Audit: {}", config.worker_pools.audit);
    info!("   Backend: {:?}", config.backend);
    println!();

    // Create node using NodeBuilder
    let node = Arc::new(
        NodeBuilder::new("finance-risk-node")
            .with_listen_address("0.0.0.0:9000")
            .build().await,
    );
    info!("✅ Node created: finance-risk-node");
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("finance-risk".to_string());

    // Spawn coordinator
    info!("🎭 Spawning coordinator...");
    metrics_tracker.start_coordinate();
    let coordinator_behavior = Box::new(LoanCoordinator::new("loan-coordinator".to_string()));
    let mut mailbox_config = plexspaces_mailbox::mailbox_config_default();
    mailbox_config.capacity = 10000;
    use plexspaces_actor::ActorBuilder;
    let ctx = plexspaces_core::RequestContext::internal();
    let coordinator_ref = ActorBuilder::new(coordinator_behavior)
        .with_id(format!("loan-coordinator@{}", node.id().as_str()))
        .with_mailbox_config(mailbox_config)
        .spawn(&ctx, node.service_locator().clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn coordinator: {}", e))?;
    metrics_tracker.end_coordinate();
    info!("✅ Coordinator spawned: {}", coordinator_ref.id());
    println!();

    // Spawn worker pools
    info!("👷 Spawning worker pools...");
    metrics_tracker.start_coordinate();

    // Credit check workers
    let mut worker_mailbox_config = plexspaces_mailbox::mailbox_config_default();
    worker_mailbox_config.capacity = 10000;
    let ctx = plexspaces_core::RequestContext::internal();
    for i in 0..config.worker_pools.credit_check {
        let worker = CreditCheckWorker::new(format!("credit-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("credit-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn credit-worker-{}: {}", i, e))?;
    }

    // Bank analysis workers
    for i in 0..config.worker_pools.bank_analysis {
        let worker = BankAnalysisWorker::new(format!("bank-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("bank-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn bank-worker-{}: {}", i, e))?;
    }

    // Employment workers
    for i in 0..config.worker_pools.employment {
        let worker = EmploymentWorker::new(format!("employment-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("employment-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn employment-worker-{}: {}", i, e))?;
    }

    // Risk scoring workers
    for i in 0..config.worker_pools.risk_scoring {
        let worker = RiskScoringWorker::new(format!("risk-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("risk-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn risk-worker-{}: {}", i, e))?;
    }

    // Decision engine worker
    let decision_worker = DecisionEngineWorker::new("decision-worker-0".to_string());
    let behavior = Box::new(decision_worker);
    let _decision_ref = ActorBuilder::new(behavior)
        .with_id(format!("decision-worker-0@{}", node.id().as_str()))
        .with_mailbox_config(worker_mailbox_config.clone())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn decision-worker: {}", e))?;

    // Document workers
    for i in 0..config.worker_pools.document {
        let worker = DocumentWorker::new(format!("document-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("document-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn document-worker-{}: {}", i, e))?;
    }

    // Notification workers
    for i in 0..config.worker_pools.notification {
        let worker = NotificationWorker::new(format!("notification-worker-{}", i));
        let behavior = Box::new(worker);
        let _actor_ref = ActorBuilder::new(behavior)
            .with_id(format!("notification-worker-{}@{}", i, node.id().as_str()))
            .with_mailbox_config(worker_mailbox_config.clone())
            .spawn(&ctx, node.service_locator().clone())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn notification-worker-{}: {}", i, e))?;
    }

    // Audit worker
    let audit_worker = AuditWorker::new("audit-worker-0".to_string());
    let behavior = Box::new(audit_worker);
    let _audit_ref = ActorBuilder::new(behavior)
        .with_id(format!("audit-worker-0@{}", node.id().as_str()))
        .with_mailbox_config(worker_mailbox_config.clone())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .map_err(|e| anyhow::anyhow!("Failed to spawn audit-worker: {}", e))?;

    metrics_tracker.end_coordinate();
    info!("✅ All workers spawned");
    info!("   Credit: {}, Bank: {}, Employment: {}, Risk: {}, Decision: 1, Document: {}, Notification: {}, Audit: 1",
        config.worker_pools.credit_check, config.worker_pools.bank_analysis,
        config.worker_pools.employment, config.worker_pools.risk_scoring,
        config.worker_pools.document, config.worker_pools.notification);
    println!();

    // Wait for actors to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Process a sample loan application
    info!("📝 Processing sample loan application...");
    let sample_app = LoanApplication {
        application_id: "APP001".to_string(),
        applicant_name: "John Doe".to_string(),
        email: "john@example.com".to_string(),
        ssn: "123-45-6789".to_string(),
        loan_amount: 50000.0,
        loan_purpose: "Home Improvement".to_string(),
        employer_id: "EMP12345".to_string(),
        bank_accounts: vec!["ACC1".to_string(), "ACC2".to_string()],
        submitted_at: chrono::Utc::now().to_rfc3339(),
    };

    let coordinator_msg = CoordinatorMessage::ProcessApplication(sample_app.clone());
    let payload = serde_json::to_vec(&coordinator_msg)?;
    let message = Message::new(payload);

    // Send message using ActorService
    use plexspaces_core::ActorService;
    let actor_service: Arc<dyn ActorService> = node.service_locator()
        .get_actor_service()
        .await
        .ok_or_else(|| anyhow::anyhow!("ActorService not found"))?;
    
    metrics_tracker.start_coordinate();
    actor_service.send(coordinator_ref.id(), message).await
        .map_err(|e| anyhow::anyhow!("Failed to send message: {}", e))?;
    metrics_tracker.end_coordinate();

    info!("✅ Application submitted: {}", sample_app.application_id);
    println!();

    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

    // Display metrics
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let metrics = metrics_tracker.finalize();
    println!("📊 Performance Metrics:");
    println!("  Coordination time: {:.2} ms", metrics.coordinate_duration_ms);
    println!("  Compute time: {:.2} ms", metrics.compute_duration_ms);
    println!("  Total time: {:.2} ms", metrics.total_duration_ms);
    println!("  Granularity ratio: {:.2}x", metrics.granularity_ratio);
    println!("  Efficiency: {:.2}%", metrics.efficiency * 100.0);
    println!("  Messages: {}", metrics.message_count);
    println!("  Barriers: {}", metrics.barrier_count);
    if !metrics.step_metrics.is_empty() {
        println!("  Step Metrics:");
        for step in &metrics.step_metrics {
            println!("    - {}: compute={:.2}ms, coordinate={:.2}ms, messages={}",
                step.step_name, step.compute_ms, step.coordinate_ms, step.message_count);
        }
    }
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║              Example Complete                                  ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Note: This is a simplified example demonstrating the workflow.");
    println!("In production, the coordinator would orchestrate the full workflow");
    println!("across all worker pools with proper error handling and retries.");

    Ok(())
}
