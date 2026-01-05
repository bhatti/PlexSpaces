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

//! Virtual Actor Manager - Orleans-inspired automatic activation/deactivation
//!
//! ## Purpose
//! Manages virtual actor lifecycle: registration, activation, passivation, and reactivation.
//! Inspired by Microsoft Orleans Virtual Actors model.
//!
//! ## Virtual Actor States (Orleans-Inspired)
//!
//! ### 1. Registered (Always Addressable)
//! - **Definition**: Virtual actor is registered in ActorRegistry with a MessageSender
//! - **Lazy Virtual Actors**: Registered with `VirtualActorWrapper` (not running)
//! - **Eager Virtual Actors**: Registered with `ActorRef` (running immediately)
//! - **Regular Actors**: Registered with `ActorRef` (always running)
//! - **Check**: `is_virtual(actor_id)` returns true
//! - **Note**: All virtual actors are always registered, even when not active
//!
//! ### 2. Active (Running)
//! - **Definition**: Actor has a running message loop processing messages from mailbox
//! - **Lazy Virtual Actors**: Become active on first message (via `VirtualActorWrapper.tell()`)
//! - **Eager Virtual Actors**: Active immediately after registration
//! - **Regular Actors**: Always active after registration
//! - **Check**: `is_active(actor_id)` returns true (checks if ActorRef is in registry, not VirtualActorWrapper)
//! - **Note**: Active actors can process messages. Inactive actors trigger activation on first message.
//!
//! ### 3. Suspended/Passivated (Idle)
//! - **Definition**: Active actor that has been deactivated due to idle timeout
//! - **State**: Actor is still registered but message loop is stopped
//! - **Check**: `is_virtual(actor_id)` returns true, `is_active(actor_id)` returns false
//! - **Reactivation**: Next message triggers reactivation (same as lazy activation)
//! - **Note**: State is preserved (if DurabilityFacet is attached) before passivation
//!
//! ## Activation Flow (Orleans-Inspired)
//!
//! ### Lazy Activation (Default)
//! ```
//! 1. Actor registered with VirtualActorWrapper (not active)
//! 2. First message arrives → VirtualActorWrapper.tell()
//! 3. VirtualActorWrapper checks is_active() → false
//! 4. VirtualActorWrapper calls activate_virtual_actor()
//! 5. activate_virtual_actor() retrieves actor instance
//! 6. activate_virtual_actor() calls spawn_built_actor() → actor.start()
//! 7. actor.start() spawns message loop task
//! 8. ActorRef replaces VirtualActorWrapper in registry
//! 9. Pending messages are sent to ActorRef
//! 10. Actor is now active and processing messages
//! ```
//!
//! ### Eager Activation
//! ```
//! 1. Actor registered with ActorRef (active immediately)
//! 2. actor.start() is called during registration
//! 3. Message loop starts immediately
//! 4. Actor can process messages right away
//! ```
//!
//! ## Key Design Principles (Orleans)
//!
//! 1. **Always Addressable**: Virtual actors always exist (virtually) - registered even when not active
//! 2. **Automatic Activation**: Activation is transparent - happens automatically on first message
//! 3. **Single Activation**: Only one instance exists at a time (per actor ID)
//! 4. **Automatic Passivation**: Actors are deactivated after idle timeout to save resources
//! 5. **State Preservation**: State is preserved across activation/deactivation cycles (if DurabilityFacet attached)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::{ActorId, ActorRegistry};
use plexspaces_mailbox::Message;

/// Virtual Actor Error types
#[derive(Debug, Clone, thiserror::Error)]
pub enum VirtualActorError {
    /// Actor not found
    #[error("Virtual actor not found: {0}")]
    ActorNotFound(ActorId),
    
    /// Activation failed
    #[error("Failed to activate virtual actor: {0}")]
    ActivationFailed(String),
    
    /// Deactivation failed
    #[error("Failed to deactivate virtual actor: {0}")]
    DeactivationFailed(String),
}

/// Virtual Actor Registry - stores virtual actor metadata
pub struct VirtualActorRegistry {
    /// Virtual actor metadata: actor_id -> VirtualActorMetadata
    virtual_actors: Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>>,
    /// Pending messages for actors being activated: actor_id -> Vec<Message>
    pending_activations: Arc<RwLock<HashMap<ActorId, Vec<Message>>>>,
}

