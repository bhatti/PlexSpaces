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

//! Facet system for dynamic behavior composition
//!
//! Facets allow runtime attachment of cross-cutting concerns to actors.

#![warn(missing_docs)]
#![warn(clippy::all)]

// Main facet module - contains core facet traits and FacetContainer
mod r#mod;
pub use r#mod::*;

// Facet Manager - centralized facet management
// Always available (no feature gate) - uses String for IDs to avoid circular dependency
pub mod facet_manager;
pub use facet_manager::FacetManager;

// Capabilities - facet implementations
pub mod capabilities;

// Event emitter
pub mod event_emitter;
pub use event_emitter::{EventEmitterFacet, EVENT_EMITTER_FACET_DEFAULT_PRIORITY};

// Facet helpers for proto extraction
pub mod facet_helpers;
pub use facet_helpers::{
    extract_facet_config, has_facet_type, extract_all_facet_configs,
    create_facet_from_json, create_facets_from_config, has_facet_attached,
    extract_facet_config_for_registration,
};
