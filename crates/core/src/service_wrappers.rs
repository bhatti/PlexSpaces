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

//! Service wrappers for ActorContext
//!
//! ## Purpose
//! Provides adapter implementations that wrap TupleSpace to implement
//! the TupleSpaceProvider trait defined in actor_context.rs.
//!
//! ## Design Decision
//! Most service wrappers are now in the `node` crate to avoid circular dependencies:
//! - `core` defines the traits (no dependencies on node/actor-service)
//! - `node` implements the wrappers (depends on core, which is fine)
//! - Only TupleSpaceProviderWrapper remains here since TupleSpace is already in core

use async_trait::async_trait;
use std::sync::Arc;

use crate::actor_context::TupleSpaceProvider;
use crate::RequestContext;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};

/// Wrapper that adapts TupleSpace to TupleSpaceProvider trait
///
/// ## Purpose
/// Allows TupleSpace to be used as Arc<dyn TupleSpaceProvider> in ActorContext.
///
/// ## Note
/// This is a working implementation since TupleSpace is already available in core.
pub struct TupleSpaceProviderWrapper {
    inner: Arc<plexspaces_tuplespace::TupleSpace>,
}

impl TupleSpaceProviderWrapper {
    /// Create a new wrapper from TupleSpace
    pub fn new(inner: Arc<plexspaces_tuplespace::TupleSpace>) -> Self {
        Self { inner }
    }
    
    /// Create a new TupleSpace from RequestContext
    ///
    /// ## Purpose
    /// Helper to create TupleSpace with tenant/namespace from RequestContext.
    /// This avoids circular dependency (tuplespace can't depend on core).
    pub fn from_context(ctx: &RequestContext) -> Arc<plexspaces_tuplespace::TupleSpace> {
        Arc::new(plexspaces_tuplespace::TupleSpace::with_tenant_namespace(
            ctx.tenant_id(),
            ctx.namespace(),
        ))
    }
}

impl crate::service_locator::Service for TupleSpaceProviderWrapper {}

#[async_trait]
impl TupleSpaceProvider for TupleSpaceProviderWrapper {
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError> {
        self.inner.write(tuple).await
    }

    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        self.inner.read_all(pattern.clone()).await
    }

    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        self.inner.take(pattern.clone()).await
    }

    async fn count(&self, pattern: &Pattern) -> Result<usize, TupleSpaceError> {
        self.inner.count(pattern.clone()).await
    }
}

// Note: ActorServiceWrapper, ObjectRegistryWrapper have been moved to crates/node/src/service_wrappers.rs to avoid circular dependencies.
// NodeOperationsWrapper has been removed - ActorFactory uses ActorRegistry and VirtualActorManager directly.
// Only TupleSpaceProviderWrapper remains here since TupleSpace is already in core.

