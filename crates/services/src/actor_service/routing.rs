// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Actor ID resolution and message routing logic for ActorServiceImpl.

use std::sync::Arc;
use std::time::Duration;
use tonic::Status;

use super::ActorServiceImpl;
use plexspaces_actor::{
    ActorId, ActorRegistry, RequestContext, RequestContextExt,
    ServiceLocator as ServiceLocatorTrait,
};
use plexspaces_proto::actor::v1::{AskReplyRequest, SendMessageRequest};
use plexspaces_proto::common::v1::Message;

impl ActorServiceImpl {
    /// Canonicalize receiver_id at the service boundary so actors always observe the
    /// resolved framework-owned actor id rather than a client alias such as `instance:type`.
    pub(crate) fn set_message_receiver_id(message: &mut Message, resolved_actor_id: &str) {
        message.receiver_id = resolved_actor_id.to_string();
    }

    /// Resolve a canonical actor id for bare-type virtual actor activation.
    ///
    /// Called only when no live instance was found via `resolve_actor_target` and the requested_actor_type
    /// is a bare type string (no colon). Generates a ULID instance name and primes the virtual actor.
    /// The `instance_name:actor_type` short form is handled upstream in `canonical_actor_id_from_client_target`.
    pub(crate) async fn resolve_target_actor_id_for_type_lookup(
        &self,
        ctx: &RequestContext,
        requested_actor_type: &str,
        discovered_actor_ids: &[ActorId],
    ) -> Result<String, Status> {
        let virtual_actor_manager = self
            .service_locator
            .virtual_actor_manager()
            .await
            .ok_or_else(|| Status::internal("VirtualActorManager not found in ServiceLocator"))?;

        for actor_id in discovered_actor_ids {
            if virtual_actor_manager.is_virtual(actor_id).await {
                return Ok(actor_id.to_string());
            }
        }

        // This function is called only with a bare actor_type (no colon) from resolve_actor_target.
        // The `instance:type` short form is handled upstream in canonical_actor_id_from_client_target.
        // Only bare type strings reach here; reject colon-format to enforce the single convention.
        if requested_actor_type.contains(':') {
            return Err(Status::invalid_argument(format!(
                "Actor address '{}' must use 'instance_name:actor_type' format or bare actor type. \
                 Did you mean '{}' with a specific instance name?",
                requested_actor_type,
                requested_actor_type
                    .split_once(':')
                    .map(|(a, b)| format!("{}:{}", b, a))
                    .unwrap_or_else(|| requested_actor_type.to_string()),
            )));
        }

        let actor_type = requested_actor_type;

        // Generate a ULID instance name for anonymous (bare-type) activation.
        let actor_name = ulid::Ulid::new().to_string();

        // Try registered virtual type first (most common hot path).
        if let Some(type_metadata) = virtual_actor_manager.get_virtual_actor_type(actor_type).await {
            let target_actor_id = self.build_canonical_actor_id(
                &actor_name,
                actor_type,
                type_metadata.namespace(),
                &self.local_node_id,
            )?;
            virtual_actor_manager
                .prime_instance_from_definition(&target_actor_id, &type_metadata)
                .await;
            return Ok(target_actor_id.to_string());
        }

        // Fallback: look up actor_type as a named virtual actor definition (e.g.
        // a WASM app registers a definition named "audit-log-test" whose real actor_type
        // is "audit_log_test_wasm").  The bare name is used as both the instance name and
        // the lookup key, producing a stable singleton-style canonical ID.
        let namespace = ctx.namespace().to_string();
        if let Some(def) = virtual_actor_manager.get_virtual_actor_definition(&namespace, actor_type).await {
            let real_actor_type = def.spec.identity.as_ref().map(|id| id.actor_type.as_str()).unwrap_or(actor_type);
            let target_actor_id = self.build_canonical_actor_id(
                actor_type,
                real_actor_type,
                &namespace,
                &self.local_node_id,
            )?;
            virtual_actor_manager
                .prime_instance_from_definition(&target_actor_id, &def)
                .await;
            return Ok(target_actor_id.to_string());
        }

        Err(Status::not_found(format!(
            "No actors found for type '{}' in tenant '{}', namespace '{}'",
            actor_type,
            ctx.tenant_id(),
            ctx.namespace()
        )))
    }

