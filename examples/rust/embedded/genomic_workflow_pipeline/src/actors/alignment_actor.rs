// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Alignment Actor for the Genomic Workflow Pipeline

use crate::types::{QCResult, AlignmentResult};
use crate::processor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

/// Message types for the AlignmentActor
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AlignmentActorMessage {
    /// Process QC results for genome alignment
    ProcessQcResults(Vec<QCResult>),
}

/// Alignment Actor Behavior
pub struct AlignmentActorBehavior {
    actor_id: String,
}

impl AlignmentActorBehavior {
    /// Create a new alignment actor behavior
    pub fn new(actor_id: String) -> Self {
        Self { actor_id }
    }
}

#[async_trait::async_trait]
impl ActorTrait for AlignmentActorBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let alignment_message: AlignmentActorMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        match alignment_message {
            AlignmentActorMessage::ProcessQcResults(qc_results) => {
                // Perform alignment processing
                let alignments = processor::process_alignment(&qc_results);

                info!(
                    "AlignmentActor {} processed {} QC results, produced {} alignments",
                    self.actor_id, qc_results.len(), alignments.len()
                );

                // Send reply if there's a sender
                if msg.sender_id().is_some() {
                    let response_payload = serde_json::to_vec(&alignments)
                        .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                    let response = Message::new(response_payload);
                    if let Some(sender_id) = &msg.sender { 
                        ctx.send_reply(msg.correlation_id.as_deref(), sender_id, msg.receiver.clone(), response).await
                            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                    }
                }
            }
        }

        Ok(())
    }
}
