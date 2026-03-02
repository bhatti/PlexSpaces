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

//! Virtual Actor Wrapper - Orleans-inspired automatic activation
//!
//! ## Purpose
//! Wraps virtual actors to provide automatic activation on `tell()` and `ask()` calls.
//! Per Orleans design: virtual actors are always addressable, activation is transparent.
//!
//! ## Design (Orleans-Inspired)
//! - **Always Addressable**: Wrapper is always available in ActorRegistry (even when actor is not active)
//! - **Automatic Activation**: `tell()` automatically activates lazy virtual actors on first message
//! - **Automatic Reactivation**: `tell()` reactivates suspended/passivated virtual actors
//! - **Simple API**: Just implements MessageSender trait - activation is transparent to caller
//!
//! ## Activation Flow (Orleans-Inspired)
//!
//! ### Lazy Virtual Actors (First Activation)
//! ```
//! 1. Actor registered with VirtualActorWrapper (not active, no message loop running)
//! 2. First message arrives → VirtualActorWrapper.tell()
//! 3. VirtualActorWrapper checks is_active() → false
//! 4. VirtualActorWrapper queues message and calls activate_virtual_actor()
//! 5. activate_virtual_actor() retrieves actor instance from VirtualActorManager
//! 6. activate_virtual_actor() calls spawn_built_actor() → actor.start()
//! 7. actor.start() spawns message loop task (ActorRef replaces VirtualActorWrapper in registry)
//! 8. Pending messages (including the first one) are sent to ActorRef
//! 9. Actor is now active and processing messages
//! ```
//!
//! ### Suspended/Passivated Virtual Actors (Reactivation)
//! ```
//! 1. Active actor is suspended/passivated (ActorRef unregistered, VirtualActorWrapper re-registered)
//! 2. Next message arrives → VirtualActorWrapper.tell()
//! 3. VirtualActorWrapper checks is_active() → false
//! 4. Same flow as lazy activation (steps 4-9 above)
//! 5. Actor reactivates and processes message
//! ```
//!
//! ## Key Design Principles (Orleans)
//!
//! 1. **Always Addressable**: Virtual actors always exist (virtually) - registered even when not active
//! 2. **Transparent Activation**: Activation happens automatically - caller doesn't need to know actor state
//! 3. **Single Activation**: Only one instance exists at a time (per actor ID)
//! 4. **State Preservation**: State is preserved across activation/deactivation cycles (if DurabilityFacet attached)

use async_trait::async_trait;
use std::sync::Arc;
use plexspaces_core::{MessageSender, ActorId, VirtualActorManager, ServiceLocator as ServiceLocatorTrait};
use crate::ActorFactory;
use plexspaces_proto::common::v1::Message;

/// Virtual Actor Wrapper - automatically activates actor on tell()
///
/// ## Purpose
/// Wraps a virtual actor to provide automatic activation. When `tell()` is called,
/// it checks if the actor is activated, and if not, activates it via VirtualActorManager.
///
/// ## Design (Orleans-Inspired)
/// Similar to Orleans grain references - always addressable, activation is transparent.
/// Uses VirtualActorManager and ActorFactory via ServiceLocator to avoid Node dependency.
pub struct VirtualActorWrapper {
    /// Actor ID
    actor_id: ActorId,
    /// ServiceLocator for accessing VirtualActorManager
    service_locator: Arc<dyn ServiceLocatorTrait>,
    /// ActorFactory for activating virtual actors
    actor_factory: Arc<dyn ActorFactory>,
}

impl VirtualActorWrapper {
    /// Create a new VirtualActorWrapper
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `service_locator` - ServiceLocator for accessing VirtualActorManager
    /// * `actor_factory` - ActorFactory for activating virtual actors
    pub fn new(
        actor_id: ActorId,
        service_locator: Arc<dyn ServiceLocatorTrait>,
        actor_factory: Arc<dyn ActorFactory>,
    ) -> Self {
        Self {
            actor_id,
            service_locator,
            actor_factory,
        }
    }
}

