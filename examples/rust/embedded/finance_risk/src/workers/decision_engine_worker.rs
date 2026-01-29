// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Decision Engine Worker
//!
//! Makes final loan approval/rejection decisions based on risk scoring.
//! Implements business rules for auto-approval, manual review, and auto-rejection.
//!
//! ## Decision Logic
//! - Risk Score > 750: Auto-approve with standard terms
//! - Risk Score 600-750: Route to manual review
//! - Risk Score < 600: Auto-reject
//!
//! ## Compliance
//! All decisions are logged for regulatory compliance (FCRA, ECOA, etc.)

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct DecisionEngineWorker {
    actor_id: String,
    decisions_made: u64,
}

impl DecisionEngineWorker {
    pub fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            decisions_made: 0,
        }
    }

    fn make_decision(&self, risk_result: &RiskScoringResult, loan_amount: f64) -> LoanDecision {
        let (approved, decision_type, terms, rejection_reasons) = if risk_result.risk_score > 750 {
            // Auto-approve with terms based on risk score
            let interest_rate = self.calculate_interest_rate(risk_result.risk_score);
            let term_months = 60; // 5 years
            let monthly_payment =
                self.calculate_monthly_payment(loan_amount, interest_rate, term_months);

            (
                true,
                "AUTO_APPROVE".to_string(),
                Some(LoanTerms {
                    principal: loan_amount,
                    interest_rate,
                    term_months,
                    monthly_payment,
                }),
                vec![],
            )
        } else if risk_result.risk_score >= 600 {
            // Manual review required
            (
                false,
                "MANUAL_REVIEW".to_string(),
                None,
                vec!["Risk score requires manual underwriting review".to_string()],
            )
        } else {
            // Auto-reject
            let mut reasons = vec![];
            if risk_result.risk_score < 600 {
                reasons.push("Credit risk score below minimum threshold".to_string());
            }
            if risk_result.fraud_probability > 0.1 {
                reasons.push("High fraud probability detected".to_string());
            }
            if risk_result.default_probability > 0.2 {
                reasons.push("High default probability".to_string());
            }

            (false, "AUTO_REJECT".to_string(), None, reasons)
        };

        LoanDecision {
            app_id: risk_result.app_id.clone(),
            approved,
            decision_type,
            terms,
            rejection_reasons,
            decided_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    fn calculate_interest_rate(&self, risk_score: u32) -> f64 {
        // Higher risk score = lower interest rate
        // Range: 4.5% (excellent) to 8.5% (good)
        let base_rate = 8.5;
        let discount = ((risk_score - 750) as f64 / 250.0) * 4.0;
        (base_rate - discount).max(4.5)
    }

    fn calculate_monthly_payment(&self, principal: f64, annual_rate: f64, term_months: u32) -> f64 {
        let monthly_rate = annual_rate / 100.0 / 12.0;
        let n = term_months as f64;

        if monthly_rate == 0.0 {
            return principal / n;
        }

        let numerator = principal * monthly_rate * (1.0 + monthly_rate).powf(n);
        let denominator = (1.0 + monthly_rate).powf(n) - 1.0;
        numerator / denominator
    }
}

#[async_trait::async_trait]
impl ActorTrait for DecisionEngineWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Expect (RiskScoringResult, loan_amount) tuple
        let (risk_result, loan_amount): (RiskScoringResult, f64) =
            serde_json::from_slice(&msg.payload)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "DecisionEngineWorker {} making decision for: {} (risk_score: {})",
            self.actor_id, risk_result.app_id, risk_result.risk_score
        );

        let decision = self.make_decision(&risk_result, loan_amount);
        self.decisions_made += 1;

        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&decision)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

            let response = Message::new(response_payload);

            if let Some(sender_id) = &msg.sender { ctx.send_reply(msg.correlation_id.as_deref(), sender_id, msg.receiver.clone(), response).await } else { Ok(()) }.map_err(|e| {
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
    fn test_decision_engine_worker_creation() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());
        assert_eq!(worker.actor_id, "decision-1");
        assert_eq!(worker.decisions_made, 0);
    }

    #[test]
    fn test_auto_approve_decision() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());
        let risk_result = RiskScoringResult {
            app_id: "APP001".to_string(),
            risk_score: 800,
            fraud_probability: 0.01,
            default_probability: 0.03,
            recommended_action: "AUTO_APPROVE".to_string(),
        };

        let decision = worker.make_decision(&risk_result, 50000.0);
        assert_eq!(decision.app_id, "APP001");
        assert!(decision.approved);
        assert_eq!(decision.decision_type, "AUTO_APPROVE");
        assert!(decision.terms.is_some());
        assert!(decision.rejection_reasons.is_empty());
    }

    #[test]
    fn test_manual_review_decision() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());
        let risk_result = RiskScoringResult {
            app_id: "APP002".to_string(),
            risk_score: 650,
            fraud_probability: 0.05,
            default_probability: 0.10,
            recommended_action: "MANUAL_REVIEW".to_string(),
        };

        let decision = worker.make_decision(&risk_result, 50000.0);
        assert_eq!(decision.app_id, "APP002");
        assert!(!decision.approved);
        assert_eq!(decision.decision_type, "MANUAL_REVIEW");
        assert!(decision.terms.is_none());
        assert!(!decision.rejection_reasons.is_empty());
    }

    #[test]
    fn test_auto_reject_decision() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());
        let risk_result = RiskScoringResult {
            app_id: "APP003".to_string(),
            risk_score: 500,
            fraud_probability: 0.15,
            default_probability: 0.30,
            recommended_action: "AUTO_REJECT".to_string(),
        };

        let decision = worker.make_decision(&risk_result, 50000.0);
        assert_eq!(decision.app_id, "APP003");
        assert!(!decision.approved);
        assert_eq!(decision.decision_type, "AUTO_REJECT");
        assert!(decision.terms.is_none());
        assert!(!decision.rejection_reasons.is_empty());
    }

    #[test]
    fn test_interest_rate_calculation() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());

        // Excellent credit (950) -> rate should be around 5.3%
        let rate_excellent = worker.calculate_interest_rate(950);
        assert!(rate_excellent >= 5.0 && rate_excellent <= 5.5);

        // Good credit (750) -> rate should be 8.5%
        let rate_good = worker.calculate_interest_rate(750);
        assert_eq!(rate_good, 8.5);

        // Perfect credit (1000) -> rate should be 4.5%
        let rate_perfect = worker.calculate_interest_rate(1000);
        assert_eq!(rate_perfect, 4.5);
    }

    #[test]
    fn test_monthly_payment_calculation() {
        let worker = DecisionEngineWorker::new("decision-1".to_string());
        let payment = worker.calculate_monthly_payment(50000.0, 6.0, 60);

        // $50k loan at 6% APR for 60 months should be around $966/month
        assert!(payment > 960.0 && payment < 970.0);
    }
}
