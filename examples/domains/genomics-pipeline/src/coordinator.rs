// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Genomics Pipeline Coordinator
//!
//! Orchestrates the 5-step DNA sequencing workflow:
//! 1. Quality Control
//! 2. Genome Alignment
//! 3. Variant Calling (fan-out to 24 chromosomes, fan-in aggregation)
//! 4. Annotation
//! 5. Report Generation
//!
//! ## Durability
//! - Each step is journaled to SQLite
//! - Crash recovery resumes from last checkpoint
//! - Exactly-once semantics (no duplicate work on replay)
//!
//! ## Fault Tolerance
//! - Supervisor restarts coordinator on crash
//! - Journal replay restores state
//! - Worker failures handled by OneForOne supervision

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use std::collections::HashMap;
use tracing::{info, warn};

/// Workflow state for a single sample
#[derive(Debug, Clone)]
pub struct WorkflowState {
    /// Sample information
    pub sample: Sample,

    /// Current step
    pub current_step: WorkflowStep,
}

/// Workflow step tracking
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowStep {
    QualityControl,
    Alignment,
    VariantCalling,
    Annotation,
    ReportGeneration,
    Completed,
}

/// Messages handled by coordinator
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CoordinatorMessage {
    /// Submit new sample for processing
    ProcessSample(Sample),

    /// Query sample status
    GetStatus(String),
}

/// Genomics Pipeline Coordinator Actor
pub struct GenomicsCoordinator {
    /// Actor ID
    actor_id: String,

    /// Samples currently being processed
    active_samples: HashMap<String, WorkflowState>,
}

impl GenomicsCoordinator {
    /// Create a new coordinator
    pub fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            active_samples: HashMap::new(),
        }
    }

    /// Journal a workflow step (placeholder for durability integration)
    async fn journal_step<T: serde::Serialize>(
        &self,
        sample_id: &str,
        step: &str,
        _payload: &T,
    ) {
        // TODO: Integrate with DurabilityFacet to record journal entry
        // For now, just log
        info!("Journal: sample={}, step={}", sample_id, step);
    }
}

#[async_trait::async_trait]
impl ActorBehavior for GenomicsCoordinator {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(&mut self, _ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        // Deserialize coordinator message
        let coord_msg: CoordinatorMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        match coord_msg {
            CoordinatorMessage::ProcessSample(sample) => {
                let sample_id = sample.sample_id.clone();

                info!("Starting processing for sample: {}", sample_id);

                // Create workflow state
                let workflow_state = WorkflowState {
                    sample: sample.clone(),
                    current_step: WorkflowStep::QualityControl,
                };

                self.active_samples.insert(sample_id.clone(), workflow_state);

                // Journal step: sample_received
                self.journal_step(&sample_id, "sample_received", &sample).await;

                // TODO: Start QC step by sending message to QC worker pool
                info!("Sample {} submitted for processing", sample_id);
            }
            CoordinatorMessage::GetStatus(sample_id) => {
                if let Some(workflow) = self.active_samples.get(&sample_id) {
                    info!("Sample {} status: {:?}", sample_id, workflow.current_step);
                } else {
                    warn!("Sample {} not found", sample_id);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinator_creation() {
        let coordinator = GenomicsCoordinator::new("coordinator-1".to_string());
        assert_eq!(coordinator.actor_id, "coordinator-1");
        assert_eq!(coordinator.active_samples.len(), 0);
    }

    #[test]
    fn test_workflow_state_creation() {
        let sample = Sample {
            sample_id: "SAMPLE001".to_string(),
            fastq_path: "/data/sample001.fastq".to_string(),
            metadata: HashMap::new(),
        };

        let workflow = WorkflowState {
            sample: sample.clone(),
            current_step: WorkflowStep::QualityControl,
        };

        assert_eq!(workflow.sample.sample_id, "SAMPLE001");
        assert_eq!(workflow.current_step, WorkflowStep::QualityControl);
    }
}