impl VirtualActorRegistry {
    pub fn new() -> Self {
        Self {
            virtual_actors: Arc::new(RwLock::new(HashMap::new())),
            pending_activations: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn virtual_actors(&self) -> &Arc<RwLock<HashMap<ActorId, VirtualActorMetadata>>> {
        &self.virtual_actors
    }

    pub fn pending_activations(&self) -> &Arc<RwLock<HashMap<ActorId, Vec<Message>>>> {
        &self.pending_activations
    }
}

/// Virtual Actor Metadata
/// 
/// ## Purpose
/// Stores all metadata needed to recreate a virtual actor after suspension.
/// Per Orleans design: Virtual actors always exist (virtually) - metadata persists
/// even when the actor instance is deactivated.
/// 
/// ## Design Principles
/// - **Source of Truth**: VirtualActorManager is the ONLY place storing virtual actor metadata
/// - **ActorRegistry Separation**: ActorRegistry only tracks active instances and MessageSenders
/// - **No Memory Leaks**: Regular actors don't have metadata here (only in ActorRegistry)
/// - **Always Rebuildable**: All info needed to rebuild suspended actors is stored here
#[derive(Clone)]
pub struct VirtualActorMetadata {
    /// Virtual actor facet (for lifecycle management)
    pub facet: Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>,
    /// Last deactivation time (None if currently active)
    pub last_deactivated: Option<std::time::SystemTime>,
    /// Actor type (e.g., "GenServer", "GenEvent") - needed to rebuild suspended actors
    pub actor_type: Option<String>,
    /// Actor configuration (resource requirements, etc.) - needed to rebuild suspended actors
    pub config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    /// Tenant ID (for proper isolation) - needed to rebuild suspended actors
    pub tenant_id: String,
    /// Namespace (for proper isolation) - needed to rebuild suspended actors
    pub namespace: String,
}

/// Virtual Actor Manager - manages virtual actor lifecycle
pub struct VirtualActorManager {
    registry: VirtualActorRegistry,
    actor_registry: Arc<ActorRegistry>,
}

impl VirtualActorManager {
    pub fn new(actor_registry: Arc<ActorRegistry>) -> Self {
        Self {
            registry: VirtualActorRegistry::new(),
            actor_registry,
        }
    }
    
    /// Get virtual actor registry (for node-level access)
    /// 
    /// ## Design
    /// VirtualActorManager is the source of truth for virtual actor metadata.
    /// This getter allows Node to access the registry for observability/debugging.
    pub fn registry(&self) -> &VirtualActorRegistry {
        &self.registry
    }

    /// Register a virtual actor (always addressable, may not be active)
    ///
    /// ## Purpose
    /// Registers a virtual actor in the system. Per Orleans design, virtual actors
    /// are always registered (even when not active) so they can receive messages.
    /// 
    /// ## Orleans Design Principles
    /// - Virtual actors always exist (virtually) - metadata persists across activation/deactivation
    /// - Metadata is stored in VirtualActorManager (source of truth)
    /// - ActorRegistry only tracks active instances and MessageSenders
    /// - Suspended actors can be rebuilt from metadata stored here
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `facet` - VirtualActorFacet for lifecycle management
    /// * `actor_type` - Actor type (e.g., "GenServer") - needed to rebuild suspended actors
    /// * `config` - Actor configuration - needed to rebuild suspended actors
    /// * `tenant_id` - Tenant ID for isolation
    /// * `namespace` - Namespace for isolation
    ///
    /// ## State After Registration
    /// - Actor is registered (always addressable)
    /// - Metadata stored in VirtualActorManager (persists across suspension)
    /// - For lazy actors: Not active (VirtualActorWrapper in registry)
    /// - For eager actors: Active immediately (ActorRef in registry, message loop running)
    pub async fn register(
        &self,
        actor_id: ActorId,
        facet: Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>,
        actor_type: Option<String>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        tenant_id: String,
        namespace: String,
    ) -> Result<(), VirtualActorError> {
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        virtual_actors.insert(
            actor_id,
            VirtualActorMetadata {
                facet,
                last_deactivated: None,
                actor_type,
                config,
                tenant_id,
                namespace,
            },
        );
        Ok(())
    }
    
    /// Update virtual actor metadata (e.g., when actor is activated with config)
    ///
    /// ## Purpose
    /// Updates metadata for an already-registered virtual actor.
    /// Used when actor is activated and we have additional info (config, etc.)
    ///
    /// ## Design
    /// Idempotent - can be called multiple times safely.
    pub async fn update_metadata(
        &self,
        actor_id: &ActorId,
        actor_type: Option<String>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    ) -> Result<(), VirtualActorError> {
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        if let Some(metadata) = virtual_actors.get_mut(actor_id) {
            if let Some(actor_type) = actor_type {
                metadata.actor_type = Some(actor_type);
            }
            if let Some(config) = config {
                metadata.config = Some(config);
            }
            Ok(())
        } else {
            Err(VirtualActorError::ActorNotFound(actor_id.clone()))
        }
    }
    
    /// Get virtual actor metadata (for rebuilding suspended actors)
    ///
    /// ## Purpose
    /// Retrieves metadata needed to rebuild a suspended virtual actor.
    /// This is the source of truth for virtual actor metadata.
    ///
    /// ## Returns
    /// `Some(VirtualActorMetadata)` if actor is virtual, `None` otherwise
    pub async fn get_metadata(&self, actor_id: &ActorId) -> Option<VirtualActorMetadata> {
        let virtual_actors = self.registry.virtual_actors().read().await;
        virtual_actors.get(actor_id).cloned()
    }

    /// Check if actor is virtual (registered as virtual actor)
    ///
    /// ## Returns
    /// true if actor is registered as virtual actor, false otherwise
    ///
    /// ## Note
    /// This checks if the actor is registered as a virtual actor, not if it's active.
    /// Virtual actors are always registered (always addressable), even when not active.
    pub async fn is_virtual(&self, actor_id: &ActorId) -> bool {
        let virtual_actors = self.registry.virtual_actors().read().await;
        virtual_actors.contains_key(actor_id)
    }

    /// Get virtual actor facet
    ///
    /// ## Note
    /// Returns the facet as Box<dyn Any> so caller can downcast to VirtualActorFacet.
    /// This avoids circular dependency with journaling crate.
    pub async fn get_facet(&self, actor_id: &ActorId) -> Result<Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>, VirtualActorError> {
        let virtual_actors = self.registry.virtual_actors().read().await;
        let virtual_meta = virtual_actors
            .get(actor_id)
            .ok_or_else(|| VirtualActorError::ActorNotFound(actor_id.clone()))?;
        
        Ok(virtual_meta.facet.clone())
    }
    
    /// Queue a message for processing after activation
    pub async fn queue_message(&self, actor_id: &ActorId, message: Message) {
        let mut pending = self.registry.pending_activations().write().await;
        pending.entry(actor_id.clone()).or_insert_with(Vec::new).push(message);
    }
    
    /// Get and clear pending messages for an actor
    pub async fn take_pending_messages(&self, actor_id: &ActorId) -> Vec<Message> {
        let mut pending = self.registry.pending_activations().write().await;
        pending.remove(actor_id).unwrap_or_default()
    }
    
    /// Check if actor is active (has running message loop)
    ///
    /// ## Purpose
    /// Checks if the actor is currently active (has a running message loop processing messages).
    ///
    /// ## Returns
    /// - `true`: Actor is active (ActorRef in registry, message loop running)
    /// - `false`: Actor is not active (VirtualActorWrapper in registry, or actor is passivated)
    ///
    /// ## Design Note (Orleans-Inspired)
    /// - **Lazy Virtual Actors**: Return `false` until first message activates them
    /// - **Eager Virtual Actors**: Return `true` immediately after registration
    /// - **Passivated Actors**: Return `false` after idle timeout (but still registered)
    ///
    /// ## Implementation
    /// Checks if ActorRef (not VirtualActorWrapper) is in ActorRegistry.
    /// VirtualActorWrapper indicates actor is registered but not active.
    pub async fn is_active(&self, actor_id: &ActorId) -> bool {
        // Simple, production-grade check: Actor is active if:
        // 1. Actor is registered (MessageSender exists)
        // 2. MessageSender is ActorRef (not VirtualActorWrapper)
        // 3. Actor instance exists (actor is running)
        //
        // Design:
        // - Lazy virtual actors: Registered with VirtualActorWrapper, instance exists but not started
        // - Active actors: Registered with ActorRef, instance exists and state is Active
        // - Suspended actors: Registered with VirtualActorWrapper, no instance
        //
        // Uses proto ActorState enum: CREATING -> ACTIVATING -> ACTIVE -> DEACTIVATING -> INACTIVE
        // We check MessageSender type by checking if it's VirtualActorWrapper (not active) or ActorRef (active)
        if let Some(sender) = self.actor_registry.lookup_actor(actor_id).await {
            // Check MessageSender type name to distinguish VirtualActorWrapper from ActorRef
            // VirtualActorWrapper indicates actor is registered but not active
            // ActorRef indicates actor is active
            let type_name = std::any::type_name_of_val(&*sender);
            if type_name.contains("VirtualActorWrapper") {
                // VirtualActorWrapper found - actor is registered but not active
                eprintln!("🔵 [VIRTUAL_ACTOR_MANAGER] is_active: actor_id={}, MessageSender=VirtualActorWrapper, is_active=false", actor_id);
                return false;
            }
            // ActorRef found - check if instance exists AND actor state is Active
            // CRITICAL: For lazy virtual actors, instance exists but actor is NOT started (state is Creating, not Active)
            // We need to check the actual actor state, not just instance existence
            let has_instance = self.actor_registry.get_actor_instance(actor_id).await.is_some();
            if !has_instance {
                eprintln!("🔵 [VIRTUAL_ACTOR_MANAGER] is_active: actor_id={}, MessageSender=ActorRef, no instance, is_active=false", actor_id);
                return false;
            }
            
            // Check actor state - actor is only active if state is Active
            // Uses ActorStateFetcher trait to fetch state and check if it's Active
            let is_state_active = self.check_actor_state_active(actor_id).await;
            eprintln!("🔵 [VIRTUAL_ACTOR_MANAGER] is_active: actor_id={}, MessageSender=ActorRef, has_instance=true, state_active={}, is_active={}", actor_id, is_state_active, is_state_active);
            is_state_active
        } else {
            // Actor not registered
            eprintln!("🔵 [VIRTUAL_ACTOR_MANAGER] is_active: actor_id={}, not registered, is_active=false", actor_id);
            false
        }
    }
    
    /// Helper function to check if actor state is Active
    /// Uses ActorRegistry's helper method that can check state via callback
    async fn check_actor_state_active(&self, actor_id: &ActorId) -> bool {
        // Use ActorRegistry's method to check if actor state is Active
        // This method uses a callback pattern to avoid circular dependency
        self.actor_registry.is_actor_state_active(actor_id).await
    }
    
    
    /// Mark actor as activated (updates facet state)
    ///
    /// ## Purpose
    /// Marks a virtual actor as activated after its message loop has started.
    /// Updates the VirtualActorFacet's lifecycle state.
    ///
    /// ## Note
    /// This method calls mark_activated on the VirtualActorFacet.
    /// The facet must be downcast by the caller.
    pub async fn mark_activated(&self, actor_id: &ActorId) -> Result<(), VirtualActorError> {
        let facet_arc = self.get_facet(actor_id).await?;
        let mut facet_guard = facet_arc.write().await;
        
        // Try to downcast to VirtualActorFacet and call mark_activated
        // We can't import VirtualActorFacet here due to circular dependency,
        // so we use a trait object approach or just update metadata
        // For now, we'll update the metadata's last_deactivated to None
        // The actual facet.mark_activated() should be called by the caller after downcasting
        
        // Update metadata to indicate actor is active
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        if let Some(metadata) = virtual_actors.get_mut(actor_id) {
            metadata.last_deactivated = None;
        }
        drop(virtual_actors);
        
        Ok(())
    }

}

// Implement Service trait for VirtualActorManager (required for ServiceLocator)
impl crate::service_locator::Service for VirtualActorManager {
    fn service_name(&self) -> String {
        crate::service_locator::service_names::VIRTUAL_ACTOR_MANAGER.to_string()
    }
}

