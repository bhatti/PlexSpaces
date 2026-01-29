// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Variant Calling Actor for the Genomic Workflow Pipeline

use crate::types::{AlignmentResult, Variant};
use crate::processor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

/// Message types for the VariantCallingActor
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum VariantCallingActorMessage {
    /// Process alignments for variant calling
    ProcessAlignments(Vec<AlignmentResult>),
}

/// Variant Calling Actor Behavior
pub struct VariantCallingActorBehavior {
    actor_id: String,
}

impl VariantCallingActorBehavior {
    /// Create a new variant calling actor behavior
    pub fn new(actor_id: String) -> Self {
        Self { actor_id }
    }
}

#[async_trait::async_trait]
impl ActorTrait for VariantCallingActorBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let variant_message: VariantCallingActorMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        match variant_message {
            VariantCallingActorMessage::ProcessAlignments(alignments) => {
                // Perform variant calling processing
                let variants = processor::process_variant_calling(&alignments);

                info!(
                    "VariantCallingActor {} processed {} alignments, produced {} variants",
                    self.actor_id, alignments.len(), variants.len()
                );

                // Send reply if there's a sender
                if msg.sender_id().is_some() {
                    let response_payload = serde_json::to_vec(&variants)
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
