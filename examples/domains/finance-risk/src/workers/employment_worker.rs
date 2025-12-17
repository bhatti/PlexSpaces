// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Employment Verification Worker
//!
//! Verifies employment via The Work Number API.
//! Confirms job title, start date, and annual income.

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct EmploymentWorker {
    worker_id: String,
    verifications_processed: u64,
}

impl EmploymentWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            verifications_processed: 0,
        }
    }

    fn verify_employment(&self, employer_id: &str, app_id: &str) -> EmploymentVerificationResult {
        // Simulate The Work Number API call
        // In production: call The Work Number Employment Verification API

        EmploymentVerificationResult {
            app_id: app_id.to_string(),
            employer_name: format!("Employer-{}", employer_id),
            employment_start_date: "2020-01-15".to_string(),
            job_title: "Software Engineer".to_string(),
            annual_income: 85000.0,
            verified: true,
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for EmploymentWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: EmploymentVerificationRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "EmploymentWorker {} verifying: {}",
            self.worker_id, request.app_id
        );

        let result = self.verify_employment(&request.employer_id, &request.app_id);
        self.verifications_processed += 1;

        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&result)
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
    fn test_employment_worker_creation() {
        let worker = EmploymentWorker::new("employment-1".to_string());
        assert_eq!(worker.worker_id, "employment-1");
    }

    #[test]
    fn test_employment_verification() {
        let worker = EmploymentWorker::new("employment-1".to_string());
        let result = worker.verify_employment("EMP12345", "APP001");
        assert_eq!(result.app_id, "APP001");
        assert!(result.verified);
        assert!(result.annual_income > 0.0);
    }
}
