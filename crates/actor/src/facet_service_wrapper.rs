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

//! Service wrappers for facet types
//!
//! ## Purpose
//! Provides Service trait implementations for facet types to break circular dependencies.
//! Facet types are defined in plexspaces-facet but need to implement Service from core.

use crate::Service;
use plexspaces_common::ServiceNameExt;
use std::sync::Arc;

/// Wrapper for FacetRegistry to implement Service trait
///
/// ## Purpose
/// Allows FacetRegistry to be registered in ServiceLocator without creating
/// a circular dependency between core and facet crates.
pub struct FacetRegistryServiceWrapper {
    inner: Arc<plexspaces_facet::FacetRegistry>,
}

/// Wrapper for FacetManager to implement Service trait
///
/// ## Purpose
/// Allows FacetManager to be registered in ServiceLocator without creating
/// a circular dependency between core and facet crates.
pub struct FacetManagerServiceWrapper {
    inner: Arc<plexspaces_facet::FacetManager>,
}

impl FacetManagerServiceWrapper {
    /// Create a new wrapper
    pub fn new(manager: Arc<plexspaces_facet::FacetManager>) -> Self {
        Self { inner: manager }
    }

    /// Get the inner FacetManager
    pub fn inner(&self) -> &Arc<plexspaces_facet::FacetManager> {
        &self.inner
    }

    /// Clone the inner FacetManager
    pub fn inner_clone(&self) -> Arc<plexspaces_facet::FacetManager> {
        self.inner.clone()
    }

    /// Returns the facet container for `actor_id` if the facet manager holds one for that actor.
    pub async fn facet_container_for_actor(
        &self,
        actor_id: &str,
    ) -> Option<Arc<tokio::sync::RwLock<plexspaces_facet::FacetContainer>>> {
        self.inner.get_facets(actor_id).await
    }
}

impl Service for FacetManagerServiceWrapper {
    fn service_name(&self) -> String {
        crate::ServiceName::ServiceNameFacetManager
            .as_str()
            .to_string()
    }
}

impl FacetRegistryServiceWrapper {
    /// Create a new wrapper
    pub fn new(registry: Arc<plexspaces_facet::FacetRegistry>) -> Self {
        Self { inner: registry }
    }

    /// Get the inner FacetRegistry
    pub fn inner(&self) -> &Arc<plexspaces_facet::FacetRegistry> {
        &self.inner
    }

    /// Clone the inner FacetRegistry
    pub fn inner_clone(&self) -> Arc<plexspaces_facet::FacetRegistry> {
        self.inner.clone()
    }
}

impl std::ops::Deref for FacetRegistryServiceWrapper {
    type Target = plexspaces_facet::FacetRegistry;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl Service for FacetRegistryServiceWrapper {
    fn service_name(&self) -> String {
        crate::ServiceName::ServiceNameFacetRegistry
            .as_str()
            .to_string()
    }
}
