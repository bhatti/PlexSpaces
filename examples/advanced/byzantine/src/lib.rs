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

//! Byzantine Generals Problem - Example demonstrating PlexSpaces capabilities
//!
//! This example shows how to use PlexSpaces to solve the Byzantine Generals Problem,
//! demonstrating all 5 foundational pillars:
//!
//! 1. **TupleSpace**: Coordination for vote collection
//! 2. **Supervision**: Fault tolerance via automatic restart
//! 3. **Journal**: Durable decisions and audit trail
//! 4. **WASM Runtime**: (Future) Portable general logic
//! 5. **Firecracker**: (Future) Isolated execution

// Re-export the implementation modules
mod byzantine;
pub use byzantine::*;

// Application framework integration (Phase 2.5)
pub mod application;

// Configuration
pub mod config;
pub use config::ByzantineConfig;
