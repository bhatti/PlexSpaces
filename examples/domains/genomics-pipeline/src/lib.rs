// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Genomics DNA Sequencing Pipeline
//!
//! ## Purpose
//! Production-grade example demonstrating PlexSpaces durable workflow capabilities
//! for DNA sequencing analysis pipelines. Processes raw FASTQ files through quality
//! control, genome alignment, variant calling, annotation, and clinical report generation.
//!
//! ## Architecture Context
//! This example showcases PlexSpaces' core capabilities:
//! - **Pillar 2 (Erlang/OTP)**: GenServer actors with supervision
//! - **Pillar 3 (Durability)**: SQLite-backed journaling for crash recovery
//! - **Actor Pools**: Multiple workers for parallel processing
//! - **Fan-Out/Fan-In**: 24 chromosome processors working in parallel
//! - **Multi-Node**: 4-node distributed deployment
//!
//! ## Workflow Steps
//! ```text
//! FASTQ Input
//!     ↓
//! 1. Quality Control (QC Worker Pool - 3 workers)
//!     ↓
//! 2. Genome Alignment (Alignment Worker Pool - 4 workers)
//!     ↓
//! 3. Variant Calling (Fan-Out to 24 Chromosome Workers)
//!     ↓
//! 4. Annotation (Annotation Worker - 1 worker)
//!     ↓
//! 5. Report Generation (Report Worker - 1 worker)
//!     ↓
//! Clinical Report PDF
//! ```
//!
//! ## Key Components
//! - [`GenomicsCoordinator`]: Orchestrates the 5-step workflow with SQLite durability
//! - [`QCWorker`]: Analyzes FASTQ quality scores, GC content, contamination
//! - [`AlignmentWorker`]: Aligns reads to reference genome (hg38)
//! - [`ChromosomeWorker`]: Calls variants for a single chromosome (24 workers)
//! - [`AnnotationWorker`]: Annotates variants with ClinVar, dbSNP, gnomAD
//! - [`ReportWorker`]: Generates clinical-grade PDF reports
//!
//! ## Durability Features
//! - **Journaling**: Every step recorded to SQLite
//! - **Exactly-Once**: No duplicate work on replay
//! - **Checkpoint Recovery**: Resume from last successful step
//! - **Side Effect Caching**: External API calls cached
//!
//! ## Examples
//!
//! ### Single-Node Mode (Development)
//! ```bash
//! cargo run -- --sample-id SAMPLE001 --fastq test_data/sample001.fastq
//! ```
//!
//! ### Multi-Node Mode (Production)
//! ```bash
//! # Terminal 1: Start Node 4 (dependencies first)
//! cargo run --bin genomics-node -- --config config/node4.toml
//!
//! # Terminal 2: Start Node 3 (chromosome workers)
//! cargo run --bin genomics-node -- --config config/node3.toml
//!
//! # Terminal 3: Start Node 2 (QC + alignment)
//! cargo run --bin genomics-node -- --config config/node2.toml
//!
//! # Terminal 4: Start Node 1 (coordinator)
//! cargo run --bin genomics-node -- --config config/node1.toml
//!
//! # Terminal 5: Submit sample
//! cargo run --bin genomics-cli -- submit --sample-id SAMPLE001 --fastq /data/sample.fastq
//! ```
//!
//! ### Docker Compose
//! ```bash
//! docker-compose up -d
//! docker-compose exec coordinator genomics-cli submit --sample-id SAMPLE001
//! ```
//!
//! ## Testing
//! ```bash
//! # Run all tests
//! cargo test
//!
//! # Run specific test
//! cargo test test_qc_worker
//!
//! # Run integration tests
//! cargo test --test integration
//! ```
//!
//! ## Performance Metrics
//! - **Throughput**: 10-15 samples/hour (single coordinator)
//! - **Coordinator Recovery**: < 10 seconds
//! - **Worker Recovery**: < 5 seconds
//! - **Exactly-Once Guarantee**: 0% duplicate work

pub mod models;
pub mod coordinator;
pub mod workers;
pub mod supervision;
pub mod application;
pub mod config;

// Re-export CoordinationComputeMetrics as WorkflowMetrics for backward compatibility
pub mod metrics {
    pub use plexspaces_proto::metrics::v1::CoordinationComputeMetrics as WorkflowMetrics;
}

pub use models::*;
pub use coordinator::GenomicsCoordinator;
pub use workers::*;
pub use supervision::{GenomicsSupervisionConfig, create_supervision_tree};
pub use application::GenomicsPipelineApplication;
pub use config::GenomicsPipelineConfig;
