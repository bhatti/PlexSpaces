// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! MessageSender implementation for WASM actors
//! Wraps ActorService to enable WASM actors to send messages

use async_trait::async_trait;
use plexspaces_core::{ActorService, ServiceLocator};
use plexspaces_mailbox::Message;
use plexspaces_wasm_runtime::MessageSender;
use std::sync::Arc;

/// MessageSender implementation that uses ActorService
pub struct ActorServiceMessageSender {
    actor_service: Arc<dyn ActorService + Send + Sync>,
    service_locator: Arc<ServiceLocator>,
    /// Monitor reference mapping: monitor_ref_u64 -> (target_id, monitor_ref_string)
    monitor_refs: Arc<tokio::sync::RwLock<std::collections::HashMap<u64, (String, String)>>>,
}

impl ActorServiceMessageSender {
    /// Create new MessageSender from ActorService
    pub fn new(
        actor_service: Arc<dyn ActorService + Send + Sync>,
        service_locator: Arc<ServiceLocator>,
    ) -> Self {
        Self {
            actor_service,
            service_locator,
            monitor_refs: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
}

#[async_trait]
impl MessageSender for ActorServiceMessageSender {
    async fn send_message(&self, from: &str, to: &str, message: &str) -> Result<(), String> {
        // Create Message from string payload
        let mut msg = Message::new(message.as_bytes().to_vec());
        msg.sender = Some(from.to_string());
        msg.receiver = to.to_string();
        
        // Use ActorService to send message
        self.actor_service
            .send(to, msg)
            .await
            .map_err(|e| e.to_string())?;
        
        Ok(())
    }

    async fn ask(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        use plexspaces_actor::ActorRef;
        use plexspaces_core::ActorId;
        
        // Create Message with correlation_id for ask pattern
        let mut msg = Message::new(payload);
        msg.sender = Some(from.to_string());
        msg.receiver = to.to_string();
        msg.message_type = message_type.to_string();
        
        // Parse actor ID to determine if local or remote
        let (actor_name, node_id) = if let Some((name, node)) = to.split_once('@') {
            (name.to_string(), Some(node.to_string()))
        } else {
            (to.to_string(), None)
        };
        
        // Create ActorRef for the target actor
        // For ask() pattern, we always need an ActorRef (not just MessageSender)
        // So we create ActorRef directly based on routing info
        let actor_ref = if let Some(node) = node_id {
            // Remote actor
            ActorRef::remote(
                to.to_string(),
                node,
                self.service_locator.clone(),
            )
        } else {
            // Local actor - look up routing to determine if local or remote
            use plexspaces_core::service_locator::service_names;
            use plexspaces_core::RequestContext;
            let ctx = RequestContext::internal();
            
            if let Some(registry) = self.service_locator.get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY).await {
                let actor_id = ActorId::from(to.to_string());
                if let Some(routing) = registry.lookup_routing(&ctx, &actor_id).await.ok().flatten() {
                    // Create ActorRef based on routing info
                    ActorRef::remote(
                        to.to_string(),
                        routing.node_id,
                        self.service_locator.clone(),
                    )
                } else {
                    return Err(format!("Actor not found: {}", to));
                }
            } else {
                // No registry available, create remote ActorRef (will fail if actor doesn't exist)
                ActorRef::remote(
                    to.to_string(),
                    "local".to_string(), // Default to local
                    self.service_locator.clone(),
                )
            }
        };
        
        // Use ActorRef::ask() for proper ask pattern
        let timeout = if timeout_ms == 0 {
            std::time::Duration::from_secs(5) // Default 5 seconds
        } else {
            std::time::Duration::from_millis(timeout_ms)
        };
        
        match actor_ref.ask(msg, timeout).await {
            Ok(reply) => {
                Ok(reply.payload)
            }
            Err(e) => {
                Err(format!("Ask request failed: {}", e))
            }
        }
    }

    async fn spawn_actor(
        &self,
        from: &str,
        module_ref: &str,
        initial_state: Vec<u8>,
        actor_id: Option<String>,
        labels: Vec<(String, String)>,
        durable: bool,
    ) -> Result<String, String> {
        // Use ActorService to spawn actor
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::internal();
        
        let labels_map: std::collections::HashMap<String, String> = labels.into_iter().collect();
        
        // For now, use module_ref as actor_type
        // TODO: Resolve module_ref to get actual actor_type
        let actor_type = module_ref.to_string();
        
        // Spawn actor using ActorService
        let spawned_id = self.actor_service
            .spawn_actor(
                &actor_id.unwrap_or_else(|| ulid::Ulid::new().to_string()),
                &actor_type,
                initial_state,
            )
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        
        Ok(spawned_id.id().to_string())
    }

    async fn stop_actor(
        &self,
        _from: &str,
        actor_id: &str,
        _timeout_ms: u64,
    ) -> Result<(), String> {
        // Use ActorFactory to stop actor
        use plexspaces_actor::ActorFactory;
        use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
        use plexspaces_core::service_locator::service_names;
        use plexspaces_core::ActorId;
        
        let actor_factory: Arc<ActorFactoryImpl> = self.service_locator
            .get_service_by_name::<ActorFactoryImpl>(service_names::ACTOR_FACTORY_IMPL)
            .await
            .ok_or_else(|| "ActorFactory not found in ServiceLocator".to_string())?;
        
        let actor_id_typed = ActorId::from(actor_id.to_string());
        actor_factory
            .stop_actor(&actor_id_typed)
            .await
            .map_err(|e| format!("Failed to stop actor: {}", e))?;
        
        Ok(())
    }

    async fn link_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        use plexspaces_core::ActorRegistry;
        use plexspaces_core::service_locator::service_names;
        use plexspaces_core::ActorId;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        
        let actor1_id = ActorId::from(actor_id.to_string());
        let actor2_id = ActorId::from(linked_actor_id.to_string());
        
        actor_registry
            .link(&actor1_id, &actor2_id)
            .await
            .map_err(|e| format!("Failed to link actors: {}", e))?;
        
        Ok(())
    }

