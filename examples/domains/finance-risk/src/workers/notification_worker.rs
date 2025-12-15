// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Notification Worker
//!
//! Sends email notifications to applicants about loan decisions.
//! Handles approval, rejection, and manual review notifications.

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct NotificationWorker {
    worker_id: String,
    notifications_sent: u64,
}

impl NotificationWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            notifications_sent: 0,
        }
    }

    fn send_notification(&self, request: &NotificationRequest) -> NotificationResult {
        // Simulate email service (SendGrid, AWS SES, etc.)
        // In production: call email API

        info!(
            "Sending {} notification to {} for application {}",
            request.notification_type, request.applicant_email, request.app_id
        );

        let sent_at = chrono::Utc::now().to_rfc3339();

        NotificationResult {
            app_id: request.app_id.clone(),
            sent_at,
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for NotificationWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: NotificationRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "NotificationWorker {} sending notification for: {}",
            self.worker_id, request.app_id
        );

        let result = self.send_notification(&request);
        self.notifications_sent += 1;

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
    fn test_notification_worker_creation() {
        let worker = NotificationWorker::new("notification-1".to_string());
        assert_eq!(worker.worker_id, "notification-1");
    }

    #[test]
    fn test_send_notification() {
        let worker = NotificationWorker::new("notification-1".to_string());
        let request = NotificationRequest {
            app_id: "APP001".to_string(),
            applicant_email: "john@example.com".to_string(),
            notification_type: "APPROVAL".to_string(),
            message: "Your loan has been approved!".to_string(),
        };

        let result = worker.send_notification(&request);
        assert_eq!(result.app_id, "APP001");
    }
}
