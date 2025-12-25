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

// No dependency on core - FacetManager uses String for IDs to avoid circular dependency

use crate::FacetContainer;

/// Facet Manager - manages actor facets
///
/// ## Purpose
/// Centralizes facet storage and access, separating facet concerns from ActorRegistry.
/// This allows ActorFactory to manage facets without depending on Node.
pub struct FacetManager {
    /// Facet storage: actor_id -> FacetContainer
    /// Stores facets for all actors (normal and virtual) for facet access
    facet_storage: Arc<RwLock<HashMap<String, Arc<RwLock<FacetContainer>>>>>,
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
    pub async fn store_facets(&self, actor_id: impl AsRef<str>, facets: Arc<RwLock<FacetContainer>>) {
        let mut storage = self.facet_storage.write().await;
        storage.insert(actor_id.as_ref().to_string(), facets);
    }

    /// Get facets for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Some(facets) if found, None otherwise
    pub async fn get_facets(&self, actor_id: impl AsRef<str>) -> Option<Arc<RwLock<FacetContainer>>> {
        let storage = self.facet_storage.read().await;
        storage.get(actor_id.as_ref()).cloned()
    }

    /// Remove facets for an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    pub async fn remove_facets(&self, actor_id: impl AsRef<str>) {
        let mut storage = self.facet_storage.write().await;
        storage.remove(actor_id.as_ref());
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
    /// Setup facets for an actor after spawn
    ///
    /// ## Purpose
    /// Configures facets that need actor_ref, node_id, or actor_service after actor is spawned.
    /// Currently handles TimerFacet and ReminderFacet setup.
    ///
    /// ## Note
    /// This method is a placeholder - actual setup should be done in Node or ActorFactory
    /// which have access to journaling crate and core types.
    /// This method exists for API completeness but doesn't require core dependency.
    pub async fn setup_facets_for_actor(
        &self,
        _actor_id: impl AsRef<str>,
    ) {
        // Note: TimerFacet setup requires journaling crate, which core doesn't depend on.
        // This method is a placeholder - actual setup should be done in Node or ActorFactory
        // which have access to journaling crate.
        // TODO: Move this logic to Node or create a trait-based approach
    }

    /// Call on_down() for all facets on an actor (Phase 4: Monitoring/Linking Integration)
    ///
    /// ## Purpose
    /// Calls facet.on_down() for all facets on the monitoring actor when a monitored actor terminates.
    /// This allows facets to handle DOWN notifications (e.g., update metrics, trigger retry logic).
    ///
    /// ## Arguments
    /// * `monitoring_actor_id` - ID of the actor that is monitoring
    /// * `monitored_actor_id` - ID of the actor that terminated
    /// * `reason` - Exit reason for termination (as string representation)
    ///
    /// ## Returns
    /// Ok(Vec<FacetError>) with any errors encountered (continues calling even if one fails)
    /// Err(String) if facets not found for this actor
    pub async fn call_on_down(
        &self,
        monitoring_actor_id: impl AsRef<str>,
        monitored_actor_id: impl AsRef<str>,
        reason: &crate::ExitReason,
    ) -> Result<Vec<crate::FacetError>, String> {
        // Get facets for the monitoring actor
        let facets = self.get_facets(monitoring_actor_id.as_ref()).await
            .ok_or_else(|| format!("Facets not found for actor: {}", monitoring_actor_id.as_ref()))?;
        
        // Call facet.on_down() for all facets (reason is already facet::ExitReason)
        let mut facets_guard = facets.write().await;
        let errors = facets_guard.call_on_down(
            monitoring_actor_id.as_ref(),
            monitored_actor_id.as_ref(),
            reason,
        ).await;
        
        Ok(errors)
    }

    /// Get facet storage (for internal access)
    ///
    /// ## Note
    /// This is exposed for backward compatibility during migration.
    /// Prefer using the public methods above.
    pub fn facet_storage(&self) -> &Arc<RwLock<HashMap<String, Arc<RwLock<FacetContainer>>>>> {
        &self.facet_storage
    }
}

impl Default for FacetManager {
    fn default() -> Self {
        Self::new()
    }
}

// Note: Service trait implementation moved to core crate to break circular dependency
// FacetManager doesn't implement Service here - core provides a wrapper if needed

