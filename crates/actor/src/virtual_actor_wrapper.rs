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
use crate::ActorFactory;
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
        // Get VirtualActorManager from ServiceLocator
        let manager: Arc<VirtualActorManager> = self.service_locator.get_service().await
            .ok_or_else(|| "VirtualActorManager not registered in ServiceLocator".to_string())?;
        
        // Check if actor is activated (has mailbox)
        if !manager.is_active(&self.actor_id).await {
            // Actor is not activated - activate it
            tracing::debug!(actor_id = %self.actor_id, "VirtualActorWrapper: Actor not activated, activating now");
            
            // Queue message for processing after activation
            manager.queue_message(&self.actor_id, message).await;
            
            // Activate the virtual actor using ActorFactory
            use crate::actor_factory_impl::ActorFactoryImpl;
            let factory: Arc<ActorFactoryImpl> = self.service_locator.get_service().await
                .ok_or_else(|| "ActorFactory not registered in ServiceLocator".to_string())?;
            
            factory.activate_virtual_actor(&self.actor_id).await
                .map_err(|e| format!("Failed to activate virtual actor: {}", e))?;
            
            // Message will be processed after activation completes
            return Ok(());
        }
        
        // Actor is activated - use MessageSender from registry
        // Get MessageSender (which will be RegularActorWrapper for activated actors)
        use plexspaces_core::ActorRegistry;
        let registry: Arc<ActorRegistry> = self.service_locator.get_service().await
            .ok_or_else(|| "ActorRegistry not registered in ServiceLocator".to_string())?;
        
        let sender = registry.lookup_actor(&self.actor_id).await
            .ok_or_else(|| format!("Actor not found: {}", self.actor_id))?;
        
        sender.tell(message).await
            .map_err(|e| format!("MessageSender.tell() failed: {}", e).into())
    }
}
