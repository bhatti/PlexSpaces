// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! OTP-style behaviors for PlexSpaces actors
//!
//! This module provides Erlang/OTP-inspired behaviors for actors:
//! - GenServer: Request/reply patterns
//! - GenEvent: Event handling
//! - GenStateMachine: Finite state machines
//! - Workflow: Restate-inspired durable workflows

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main behavior module
mod r#mod;
pub use r#mod::*;

// Workflow execution context
pub mod workflow;

// Simplified behavior (experimental)
pub mod simplified;

// Comprehensive tests for GenServer behavior
#[cfg(test)]
mod genserver_tests;

// Comprehensive tests for GenEvent behavior
#[cfg(test)]
mod genevent_tests;

// Comprehensive tests for GenStateMachine (FSM) behavior
#[cfg(test)]
mod genfsm_tests;

// Integration tests for reply routing
// Note: reply_routing_tests.rs was removed - functionality moved to behavior implementations
