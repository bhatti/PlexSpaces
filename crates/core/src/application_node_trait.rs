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

//! ApplicationNode trait - minimal Node interface for Applications

use async_trait::async_trait;

/// Minimal Node interface needed by Applications
///
/// ## Purpose
/// Defines the minimal infrastructure services that Applications need from the Node.
/// This trait allows applications to be tested with mock implementations.
///
/// ## Design Notes
/// - Applications should NOT access Node internals directly
/// - This trait provides a stable interface for application development
/// - Mock implementations can be used for testing applications in isolation
#[async_trait]
pub trait ApplicationNode: Send + Sync {
    /// Get node ID
    fn id(&self) -> &str;

    /// Get node listen address (gRPC server address)
    fn listen_addr(&self) -> &str;
}


