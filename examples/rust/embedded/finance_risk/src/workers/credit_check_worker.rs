// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Credit Check Worker
//!
//! Performs credit bureau checks (Equifax, Experian, TransUnion).
//! Retrieves credit score, history, open accounts, and delinquencies.
//!
//! ## Simulation
//! In production, this would call actual credit bureau APIs.
//! For demo purposes, we simulate the checks with realistic data.

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct CreditCheckWorker {
    worker_id: String,
    checks_processed: u64,
}

impl CreditCheckWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            checks_processed: 0,
        }
    }

    fn check_credit(&self, _ssn: &str, app_id: &str) -> CreditCheckResult {
        // Simulate credit bureau API call
        // In production: call Equifax/Experian/TransUnion API

        // Simulated credit score based on app_id hash
        let score = (app_id.len() * 37 + 600) % 300 + 550;

        CreditCheckResult {
            app_id: app_id.to_string(),
            credit_score: score as u32,
            credit_history_years: 10,
            open_accounts: 5,
            delinquencies: if score > 700 { 0 } else { 1 },
            bureau: "Equifax".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for CreditCheckWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: CreditCheckRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "CreditCheckWorker {} processing: {}",
            self.worker_id, request.app_id
        );

        let result = self.check_credit(&request.ssn, &request.app_id);
        self.checks_processed += 1;

        // Send reply if there's a sender
        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&result)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

            let response = Message::new(response_payload);

            // Use ctx.reply() to send reply (preserves correlation_id automatically)
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
    fn test_credit_check_worker_creation() {
        let worker = CreditCheckWorker::new("credit-1".to_string());
        assert_eq!(worker.worker_id, "credit-1");
        assert_eq!(worker.checks_processed, 0);
    }

    #[test]
    fn test_credit_check() {
        let worker = CreditCheckWorker::new("credit-1".to_string());
        let result = worker.check_credit("123-45-6789", "APP001");
        assert_eq!(result.app_id, "APP001");
        assert!(result.credit_score >= 550 && result.credit_score <= 850);
        assert_eq!(result.bureau, "Equifax");
    }
}
