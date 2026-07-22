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

//! Capability Provider Facets
//!
//! This module implements WasmCloud-style capability providers as facets,
//! providing a unified extension model for actors.
//!
//! ## Design Note
//! Capabilities are just facets with I/O focus. There's no separate `CapabilityFacet` trait;
//! all capabilities implement the standard `Facet` trait. If namespace/contract information
//! is needed, it can be stored in facet metadata/config.

pub mod http_client;
pub mod keyvalue;
pub mod locks;
pub mod process_groups;
pub mod registry;
