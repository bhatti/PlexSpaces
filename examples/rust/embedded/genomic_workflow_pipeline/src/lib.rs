// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # Genomic Workflow Pipeline
//!
//! ## Purpose
//! Demonstrates PlexSpaces workflow orchestration with:
//! - Multi-step genomic pipeline
//! - Metrics tracking (compute vs coordinate overhead)
//! - Workflow definition and execution
//!
//! ## Modules
//! - `types`: Data structures and metrics
//! - `processor`: Simulated genomic operations
//! - `config`: Configuration using ConfigBootstrap
//! - `actors`: Actor implementations for workflow steps
//! - `application`: Application trait implementation (for node deployment)

pub mod types;
pub mod processor;
pub mod config;
pub mod actors;
pub mod application;
pub mod recovery;

// Re-export workflow types for convenience
pub use plexspaces_workflow::*;
pub use config::{GenomicPipelineConfig, NodeConfig};
