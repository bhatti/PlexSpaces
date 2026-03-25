// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Integration Tests for Multi-Node ActorService
//!
//! These tests spawn actual ActorService processes and test real gRPC communication.
//!
//! To run:
//!   make test
//! or (build node_runner first):
//!   cargo build --bin node_runner -p plexspaces-services --all-features
//!   cargo test -p plexspaces-services --test integration_tests -- auth_tests -- --test-threads=1
//!
//! Note: Use --test-threads=1 to avoid port conflicts when tests spawn processes.

#[path = "integration/harness.rs"]
mod harness;

#[path = "integration/helpers.rs"]
mod helpers;

#[path = "integration/multi_node_tests.rs"]
mod multi_node_tests;

#[path = "integration/ask_reply_grpc_tests.rs"]
mod ask_reply_grpc_tests;

#[path = "integration/auth_tests.rs"]
mod auth_tests;

pub use harness::{NodeProcess, TestHarness};
pub use helpers::*;
