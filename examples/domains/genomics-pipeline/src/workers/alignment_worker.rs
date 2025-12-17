// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Genome Alignment Worker
//!
//! Aligns sequencing reads to reference genome (hg38).
//! Generates BAM file with alignment coordinates.
//!
//! ## Simulated Analysis
//! In production, this would run BWA, Bowtie2, or similar tools.

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct AlignmentWorker {
    worker_id: String,
    samples_processed: u64,
}

impl AlignmentWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            samples_processed: 0,
        }
    }

    fn align_to_reference(&self, sample_id: &str) -> AlignmentResult {
        // Simulate alignment
        AlignmentResult {
            sample_id: sample_id.to_string(),
            bam_path: format!("/data/{}.bam", sample_id),
            alignment_rate: 95.3,
            coverage_depth: 30.5,
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for AlignmentWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        let request: AlignmentRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!("AlignmentWorker {} processing: {}", self.worker_id, request.sample_id);

        let result = self.align_to_reference(&request.sample_id);
        self.samples_processed += 1;

        // Send reply if there's a sender
        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&result)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

            let response = Message::new(response_payload);

            if let Some(sender_id) = &msg.sender {
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    response,
                ).await.map_err(|e| {
                    BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                })?;
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alignment_worker_creation() {
        let worker = AlignmentWorker::new("alignment-1".to_string());
        assert_eq!(worker.worker_id, "alignment-1");
    }

    #[test]
    fn test_alignment() {
        let worker = AlignmentWorker::new("alignment-1".to_string());
        let result = worker.align_to_reference("SAMPLE001");
        assert_eq!(result.sample_id, "SAMPLE001");
        assert!(result.alignment_rate > 90.0);
    }
}
