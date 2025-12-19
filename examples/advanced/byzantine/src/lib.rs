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

//! Byzantine Generals Problem - Simple implementation based on Erlang version
//!
//! This is a simplified implementation that follows the Erlang algorithm closely:
//! - Processes communicate via messages
//! - Source process sends initial messages in round 0
//! - Other processes relay messages in subsequent rounds
//! - Processes vote and make decisions
//! - Some processes can be faulty (traitors)

// Re-export the implementation modules
pub mod byzantine;
pub use byzantine::*;

// Application framework integration (Phase 2.5)
pub mod application;

// Configuration
pub mod config;
pub use config::ByzantineConfig;

// BehaviorFactory for dynamic actor spawning
pub mod behavior_factory;
pub use behavior_factory::register_byzantine_behaviors;
