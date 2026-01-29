// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Chromosome Variant Calling Worker
//!
//! Calls variants (SNPs and indels) for a single chromosome.
//! Uses GATK or FreeBayes algorithms.
//!
//! One worker per chromosome (24 total: chr1-22, X, Y)
//!
//! ## Durability Support
//! This worker uses ExecutionContext for deterministic side effect caching.
//! When variant calling (external GATK/FreeBayes calls) is recorded as a side effect,
//! results are cached in the journal for exactly-once semantics on replay.

use crate::models::*;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_journaling::{ExecutionContextImpl, ExecutionMode, JournalError};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

pub struct ChromosomeWorker {
    worker_id: String,
    chromosome: String,
    samples_processed: u64,
    /// Execution context for deterministic replay (side effect caching)
    execution_context: Option<Arc<RwLock<ExecutionContextImpl>>>,
}

impl ChromosomeWorker {
    pub fn new(worker_id: String, chromosome: String) -> Self {
        Self {
            worker_id,
            chromosome,
            samples_processed: 0,
            execution_context: None,
        }
    }

    /// Set execution context for deterministic replay
    pub fn set_execution_context(&mut self, ctx: Arc<RwLock<ExecutionContextImpl>>) {
        self.execution_context = Some(ctx);
    }

    /// Call variants with side effect caching for exactly-once semantics
    async fn call_variants_cached(&self, sample_id: &str, bam_path: &str) -> Result<ChromosomeVariants, JournalError> {
        if let Some(ctx) = &self.execution_context {
            // Record side effect with unique ID (for deterministic replay)
            let side_effect_id = format!("gatk_call_{}_{}", sample_id, self.chromosome);

            let sample_id_owned = sample_id.to_string();
            let chromosome_owned = self.chromosome.clone();
            let bam_path_owned = bam_path.to_string();

            // Use record_side_effect_raw which works with raw bytes (not protobuf)
            let result_bytes = ctx.read().await.record_side_effect_raw(
                side_effect_id,
                "variant_calling",
                || async move {
                    // Simulate GATK/FreeBayes call (in production, this would be real external call)
                    let variants = Self::simulate_gatk_call(&sample_id_owned, &bam_path_owned, &chromosome_owned);

                    // Serialize result to JSON bytes
                    serde_json::to_vec(&variants)
                        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                },
            ).await?;

            // Deserialize from cached bytes
            let variants = serde_json::from_slice(&result_bytes)
                .map_err(|e| JournalError::Serialization(e.to_string()))?;

            Ok(variants)
        } else {
            // No execution context - call directly (testing mode)
            Ok(Self::simulate_gatk_call(sample_id, bam_path, &self.chromosome))
        }
    }

    /// Simulate GATK variant calling (in production, this calls real GATK/FreeBayes)
    fn simulate_gatk_call(sample_id: &str, _bam_path: &str, chromosome: &str) -> ChromosomeVariants {
        // Simulate variant calling
        // Different chromosomes have different variant counts
        let base_snps = match chromosome {
            "chr1" => 500,
            "chr2" => 480,
            "chrX" => 200,
            "chrY" => 50,
            _ => 300,
        };

        ChromosomeVariants {
            sample_id: sample_id.to_string(),
            chromosome: chromosome.to_string(),
            snp_count: base_snps,
            indel_count: base_snps / 10,
            vcf_path: format!("/data/{}_{}.vcf", sample_id, chromosome),
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for ChromosomeWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        let request: VariantCallingRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "ChromosomeWorker {} calling variants for sample: {} ({})",
            self.worker_id, request.sample_id, self.chromosome
        );

        // Call variants with side effect caching (deterministic replay)
        let result = self.call_variants_cached(&request.sample_id, &request.alignment_bam)
            .await
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
        self.samples_processed += 1;

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
    fn test_chromosome_worker_creation() {
        let worker = ChromosomeWorker::new("chr1-worker".to_string(), "chr1".to_string());
        assert_eq!(worker.chromosome, "chr1");
    }

    #[test]
    fn test_variant_calling_simulation() {
        let result = ChromosomeWorker::simulate_gatk_call("SAMPLE001", "/data/sample.bam", "chr1");
        assert_eq!(result.chromosome, "chr1");
        assert_eq!(result.snp_count, 500); // chr1 has 500 SNPs
        assert_eq!(result.indel_count, 50); // chr1 has 50 indels (500/10)
    }

    #[test]
    fn test_different_chromosomes_simulation() {
        let result1 = ChromosomeWorker::simulate_gatk_call("SAMPLE001", "/data/sample.bam", "chr1");
        let resultx = ChromosomeWorker::simulate_gatk_call("SAMPLE001", "/data/sample.bam", "chrX");

        // chr1 should have more variants than chrX
        assert!(result1.snp_count > resultx.snp_count);
        assert_eq!(result1.snp_count, 500);
        assert_eq!(resultx.snp_count, 200);
    }

    #[tokio::test]
    async fn test_cached_variant_calling_without_context() {
        // When no execution context is set, should call directly
        let worker = ChromosomeWorker::new("chr1-worker".to_string(), "chr1".to_string());
        let result = worker.call_variants_cached("SAMPLE001", "/data/sample.bam").await.unwrap();
        assert_eq!(result.chromosome, "chr1");
        assert_eq!(result.snp_count, 500);
    }
}
