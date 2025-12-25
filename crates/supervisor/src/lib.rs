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

//! Supervision tree implementation for PlexSpaces
//!
//! This crate provides Erlang/OTP-inspired supervision trees with
//! elevated abstractions for adaptive recovery.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main supervision module
mod r#mod;
pub use r#mod::*;

// Child specification module (Phase 2: Unified ChildSpec)
mod child_spec;
pub use child_spec::{ChildSpec, StartedChild, StartFn, ShutdownSpec};
// Note: ChildType and RestartStrategy are re-exported from mod.rs for backward compatibility
// The new child_spec module has its own ChildType and RestartStrategy that align with proto

// Facet helpers module (Phase 1: Unified Lifecycle)
mod facet_helpers;
pub use facet_helpers::{create_facet_from_proto, create_facets_from_proto};

// Re-export SupervisorStats from proto (for public API)
pub use plexspaces_proto::supervision::v1::SupervisorStats;
