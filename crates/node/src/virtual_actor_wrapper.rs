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
//! Wraps virtual actors to provide automatic activation on `tell()` calls.
//! Checks if actor is activated, activates if needed, then uses ActorRef's tell().
//!
//! ## Design (Orleans-Inspired)
//! - **Always Addressable**: Wrapper is always available in ActorRegistry
//! - **Automatic Activation**: `tell()` automatically activates actor if needed
//! - **Simple API**: Just implements Actor trait - activation is transparent

use async_trait::async_trait;
use std::sync::Arc;
use plexspaces_core::{MessageSender, ActorId, VirtualActorManager, ServiceLocator};
use plexspaces_actor::ActorFactory;
use plexspaces_mailbox::Message;

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
    /// ServiceLocator for accessing VirtualActorManager and ActorFactory
    service_locator: Arc<ServiceLocator>,
}

impl VirtualActorWrapper {
    /// Create a new VirtualActorWrapper
    pub fn new(actor_id: ActorId, service_locator: Arc<ServiceLocator>) -> Self {
        Self {
            actor_id,
            service_locator,
        }
    }
}

#[async_trait]
impl MessageSender for VirtualActorWrapper {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] tell() called: actor_id={}, message_id={}, message_type={:?}", 
            self.actor_id, message.id, message.message_type);
        // Get VirtualActorManager from ServiceLocator
        let manager: Arc<VirtualActorManager> = self.service_locator.get_service_by_name(plexspaces_core::service_locator::service_names::VIRTUAL_ACTOR_MANAGER).await
            .ok_or_else(|| "VirtualActorManager not registered in ServiceLocator".to_string())?;
        
        // Check if actor is activated (has mailbox)
        let is_active = manager.is_active(&self.actor_id).await;
        let message_id = message.id.clone();
        tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] is_active check: actor_id={}, is_active={}, message_id={}", self.actor_id, is_active, message_id);
        if !is_active {
            // Actor is not activated - activate it
            tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] Actor not activated, activating now: actor_id={}, message_id={}", self.actor_id, message_id);
            if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %self.actor_id, "VirtualActorWrapper: Actor not activated, activating now");
            }
            
            // Queue message for processing after activation
            manager.queue_message(&self.actor_id, message).await;
            tracing::warn!("🟢 [VIRTUAL_ACTOR_WRAPPER] Message queued: actor_id={}, message_id={}", self.actor_id, message_id);
            
            // Activate the virtual actor using ActorFactory
            use plexspaces_actor::get_actor_factory;
            let factory = get_actor_factory(self.service_locator.as_ref()).await
                .ok_or_else(|| "ActorFactory not registered in ServiceLocator".to_string())?;
            
            tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] Calling activate_virtual_actor: actor_id={}, message_id={}", self.actor_id, message_id);
            factory.activate_virtual_actor(&self.actor_id).await
                .map_err(|e| {
                    tracing::warn!("🔴 [VIRTUAL_ACTOR_WRAPPER] Failed to activate virtual actor: actor_id={}, message_id={}, error={}", self.actor_id, message_id, e);
                    format!("Failed to activate virtual actor: {}", e)
                })?;
            tracing::warn!("🟢 [VIRTUAL_ACTOR_WRAPPER] Actor activated: actor_id={}, message_id={}", self.actor_id, message_id);
            
            // CRITICAL: After activation, get the ActorRef and send the message
            // activate_virtual_actor is synchronous (awaits actor.start()), so actor is ready
            use plexspaces_core::ActorRegistry;
            let registry: Arc<ActorRegistry> = self.service_locator.get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
                .ok_or_else(|| "ActorRegistry not registered in ServiceLocator".to_string())?;
            
            // Get ActorRef (should be available now since activation is synchronous)
            let sender = registry.lookup_actor(&self.actor_id).await
                .ok_or_else(|| {
                    tracing::warn!("🔴 [VIRTUAL_ACTOR_WRAPPER] Actor not found in registry after activation: actor_id={}", self.actor_id);
                    format!("Actor not found after activation: {}", self.actor_id)
                })?;
            
            tracing::warn!("🟢 [VIRTUAL_ACTOR_WRAPPER] Got ActorRef after activation, sending queued message: actor_id={}, message_id={}", self.actor_id, message_id);
            // Send the queued message (take_pending_messages returns all queued messages)
            let queued_messages = manager.take_pending_messages(&self.actor_id).await;
            tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] Retrieved {} queued messages after activation: actor_id={}", queued_messages.len(), self.actor_id);
            // Send all queued messages
            for msg in queued_messages {
                sender.tell(msg).await
                    .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                        format!("Failed to send queued message: {}", e).into()
                    })?;
            }
            tracing::warn!("🟢 [VIRTUAL_ACTOR_WRAPPER] All queued messages sent after activation: actor_id={}, message_id={}", self.actor_id, message_id);
            return Ok(());
        }
        
        // Actor is activated - use MessageSender from registry
        // Get MessageSender (which will be ActorRef for activated actors)
        tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] Actor is active, using ActorRef: actor_id={}", self.actor_id);
        use plexspaces_core::ActorRegistry;
        let registry: Arc<ActorRegistry> = self.service_locator.get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
            .ok_or_else(|| "ActorRegistry not registered in ServiceLocator".to_string())?;
        
        let sender = registry.lookup_actor(&self.actor_id).await
            .ok_or_else(|| {
                tracing::warn!("🔴 [VIRTUAL_ACTOR_WRAPPER] Actor not found in registry: actor_id={}", self.actor_id);
                format!("Actor not found: {}", self.actor_id)
            })?;
        
        let message_id_for_send = message.id.clone();
        tracing::warn!("🔵 [VIRTUAL_ACTOR_WRAPPER] Sending message via ActorRef: actor_id={}, message_id={}", self.actor_id, message_id_for_send);
        sender.tell(message).await
            .map_err(|e| {
                tracing::warn!("🔴 [VIRTUAL_ACTOR_WRAPPER] MessageSender.tell() failed: actor_id={}, message_id={}, error={}", self.actor_id, message_id_for_send, e);
                format!("MessageSender.tell() failed: {}", e).into()
            })
    }
}
