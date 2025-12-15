// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Audit Logging Worker
//!
//! Logs all loan decisions for regulatory compliance and audit trail.
//! Maintains immutable record of all decisions, risk assessments, and rationale.
//!
//! ## Compliance Requirements
//! - Fair Credit Reporting Act (FCRA)
//! - Equal Credit Opportunity Act (ECOA)
//! - Truth in Lending Act (TILA)
//! - Bank Secrecy Act (BSA)
//! - Anti-Money Laundering (AML)
//!
//! ## Audit Trail
//! All records include:
//! - Application data
//! - Risk assessment results
//! - Decision outcome and rationale
//! - Timestamps and actor IDs
//! - Data sources used
//!
//! ## Production Implementation
//! In production, this would integrate with:
//! - Immutable log storage (WORM storage, blockchain)
//! - Compliance databases (PostgreSQL with audit triggers)
//! - SIEM systems (Splunk, ELK Stack)
//! - Regulatory reporting systems

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct AuditWorker {
    actor_id: String,
    records_logged: u64,
}

impl AuditWorker {
    pub fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            records_logged: 0,
        }
    }

    fn log_audit_record(&self, request: &AuditLogRequest) -> AuditLogResult {
        // Simulated: Write to immutable audit log
        // In production: Write to append-only database or WORM storage

        let _audit_record = self.format_audit_record(request);

        info!(
            "AuditWorker {} logging decision for {}: {} (risk_score: {})",
            self.actor_id,
            request.app_id,
            request.decision.decision_type,
            request.risk_score.risk_score
        );

        // Log compliance-relevant details
        self.log_compliance_data(request);

        AuditLogResult {
            app_id: request.app_id.clone(),
            logged_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn format_audit_record(&self, request: &AuditLogRequest) -> String {
        // Structured audit log entry
        format!(
            "AUDIT RECORD\n\
             Application ID: {}\n\
             Decision: {} (Approved: {})\n\
             Risk Score: {}\n\
             Fraud Probability: {:.2}%\n\
             Default Probability: {:.2}%\n\
             Timestamp: {}\n\
             Logged By: {}",
            request.app_id,
            request.decision.decision_type,
            request.decision.approved,
            request.risk_score.risk_score,
            request.risk_score.fraud_probability * 100.0,
            request.risk_score.default_probability * 100.0,
            request.timestamp,
            self.actor_id
        )
    }

    fn log_compliance_data(&self, request: &AuditLogRequest) {
        // FCRA compliance: Log credit decision factors
        if !request.decision.approved {
            for reason in &request.decision.rejection_reasons {
                info!("Adverse Action Reason (FCRA): {}", reason);
            }
        }

        // ECOA compliance: Log decision basis (ensure no discrimination)
        info!(
            "ECOA Compliance Check: Decision based solely on creditworthiness metrics (risk_score: {})",
            request.risk_score.risk_score
        );

        // BSA/AML compliance: Log fraud indicators
        if request.risk_score.fraud_probability > 0.1 {
            info!(
                "BSA/AML Alert: High fraud probability ({:.2}%) for application {}",
                request.risk_score.fraud_probability * 100.0,
                request.app_id
            );
        }
    }

    #[allow(dead_code)]
    fn generate_adverse_action_notice(&self, decision: &LoanDecision) -> String {
        // FCRA requirement: Adverse action notice for rejections
        if !decision.approved {
            format!(
                "NOTICE OF ADVERSE ACTION (FCRA)\n\
                 \n\
                 Application ID: {}\n\
                 Decision: Declined\n\
                 \n\
                 Primary Reasons:\n{}",
                decision.app_id,
                decision
                    .rejection_reasons
                    .iter()
                    .enumerate()
                    .map(|(i, r)| format!("  {}. {}", i + 1, r))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        } else {
            String::new()
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for AuditWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: AuditLogRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "AuditWorker {} logging audit record for: {}",
            self.actor_id, request.app_id
        );

        let result = self.log_audit_record(&request);
        self.records_logged += 1;

        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&result)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

            let response = Message::new(response_payload);

            ctx.reply(response).await.map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
            })?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_worker_creation() {
        let worker = AuditWorker::new("audit-1".to_string());
        assert_eq!(worker.actor_id, "audit-1");
        assert_eq!(worker.records_logged, 0);
    }

    #[test]
    fn test_log_audit_record_approved() {
        let worker = AuditWorker::new("audit-1".to_string());
        let request = AuditLogRequest {
            app_id: "APP001".to_string(),
            decision: LoanDecision {
                app_id: "APP001".to_string(),
                approved: true,
                decision_type: "AUTO_APPROVE".to_string(),
                terms: Some(LoanTerms {
                    principal: 50000.0,
                    interest_rate: 6.0,
                    term_months: 60,
                    monthly_payment: 966.64,
                }),
                rejection_reasons: vec![],
                decided_at: "2025-01-12T10:00:00Z".to_string(),
            },
            risk_score: RiskScoringResult {
                app_id: "APP001".to_string(),
                risk_score: 800,
                fraud_probability: 0.01,
                default_probability: 0.03,
                recommended_action: "AUTO_APPROVE".to_string(),
            },
            timestamp: "2025-01-12T10:00:00Z".to_string(),
        };

        let result = worker.log_audit_record(&request);
        assert_eq!(result.app_id, "APP001");
        assert!(!result.logged_at.is_empty());
    }

    #[test]
    fn test_log_audit_record_rejected() {
        let worker = AuditWorker::new("audit-1".to_string());
        let request = AuditLogRequest {
            app_id: "APP002".to_string(),
            decision: LoanDecision {
                app_id: "APP002".to_string(),
                approved: false,
                decision_type: "AUTO_REJECT".to_string(),
                terms: None,
                rejection_reasons: vec![
                    "Credit score below minimum threshold".to_string(),
                    "High default probability".to_string(),
                ],
                decided_at: "2025-01-12T10:00:00Z".to_string(),
            },
            risk_score: RiskScoringResult {
                app_id: "APP002".to_string(),
                risk_score: 500,
                fraud_probability: 0.15,
                default_probability: 0.30,
                recommended_action: "AUTO_REJECT".to_string(),
            },
            timestamp: "2025-01-12T10:00:00Z".to_string(),
        };

        let result = worker.log_audit_record(&request);
        assert_eq!(result.app_id, "APP002");
    }

    #[test]
    fn test_format_audit_record() {
        let worker = AuditWorker::new("audit-1".to_string());
        let request = AuditLogRequest {
            app_id: "APP001".to_string(),
            decision: LoanDecision {
                app_id: "APP001".to_string(),
                approved: true,
                decision_type: "AUTO_APPROVE".to_string(),
                terms: None,
                rejection_reasons: vec![],
                decided_at: "2025-01-12T10:00:00Z".to_string(),
            },
            risk_score: RiskScoringResult {
                app_id: "APP001".to_string(),
                risk_score: 800,
                fraud_probability: 0.01,
                default_probability: 0.03,
                recommended_action: "AUTO_APPROVE".to_string(),
            },
            timestamp: "2025-01-12T10:00:00Z".to_string(),
        };

        let record = worker.format_audit_record(&request);
        assert!(record.contains("AUDIT RECORD"));
        assert!(record.contains("APP001"));
        assert!(record.contains("AUTO_APPROVE"));
        assert!(record.contains("800"));
    }

    #[test]
    fn test_generate_adverse_action_notice() {
        let worker = AuditWorker::new("audit-1".to_string());
        let decision = LoanDecision {
            app_id: "APP002".to_string(),
            approved: false,
            decision_type: "AUTO_REJECT".to_string(),
            terms: None,
            rejection_reasons: vec![
                "Credit score below minimum threshold".to_string(),
                "High default probability".to_string(),
            ],
            decided_at: "2025-01-12T10:00:00Z".to_string(),
        };

        let notice = worker.generate_adverse_action_notice(&decision);
        assert!(notice.contains("NOTICE OF ADVERSE ACTION"));
        assert!(notice.contains("APP002"));
        assert!(notice.contains("Credit score below minimum threshold"));
    }
}
