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

//! Facet Manager - Centralized facet management
//!
//! ## Purpose
//! Provides centralized management of actor facets, including storage, access,
//! and lifecycle operations. This module separates facet concerns from ActorRegistry
//! and makes facet management available to ActorFactory without Node dependency.
//!
//! ## Design
//! - Stores facets for all actors (normal and virtual)
//! - Provides facet access via get_facet()
//! - Handles facet setup after actor spawn (TimerFacet, ReminderFacet)
//! - Independent of Node - can be used by ActorFactory

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{ActorId, ActorRef, Service};
use plexspaces_facet::FacetContainer;

/// Facet Manager - manages actor facets
///
/// ## Purpose
/// Centralizes facet storage and access, separating facet concerns from ActorRegistry.
/// This allows ActorFactory to manage facets without depending on Node.
pub struct FacetManager {
    /// Facet storage: actor_id -> FacetContainer
    /// Stores facets for all actors (normal and virtual) for facet access
    facet_storage: Arc<RwLock<HashMap<ActorId, Arc<RwLock<FacetContainer>>>>>,
}

impl FacetManager {
    /// Create a new FacetManager
    pub fn new() -> Self {
        Self {
            facet_storage: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Store facets for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `facets` - Facet container to store
    pub async fn store_facets(&self, actor_id: ActorId, facets: Arc<RwLock<FacetContainer>>) {
        let mut storage = self.facet_storage.write().await;
        storage.insert(actor_id, facets);
    }

    /// Get facets for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Some(facets) if found, None otherwise
    pub async fn get_facets(&self, actor_id: &ActorId) -> Option<Arc<RwLock<FacetContainer>>> {
        let storage = self.facet_storage.read().await;
        storage.get(actor_id).cloned()
    }

    /// Remove facets for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    pub async fn remove_facets(&self, actor_id: &ActorId) {
        let mut storage = self.facet_storage.write().await;
        storage.remove(actor_id);
    }

    /// Setup facets for an actor after spawn
    ///
    /// ## Purpose
    /// Configures facets that need actor_ref, node_id, or actor_service after actor is spawned.
    /// Currently handles TimerFacet and ReminderFacet setup.
    ///
    /// ## Note
    /// TimerFacet is in the journaling crate, which core doesn't depend on.
    /// This method uses dynamic downcasting via Any trait to avoid the dependency.
    /// The actual setup is done by the caller (Node) which has access to journaling crate.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `actor_ref` - ActorRef for the actor
    /// * `node_id` - Node ID
    /// * `actor_service` - ActorService for sending messages
    pub async fn setup_facets_for_actor(
        &self,
        _actor_id: &ActorId,
        _actor_ref: &ActorRef,
        _node_id: &str,
        _actor_service: Arc<dyn crate::actor_context::ActorService>,
    ) {
        // Note: TimerFacet setup requires journaling crate, which core doesn't depend on.
        // This method is a placeholder - actual setup should be done in Node or ActorFactory
        // which have access to journaling crate.
        // TODO: Move this logic to Node or create a trait-based approach
    }

    /// Get facet storage (for internal access)
    ///
    /// ## Note
    /// This is exposed for backward compatibility during migration.
    /// Prefer using the public methods above.
    pub fn facet_storage(&self) -> &Arc<RwLock<HashMap<ActorId, Arc<RwLock<FacetContainer>>>>> {
        &self.facet_storage
    }
}

impl Default for FacetManager {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Service trait so FacetManager can be registered in ServiceLocator
impl Service for FacetManager {}
