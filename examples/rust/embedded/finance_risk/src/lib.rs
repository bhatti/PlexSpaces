// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Financial Risk Assessment Workflow Example
//!
//! Demonstrates PlexSpaces durable actors with a realistic loan application workflow.
//!
//! ## Workflow Steps
//!
//! 1. **Data Collection** (parallel):
//!    - Credit Check (Equifax/Experian/TransUnion API)
//!    - Bank Analysis (Plaid API for statements)
//!    - Employment Verification (The Work Number API)
//!
//! 2. **Risk Scoring**:
//!    - ML model inference (TensorFlow/PyTorch)
//!    - Fraud detection
//!    - Default probability calculation
//!
//! 3. **Decision Logic**:
//!    - Auto-approve (score > 750)
//!    - Manual review (600 < score ≤ 750)
//!    - Auto-reject (score ≤ 600)
//!
//! 4. **Post-Decision Actions**:
//!    - Document generation (loan agreements)
//!    - DocuSign integration
//!    - Email notifications
//!
//! 5. **Audit Trail**:
//!    - Compliance logging
//!
//! ## Architecture
//!
//! ```text
//! Node 1: LoanCoordinator + SQLite Journal
//!     ↓
//!     ├─> Node 2: Data Collection Pools (Credit: 5, Bank: 5, Employment: 3)
//!     ├─> Node 3: Risk Scoring + Decision Engine
//!     └─> Node 4: Post-Decision Services (Documents, DocuSign, Notifications, Audit)
//! ```
//!
//! ## Fault Tolerance
//!
//! - **Durability**: All steps journaled to SQLite
//! - **Idempotency**: External API calls cached (no duplicates on replay)
//! - **Supervision**: OneForOne strategy per worker pool
//! - **Human-in-the-loop**: Manual review with durable timeouts (24hr)
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use finance_risk::{LoanCoordinator, LoanApplication};
//! use plexspaces_actor::Actor;
//! use plexspaces_mailbox::Mailbox;
//! use plexspaces_persistence::MemoryJournal;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create coordinator
//! let coordinator = LoanCoordinator::new("loan-coordinator".to_string());
//!
//! // Submit application
//! let app = LoanApplication {
//!     application_id: "APP001".to_string(),
//!     applicant_name: "John Doe".to_string(),
//!     email: "john@example.com".to_string(),
//!     ssn: "123-45-6789".to_string(),
//!     loan_amount: 50000.0,
//!     loan_purpose: "Home Improvement".to_string(),
//!     employer_id: "EMP12345".to_string(),
//!     bank_accounts: vec!["ACC1".to_string()],
//!     submitted_at: "2025-01-01T10:00:00Z".to_string(),
//! };
//! # Ok(())
//! # }
//! ```

pub mod actors;
pub mod application;
pub mod config;
pub mod coordinator;
pub mod metrics;
pub mod models;
pub mod storage_config;
pub mod supervision;
pub mod workers;

pub use application::FinanceRiskApplication;
pub use config::{BackendType, FinanceRiskConfig, NodeConfig, ResourceCapacity, WorkerPoolConfig};
pub use coordinator::{CoordinatorMessage, LoanCoordinator, WorkflowState, WorkflowStep};
pub use metrics::{ApiTimer, ComputeTimer, CoordinateTimer, LoanMetrics, StepMetric};
pub use models::*;
pub use storage_config::StorageConfig;
pub use supervision::{create_supervision_tree, FinanceSupervisionConfig};
pub use workers::*;
