// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Quality Control Worker
//!
//! Analyzes FASTQ files for quality metrics:
//! - Quality scores (Phred scores)
//! - GC content
//! - Adapter contamination
//! - Total read count
//!
//! ## Simulated Analysis
//! In this example, we simulate QC analysis. A real implementation would:
//! - Parse FASTQ files
//! - Calculate per-base quality scores
//! - Detect adapter sequences
//! - Run FastQC or similar tools

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

/// QC Worker Actor
pub struct QCWorker {
    /// Worker ID
    worker_id: String,

    /// Number of samples processed
    samples_processed: u64,
}

impl QCWorker {
    /// Create a new QC worker
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            samples_processed: 0,
        }
    }

    /// Perform quality control analysis (simulated)
    fn analyze_fastq(&self, fastq_path: &str) -> QCResult {
        // Simulate QC analysis with random-ish values
        // In a real implementation, this would:
        // 1. Parse FASTQ file
        // 2. Calculate quality scores
        // 3. Detect contamination
        // 4. Count reads

        let sample_id = fastq_path
            .split('/')
            .last()
            .unwrap_or("unknown")
            .replace(".fastq", "");

        // Simulate good quality for test data
        let quality_score = 35.5;
        let gc_content = 42.3;
        let total_reads = 100_000_000;
        let adapter_contamination = false;
        let passed = quality_score >= 30.0; // Threshold: Q30

        QCResult {
            sample_id,
            quality_score,
            gc_content,
            total_reads,
            adapter_contamination,
            passed,
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for QCWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        // Deserialize QC request from message payload
        let request: QCRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "QCWorker {} processing sample: {}",
            self.worker_id, request.sample_id
        );

        // Perform QC analysis
        let result = self.analyze_fastq(&request.fastq_path);

        self.samples_processed += 1;

        info!(
            "QCWorker {} completed QC for {}: quality_score={}, passed={}",
            self.worker_id, request.sample_id, result.quality_score, result.passed
        );

        // Send reply if there's a sender
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
    fn test_qc_worker_creation() {
        let worker = QCWorker::new("qc-worker-1".to_string());
        assert_eq!(worker.worker_id, "qc-worker-1");
        assert_eq!(worker.samples_processed, 0);
    }

    #[test]
    fn test_qc_analysis() {
        let worker = QCWorker::new("qc-worker-1".to_string());
        let result = worker.analyze_fastq("/data/test_sample.fastq");

        assert_eq!(result.sample_id, "test_sample");
        assert!(result.quality_score >= 30.0);
        assert!(result.passed);
        assert_eq!(result.total_reads, 100_000_000);
    }

    #[test]
    fn test_qc_result_serialization() {
        let result = QCResult {
            sample_id: "TEST001".to_string(),
            quality_score: 35.5,
            gc_content: 42.3,
            total_reads: 100_000_000,
            adapter_contamination: false,
            passed: true,
        };

        // Test serialization
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("TEST001"));
        assert!(json.contains("35.5"));

        // Test deserialization
        let deserialized: QCResult = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.sample_id, "TEST001");
        assert_eq!(deserialized.quality_score, 35.5);
    }
}
