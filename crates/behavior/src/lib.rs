// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Shim crate: behavior is now part of plexspaces-actor.
//!
//! This crate re-exports everything from `plexspaces_actor::behavior` for
//! backward compatibility during the transition period.

// Re-export everything from plexspaces_actor::behavior
pub use plexspaces_actor::behavior::*;
// Also re-export the sub-modules
pub use plexspaces_actor::behavior::workflow;
pub use plexspaces_actor::behavior::simplified;
