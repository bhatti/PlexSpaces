// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Bank Analysis Worker
//!
//! Analyzes bank account statements via Plaid API.
//! Calculates average balance, minimum balance, NSF incidents, and verified income.

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct BankAnalysisWorker {
    worker_id: String,
    analyses_processed: u64,
}

impl BankAnalysisWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            analyses_processed: 0,
        }
    }

    fn analyze_bank_accounts(&self, accounts: &[String], app_id: &str) -> BankAnalysisResult {
        // Simulate Plaid API call for bank statement analysis
        // In production: call Plaid Transactions API

        // Simulated values based on number of accounts
        let account_factor = accounts.len() as f64;

        BankAnalysisResult {
            app_id: app_id.to_string(),
            average_balance: 5000.0 * account_factor,
            minimum_balance: 1000.0 * account_factor,
            months_analyzed: 12,
            nsf_count: if account_factor > 2.0 { 0 } else { 1 },
            verified_income: 4500.0 * account_factor,
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for BankAnalysisWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: BankAnalysisRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "BankAnalysisWorker {} processing: {}",
            self.worker_id, request.app_id
        );

        let result = self.analyze_bank_accounts(&request.accounts, &request.app_id);
        self.analyses_processed += 1;

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
    fn test_bank_analysis_worker_creation() {
        let worker = BankAnalysisWorker::new("bank-1".to_string());
        assert_eq!(worker.worker_id, "bank-1");
    }

    #[test]
    fn test_bank_analysis() {
        let worker = BankAnalysisWorker::new("bank-1".to_string());
        let accounts = vec!["ACC1".to_string(), "ACC2".to_string()];
        let result = worker.analyze_bank_accounts(&accounts, "APP001");
        assert_eq!(result.app_id, "APP001");
        assert!(result.average_balance > 0.0);
        assert!(result.verified_income > 0.0);
    }
}
