// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Loan Application Coordinator
//!
//! Orchestrates the multi-step loan approval workflow:
//! 1. Data Collection (credit check, bank analysis, employment verification)
//! 2. Risk Scoring (ML model inference)
//! 3. Decision Logic (auto-approve/reject or manual review)
//! 4. Post-Decision Actions (document generation, e-signature, notifications)
//! 5. Audit Trail (compliance logging)
//!
//! ## Durability
//! - Each step is journaled to SQLite
//! - Crash recovery resumes from last checkpoint
//! - Exactly-once semantics (no duplicate API calls on replay)
//!
//! ## Fault Tolerance
//! - Supervisor restarts coordinator on crash
//! - Journal replay restores state
//! - Worker failures handled by OneForOne supervision

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use std::collections::HashMap;
use tracing::{info, warn};

/// Workflow state for a single loan application
#[derive(Debug, Clone)]
pub struct WorkflowState {
    /// Application information
    pub application: LoanApplication,

    /// Current step
    pub current_step: WorkflowStep,

    /// Collected data (cached for replay)
    pub credit_data: Option<CreditCheckResult>,
    pub bank_data: Option<BankAnalysisResult>,
    pub employment_data: Option<EmploymentVerificationResult>,
    pub risk_score: Option<RiskScoringResult>,
    pub decision: Option<LoanDecision>,
}

/// Workflow step tracking
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStep {
    DataCollection,
    RiskScoring,
    DecisionLogic,
    PostDecisionActions,
    AuditTrail,
    Completed,
}

/// Messages handled by coordinator
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CoordinatorMessage {
    /// Submit new application for processing
    ProcessApplication(LoanApplication),

    /// Query application status
    GetStatus(String),
}

/// Loan Application Coordinator Actor
pub struct LoanCoordinator {
    /// Applications currently being processed
    active_applications: HashMap<String, WorkflowState>,
}

impl LoanCoordinator {
    /// Create a new coordinator
    pub fn new(_actor_id: String) -> Self {
        Self {
            active_applications: HashMap::new(),
        }
    }

    /// Journal a workflow step
    ///
    /// Note: In a production implementation with durability, this would:
    /// 1. Use DurabilityFacet to record journal entries
    /// 2. Store entries in SQLite/PostgreSQL for crash recovery
    /// 3. Support exactly-once semantics via idempotency keys
    async fn journal_step<T: serde::Serialize>(&self, app_id: &str, step: &str, _payload: &T) {
        // For this simplified example, we just log
        // In production: ctx.facet_service.get_facet("durability")?.journal_entry(...)
        info!("Journal: app={}, step={}", app_id, step);
    }
}

#[async_trait::async_trait]
impl ActorBehavior for LoanCoordinator {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Deserialize coordinator message
        let coordinator_msg: CoordinatorMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        match coordinator_msg {
            CoordinatorMessage::ProcessApplication(application) => {
                let app_id = application.application_id.clone();

                info!("Starting loan application processing: {}", app_id);

                // Create workflow state
                let workflow_state = WorkflowState {
                    application: application.clone(),
                    current_step: WorkflowStep::DataCollection,
                    credit_data: None,
                    bank_data: None,
                    employment_data: None,
                    risk_score: None,
                    decision: None,
                };

                self.active_applications
                    .insert(app_id.clone(), workflow_state);

                // Journal step: application_received
                self.journal_step(&app_id, "application_received", &application)
                    .await;

                // Note: In a production implementation, the coordinator would:
                // 1. Discover workers via ObjectRegistry (ctx.object_registry.lookup("credit-worker-pool"))
                // 2. Send parallel requests to credit, bank, and employment workers
                // 3. Collect responses and proceed to risk scoring
                // 4. Continue through the workflow steps
                // For this simplified example, we just log the submission
                info!("Application {} submitted for processing", app_id);
                info!("  In production: Coordinator would send messages to worker pools for data collection");
            }
            CoordinatorMessage::GetStatus(app_id) => {
                if let Some(workflow) = self.active_applications.get(&app_id) {
                    info!("Application {} status: {:?}", app_id, workflow.current_step);
                } else {
                    warn!("Application {} not found", app_id);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_creation() {
        let coordinator = LoanCoordinator::new("loan-coordinator-1".to_string());
        assert_eq!(coordinator.active_applications.len(), 0);
    }

    #[test]
    fn test_workflow_state_creation() {
        let application = LoanApplication {
            application_id: "APP001".to_string(),
            applicant_name: "John Doe".to_string(),
            email: "john@example.com".to_string(),
            ssn: "123-45-6789".to_string(),
            loan_amount: 50000.0,
            loan_purpose: "Home Improvement".to_string(),
            employer_id: "EMP12345".to_string(),
            bank_accounts: vec!["ACC1".to_string(), "ACC2".to_string()],
            submitted_at: "2025-01-01T10:00:00Z".to_string(),
        };

        let workflow = WorkflowState {
            application: application.clone(),
            current_step: WorkflowStep::DataCollection,
            credit_data: None,
            bank_data: None,
            employment_data: None,
            risk_score: None,
            decision: None,
        };

        assert_eq!(workflow.application.application_id, "APP001");
        assert_eq!(workflow.current_step, WorkflowStep::DataCollection);
    }
}
