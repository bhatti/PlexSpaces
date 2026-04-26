// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM MessageSender implementation
//!
//! Bridges WASM host function calls to the actor framework.
//! Implements [`plexspaces_wasm_runtime::MessageSender`] using:
//! - ActorService for tell (fire-and-forget) operations
//! - ActorRegistry for ask (request-reply) via registered ActorRef
//! - ActorFactory for stop_actor with tenant isolation
//!
//! Actor IDs follow the canonical `{name}//{actor_type}::{namespace}@{node_id}` format (e.g.,
//! `parameter-server:ray-ps@test-node`). The actor registry stores and looks
//! up actors by their full ID — callers must always provide fully-qualified
//! IDs. The WASM host bridge passes IDs through unchanged.

use async_trait::async_trait;
use plexspaces_common::dialable_node_address;
use plexspaces_core::{ActorService, ServiceLocator};
use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, AllReduceShardGroupResponse, BarrierShardGroupRequest,
    BarrierShardGroupResponse, BroadcastShardGroupRequest, BroadcastShardGroupResponse,
    BulkUpdateShardGroupRequest, BulkUpdateShardGroupResponse, CreateShardGroupRequest,
    CreateShardGroupResponse, ReduceShardGroupRequest, ReduceShardGroupResponse,
    ScatterGatherRequest, ScatterGatherResponse, SpawnActorsRequest, SpawnActorsResponse,
};
use plexspaces_proto::application::v1::{
    application_service_client::ApplicationServiceClient, ApplicationInfo, ApplicationMetrics,
    GetApplicationStatusRequest,
};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::node::v1::NodeRegistration;
use plexspaces_wasm_runtime::MessageSender;
use std::sync::Arc;
use tracing::{debug, instrument, trace, warn};

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
    ///
    /// ## Arguments
    /// * `actor_service` - ActorService for routing messages
    /// * `service_locator` - ServiceLocator for accessing registry and other services
    ///
    /// ## Returns
    /// New ActorServiceMessageSender instance
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

    async fn local_node_id(&self) -> Result<String, String> {
        self.service_locator
            .get_node_config()
            .await
            .filter(|c| !c.id.is_empty())
            .map(|c| c.id)
            .ok_or_else(|| "NodeConfig not found in ServiceLocator".to_string())
    }

    fn canonical_node_address_key(address: &str) -> String {
        let trimmed = address.trim();
        let without_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(trimmed);
        let normalized_host = without_scheme
            .replace("127.0.0.1", "localhost")
            .replace("0.0.0.0", "localhost");
        normalized_host.to_ascii_lowercase()
    }

    fn normalize_node_address(address: &str) -> String {
        dialable_node_address(address)
    }

    /// Resolve tenant/namespace for a WASM caller from the registered sender actor (same rules as
    /// [`Self::stop_actor`]). Used so monitor registration stores a real [`plexspaces_core::RequestContext`]
    /// for cross-node `__DOWN__` routing instead of ad hoc parallel fields.
    async fn request_context_from_registered_sender_actor(
        &self,
        from: &str,
    ) -> Result<plexspaces_core::RequestContext, String> {
        use plexspaces_core::{ActorId, RequestContext};

        let registry = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let sender_id = ActorId::from_canonical(from)
            .map_err(|e| format!("Invalid canonical sender actor ID '{from}': {e}"))?;

        if let Some((tenant_id, namespace)) = registry.get_actor_metadata(&sender_id).await {
            return Ok(RequestContext::new_without_auth(tenant_id, namespace));
        }

        Err(format!(
            "Registered sender actor scope not found for '{from}'"
        ))
    }

    fn looks_like_node_address(target: &str) -> bool {
        let trimmed = target.trim();
        trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.contains(':')
            || trimmed == "localhost"
    }

    fn matching_node_registration<'a>(
        target: &str,
        registrations: &'a [NodeRegistration],
    ) -> Option<&'a NodeRegistration> {
        let target_key = Self::canonical_node_address_key(target);
        registrations.iter().find(|registration| {
            registration.node_id == target
                || Self::canonical_node_address_key(&registration.node_address) == target_key
        })
    }

    async fn resolve_node_endpoint(&self, target: &str) -> Result<(String, String), String> {
        let ctx = self
            .service_locator
            .request_context_for_system_operations()
            .await;
        let registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| "NodeRegistry not found in ServiceLocator".to_string())?;
        if let Some(registration) = registry
            .lookup_node(&ctx, target)
            .await
            .map_err(|e| format!("NodeRegistry lookup failed: {}", e))?
        {
            return Ok((
                registration.node_id,
                Self::normalize_node_address(&registration.node_address),
            ));
        }

        let (registrations, _) = registry
            .list_nodes(&ctx, None, 0, "")
            .await
            .map_err(|e| format!("NodeRegistry list failed: {}", e))?;
        if let Some(registration) = Self::matching_node_registration(target, &registrations) {
            return Ok((
                registration.node_id.clone(),
                Self::normalize_node_address(&registration.node_address),
            ));
        }

        if Self::looks_like_node_address(target) {
            let normalized = Self::normalize_node_address(target);
            return Ok((normalized.clone(), normalized));
        }

        Err(format!("Node not found: {}", target))
    }

    async fn known_node_endpoints(&self) -> Result<Vec<(String, String)>, String> {
        let ctx = self
            .service_locator
            .request_context_for_system_operations()
            .await;
        let registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| "NodeRegistry not found in ServiceLocator".to_string())?;
        let (registrations, _) = registry
            .list_nodes(&ctx, None, 0, "")
            .await
            .map_err(|e| format!("NodeRegistry list failed: {}", e))?;
        Ok(registrations
            .into_iter()
            .map(|registration| {
                (
                    registration.node_id,
                    Self::normalize_node_address(&registration.node_address),
                )
            })
            .collect())
    }

    async fn fetch_remote_application_status(
        &self,
        application_id: &str,
        connection_target: &str,
    ) -> Result<(ApplicationInfo, String, String), String> {
        let channel = self
            .service_locator
            .get_application_service_client(connection_target)
            .await
            .map_err(|e| e.to_string())?;
        let mut client = ApplicationServiceClient::new(channel);
        let response = client
            .get_application_status(GetApplicationStatusRequest {
                application_id: application_id.to_string(),
            })
            .await
            .map_err(|e| format!("GetApplicationStatus failed: {}", e.message()))?
            .into_inner();
        let info = response.application.ok_or_else(|| {
            response
                .error
                .unwrap_or_else(|| "Application status missing".to_string())
        })?;
        let response_node_id = if response.node_id.is_empty() {
            connection_target.to_string()
        } else {
            response.node_id
        };
        let response_node_address = if response.node_address.is_empty() {
            Self::normalize_node_address(connection_target)
        } else {
            Self::normalize_node_address(&response.node_address)
        };
        Ok((info, response_node_id, response_node_address))
    }
}