#[async_trait]
impl MessageSender for VirtualActorWrapper {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
            "[VIRTUAL_ACTOR_WRAPPER] tell() called: actor_id={}, message_id={}, correlation_id={:?}, sender={:?}, receiver={}",
            self.actor_id, message.id, message.correlation_id, message.sender_id, message.receiver_id
            );
        }
        
        // VALIDATION: Check if actor is registered before processing
        // tell() should fail immediately if actor is not registered (synchronous check)
        // VirtualActorWrapper should always be in registry for virtual actors
        use plexspaces_core::ActorRegistry;
        let registry: Arc<ActorRegistry> = self.service_locator.actor_registry().await
            .ok_or_else(|| "ActorRegistry not registered in ServiceLocator".to_string())?;
        
        // Check if actor is registered (VirtualActorWrapper should be in registry for virtual actors)
        if registry.lookup_actor(&self.actor_id).await.is_none() {
            tracing::warn!("[VIRTUAL_ACTOR_WRAPPER] Actor not registered: actor_id={}", self.actor_id);
            return Err(format!(
                "Virtual actor {} is not registered - cannot send message. Actor must be registered before tell() can be called.",
                self.actor_id
            ).into());
        }
        
        // Get VirtualActorManager from ServiceLocator (reuse registry from above)
        let manager: Arc<VirtualActorManager> = self.service_locator.virtual_actor_manager().await
            .ok_or_else(|| "VirtualActorManager not registered in ServiceLocator".to_string())?;
        
        // ORLEANS DESIGN: Check if actor is activated (has running message loop)
        // - For lazy virtual actors: false until first message activates them
        // - For eager virtual actors: true immediately after registration
        // - For suspended/passivated actors: false until reactivated
        // This check determines if we need to activate/reactivate the actor
        let is_active = manager.is_active(&self.actor_id).await;
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Actor active check: actor_id={}, is_active={}", self.actor_id, is_active);
        }
        if !is_active {
            // ORLEANS: Actor is not active - activate/reactivate it
            // This handles both lazy activation (first message) and reactivation (after suspension)
            // Actor is not activated - check if activation is in progress
            // Use VirtualActorFacet's is_activating flag to coordinate concurrent requests
            let facet_arc = manager.get_facet(&self.actor_id).await
                .map_err(|e| format!("Failed to get virtual actor facet: {}", e))?;
            let facet_guard = facet_arc.read().await;
            
            // Check if activation is already in progress and try to start activation
            // This uses the is_activating flag internally to prevent concurrent activations
            // facet_guard is RwLockReadGuard<Box<dyn VirtualActorLifecycleFacet>>
            // Use trait methods directly - no downcasting needed
            let activation_started = facet_guard.start_activation().await;
            drop(facet_guard);
            
            if !activation_started {
                // Activation is in progress - queue message and return
                // The in-progress activation will send pending messages when it completes
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, "VirtualActorWrapper: Activation in progress, queueing message");
                }
                manager.queue_message(&self.actor_id, message).await;
                return Ok(());
            }
            
            // Queue message for processing after activation
            // IMPORTANT: Message preserves correlation_id and sender for reply routing
            // This is critical for ask() pattern - the reply must route back via correlation_id
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Queueing message for activation: id={}, correlation_id={:?}, sender={:?}, receiver={}", 
                    message.id, message.correlation_id, message.sender_id, message.receiver_id);
            }
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                actor_id = %self.actor_id,
                message_id = %message.id,
                correlation_id = ?message.correlation_id,
                sender = ?message.sender_id,
                "VirtualActorWrapper: Queueing message for activation (preserves correlation_id and sender for reply routing)"
            );
            }
            manager.queue_message(&self.actor_id, message).await;
            
            // Activate the virtual actor using ActorFactory
            // This is synchronous - it awaits actor.start() which registers the actor
            // ActorFactory is stored in VirtualActorWrapper to avoid circular dependency issues
            
            // Activate (synchronous - completes when actor is registered and message loop is running)
            // mark_activated() in activate_virtual_actor will clear the is_activating flag
            // activate_virtual_actor will send all pending messages (including this one) after activation
            // The message's correlation_id and sender are preserved when sent to the ActorRef
            // CRITICAL: This await ensures activation is synchronous - actor is fully ready when this returns
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Calling activate_virtual_actor (SYNC): actor_id={}", self.actor_id);
            }
            self.actor_factory.activate_virtual_actor(&self.actor_id).await
                .map_err(|e| {
                    tracing::warn!("[VIRTUAL_ACTOR_WRAPPER] Failed to activate virtual actor: actor_id={}, error={}", self.actor_id, e);
                    format!("Failed to activate virtual actor: {}", e)
                })?;
            
            // Verify actor is now active after synchronous activation
            let is_active_after = manager.is_active(&self.actor_id).await;
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] activate_virtual_actor completed (SYNC): actor_id={}, is_active={}", self.actor_id, is_active_after);
            }
            
            if !is_active_after {
                tracing::warn!("[VIRTUAL_ACTOR_WRAPPER] Actor not active after activation: actor_id={}", self.actor_id);
                return Err(format!("Actor {} is not active after synchronous activation", self.actor_id).into());
            }
            
            // Update last_access for LRU tracking (actor is now active)
            manager.update_last_access(&self.actor_id).await;
            
            // Message was queued and sent by activate_virtual_actor after activation
            // The message preserves correlation_id and sender, so reply routing will work correctly
            // No need to forward - activate_virtual_actor handles it
            return Ok(());
        }
        
        // Actor is activated - use MessageSender from registry
        // Get MessageSender (which will be ActorRef for activated actors, replacing VirtualActorWrapper)
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Actor is already active, using MessageSender from registry: actor_id={}", self.actor_id);
        }
        // Note: registry was already obtained above for registration check, reuse it here (no duplicate import)
        let sender = registry.lookup_actor(&self.actor_id).await
            .ok_or_else(|| format!("Actor not found: {}", self.actor_id))?;
        
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Found MessageSender, calling tell(): actor_id={}, correlation_id={:?}", 
                self.actor_id, message.correlation_id);
        }
        let result = sender.tell(message).await;
        match &result {
            Ok(_) => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("[VIRTUAL_ACTOR_WRAPPER] Successfully sent message via MessageSender");
                }
            }
            Err(e) => {
                tracing::warn!("[VIRTUAL_ACTOR_WRAPPER] Failed to send message via MessageSender: {}", e);
            }
        }
        result.map_err(|e| format!("MessageSender.tell() failed: {}", e).into())
    }
}