    async fn unlink_actor(
        &self,
        _from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        use plexspaces_core::ActorRegistry;
        use plexspaces_core::service_locator::service_names;
        use plexspaces_core::ActorId;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        
        let actor1_id = ActorId::from(actor_id.to_string());
        let actor2_id = ActorId::from(linked_actor_id.to_string());
        
        actor_registry
            .unlink(&actor1_id, &actor2_id)
            .await
            .map_err(|e| format!("Failed to unlink actors: {}", e))?;
        
        Ok(())
    }

    async fn monitor_actor(
        &self,
        from: &str,
        actor_id: &str,
    ) -> Result<u64, String> {
        use plexspaces_core::ActorRegistry;
        use plexspaces_core::service_locator::service_names;
        use plexspaces_core::ActorId;
        use plexspaces_core::ExitReason;
        use tokio::sync::mpsc;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        
        let target_id = ActorId::from(actor_id.to_string());
        let monitor_id = ActorId::from(from.to_string());
        let monitor_ref = ulid::Ulid::new().to_string();
        
        // Create a channel for termination notifications
        // For WASM actors, we'll need to handle DOWN messages differently
        // For now, create a channel but we may need to integrate with WASM message handling
        let (tx, mut rx) = mpsc::channel(10);
        
        // Spawn a task to handle DOWN messages
        let monitor_id_clone = monitor_id.clone();
        tokio::spawn(async move {
            while let Some((terminated_id, reason)) = rx.recv().await {
                tracing::debug!(
                    monitor = %monitor_id_clone,
                    terminated = %terminated_id,
                    reason = ?reason,
                    "Received DOWN notification for monitored actor"
                );
                // TODO: Send DOWN message to WASM actor's mailbox
                // This requires integration with WASM message handling
            }
        });
        
        actor_registry
            .monitor(&target_id, &monitor_id, monitor_ref.clone(), tx)
            .await
            .map_err(|e| format!("Failed to monitor actor: {}", e))?;
        
        // Convert monitor_ref (String) to u64 for WIT interface
        // Use hash of string as u64 (not perfect but works for now)
        let monitor_ref_u64 = monitor_ref
            .as_bytes()
            .iter()
            .fold(0u64, |acc, &b| acc.wrapping_mul(31).wrapping_add(b as u64));
        
        // Store mapping: monitor_ref_u64 -> (target_id, monitor_ref_string)
        {
            let mut monitor_refs = self.monitor_refs.write().await;
            monitor_refs.insert(monitor_ref_u64, (actor_id.to_string(), monitor_ref));
        }
        
        Ok(monitor_ref_u64)
    }

    async fn demonitor_actor(
        &self,
        from: &str,
        actor_id: &str,
        monitor_ref: u64,
    ) -> Result<(), String> {
        use plexspaces_core::ActorRegistry;
        use plexspaces_core::service_locator::service_names;
        use plexspaces_core::ActorId;
        
        // Look up the monitor_ref_string from our mapping
        let monitor_info = {
            let monitor_refs = self.monitor_refs.read().await;
            monitor_refs.get(&monitor_ref).cloned()
        };
        
        if let Some((stored_target_id, monitor_ref_string)) = monitor_info {
            // Verify the target_id matches
            if stored_target_id != actor_id {
                return Err(format!("Monitor reference {} does not match target actor {}", monitor_ref, actor_id));
            }
            
            // Remove from mapping
            {
                let mut monitor_refs = self.monitor_refs.write().await;
                monitor_refs.remove(&monitor_ref);
            }
            
            let actor_registry: Arc<ActorRegistry> = self.service_locator
                .get_service_by_name::<ActorRegistry>(service_names::ACTOR_REGISTRY)
                .await
                .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
            
            let target_id = ActorId::from(actor_id.to_string());
            let monitor_id = ActorId::from(from.to_string());
            
            actor_registry
                .demonitor(&target_id, &monitor_id, &monitor_ref_string)
                .await
                .map_err(|e| format!("Failed to demonitor actor: {}", e))?;
            
            Ok(())
        } else {
            Err(format!("Monitor reference {} not found", monitor_ref))
        }
    }
}

