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

// Virtual actor lifecycle facet trait (moved from core to break journaling→core dep)
pub mod virtual_actor_lifecycle_facet;
pub use virtual_actor_lifecycle_facet::{VirtualActorLifecycleFacet, VirtualActorLifecycleState};

// Facet helpers for proto extraction
pub mod facet_helpers;
pub use facet_helpers::{
    create_facet_from_json, create_facets_from_config, extract_all_facet_configs,
    extract_facet_config, extract_facet_config_for_registration, has_facet_attached,
    has_facet_type,
};
