// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM MessageSender implementation
//!
//! Bridges WASM host function calls to the actor framework.
//! Implements [`plexspaces_wasm_runtime::MessageSender`] using:
//! - `ActorRegistry` for `tell` and `ask` so passivated virtual actors are reactivated (`tell`
//!   uses the sender actor’s registered tenant/namespace as `RequestContext`)
//! - ActorFactory for stop_actor with tenant isolation
//!
//! Actor IDs follow the canonical `{name}//{actor_type}::{namespace}@{node_id}` format (e.g.,
//! `parameter-server:ray-ps@test-node`). The actor registry stores and looks
//! up actors by their full ID — callers must always provide fully-qualified
//! IDs. The WASM host bridge passes IDs through unchanged.

use async_trait::async_trait;
use plexspaces_actor::ActorService;
use plexspaces_common::dialable_node_address;
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
    service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
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
        service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
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
    /// [`Self::stop_actor`]). Used so monitor registration stores a real [`plexspaces_actor::RequestContext`]
    /// for cross-node `__DOWN__` routing instead of ad hoc parallel fields.
    async fn request_context_from_registered_sender_actor(
        &self,
        from: &str,
    ) -> Result<plexspaces_actor::RequestContext, String> {
        use plexspaces_actor::{ActorId, RequestContext, RequestContextExt};

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
                request_id: ulid::Ulid::new().to_string(),
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
        use plexspaces_actor::ActorRegistry;

        trace!(from = %from, to = %to, message_type = %message_type, "WASM send_message (tell)");

        // Route through ActorRegistry so virtual actors are reactivated if passivated.
        // This is the canonical tell path — do NOT use actor_service.send() directly here,
        // as that bypasses virtual actor activation owned by the registry.
        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;
        // resolve_actor_id handles canonical, "type:name", and bare-type addressing.
        let to_id = registry.resolve_actor_id(&ctx, to).await?;
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: message.to_vec(),
            sender_id: from.to_string(),
            receiver_id: to_id.to_string(),
            message_type: message_type.to_string(),
            ..Default::default()
        };
        registry
            .tell(&ctx, &to_id, msg)
            .await
            .map_err(|e| e.to_string())
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
            "WASM ask: sending request via ActorRegistry"
        );

        // Route through ActorRegistry::ask so virtual actors are reactivated if passivated.
        // The registry owns virtual actor activation — do NOT bypass it via actor_service.
        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        use plexspaces_actor::ActorRegistry;

        let registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        // resolve_actor_id handles canonical, "type:name", and bare-type addressing.
        let to_id = registry.resolve_actor_id(&ctx, to).await?;

        let msg = Message {
            id: request_id.clone(),
            payload,
            sender_id: from.to_string(),
            receiver_id: to_id.to_string(),
            message_type: message_type.to_string(),
            ..Default::default()
        };

        match registry.ask(&ctx, &to_id, msg, timeout).await {
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
        role: String,
        args: Vec<(String, String)>,
        actor_name: Option<String>,
    ) -> Result<String, String> {
        let actor_type = module_ref.to_string();
        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;
        let args_map: std::collections::HashMap<String, String> = args.into_iter().collect();
        use plexspaces_proto::actor::v1::{ActorSpawnSpec, ActorVisibility};
        use plexspaces_proto::common::v1::ActorIdentity;
        let spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: actor_name.unwrap_or_else(|| ulid::Ulid::new().to_string()),
                actor_type,
            }),
            role,
            namespace: String::new(),
            tenant_id: String::new(),
            visibility: ActorVisibility::ActorVisibilityPublic as i32,
            behavior_kind: String::new(),
            args: args_map,
            facets: vec![],
            config: None,
            labels: std::collections::HashMap::new(),
            ..Default::default()
        };
        let spawned_id = self
            .actor_service
            .spawn_actor(&ctx, &spec)
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;

        Ok(spawned_id.id().to_string())
    }

    #[instrument(skip(self), fields(from = %from, actor_id = %actor_id))]
    async fn stop_actor(&self, from: &str, actor_id: &str, _timeout_ms: u64) -> Result<(), String> {
        use plexspaces_actor::{ActorFactory, ActorId};

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
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        use plexspaces_actor::ActorRegistry;

        use plexspaces_actor::ActorId;

        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

        let actor1_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        let actor2_id = ActorId::from_canonical(linked_actor_id)
            .map_err(|e| format!("Invalid canonical linked actor ID '{linked_actor_id}': {e}"))?;

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        actor_registry
            .link(&ctx, &actor1_id, &actor2_id)
            .await
            .map_err(|e| format!("Failed to link actors: {}", e))?;

        Ok(())
    }

    async fn unlink_actor(
        &self,
        from: &str,
        actor_id: &str,
        linked_actor_id: &str,
    ) -> Result<(), String> {
        use plexspaces_actor::ActorRegistry;

        use plexspaces_actor::ActorId;

        let actor_registry: Arc<ActorRegistry> = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;

        let actor1_id = ActorId::from_canonical(actor_id)
            .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
        let actor2_id = ActorId::from_canonical(linked_actor_id)
            .map_err(|e| format!("Invalid canonical linked actor ID '{linked_actor_id}': {e}"))?;

        let ctx = self
            .request_context_from_registered_sender_actor(from)
            .await?;

        actor_registry
            .unlink(&ctx, &actor1_id, &actor2_id)
            .await
            .map_err(|e| format!("Failed to unlink actors: {}", e))?;

        Ok(())
    }

    async fn monitor_actor(&self, from: &str, actor_id: &str) -> Result<u64, String> {
        use plexspaces_actor::ActorId;
        use plexspaces_actor::ActorRegistry;

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

        let monitor_ref = actor_registry
            .monitor(&ctx, &target_id, &monitor_id)
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
        use plexspaces_actor::ActorRegistry;

        use plexspaces_actor::ActorId;

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

            let ctx = self
                .request_context_from_registered_sender_actor(from)
                .await?;

            let target_id = ActorId::from_canonical(actor_id)
                .map_err(|e| format!("Invalid canonical actor ID '{actor_id}': {e}"))?;
            let monitor_id = ActorId::from_canonical(from)
                .map_err(|e| format!("Invalid canonical monitor actor ID '{from}': {e}"))?;

            actor_registry
                .demonitor(&ctx, &target_id, &monitor_id, &monitor_ref_string)
                .await
                .map_err(|e| format!("Failed to demonitor actor: {}", e))?;

            Ok(())
        } else {
            Err(format!("Monitor reference {} not found", monitor_ref))
        }
    }

    async fn create_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, String> {
        self.actor_service
            .create_shard_group(ctx, req)
            .await
            .map_err(|e| format!("CreateShardGroup failed: {}", e))
    }

    async fn bulk_update_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, String> {
        self.actor_service
            .bulk_update_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BulkUpdateShardGroup failed: {}", e))
    }

    async fn scatter_gather(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, String> {
        self.actor_service
            .scatter_gather(ctx, req)
            .await
            .map_err(|e| format!("ScatterGather failed: {}", e))
    }

    async fn broadcast_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, String> {
        self.actor_service
            .broadcast_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BroadcastShardGroup failed: {}", e))
    }

    async fn reduce_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, String> {
        self.actor_service
            .reduce_shard_group(ctx, req)
            .await
            .map_err(|e| format!("ReduceShardGroup failed: {}", e))
    }

    async fn all_reduce_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, String> {
        self.actor_service
            .all_reduce_shard_group(ctx, req)
            .await
            .map_err(|e| format!("AllReduceShardGroup failed: {}", e))
    }

    async fn barrier_shard_group(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, String> {
        self.actor_service
            .barrier_shard_group(ctx, req)
            .await
            .map_err(|e| format!("BarrierShardGroup failed: {}", e))
    }

    async fn spawn_actors(
        &self,
        ctx: &plexspaces_actor::RequestContext,
        req: SpawnActorsRequest,
    ) -> Result<SpawnActorsResponse, String> {
        self.actor_service
            .spawn_actors(ctx, req)
            .await
            .map_err(|e| format!("SpawnActors failed: {}", e))
    }

    async fn merge_application_metrics(
        &self,
        _ctx: &plexspaces_actor::RequestContext,
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
        _ctx: &plexspaces_actor::RequestContext,
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
        _ctx: &plexspaces_actor::RequestContext,
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
    use plexspaces_actor::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_actor::{ActorId, ActorRegistry, ActorService as ActorServiceTrait};
    use plexspaces_common::RequestContextExt;
    use plexspaces_proto::common::v1::Message;
    use plexspaces_proto::node::v1::NodeRegistration;
    use plexspaces_proto::object_registry::v1::ObjectRegistration;
    use std::sync::Arc;

    struct RecordingActorService {
        reply_payload: Vec<u8>,
        sent_messages: Arc<tokio::sync::Mutex<Vec<(String, String)>>>,
    }

    #[async_trait]
    impl ActorServiceTrait for RecordingActorService {
        async fn spawn_actor(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            _spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
        ) -> Result<plexspaces_actor::ServiceTraitsActorRef, Box<dyn std::error::Error + Send + Sync>>
        {
            Err("not needed for this test".into())
        }

        async fn send(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            actor_id: &str,
            message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            let mut sent = self.sent_messages.lock().await;
            sent.push((actor_id.to_string(), message.message_type.clone()));
            Ok("ok".to_string())
        }

        async fn send_and_wait(
            &self,
            _ctx: &plexspaces_actor::RequestContext,
            actor_id: &str,
            _message: Message,
            _timeout: Option<std::time::Duration>,
        ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Message {
                id: "res-1".to_string(),
                sender_id: actor_id.to_string(),
                payload: self.reply_payload.clone(),
                ..Default::default()
            })
        }
    }

    #[test]
    fn node_endpoint_helpers_normalize_and_match_registrations() {
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
        assert_eq!(
            ActorServiceMessageSender::normalize_node_address("http://0.0.0.0:8093"),
            "http://localhost:8093"
        );
        assert_eq!(
            ActorServiceMessageSender::normalize_node_address("127.0.0.1:8094"),
            "http://localhost:8094"
        );
    }

    /// send_message must route through ActorRegistry::tell so that passivated virtual
    /// actors are reactivated. A passivated sender still needs scoped registry metadata
    /// so the host bridge can rebuild the RequestContext used for routing.
    #[tokio::test]
    async fn send_message_routes_via_registry_for_passivated_virtual_sender() {
        let sent = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let actor_service = Arc::new(RecordingActorService {
            reply_payload: vec![],
            sent_messages: sent.clone(),
        });
        let service_locator = Arc::new(plexspaces_services::ServiceLocatorImpl::new());
        let sender_id =
            ActorId::from_canonical("health_monitor//miniclaw_wasm::go-miniclaw@test-node-8091")
                .expect("sender actor id should be valid");
        let sender_ctx = plexspaces_actor::RequestContext::new_without_auth(
            "tenant-go".to_string(),
            sender_id.namespace().to_string(),
        );
        // ActorRegistry with local node test-node-8091. The target is on test-node-8093
        // (remote), so tell routes through actor_service.send.
        let actor_registry = Arc::new(ActorRegistry::new("test-node-8091".to_string()));
        let virtual_actor_manager = Arc::new(plexspaces_actor::VirtualActorManager::new(
            actor_registry.clone(),
        ));
        actor_registry
            .set_actor_service(actor_service.clone())
            .await;
        actor_registry
            .set_virtual_actor_manager(virtual_actor_manager.clone())
            .await;
        service_locator
            .register_actor_registry(actor_registry.clone())
            .await;
        virtual_actor_manager
            .register_virtual_actor_type(
                sender_id.actor_type().to_string(),
                None,
                sender_ctx.namespace().to_string(),
                serde_json::json!({
                    "virtual_actor": {
                        "activation_strategy": "lazy"
                    }
                }),
                Some(sender_ctx.tenant_id().to_string()),
                None,
            )
            .await
            .expect("virtual actor type registration should succeed");

        let wasm_sender = ActorServiceMessageSender::new(actor_service, service_locator);

        // The sender has only passivated virtual-actor metadata, not a live sender.
        let result = plexspaces_wasm_runtime::MessageSender::send_message(
            &wasm_sender,
            &sender_id.to_string(),
            "worker//miniclaw_wasm::go-miniclaw@test-node-8093",
            "tick",
            b"{}",
        )
        .await;

        assert!(result.is_ok(), "send_message failed: {:?}", result);
        let msgs = sent.lock().await;
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].1, "tick");
    }
}
