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

//! Linda-style tuplespace coordination
//!
//! Provides distributed shared memory abstraction with pattern matching.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main tuplespace module
pub mod lattice_space;
pub mod r#mod;
pub mod storage;

// Configuration module
pub mod config;

// Provider abstraction and capability system
pub mod provider;

// Redis backend (optional)
#[cfg(feature = "redis-backend")]
pub mod redis_space;

// Re-export main types
pub use lattice_space::LatticeTupleSpace;
pub use r#mod::*;

#[cfg(feature = "redis-backend")]
pub use redis_space::RedisTupleSpace;