    /// Resolves a canonical actor ID from a client-supplied target string.
    ///
    /// # Supported address formats
    ///
    /// | Format | Example | Behavior |
    /// |--------|---------|----------|
    /// | `instance:type` | `vu0:gen_server` | O(1) registry lookup; if miss + type is virtual, prime on-demand |
    /// | bare `type` | `gen_server` | Pick a random live instance of that type (load-balancing) |
    /// | canonical `//` | `vu0//gen_server::ns@node` | Returned as-is; reactivates virtual if definition exists |
    ///
    /// Any other format (e.g. reversed `type:instance`) is rejected and returns `None`.
    ///
    /// # Why `instance:type` and not `type:instance`
    ///
    /// `build_canonical_actor_id(name, actor_type)` takes name first. The short form mirrors this
    /// order so the hot path (Step 1 below) is a single O(1) HashMap lookup with no ambiguity.
    pub async fn canonical_actor_id_from_client_target(
        &self,
        ctx: &RequestContext,
        requested_actor_type: &str,
    ) -> Option<String> {
        if requested_actor_type.contains("//") {
            // Canonical actor ID passed directly — prime instance from named definition
            // so reactivation after explicit stop re-derives init from declaration args.
            if let Ok(actor_id) = plexspaces_actor::ActorId::from_canonical(requested_actor_type) {
                if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                    let name = actor_id.name();
                    if let Some(def) = manager
                        .get_virtual_actor_definition(actor_id.namespace(), name)
                        .await
                    {
                        manager
                            .prime_instance_from_definition(&actor_id, &def)
                            .await;
                    }
                }
            }
            return Some(requested_actor_type.to_string());
        }

        if !requested_actor_type.contains(':') {
            // Bare type — delegate to resolve_actor_target for random-instance selection.
            return self
                .resolve_actor_target(ctx, requested_actor_type)
                .await
                .ok();
        }

        let (instance_name, actor_type) = requested_actor_type.split_once(':')?;
        if instance_name.is_empty() || actor_type.is_empty() {
            return None;
        }

        let namespace = ctx.namespace().to_string();

        // Step 1: O(1) — direct registry lookup for live actors.
        if let Ok(candidate_id) =
            self.build_canonical_actor_id(instance_name, actor_type, &namespace, &self.local_node_id)
        {
            if let Some(registry) = self.service_locator.actor_registry().await {
                if registry
                    .lookup_actor_in_scope(ctx.tenant_id(), &namespace, &candidate_id)
                    .await
                    .is_some()
                {
                    return Some(candidate_id.to_string());
                }
            }

            // Step 2: actor not live in actors map — check virtual manager and type registrations.
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                // Step 2a: actor already registered in the instance-level index (e.g. lazy spawn via
                // node.spawn() with VirtualActorFacet). Not yet running but the spec is already stored
                // in the virtual_actors map — activation happens inside ActorRegistry::tell/ask.
                if manager.get_metadata(&candidate_id).await.is_some() {
                    return Some(candidate_id.to_string());
                }

                // Step 2b: registered virtual type — prime the instance spec on-demand so the factory
                // has a per-instance entry with refreshed init args before the first activation.
                if let Some(type_meta) = manager.get_virtual_actor_type(actor_type).await {
                    manager
                        .prime_instance_from_definition(&candidate_id, &type_meta)
                        .await;
                    return Some(candidate_id.to_string());
                }

                // Step 3a: look up actor_type as a named virtual actor definition slot.
                // Handles WASM apps where multiple named roles share one behavior class:
                // "alerts:channel" → instance_name="alerts", actor_type="channel"
                // "channel" is a named definition whose actor_type provides the real behavior.
                if let Some(def) = manager.get_virtual_actor_definition(&namespace, actor_type).await {
                    let real_actor_type = def.spec.identity.as_ref().map(|id| id.actor_type.as_str()).unwrap_or(actor_type);
                    if let Ok(real_id) = self.build_canonical_actor_id(instance_name, real_actor_type, &namespace, &self.local_node_id) {
                        manager.prime_instance_from_definition(&real_id, &def).await;
                        return Some(real_id.to_string());
                    }
                }

                // Step 3b: look up instance_name as a named virtual actor definition slot.
                // Handles the "definition_name:instance_id" pattern where the left side is the
                // definition name (e.g. "weather") and the right side is the specific instance
                // (e.g. "session-1"): "weather:session-1" → "session-1//weather_actor_wasm::ns@node"
                if let Some(def) = manager.get_virtual_actor_definition(&namespace, instance_name).await {
                    let real_actor_type = def.spec.identity.as_ref().map(|id| id.actor_type.as_str()).unwrap_or(instance_name);
                    if let Ok(real_id) = self.build_canonical_actor_id(actor_type, real_actor_type, &namespace, &self.local_node_id) {
                        manager.prime_instance_from_definition(&real_id, &def).await;
                        return Some(real_id.to_string());
                    }
                }
            }

