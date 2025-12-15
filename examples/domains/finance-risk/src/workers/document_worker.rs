// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Document Generation Worker
//!
//! Generates loan agreement documents for approved applications.
//! Creates PDFs with loan terms, disclosures, and signatures pages.
//!
//! ## Document Types
//! - Loan Agreement (principal terms)
//! - Truth in Lending Disclosure (TILA)
//! - Privacy Notice
//! - Promissory Note
//!
//! ## Production Implementation
//! In production, this would integrate with:
//! - Document generation service (DocuSign, HelloSign)
//! - PDF generation library (wkhtmltopdf, PDFKit)
//! - Template engine (Handlebars, Jinja2)
//! - Cloud storage (S3, Google Cloud Storage)

use crate::models::*;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use tracing::info;

pub struct DocumentWorker {
    actor_id: String,
    documents_generated: u64,
}

impl DocumentWorker {
    pub fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            documents_generated: 0,
        }
    }

    fn generate_documents(&self, request: &DocumentRequest) -> DocumentResult {
        // Simulated document generation
        // In production: Generate PDFs using template engine + PDF library

        let base_url = format!("https://documents.example.com/{}", request.app_id);

        let document_urls = vec![
            format!("{}/loan_agreement.pdf", base_url),
            format!("{}/truth_in_lending.pdf", base_url),
            format!("{}/privacy_notice.pdf", base_url),
            format!("{}/promissory_note.pdf", base_url),
        ];

        info!(
            "Generated {} documents for application: {}",
            document_urls.len(),
            request.app_id
        );

        DocumentResult {
            app_id: request.app_id.clone(),
            document_urls,
        }
    }

    #[allow(dead_code)]
    fn generate_loan_agreement(&self, terms: &LoanTerms) -> String {
        // Simulated: Generate loan agreement PDF
        format!(
            "LOAN AGREEMENT\n\
             Principal: ${:.2}\n\
             Interest Rate: {:.2}%\n\
             Term: {} months\n\
             Monthly Payment: ${:.2}",
            terms.principal, terms.interest_rate, terms.term_months, terms.monthly_payment
        )
    }

    #[allow(dead_code)]
    fn generate_tila_disclosure(&self, terms: &LoanTerms) -> String {
        // Simulated: Generate Truth in Lending Act disclosure
        let total_payments = terms.monthly_payment * terms.term_months as f64;
        let total_interest = total_payments - terms.principal;

        format!(
            "TRUTH IN LENDING DISCLOSURE\n\
             Amount Financed: ${:.2}\n\
             Finance Charge: ${:.2}\n\
             Total of Payments: ${:.2}\n\
             Annual Percentage Rate (APR): {:.2}%",
            terms.principal, total_interest, total_payments, terms.interest_rate
        )
    }
}

#[async_trait::async_trait]
impl ActorBehavior for DocumentWorker {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: DocumentRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        info!(
            "DocumentWorker {} generating documents for: {}",
            self.actor_id, request.app_id
        );

        let result = self.generate_documents(&request);
        self.documents_generated += 1;

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
    fn test_document_worker_creation() {
        let worker = DocumentWorker::new("document-1".to_string());
        assert_eq!(worker.actor_id, "document-1");
        assert_eq!(worker.documents_generated, 0);
    }

    #[test]
    fn test_generate_documents() {
        let worker = DocumentWorker::new("document-1".to_string());
        let request = DocumentRequest {
            app_id: "APP001".to_string(),
            terms: LoanTerms {
                principal: 50000.0,
                interest_rate: 6.0,
                term_months: 60,
                monthly_payment: 966.64,
            },
        };

        let result = worker.generate_documents(&request);
        assert_eq!(result.app_id, "APP001");
        assert_eq!(result.document_urls.len(), 4);
        assert!(result.document_urls[0].contains("loan_agreement.pdf"));
        assert!(result.document_urls[1].contains("truth_in_lending.pdf"));
    }

    #[test]
    fn test_generate_loan_agreement() {
        let worker = DocumentWorker::new("document-1".to_string());
        let terms = LoanTerms {
            principal: 50000.0,
            interest_rate: 6.0,
            term_months: 60,
            monthly_payment: 966.64,
        };

        let agreement = worker.generate_loan_agreement(&terms);
        assert!(agreement.contains("LOAN AGREEMENT"));
        assert!(agreement.contains("$50000.00"));
        assert!(agreement.contains("6.00%"));
    }

    #[test]
    fn test_generate_tila_disclosure() {
        let worker = DocumentWorker::new("document-1".to_string());
        let terms = LoanTerms {
            principal: 50000.0,
            interest_rate: 6.0,
            term_months: 60,
            monthly_payment: 966.64,
        };

        let disclosure = worker.generate_tila_disclosure(&terms);
        assert!(disclosure.contains("TRUTH IN LENDING"));
        assert!(disclosure.contains("Amount Financed"));
        assert!(disclosure.contains("Finance Charge"));
    }
}
