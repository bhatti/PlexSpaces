//! Financial Risk Assessment Data Models
//!
//! This module defines all data structures for the loan application workflow including:
//! - Loan applications
//! - Credit check results
//! - Bank analysis
//! - Employment verification
//! - Risk scoring
//! - Decision outcomes

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Application Data
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanApplication {
    pub application_id: String,
    pub applicant_name: String,
    pub email: String,
    pub ssn: String,
    pub loan_amount: f64,
    pub loan_purpose: String,
    pub employer_id: String,
    pub bank_accounts: Vec<String>,
    pub submitted_at: String,
}

// ============================================================================
// Worker Request/Response Messages
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditCheckRequest {
    pub app_id: String,
    pub ssn: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditCheckResult {
    pub app_id: String,
    pub credit_score: u32,
    pub credit_history_years: u32,
    pub open_accounts: u32,
    pub delinquencies: u32,
    pub bureau: String, // Equifax, Experian, TransUnion
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankAnalysisRequest {
    pub app_id: String,
    pub accounts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankAnalysisResult {
    pub app_id: String,
    pub average_balance: f64,
    pub minimum_balance: f64,
    pub months_analyzed: u32,
    pub nsf_count: u32, // Non-sufficient funds incidents
    pub verified_income: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmploymentVerificationRequest {
    pub app_id: String,
    pub employer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmploymentVerificationResult {
    pub app_id: String,
    pub employer_name: String,
    pub employment_start_date: String,
    pub job_title: String,
    pub annual_income: f64,
    pub verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScoringRequest {
    pub app_id: String,
    pub credit_data: CreditCheckResult,
    pub bank_data: BankAnalysisResult,
    pub employment_data: EmploymentVerificationResult,
    pub loan_amount: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RiskScoringResult {
    pub app_id: String,
    pub risk_score: u32,            // 0-1000
    pub fraud_probability: f64,     // 0.0-1.0
    pub default_probability: f64,   // 0.0-1.0
    pub recommended_action: String, // AUTO_APPROVE, MANUAL_REVIEW, AUTO_REJECT
}

// ============================================================================
// Decision Outcomes
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanDecision {
    pub app_id: String,
    pub approved: bool,
    pub decision_type: String, // AUTO_APPROVE, MANUAL_APPROVE, AUTO_REJECT, MANUAL_REJECT
    pub terms: Option<LoanTerms>,
    pub rejection_reasons: Vec<String>,
    pub decided_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanTerms {
    pub principal: f64,
    pub interest_rate: f64,
    pub term_months: u32,
    pub monthly_payment: f64,
}

// ============================================================================
// Post-Decision Services
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRequest {
    pub app_id: String,
    pub terms: LoanTerms,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentResult {
    pub app_id: String,
    pub document_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocuSignRequest {
    pub app_id: String,
    pub documents: DocumentResult,
    pub applicant_email: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocuSignResult {
    pub app_id: String,
    pub envelope_id: String,
    pub sent_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationRequest {
    pub app_id: String,
    pub applicant_email: String,
    pub notification_type: String, // APPROVAL, REJECTION, REVIEW_REQUIRED
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationResult {
    pub app_id: String,
    pub sent_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogRequest {
    pub app_id: String,
    pub decision: LoanDecision,
    pub risk_score: RiskScoringResult,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogResult {
    pub app_id: String,
    pub logged_at: String,
}
