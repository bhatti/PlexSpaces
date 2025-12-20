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

//! Actor Factory - for spawning and activating actors
//!
//! ## Purpose
//! Provides a trait for spawning actors without depending on Node directly.
//! This allows VirtualActorManager, ActorService, and other components to spawn actors
//! without tight coupling to Node.
//!
//! ## Design
//! ActorFactory implementations should use ServiceLocator to access ActorRegistry
//! and other services needed for spawning actors.

use async_trait::async_trait;
use std::sync::Arc;
use std::collections::HashMap;
use plexspaces_core::{ActorId, Service};

/// Trait for spawning and activating actors
///
/// ## Purpose
/// Allows components like VirtualActorManager and ActorService to spawn actors without
/// depending on Node directly. ActorFactory implementations should use ServiceLocator
/// to access ActorRegistry and other services needed for spawning.
#[async_trait]
pub trait ActorFactory: Send + Sync {
    /// Activate a virtual actor (start it if not already started)
    ///
    /// ## Arguments
    /// * `actor_id` - The actor ID to activate
    ///
    /// ## Returns
    /// Ok(()) if activation successful, error otherwise
    async fn activate_virtual_actor(&self, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    
    /// Spawn a new actor locally
    ///
    /// ## Purpose
    /// Creates and starts a new actor on the local node. The actor will be registered
    /// in ActorRegistry automatically.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `actor_id` - Actor ID (format: "actor_name@node_id")
    /// * `actor_type` - Type of actor to spawn (used by BehaviorFactory if available)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Returns
    /// ActorRef for the spawned actor (as MessageSender trait object for flexibility)
    ///
    /// ## Note
    /// ActorFactory implementations should use ActorRegistry to register the actor
    /// after spawning. The ActorRef should be created from the actor's mailbox.
    /// Returns MessageSender to allow different implementations to return different types.
    async fn spawn_actor(
        &self,
        ctx: &plexspaces_core::RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn plexspaces_core::MessageSender>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Spawn a pre-built actor
    ///
    /// ## Purpose
    /// Spawns an actor that has already been built (e.g., by ActorBuilder).
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `actor` - The pre-built actor to spawn
    /// * `actor_type` - Optional actor type
    ///
    /// ## Returns
    /// MessageSender for the spawned actor
    ///
    /// ## Note
    /// This method is used by ActorBuilder when the actor has already been
    /// constructed. The actor should already have its ID, context, etc. set.
    async fn spawn_built_actor(
        &self,
        ctx: &plexspaces_core::RequestContext,
        actor: Arc<crate::Actor>,
        actor_type: Option<String>,
    ) -> Result<Arc<dyn plexspaces_core::MessageSender>, Box<dyn std::error::Error + Send + Sync>>;
    
    /// Stop an actor
    ///
    /// ## Purpose
    /// Stops and unregisters an actor from the ActorRegistry.
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID to stop
    ///
    /// ## Returns
    /// Ok(()) on success, error otherwise
    ///
    /// ## Note
    /// This method unregisters the actor from ActorRegistry and performs cleanup.
    /// The actor will be garbage collected after unregistration.
    async fn stop_actor(&self, actor_id: &ActorId) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

// Note: ActorFactory implementations should implement Service trait separately
// This allows them to be registered in ServiceLocator
