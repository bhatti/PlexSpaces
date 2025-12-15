// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # WASM Calculator - Distributed Calculator with WASM Actors
//!
//! ## Purpose
//! Demonstrates PlexSpaces WASM runtime with a practical distributed calculator
//! example. Shows how to compile actors to WASM, deploy dynamically, and execute
//! with durable execution guarantees.
//!
//! ## Architecture
//! - **Calculator Actor** (WASM): Performs operations (add, multiply, etc.)
//! - **Coordinator Actor** (Native): Coordinates multi-step calculations
//! - **Result Aggregator** (WASM): Aggregates results from distributed calculations
//!
//! ## Features Demonstrated
//! - ✅ WASM actor compilation (Rust → WASM)
//! - ✅ Dynamic WASM module deployment
//! - ✅ Multi-node distributed execution
//! - ✅ Durable execution (calculations survive failures)
//! - ✅ TupleSpace coordination
//! - ✅ Actor supervision and recovery

pub mod actors;
pub mod application;
pub mod models;

pub use actors::*;
pub use application::*;
pub use models::*;
