// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Pure Rust CSP-Style Structured Concurrency
//!
//! Demonstrates three approaches to the scatter-gather pattern:
//! 1. Naive (fire-and-forget spawns — shows task leak)
//! 2. JoinSet + timeout (structured lifetime)
//! 3. CSP-style rendezvous channels with select! (Hoare-style composition)

pub mod csp;
pub mod scatter_gather;