            // Step 4: all dynamic lookups missed — caller supplied both instance name and actor
            // type explicitly (e.g. "weather:weather_actor_wasm"), so build the canonical ID
            // directly.  The actor may not be live yet; the caller is responsible for
            // activating it (e.g. via ActorRegistry::tell/ask).
            return Some(candidate_id.to_string());
        }

        None
    }

    pub(crate) async fn canonical_actor_id_for_runtime_target(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
    ) -> Result<String, String> {
        if actor_id.contains("//") {
            return Ok(actor_id.to_string());
        }

        self.canonical_actor_id_from_client_target(ctx, actor_id)
            .await
            .ok_or_else(|| {
                format!(
                    "Failed to resolve actor target '{}' in tenant '{}', namespace '{}'",
                    actor_id,
                    ctx.tenant_id(),
                    ctx.namespace()
                )
            })
    }

    pub(crate) async fn resolve_actor_target(
        &self,
        ctx: &RequestContext,
        requested_actor_type: &str,
    ) -> Result<String, Status> {
        let actor_registry = self.get_actor_registry().await;
        let discovered_actor_ids = actor_registry
            .discover_actors_by_type(ctx, requested_actor_type)
            .await;
        let mut active_actor_ids = Vec::with_capacity(discovered_actor_ids.len());
        for actor_id in &discovered_actor_ids {
            if actor_registry
                .lookup_actor_in_scope(ctx.tenant_id(), ctx.namespace(), actor_id)
                .await
                .is_some()
            {
                active_actor_ids.push(actor_id.clone());
            }
        }

        if !active_actor_ids.is_empty() {
            if active_actor_ids.len() == 1 {
                return Ok(active_actor_ids[0].to_string());
            }
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..active_actor_ids.len());
            return Ok(active_actor_ids[idx].to_string());
        }

        self.resolve_target_actor_id_for_type_lookup(
            ctx,
            requested_actor_type,
            &discovered_actor_ids,
        )
        .await
    }

    pub(crate) async fn route_actor_request(
        &self,
        ctx: RequestContext,
        requested_actor_type: &str,
        mut message: Message,
        wait_for_response: bool,
        timeout: Option<Duration>,
    ) -> Result<(String, String, Option<Message>), Status> {
        // If the target already looks like a canonical ID, skip resolution.
        let requested_target = if requested_actor_type.contains("//") {
            requested_actor_type.to_string()
        } else {
            self.canonical_actor_id_from_client_target(&ctx, requested_actor_type)
                .await
                .ok_or_else(|| {
                    Status::not_found(format!(
                        "Actor '{}' not found in tenant '{}', namespace '{}'",
                        requested_actor_type,
                        ctx.tenant_id(),
                        ctx.namespace()
                    ))
                })?
        };
        Self::set_message_receiver_id(&mut message, &requested_target);

        self.route_message(
            ctx,
            &requested_target,
            message,
            wait_for_response,
            timeout,
        )
        .await
        .map(|(message_id, reply)| (requested_target, message_id, reply))
    }

    /// Send a message to an actor (local or remote).
    ///
    /// All messages — including ask() replies — route through `ActorRegistry::tell()`/`ask()`,
    /// which handles local delivery (virtual actor activation, temporary-sender reply routing
    /// via `PendingAsks` oneshot channels) and gRPC for remote delivery.
    ///
    /// ## Arguments
    /// * `actor_id` - Canonical actor ID in format `name//actor_type::namespace@node_id`
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Message ID if successful
    pub async fn send_message(
        &self,
        actor_id: &str,
        message: Message,
        routing_ctx: Option<&RequestContext>,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}",
                message.id, actor_id, message.sender_id, message.receiver_id, message.message_type, message.correlation_id
            );
        }

        // Route all messages through the unified routing module.
        // `actor_id` MUST be a full canonical ID. gRPC handlers resolve type names to canonical
        // IDs at the transport boundary; WASM actors must supply canonical IDs from PGs or config.
        // Derive namespace from sender's canonical ID so route_message can build a valid
        // temporary-sender ActorId when needed — unless an explicit routing context was supplied
        // (e.g. `__DOWN__` replaying the monitor-establishing scope).
        let ctx = routing_ctx.cloned().unwrap_or_else(|| {
            ActorId::from_canonical(&message.sender_id)
                .map(|id| {
                    RequestContext::new_without_auth(String::new(), id.namespace().to_string())
                })
                .unwrap_or_else(|_| RequestContext::new_without_auth(String::new(), String::new()))
        });

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message] NORMAL ROUTING: message_id={}, actor_id={}, calling route_message",
                message.id, actor_id
            );
        }
        let routed_actor_id = self
            .canonical_actor_id_for_runtime_target(&ctx, actor_id)
            .await
            .map_err(|e| format!("Failed to resolve actor target: {}", e))?;
        let (msg_id, _) = self
            .route_message(ctx, &routed_actor_id, message, false, None)
            .await
            .map_err(|e| format!("Failed to send message: {}", e))?;
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message] COMPLETED: message_id={}, actor_id={}",
                msg_id,
                actor_id
            );
        }
        Ok(msg_id)
    }

    /// Send a message to a canonical actor ID and wait for the reply.
    ///
    /// `actor_id` **must** be a full canonical ID (`name//type::namespace@node`).
    /// gRPC API handlers (`ask_reply`, `send_message`) resolve type names to canonical IDs
    /// at the transport boundary via `route_actor_request`.  WASM actors must always supply
    /// canonical IDs obtained from Process Groups, TupleSpace, or explicit configuration.
    pub async fn send_message_and_wait(
        &self,
        actor_id: &str,
        message: Message,
        timeout: Option<std::time::Duration>,
        ctx: RequestContext,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, timeout={:?}",
                message.id, actor_id, message.sender_id, message.receiver_id, message.message_type, message.correlation_id, timeout
            );
        }

        let routed_actor_id = self
            .canonical_actor_id_for_runtime_target(&ctx, actor_id)
            .await
            .map_err(|e| format!("Failed to resolve actor target: {}", e))?;
        let target_actor_id = self
            .parse_canonical_actor_id(&routed_actor_id)
            .map_err(|e| e.to_string())?;
        let node_id = target_actor_id.node_id().to_string();

        if node_id == self.local_node_id {
            // LOCAL: use ActorRegistry ask() so virtual activation stays inside the registry.
            let registry = self.get_actor_registry().await;
            let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(5));
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL: message_id={}, actor_id={}, calling ActorRegistry::ask()",
                    message.id, target_actor_id
                );
            }
            let result = registry
                .ask(&ctx, &target_actor_id, message, timeout_duration)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    use plexspaces_actor::ActorRegistryError;
                    match e {
                        ActorRegistryError::ActorNotFound(_) => "Actor not found".into(),
                        ActorRegistryError::Timeout => "Request timed out".into(),
                        ActorRegistryError::VisibilityDenied(m) => m.into(),
                        _ => format!("Failed to send ask request: {}", e).into(),
                    }
                });
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL COMPLETED: actor_id={}, result={:?}",
                    target_actor_id, result.is_ok()
                );
            }
            result
        } else {
            // REMOTE: Use route_message (which handles remote routing via gRPC)
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] REMOTE: message_id={}, actor_id={}, calling route_message",
                    message.id, routed_actor_id
                );
            }
            let (_, response) = self
                .route_message(ctx, &routed_actor_id, message, true, timeout)
                .await
                .map_err(|e| format!("Failed to send message and wait: {}", e))?;

            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] REMOTE COMPLETED: actor_id={}, has_response={}",
                    actor_id, response.is_some()
                );
            }
            response.ok_or_else(|| "No response received".into())
        }
    }

    /// Helper function to create ActorRef for local actor
    /// Tries to get mailbox from actor instance and create local ActorRef, falls back to remote if unavailable
    pub(crate) async fn create_actor_ref_for_local_actor(
        ctx: &RequestContext,
        _registry: &Arc<ActorRegistry>,
        actor_id: &ActorId,
        local_node_id: &str,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> plexspaces_actor::ActorRef {
        plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            local_node_id.to_string(),
            service_locator,
            plexspaces_proto::actor::v1::ActorVisibility::ActorVisibilityPublic,
        )
    }

    /// Route message to local or remote actor.
    ///
    /// # Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
    /// * `actor_id` - Target canonical actor ID
    /// * `message` - Message to send
    /// * `wait_for_response` - Whether to wait for reply
    /// * `timeout` - Optional timeout for request-reply
    ///
    /// # Returns
    /// * `Ok(message_id, response)` - Message delivered successfully
    /// * `Err(Status)` - Delivery failed
    ///
    /// ## Note
    /// Made public for use by ActorService trait implementation.
    /// Delegates to unified routing module.
    pub async fn route_message(
        &self,
        ctx: RequestContext,
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        let message_id = message.id.clone();

        let target_id = plexspaces_actor::ActorId::from_canonical(actor_id).map_err(|e| {
            Status::invalid_argument(format!("Invalid actor ID '{}': {}", actor_id, e))
        })?;

        // Local actors: use ActorRegistry (handles virtual activation, temp senders, etc.)
        // Remote actors: use direct gRPC to avoid registry → ActorService → registry recursion.
        let is_remote = target_id.node_id() != self.local_node_id;

        if is_remote {
            let node_id = target_id.node_id().to_string();
            let transport = self
                .service_locator
                .get_actor_transport_client()
                .await
                .ok_or_else(|| Status::internal("ActorTransportClient not available"))?;

            // Decompose canonical ID into name + type + namespace for the gRPC API.
            // The receiving handler reconstructs the canonical ID via actor_name + actor_type + namespace.
            let actor_name = target_id.name().to_string();
            let actor_type_str = target_id.actor_type().to_string();
            let actor_namespace = target_id.namespace().to_string();

            if wait_for_response {
                let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(30));
                let timeout_proto = prost_types::Duration {
                    seconds: timeout_duration.as_secs() as i64,
                    nanos: timeout_duration.subsec_nanos() as i32,
                };
                let ask_req = AskReplyRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    namespace: actor_namespace.clone(),
                    actor_type: actor_type_str.clone(),
                    actor_name: actor_name.clone(),
                    payload: message.payload,
                    headers: message.headers,
                    sender_id: message.sender_id,
                    message_type: message.message_type,
                    correlation_id: message.correlation_id,
                    reply_to: message.reply_to,
                    message_id: message.id,
                    timeout: Some(timeout_proto),
                    // Must be POST so the ask_reply handler passes payload through unchanged.
                    // The default "" is treated as GET which would replace the payload with
                    // serialized query_params (empty), discarding the actual message payload.
                    http_method: "POST".to_string(),
                    ..Default::default()
                };
                let mut req = tonic::Request::new(ask_req);
                plexspaces_actor::apply_request_context_to_grpc_metadata(&ctx, req.metadata_mut());
                let resp = transport
                    .ask_reply(&node_id, req)
                    .await
                    .map_err(|e| {
                        Status::internal(format!("Remote ask to '{}' failed: {}", node_id, e))
                    })?
                    .into_inner();
                if !resp.success {
                    return Err(Status::internal(format!(
                        "Remote ask failed: {}",
                        resp.error_message
                    )));
                }
                let reply_msg = plexspaces_proto::common::v1::Message {
                    id: ulid::Ulid::new().to_string(),
                    payload: resp.payload,
                    headers: resp.headers,
                    message_type: "reply".to_string(),
                    ..Default::default()
                };
                Ok((message_id, Some(reply_msg)))
            } else {
                let send_req = SendMessageRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    namespace: actor_namespace,
                    actor_type: actor_type_str,
                    actor_name,
                    payload: message.payload,
                    headers: message.headers,
                    sender_id: message.sender_id,
                    message_type: message.message_type,
                    correlation_id: message.correlation_id,
                    reply_to: message.reply_to,
                    message_id: message.id,
                    ..Default::default()
                };
                let mut req = tonic::Request::new(send_req);
                plexspaces_actor::apply_request_context_to_grpc_metadata(&ctx, req.metadata_mut());
                transport.send_message(&node_id, req).await.map_err(|e| {
                    Status::internal(format!("Remote send to '{}' failed: {}", node_id, e))
                })?;
                Ok((message_id, None))
            }
        } else {
            let registry = self
                .service_locator
                .actor_registry()
                .await
                .ok_or_else(|| Status::internal("ActorRegistry unavailable"))?;

            if wait_for_response {
                let timeout = timeout.unwrap_or(std::time::Duration::from_secs(30));
                registry
                    .ask(&ctx, &target_id, message, timeout)
                    .await
                    .map(|reply| (message_id, Some(reply)))
                    .map_err(|e| match e {
                        plexspaces_actor::actor_registry::ActorRegistryError::Timeout => {
                            Status::deadline_exceeded("No reply received within timeout")
                        }
                        plexspaces_actor::actor_registry::ActorRegistryError::ActorNotFound(id) => {
                            Status::not_found(format!("Actor not found: {}", id))
                        }
                        plexspaces_actor::actor_registry::ActorRegistryError::MailboxFull { retry_after_ms, .. } => {
                            let mut status = Status::resource_exhausted(
                                format!("Mailbox full; retry after {}ms", retry_after_ms)
                            );
                            status.metadata_mut().insert(
                                "retry-after-ms",
                                retry_after_ms.to_string().parse().unwrap(),
                            );
                            status
                        }
                        other => Status::internal(format!("Routing error: {}", other)),
                    })
            } else {
                registry
                    .tell(&ctx, &target_id, message)
                    .await
                    .map(|()| (message_id, None))
                    .map_err(|e| match e {
                        plexspaces_actor::actor_registry::ActorRegistryError::ActorNotFound(id) => {
                            Status::not_found(format!("Actor not found: {}", id))
                        }
                        plexspaces_actor::actor_registry::ActorRegistryError::MailboxFull { retry_after_ms, .. } => {
                            let mut status = Status::resource_exhausted(
                                format!("Mailbox full; retry after {}ms", retry_after_ms)
                            );
                            status.metadata_mut().insert(
                                "retry-after-ms",
                                retry_after_ms.to_string().parse().unwrap(),
                            );
                            status
                        }
                        other => Status::internal(format!("Routing error: {}", other)),
                    })
            }
        }
    }

    /// Send message to actor with location transparency
    ///
    /// ## Purpose
    /// Public API for sending messages to local or remote actors.
    /// Automatically routes to the correct node based on the canonical actor ID.
    ///
    /// ## Arguments
    /// * `actor_id` - Target canonical actor ID
    /// * `message` - Message to send
    /// * `wait_for_response` - Whether to wait for reply (ask pattern)
    /// * `timeout` - Optional timeout for request-reply
    ///
    /// ## Returns
    /// * `Ok((message_id, Some(reply)))` - For ask pattern with reply
    /// * `Ok((message_id, None))` - For tell pattern (fire-and-forget)
    /// * `Err(...)` - Delivery failed
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Fire-and-forget (tell)
    /// let (msg_id, _) = actor_service.send(
    ///     "payment//payment::default@node1",
    ///     message,
    ///     false,
    ///     None
    /// ).await?;
    ///
    /// // Request-reply (ask)
    /// let (msg_id, Some(reply)) = actor_service.send(
    ///     "inventory//inventory::default@node2",
    ///     message,
    ///     true,
    ///     Some(Duration::from_secs(5))
    /// ).await?;
    /// ```
    pub async fn send(
        &self,
        ctx: RequestContext,
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), String> {
        self.route_message(ctx, actor_id, message, wait_for_response, timeout)
            .await
            .map_err(|e| e.to_string())
    }
}
