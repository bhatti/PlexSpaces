// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration Tests for Multi-Node ActorService
//!
//! These tests spawn actual ActorService processes and test real gRPC communication.
//!
//! To run:
//!   cargo test --test integration_tests -- --ignored --test-threads=1
//!
//! Note: Tests are marked #[ignore] because they spawn processes and are slower.
//! Use --test-threads=1 to avoid port conflicts.

#[path = "integration/harness.rs"]
mod harness;

#[path = "integration/helpers.rs"]
mod helpers;

#[path = "integration/multi_node_tests.rs"]
mod multi_node_tests;

pub use harness::{NodeProcess, TestHarness};
pub use helpers::*;
