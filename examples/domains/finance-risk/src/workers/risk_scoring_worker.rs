// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Risk Scoring Worker
//!
//! Performs ML model inference for credit risk assessment.
//! Calculates risk score, fraud probability, and default probability.
//!
//! ## Simulated ML Model
//! In production, this would use TensorFlow, PyTorch, or similar ML framework.
//! For demo, we use a weighted scoring formula based on input data.

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct RiskScoringWorker {
    worker_id: String,
    scores_calculated: u64,
}

impl RiskScoringWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            scores_calculated: 0,
        }
    }

    fn calculate_risk_score(&self, request: &RiskScoringRequest) -> RiskScoringResult {
        // Simulated ML model inference
        // In production: TensorFlow/PyTorch model prediction

        // Weighted scoring based on multiple factors
        let credit_factor = request.credit_data.credit_score as f64 / 850.0;
        let income_factor = request.employment_data.annual_income / request.loan_amount;
        let bank_factor = request.bank_data.average_balance / request.loan_amount;

        // Combined risk score (0-1000)
        let risk_score =
            ((credit_factor * 0.5 + income_factor * 0.3 + bank_factor * 0.2) * 1000.0) as u32;

        // Fraud detection (simulated)
        let fraud_probability = if request.credit_data.delinquencies > 2 {
            0.15
        } else {
            0.02
        };

        // Default probability (simulated)
        let default_probability = if risk_score < 600 { 0.25 } else { 0.05 };

        // Recommended action
        let recommended_action = if risk_score > 750 {
            "AUTO_APPROVE"
        } else if risk_score > 600 {
            "MANUAL_REVIEW"
        } else {
            "AUTO_REJECT"
        }
        .to_string();

        RiskScoringResult {
            app_id: request.app_id.clone(),
            risk_score,
            fraud_probability,
            default_probability,
            recommended_action,
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for RiskScoringWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: RiskScoringRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "RiskScoringWorker {} scoring: {}",
            self.worker_id, request.app_id
        );

        let result = self.calculate_risk_score(&request);
        self.scores_calculated += 1;

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
    fn test_risk_scoring_worker_creation() {
        let worker = RiskScoringWorker::new("risk-1".to_string());
        assert_eq!(worker.worker_id, "risk-1");
    }

    #[test]
    fn test_risk_scoring() {
        let worker = RiskScoringWorker::new("risk-1".to_string());
        let request = RiskScoringRequest {
            app_id: "APP001".to_string(),
            credit_data: CreditCheckResult {
                app_id: "APP001".to_string(),
                credit_score: 750,
                credit_history_years: 10,
                open_accounts: 5,
                delinquencies: 0,
                bureau: "Equifax".to_string(),
            },
            bank_data: BankAnalysisResult {
                app_id: "APP001".to_string(),
                average_balance: 10000.0,
                minimum_balance: 2000.0,
                months_analyzed: 12,
                nsf_count: 0,
                verified_income: 9000.0,
            },
            employment_data: EmploymentVerificationResult {
                app_id: "APP001".to_string(),
                employer_name: "Employer-EMP123".to_string(),
                employment_start_date: "2020-01-15".to_string(),
                job_title: "Software Engineer".to_string(),
                annual_income: 85000.0,
                verified: true,
            },
            loan_amount: 50000.0,
        };

        let result = worker.calculate_risk_score(&request);
        assert_eq!(result.app_id, "APP001");
        assert!(result.risk_score > 0);
        assert!(result.fraud_probability >= 0.0 && result.fraud_probability <= 1.0);
    }
}
