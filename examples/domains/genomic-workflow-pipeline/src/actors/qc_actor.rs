// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Quality Control Actor for the Genomic Workflow Pipeline

use crate::types::{SequenceRead, QCResult};
use crate::processor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

/// Message types for the QcActor
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum QcActorMessage {
    /// Process sequence reads for quality control
    ProcessReads(Vec<SequenceRead>),
}

/// Quality Control Actor Behavior
pub struct QcActorBehavior {
    actor_id: String,
}

impl QcActorBehavior {
    /// Create a new QC actor behavior
    pub fn new(actor_id: String) -> Self {
        Self { actor_id }
    }
}

#[async_trait::async_trait]
impl ActorTrait for QcActorBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let qc_message: QcActorMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        match qc_message {
            QcActorMessage::ProcessReads(reads) => {
                // Perform QC processing
                let qc_results = processor::process_qc(&reads);
                let passed = qc_results.iter().filter(|r| r.passed).count();

                info!(
                    "QcActor {} processed {} reads, {} passed QC",
                    self.actor_id, reads.len(), passed
                );

                // Send reply if there's a sender
                if msg.sender_id().is_some() {
                    let response_payload = serde_json::to_vec(&qc_results)
                        .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                    let response = Message::new(response_payload);
                    _if let Some(sender_id) = &msg.sender { ctx.send_reply(msg.correlation_id.as_deref(), sender_id, msg.receiver.clone(), response).await } else { Ok(()) }.map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
            }
        }

        Ok(())
    }
}
