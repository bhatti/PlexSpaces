// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Actors for the Genomic Workflow Pipeline
//!
//! These actors implement ActorBehavior and can be used in workflow orchestration
//! or directly via NodeBuilder/ActorBuilder patterns.

pub mod qc_actor;
pub mod alignment_actor;
pub mod variant_calling_actor;

// Re-export for convenience
pub use qc_actor::QcActorBehavior;
pub use alignment_actor::AlignmentActorBehavior;
pub use variant_calling_actor::VariantCallingActorBehavior;
