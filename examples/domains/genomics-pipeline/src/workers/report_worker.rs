// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Clinical Report Generation Worker
//!
//! Generates comprehensive clinical reports including:
//! - QC metrics
//! - Alignment statistics
//! - Variant summary
//! - Clinical interpretation
//! - Recommendations

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct ReportWorker {
    worker_id: String,
    reports_generated: u64,
}

impl ReportWorker {
    pub fn new(worker_id: String) -> Self {
        Self {
            worker_id,
            reports_generated: 0,
        }
    }

    fn generate_report(&self, request: &ReportRequest) -> ClinicalReport {
        let interpretation = if request.annotations.pathogenic_count > 0 {
            format!(
                "CLINICALLY SIGNIFICANT FINDINGS: {} pathogenic variant(s) detected.",
                request.annotations.pathogenic_count
            )
        } else {
            String::from("NO CLINICALLY SIGNIFICANT FINDINGS")
        };

        let recommendations = vec![
            "1. No immediate action required".to_string(),
            "2. Maintain regular health screenings".to_string(),
        ];

        let generated_at = chrono::Utc::now().to_rfc3339();

        ClinicalReport {
            sample_id: request.sample_id.clone(),
            qc_metrics: request.qc_metrics.clone(),
            alignment_metrics: request.alignment_metrics.clone(),
            variant_summary: request.variant_summary.clone(),
            annotations: request.annotations.clone(),
            interpretation,
            recommendations,
            generated_at,
        }
    }
}

#[async_trait::async_trait]
impl ActorBehavior for ReportWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        let request: ReportRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!("ReportWorker {} generating report for sample: {}", self.worker_id, request.sample_id);

        let report = self.generate_report(&request);
        self.reports_generated += 1;

        // Send reply if there's a sender
        if msg.sender_id().is_some() {
            let response_payload = serde_json::to_vec(&report)
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
    use std::collections::HashMap;

    #[test]
    fn test_report_worker_creation() {
        let worker = ReportWorker::new("report-1".to_string());
        assert_eq!(worker.worker_id, "report-1");
        assert_eq!(worker.reports_generated, 0);
    }
}
