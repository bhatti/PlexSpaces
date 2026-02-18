// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM MessageSender implementation
//!
//! Bridges WASM host function calls (JSON-in/JSON-out) to the actor framework.
//! Implements [`plexspaces_wasm_runtime::MessageSender`] using:
//! - ActorService for tell (fire-and-forget) operations
//! - ActorRegistry for ask (request-reply) via registered ActorRef
//! - ActorFactory for stop_actor with tenant isolation
//!
//! Actor IDs follow the `name@node_id` format. The actor registry stores and
//! looks up actors by their full ID — callers must always provide fully-qualified
//! IDs (no short-name inference).

use async_trait::async_trait;
use plexspaces_core::{ActorService, ServiceLocator};
use plexspaces_proto::common::v1::Message;
use plexspaces_wasm_runtime::MessageSender;
use std::sync::Arc;
use tracing::{debug, trace, warn, instrument};

/// MessageSender implementation that delegates WASM host function calls to the actor framework.
///
/// Created per WASM application and shared by all actors within that application.
/// Thread-safe: all fields are Arc-wrapped.
pub struct ActorServiceMessageSender {
    actor_service: Arc<dyn ActorService + Send + Sync>,
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
    /// Monitor reference mapping: monitor_ref_u64 -> (target_id, monitor_ref_string).
    /// Used to convert between WIT u64 refs and framework string refs.
    monitor_refs: Arc<tokio::sync::RwLock<std::collections::HashMap<u64, (String, String)>>>,
}

impl ActorServiceMessageSender {
    /// Create new MessageSender from ActorService
    pub fn new(
        actor_service: Arc<dyn ActorService + Send + Sync>,
        service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
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
        trace!(from = %from, to = %to, "WASM send_message (tell)");

        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: message.as_bytes().to_vec(),
            sender_id: from.to_string(),
            receiver_id: to.to_string(),
            ..Default::default()
        };

        self.actor_service
            .send(to, msg)
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    #[instrument(skip(self, payload), fields(from = %from, to = %to, msg_type = %message_type))]
    async fn ask(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        payload: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        use plexspaces_core::ActorId;

        debug!(from = %from, to = %to, timeout_ms = timeout_ms, "WASM ask: routing to registry");

        // Look up the target actor's ActorRef from the registry.
        // Each actor has an ActorRef created during spawn that handles local/remote routing.
        let registry = self.service_locator.actor_registry().await
            .ok_or_else(|| "ActorRegistry not available".to_string())?;

        let actor_id = ActorId::from(to.to_string());
        let actor_ref = registry.lookup_actor(&actor_id).await
            .ok_or_else(|| {
                warn!(actor_id = %to, "WASM ask: actor not found in registry");
                format!("Actor not found in registry: {}", to)
            })?;

        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload,
            sender_id: from.to_string(),
            receiver_id: to.to_string(),
            message_type: message_type.to_string(),
            ..Default::default()
        };

        let timeout = if timeout_ms == 0 {
            std::time::Duration::from_secs(5)
        } else {
            std::time::Duration::from_millis(timeout_ms)
        };

        // Use the registered ActorRef's ask() (handles local/remote routing)
        match actor_ref.ask(msg, timeout).await {
            Ok(reply) => {
                trace!(to = %to, reply_len = reply.payload.len(), "WASM ask: reply received");
                Ok(reply.payload)
            }
            Err(e) => {
                warn!(to = %to, error = %e, "WASM ask: failed");
                Err(format!("Ask failed: {}", e))
            }
        }
    }

    async fn spawn_actor(
        &self,
        _from: &str,
        module_ref: &str,
        initial_state: Vec<u8>,
        actor_id: Option<String>,
        labels: Vec<(String, String)>,
        _durable: bool,
    ) -> Result<String, String> {
        // Use ActorService to spawn actor
        // WASM spawn path: tenant/namespace come from auth, not config
        use plexspaces_core::RequestContext;
        let _ctx = RequestContext::new_without_auth(String::new(), String::new());

        let _labels_map: std::collections::HashMap<String, String> = labels.into_iter().collect();
        
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

    #[instrument(skip(self), fields(from = %from, actor_id = %actor_id))]
    async fn stop_actor(
        &self,
        from: &str,
        actor_id: &str,
        _timeout_ms: u64,
    ) -> Result<(), String> {
        use plexspaces_core::{ActorId, ActorFactory, RequestContext};

        let actor_factory: Arc<dyn ActorFactory> = self.service_locator.get_actor_factory().await
            .ok_or_else(|| "ActorFactory not found in ServiceLocator".to_string())?;

        // Get caller's tenant/namespace from ActorRegistry metadata for tenant isolation.
        // This ensures stop_actor respects namespace boundaries.
        let ctx = if let Some(registry) = self.service_locator.actor_registry().await {
            if let Some((tenant_id, namespace)) = registry.get_actor_metadata(&from.to_string()).await {
                debug!(tenant_id = %tenant_id, namespace = %namespace, "stop_actor: resolved caller metadata");
                RequestContext::new_without_auth(tenant_id, namespace)
            } else {
                warn!(from = %from, "stop_actor: no metadata found for caller, using empty context");
                RequestContext::new_without_auth(String::new(), String::new())
            }
        } else {
            warn!("stop_actor: ActorRegistry not available");
            RequestContext::new_without_auth(String::new(), String::new())
        };

        let actor_id_typed = ActorId::from(actor_id.to_string());
        actor_factory
            .stop_actor(&ctx, &actor_id_typed)
            .await
            .map_err(|e| {
                warn!(actor_id = %actor_id, error = %e, "stop_actor failed");
                format!("Failed to stop actor: {}", e)
            })?;

        debug!(actor_id = %actor_id, "stop_actor: success");
        Ok(())
    }

    async fn link_actor(
        &self,
        _from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        use plexspaces_core::ActorRegistry;
        
        use plexspaces_core::ActorId;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .actor_registry()
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
        
        use plexspaces_core::ActorId;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .actor_registry()
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
        
        use plexspaces_core::ActorId;
        
        use tokio::sync::mpsc;
        
        let actor_registry: Arc<ActorRegistry> = self.service_locator
            .actor_registry()
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
                if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    monitor = %monitor_id_clone,
                    terminated = %terminated_id,
                    reason = ?reason,
                    "Received DOWN notification for monitored actor"
                );
                }
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
                .actor_registry()
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