#[async_trait]
impl MessageSender for ActorServiceMessageSender {
    async fn send_message(
        &self,
        from: &str,
        to: &str,
        message_type: &str,
        message: &[u8],
    ) -> Result<(), String> {
        trace!(from = %from, to = %to, message_type = %message_type, "WASM send_message (tell)");

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: message.to_vec(),
            sender_id: from.to_string(),
            receiver_id: to.to_string(),
            message_type: message_type.to_string(),
            ..Default::default()
        };

        self.actor_service
            .send(&ctx, to, msg)
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
        // Build request message with req- prefix (ActorRef::ask will also add req- if missing)
        let request_id = format!("req-{}", ulid::Ulid::new());

        let timeout = if timeout_ms == 0 {
            std::time::Duration::from_secs(5)
        } else {
            std::time::Duration::from_millis(timeout_ms)
        };

        debug!(
            message_id = %request_id,
            sender_id = %from,
            recipient_id = %to,
            msg_type = %message_type,
            timeout_ms = timeout_ms,
            "WASM ask: sending request via ActorService"
        );

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        let msg = Message {
            id: request_id.clone(),
            payload,
            sender_id: from.to_string(),
            receiver_id: to.to_string(),
            message_type: message_type.to_string(),
            ..Default::default()
        };

        match self
            .actor_service
            .send_and_wait(&ctx, to, msg, Some(timeout))
            .await
        {
            Ok(reply) => {
                debug!(
                    request_id = %request_id,
                    reply_id = %reply.id,
                    sender_id = %from,
                    recipient_id = %to,
                    reply_sender = %reply.sender_id,
                    reply_receiver = %reply.receiver_id,
                    reply_len = reply.payload.len(),
                    "WASM ask: reply received"
                );
                Ok(reply.payload)
            }
            Err(e) => {
                warn!(
                    request_id = %request_id,
                    sender_id = %from,
                    recipient_id = %to,
                    error = %e,
                    "WASM ask: failed"
                );
                Err(format!("Ask failed: {}", e))
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
        _durable: bool,
    ) -> Result<String, String> {
        let _labels_map: std::collections::HashMap<String, String> = labels.into_iter().collect();

        // For now, use module_ref as actor_type
        // TODO: Resolve module_ref to get actual actor_type
        let actor_type = module_ref.to_string();

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;
        let spawned_id = self
            .actor_service
            .spawn_actor(
                &ctx,
                &actor_id.unwrap_or_else(|| ulid::Ulid::new().to_string()),
                &actor_type,
                initial_state,
            )
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;

        Ok(spawned_id.id().to_string())
    }

    #[instrument(skip(self), fields(from = %from, actor_id = %actor_id))]
    async fn stop_actor(&self, from: &str, actor_id: &str, _timeout_ms: u64) -> Result<(), String> {
        use plexspaces_core::{ActorFactory, ActorId};

        let actor_factory: Arc<dyn ActorFactory> =
            self.service_locator
                .get_actor_factory()
                .await
                .ok_or_else(|| "ActorFactory not found in ServiceLocator".to_string())?;

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        let actor_id_typed = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        actor_factory
            .stop_actor(&ctx, &actor_id_typed)
            .await
            .map_err(|e| format!("Failed to stop actor: {}", e))?;
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

        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

        let actor1_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        let actor2_id = ActorId::from_canonical(linked_actor_id)
            .map_err(|e| format!("Invalid canonical linked actor ID '{linked_actor_id}': {e}"))?;

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

        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

        let actor1_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        let actor2_id = ActorId::from_canonical(linked_actor_id)
            .map_err(|e| format!("Invalid canonical linked actor ID '{linked_actor_id}': {e}"))?;

        actor_registry
            .unlink(&actor1_id, &actor2_id)
            .await
            .map_err(|e| format!("Failed to unlink actors: {}", e))?;

        Ok(())
    }

    async fn monitor_actor(&self, from: &str, actor_id: &str) -> Result<u64, String> {
        use plexspaces_core::ActorId;
        use plexspaces_core::ActorRegistry;

        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        let target_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        let monitor_id = ActorId::from_canonical(from)
            .map_err(|e| format!("Invalid canonical monitor actor ID '{from}': {e}"))?;
        let monitor_ref = ulid::Ulid::new().to_string();

        // Register the monitor. When the target terminates, ActorRegistry delivers a
        // __DOWN__ message directly to the monitoring actor's mailbox — no separate channel needed.
        actor_registry
            .monitor(&target_id, &monitor_id, monitor_ref.clone(), &ctx)
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
                return Err(format!(
                    "Monitor reference {} does not match target actor {}",
                    monitor_ref, actor_id
                ));
            }

            // Remove from mapping
            {
                let mut monitor_refs = self.monitor_refs.write().await;
                monitor_refs.remove(&monitor_ref);
            }

            let actor_registry: Arc<ActorRegistry> = self
                .service_locator
                .actor_registry()
                .await
                .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

            let target_id = ActorId::from_canonical(actor_id)
                .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
            let monitor_id = ActorId::from_canonical(from)
                .map_err(|e| format!("Invalid canonical monitor actor ID '{from}': {e}"))?;

            actor_registry
                .demonitor(&target_id, &monitor_id, &monitor_ref_string)
                .await
                .map_err(|e| format!("Failed to demonitor actor: {}", e))?;

            Ok(())
        } else {
            Err(format!("Monitor reference {} not found", monitor_ref))
        }
    }

    async fn create_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, String> {
        self.actor_service
            .create_shard_group(ctx, req)
            .await
            .map_err(|e| format!("CreateShardGroup failed: {}", e))
    }

    async fn bulk_update_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, String> {
        self.actor_service
            .bulk_update_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BulkUpdateShardGroup failed: {}", e))
    }

    async fn scatter_gather(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, String> {
        self.actor_service
            .scatter_gather(ctx, req)
            .await
            .map_err(|e| format!("ScatterGather failed: {}", e))
    }

    async fn broadcast_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, String> {
        self.actor_service
            .broadcast_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BroadcastShardGroup failed: {}", e))
    }

    async fn reduce_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, String> {
        self.actor_service
            .reduce_shard_group(ctx, req)
            .await
            .map_err(|e| format!("ReduceShardGroup failed: {}", e))
    }

    async fn all_reduce_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, String> {
        self.actor_service
            .all_reduce_shard_group(ctx, req)
            .await
            .map_err(|e| format!("AllReduceShardGroup failed: {}", e))
    }

    async fn barrier_shard_group(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, String> {
        self.actor_service
            .barrier_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BarrierShardGroup failed: {}", e))
    }

    async fn spawn_actors(
        &self,
        ctx: &plexspaces_core::RequestContext,
        req: SpawnActorsRequest,
    ) -> Result<SpawnActorsResponse, String> {
        self.actor_service
            .spawn_actors(ctx, req)
            .await
            .map_err(|e| format!("SpawnActors failed: {}", e))
    }

    async fn merge_application_metrics(
        &self,
        _ctx: &plexspaces_core::RequestContext,
        application_id: &str,
        metrics: ApplicationMetrics,
    ) -> Result<ApplicationMetrics, String> {
        let manager = self
            .service_locator
            .application_manager()
            .await
            .ok_or_else(|| "ApplicationManager not found in ServiceLocator".to_string())?;
        manager
            .merge_application_metrics(application_id, metrics)
            .await?;
        manager
            .get_application_metrics(application_id)
            .await
            .ok_or_else(|| format!("Application '{}' metrics not found", application_id))
    }

    async fn get_application_metrics(
        &self,
        _ctx: &plexspaces_core::RequestContext,
        application_id: &str,
        target_node: &str,
    ) -> Result<ApplicationMetrics, String> {
        let local_node_id = self.local_node_id().await?;
        let resolved = self.resolve_node_endpoint(target_node).await.ok();
        let target_is_address = Self::looks_like_node_address(target_node);
        let (resolved_node_id, resolved_node_address) = resolved.clone().unwrap_or_else(|| {
            (
                target_node.to_string(),
                Self::normalize_node_address(target_node),
            )
        });

        let extract_metrics = |info: ApplicationInfo| {
            info.metrics
                .ok_or_else(|| format!("Application '{}' metrics not found", application_id))
        };

        if resolved_node_id == local_node_id {
            let manager = self
                .service_locator
                .application_manager()
                .await
                .ok_or_else(|| "ApplicationManager not found in ServiceLocator".to_string())?;
            return manager
                .get_application_metrics(application_id)
                .await
                .ok_or_else(|| format!("Application '{}' metrics not found", application_id));
        }

        if let Ok((info, response_node_id, _response_node_address)) = self
            .fetch_remote_application_status(application_id, &resolved_node_address)
            .await
        {
            if target_is_address || response_node_id == target_node {
                return extract_metrics(info);
            }
        }

        for (candidate_id, candidate_address) in self.known_node_endpoints().await? {
            let result = self
                .fetch_remote_application_status(application_id, &candidate_address)
                .await;
            if let Ok((info, response_node_id, response_node_address)) = result {
                if target_is_address
                    || response_node_id == target_node
                    || candidate_id == target_node
                    || Self::canonical_node_address_key(&response_node_address)
                        == Self::canonical_node_address_key(target_node)
                {
                    return extract_metrics(info);
                }
            }
        }

        Err(format!("Node not found: {}", target_node))
    }

    async fn get_application_status(
        &self,
        _ctx: &plexspaces_core::RequestContext,
        application_id: &str,
        target_node: &str,
    ) -> Result<(ApplicationInfo, String), String> {
        let local_node_id = self.local_node_id().await?;
        let resolved = self.resolve_node_endpoint(target_node).await.ok();
        let target_is_address = Self::looks_like_node_address(target_node);
        let (resolved_node_id, resolved_node_address) = resolved.clone().unwrap_or_else(|| {
            (
                target_node.to_string(),
                Self::normalize_node_address(target_node),
            )
        });

        if resolved_node_id == local_node_id {
            let manager = self
                .service_locator
                .application_manager()
                .await
                .ok_or_else(|| "ApplicationManager not found in ServiceLocator".to_string())?;
            let info = manager
                .get_application_info(application_id)
                .await
                .ok_or_else(|| format!("Application '{}' not found", application_id))?;
            return Ok((info, resolved_node_address));
        }

        if let Ok((info, response_node_id, response_node_address)) = self
            .fetch_remote_application_status(application_id, &resolved_node_address)
            .await
        {
            if target_is_address || response_node_id == target_node {
                return Ok((info, response_node_address));
            }
        }

        for (candidate_id, candidate_address) in self.known_node_endpoints().await? {
            let result = self
                .fetch_remote_application_status(application_id, &candidate_address)
                .await;
            if let Ok((info, response_node_id, response_node_address)) = result {
                if target_is_address
                    || response_node_id == target_node
                    || candidate_id == target_node
                    || Self::canonical_node_address_key(&response_node_address)
                        == Self::canonical_node_address_key(target_node)
                {
                    return Ok((info, response_node_address));
                }
            }
        }

        Err(format!("Node not found: {}", target_node))
    }
}

#[cfg(test)]
mod tests {
    use super::ActorServiceMessageSender;
    use async_trait::async_trait;
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_core::{ActorId, ActorRegistry, ActorService as ActorServiceTrait};
    use plexspaces_proto::common::v1::Message;
    use plexspaces_proto::node::v1::NodeRegistration;
    use plexspaces_proto::object_registry::v1::ObjectRegistration;
    use std::sync::Arc;
    use std::sync::RwLock;

    struct RecordingActorService {
        reply_payload: Vec<u8>,
    }

    struct NoopObjectRegistry;

    #[async_trait]
    impl ObjectRegistryTrait for NoopObjectRegistry {
        async fn lookup(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_id: &str,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn lookup_full(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_type: plexspaces_proto::object_registry::v1::ObjectType,
            _object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }

        async fn register(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn discover(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            _object_category: Option<String>,
            _capabilities: Option<Vec<String>>,
            _labels: Option<Vec<String>>,
            _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            _offset: usize,
            _limit: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }

        async fn unregister(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_type: plexspaces_proto::object_registry::v1::ObjectType,
            _object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn heartbeat(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_type: plexspaces_proto::object_registry::v1::ObjectType,
            _object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    struct TestFrameworkSender {
        actor_id: ActorId,
        tenant_id: String,
        namespace: String,
        actor_type: RwLock<Option<String>>,
        behavior_kind: RwLock<Option<String>>,
        local_state_handle: RwLock<Option<Arc<dyn plexspaces_core::ActorStateHandle>>>,
    }

    impl TestFrameworkSender {
        fn new(
            actor_id: ActorId,
            tenant_id: impl Into<String>,
            namespace: impl Into<String>,
        ) -> Self {
            Self {
                actor_id,
                tenant_id: tenant_id.into(),
                namespace: namespace.into(),
                actor_type: RwLock::new(None),
                behavior_kind: RwLock::new(None),
                local_state_handle: RwLock::new(None),
            }
        }
    }

    #[async_trait]
    impl plexspaces_core::MessageSender for TestFrameworkSender {
        async fn tell(
            &self,
            _message: Message,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn actor_id(&self) -> Option<String> {
            Some(self.actor_id.to_string())
        }

        fn tenant_id(&self) -> Option<&str> {
            Some(&self.tenant_id)
        }

        fn namespace(&self) -> Option<&str> {
            Some(&self.namespace)
        }

        fn actor_type(&self) -> Option<String> {
            self.actor_type.read().ok().and_then(|guard| guard.clone())
        }

        async fn set_actor_type(&self, actor_type: Option<String>) {
            if let Ok(mut guard) = self.actor_type.write() {
                *guard = actor_type;
            }
        }

        fn behavior_kind(&self) -> Option<String> {
            self.behavior_kind
                .read()
                .ok()
                .and_then(|guard| guard.clone())
        }

        async fn set_behavior_kind(&self, behavior_kind: Option<String>) {
            if let Ok(mut guard) = self.behavior_kind.write() {
                *guard = behavior_kind;
            }
        }

        fn local_state_handle(&self) -> Option<Arc<dyn plexspaces_core::ActorStateHandle>> {
            self.local_state_handle
                .read()
                .ok()
                .and_then(|guard| guard.clone())
        }

        async fn set_local_state_handle(
            &self,
            handle: Option<Arc<dyn plexspaces_core::ActorStateHandle>>,
        ) {
            if let Ok(mut guard) = self.local_state_handle.write() {
                *guard = handle;
            }
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[async_trait]
    impl ActorServiceTrait for RecordingActorService {
        async fn spawn_actor(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _actor_id: &str,
            _actor_type: &str,
            _initial_state: Vec<u8>,
        ) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("not needed for this test".into())
        }

        async fn send(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _actor_id: &str,
            _message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Err("not needed for this test".into())
        }

        async fn send_and_wait(
            &self,
            ctx: &plexspaces_core::RequestContext,
            actor_id: &str,
            message: Message,
            _timeout: Option<std::time::Duration>,
        ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
            assert_eq!(ctx.tenant_id(), "tenant-a");
            assert_eq!(ctx.namespace(), "heat-diffusion-rust");
            assert_eq!(
                actor_id,
                "heat-diffusion-1//worker::heat-diffusion-rust@test-node-8093"
            );
            assert_eq!(
                message.sender_id,
                "leader//gen_server::heat-diffusion-rust@test-node-8091"
            );
            assert_eq!(message.message_type, "init");
            Ok(Message {
                id: "res-1".to_string(),
                sender_id: actor_id.to_string(),
                receiver_id: message.sender_id,
                payload: self.reply_payload.clone(),
                ..Default::default()
            })
        }
    }

    #[test]
    fn matching_node_registration_supports_node_id_and_address() {
        let registrations = vec![
            NodeRegistration {
                node_id: "test-node-8093".to_string(),
                node_address: "http://0.0.0.0:8093".to_string(),
                ..Default::default()
            },
            NodeRegistration {
                node_id: "test-node-8091".to_string(),
                node_address: "http://localhost:8091".to_string(),
                ..Default::default()
            },
        ];

        let by_id =
            ActorServiceMessageSender::matching_node_registration("test-node-8093", &registrations)
                .expect("node id should resolve");
        assert_eq!(by_id.node_address, "http://0.0.0.0:8093");

        let by_address = ActorServiceMessageSender::matching_node_registration(
            "http://localhost:8093",
            &registrations,
        )
        .expect("canonical address should resolve");
        assert_eq!(by_address.node_id, "test-node-8093");
    }

    #[test]
    fn normalize_node_address_returns_dialable_loopback_endpoint() {
        assert_eq!(
            ActorServiceMessageSender::normalize_node_address("http://0.0.0.0:8093"),
            "http://localhost:8093"
        );
        assert_eq!(
            ActorServiceMessageSender::normalize_node_address("127.0.0.1:8094"),
            "http://localhost:8094"
        );
    }

    #[tokio::test]
    async fn ask_routes_via_actor_service_for_remote_actor_ids() {
        let actor_service = Arc::new(RecordingActorService {
            reply_payload: br#"{"status":"ok"}"#.to_vec(),
        });
        let service_locator = Arc::new(plexspaces_services::ServiceLocatorImpl::new());
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(NoopObjectRegistry);
        let actor_registry = Arc::new(ActorRegistry::new(
            object_registry,
            "test-node-8091".to_string(),
        ));
        service_locator
            .register_actor_registry(actor_registry.clone())
            .await;

        let sender_id =
            ActorId::new("leader", "gen_server", "heat-diffusion-rust", "test-node-8091")
                .expect("sender actor id");
        let sender_ctx = plexspaces_core::RequestContext::new_without_auth(
            "tenant-a".to_string(),
            "heat-diffusion-rust".to_string(),
        );
        let sender_ref: Arc<dyn plexspaces_core::MessageSender> =
            Arc::new(TestFrameworkSender::new(
                sender_id.clone(),
                sender_ctx.tenant_id().to_string(),
                sender_ctx.namespace().to_string(),
            ));
        actor_registry
            .register_actor(
                &sender_ctx,
                sender_id.clone(),
                sender_ref,
                "gen_server".to_string(),
                None,
                None,
                None,
            )
            .await;

        let sender = ActorServiceMessageSender::new(actor_service, service_locator);

        let reply = plexspaces_wasm_runtime::MessageSender::ask(
            &sender,
            &sender_id.to_string(),
            "heat-diffusion-1//worker::heat-diffusion-rust@test-node-8093",
            "init",
            br#"{"region":0}"#.to_vec(),
            2500,
        )
        .await
        .expect("remote ask should route via ActorService");

        assert_eq!(reply, br#"{"status":"ok"}"#.to_vec());
    }
}
