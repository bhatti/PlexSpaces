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

//! Service wrappers for facet types
//!
//! ## Purpose
//! Provides Service trait implementations for facet types to break circular dependencies.
//! Facet types are defined in plexspaces-facet but need to implement Service from core.

use std::sync::Arc;
use crate::Service;

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
}

impl Service for FacetManagerServiceWrapper {
    fn service_name(&self) -> String {
        crate::service_locator::service_names::FACET_MANAGER.to_string()
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
        crate::service_locator::service_names::FACET_REGISTRY.to_string()
    }
}



