// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Worker actors for genomics pipeline
//!
//! Each worker handles a specific step in the workflow:
//! - QCWorker: Quality control analysis
//! - AlignmentWorker: Genome alignment to reference
//! - ChromosomeWorker: Variant calling for a single chromosome
//! - AnnotationWorker: Variant annotation with clinical databases
//! - ReportWorker: Clinical report generation

pub mod qc_worker;
pub mod alignment_worker;
pub mod chromosome_worker;
pub mod annotation_worker;
pub mod report_worker;

pub use qc_worker::QCWorker;
pub use alignment_worker::AlignmentWorker;
pub use chromosome_worker::ChromosomeWorker;
pub use annotation_worker::AnnotationWorker;
pub use report_worker::ReportWorker;
