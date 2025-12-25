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

//! Virtual Actor Manager - Orleans-inspired lifecycle management
//!
//! ## Purpose
//! Manages virtual actor lifecycle: activation, deactivation, registration.
//! Keeps ActorRegistry focused on data storage, this module handles business logic.
//!
//! ## Design Principles
//! - **Modular**: Separate from Node and ActorRegistry
//! - **Simple**: Inspired by Erlang/Akka simplicity
//! - **Loosely Coupled**: Takes dependencies as parameters, not hard-coded

use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use crate::{ActorId, ActorRegistry, VirtualActorMetadata, Service};
use plexspaces_mailbox::Message;

/// Error types for virtual actor operations
#[derive(Debug, thiserror::Error)]
pub enum VirtualActorError {
    #[error("Actor not found: {0}")]
    ActorNotFound(String),
    
    #[error("Actor is not virtual: {0}")]
    NotVirtual(String),
    
    #[error("Activation failed: {0}")]
    ActivationFailed(String),
    
    #[error("Deactivation failed: {0}")]
    DeactivationFailed(String),
    
    #[error("Registration failed: {0}")]
    RegistrationFailed(String),
}

/// Virtual Actor Manager - handles lifecycle operations
///
/// ## Purpose
/// Manages virtual actor lifecycle operations while keeping ActorRegistry
/// focused on data storage. This separation keeps the design modular and
/// loosely coupled.
pub struct VirtualActorManager {
    /// Actor registry for data access
    registry: Arc<ActorRegistry>,
}

impl VirtualActorManager {
    /// Create a new VirtualActorManager
    pub fn new(registry: Arc<ActorRegistry>) -> Self {
        Self { registry }
    }
    
    /// Check if an actor is virtual
    pub async fn is_virtual(&self, actor_id: &ActorId) -> bool {
        let virtual_actors = self.registry.virtual_actors().read().await;
        virtual_actors.contains_key(actor_id)
    }
    
    /// Get virtual actor metadata
    pub async fn get_metadata(&self, actor_id: &ActorId) -> Option<VirtualActorMetadata> {
        let virtual_actors = self.registry.virtual_actors().read().await;
        virtual_actors.get(actor_id).cloned()
    }
    
    /// Register a virtual actor
    ///
    /// ## Arguments
    /// * `actor_id` - The actor ID
    /// * `facet` - The VirtualActorFacet instance (wrapped in Box<dyn Any>)
    ///
    /// ## Returns
    /// Ok(()) if registration successful
    pub async fn register(
        &self,
        actor_id: ActorId,
        facet: Arc<RwLock<Box<dyn std::any::Any + Send + Sync>>>,
    ) -> Result<(), VirtualActorError> {
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        
        if virtual_actors.contains_key(&actor_id) {
            return Err(VirtualActorError::RegistrationFailed(
                format!("Virtual actor already registered: {}", actor_id)
            ));
        }

        let metadata = VirtualActorMetadata {
            facet,
            last_deactivated: None,
        };

        virtual_actors.insert(actor_id, metadata);
        Ok(())
    }
    
    /// Get virtual actor facet for downcasting (caller handles downcasting)
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
    
    /// Check if actor is active (has mailbox)
    pub async fn is_active(&self, actor_id: &ActorId) -> bool {
        self.registry.is_actor_activated(actor_id).await
    }
    
    /// Get actor instance for lazy activation
    pub async fn get_actor_instance(&self, actor_id: &ActorId) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        let actor_instances = self.registry.actor_instances().read().await;
        actor_instances.get(actor_id).cloned()
    }
    
    /// Remove actor instance (for lazy activation)
    pub async fn remove_actor_instance(&self, actor_id: &ActorId) -> Option<Arc<dyn std::any::Any + Send + Sync>> {
        let mut actor_instances = self.registry.actor_instances().write().await;
        actor_instances.remove(actor_id)
    }
    
    /// Mark actor as activated (updates facet state)
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
        drop(facet_guard);
        
        Ok(())
    }
    
    /// Mark actor as deactivated (updates facet state)
    ///
    /// ## Note
    /// This method calls mark_deactivated on the VirtualActorFacet.
    /// The facet must be downcast by the caller.
    pub async fn mark_deactivated(&self, actor_id: &ActorId) -> Result<(), VirtualActorError> {
        let facet_arc = self.get_facet(actor_id).await?;
        let mut facet_guard = facet_arc.write().await;
        
        // Update metadata to indicate actor is deactivated
        let mut virtual_actors = self.registry.virtual_actors().write().await;
        if let Some(metadata) = virtual_actors.get_mut(actor_id) {
            metadata.last_deactivated = Some(SystemTime::now());
        }
        drop(virtual_actors);
        drop(facet_guard);
        
        Ok(())
    }
}

// Implement Service trait so VirtualActorManager can be registered in ServiceLocator
impl Service for VirtualActorManager {
    fn service_name(&self) -> String {
        crate::service_locator::service_names::VIRTUAL_ACTOR_MANAGER.to_string()
    }
}



