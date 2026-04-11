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

//! ActorService - gRPC Gateway for Distributed Actor Messaging
//!
//! ## Design Principle (Erlang-Inspired)
//!
//! ActorService is the **ONLY** gRPC entry point for actor messaging. It acts as a gateway
//! that routes messages to local or remote actors based on canonical actor IDs.
//!
//! ### Key Responsibilities
//!
//! 1. **Parse canonical actor IDs** to determine routing (local vs remote)
//! 2. **Local routing**: Lookup actor in registry, deliver to local mailbox
//! 3. **Remote routing**: Forward to remote node's ActorService via gRPC (using ServiceLocator for client caching)
//! 4. **Keep actors lightweight**: Actors never directly use gRPC
//! 5. **Local-only actor creation**: CreateActor and spawn_actor ALWAYS create actors locally on the node where called
//!
//! ### Message Flow
//!
//! ```text
//! Client -> ActorService.SendMessage("counter//counter::default@node2", msg)
//!   |
//!   +--> Parse: name="counter", actor_type="counter", namespace="default", node_id="node2"
//!   |
//!   +--> If node2 == local_node_id:
//!   |      -> Registry.lookup("counter//counter::default@node2") -> ActorRef
//!   |      -> ActorRef.tell(msg) -> Direct mailbox delivery
//!   |
//!   +--> If node2 != local_node_id:
//!          -> Registry.get_node_address("node2") -> "remote_host:8001"
//!          -> gRPC client.SendMessage("remote_host:8001", msg)
//!          -> Remote node's ActorService receives
//!          -> Remote node routes locally
//! ```
//!
//! ## Features
//!
//! - **SendMessage**: Fire-and-forget (Erlang cast) or request-reply (Erlang call)
//! - **StreamMessages**: Bidirectional streaming for high-throughput
//! - **Location transparency**: Same API for local and remote actors
//! - **Full observability**: Metrics for all operations
//!
//! ## Reply Routing in Ask Pattern
//!
//! **Important**: ReplyWaiter is used ONLY for async waiting, NOT for routing.
//!
//! When an actor calls `ctx.send_reply()`, this service handles routing the reply:
//!
//! ### Local Sender (Same Node)
//! ```
//! 1. Look up sender's ActorRef in ActorRegistry
//! 2. Call sender_ref.tell(reply_message) with correlation_id
//! 3. tell() checks ReplyWaiterRegistry for waiting ReplyWaiter
//!    (ReplyWaiter is registered in ReplyWaiterRegistry by ask())
//! 4. If found, routes reply to ReplyWaiter (bypasses mailbox)
//! ```
//!
//! ### Remote Sender (Different Node)
//! ```
//! 1. Create remote ActorRef for sender
//! 2. Call sender_ref.tell(reply_message) with correlation_id
//! 3. tell() sends reply via gRPC to remote node
//! 4. Remote tell() checks ReplyWaiterRegistry for waiting ReplyWaiter
//! 5. If found, routes reply to ReplyWaiter (bypasses mailbox)
//! ```
//!
//! ### Temporary Sender (External Caller)
//! ```
//! 1. Route to the canonical temporary sender actor ID
//! 2. Call its ActorRef with the reply message and correlation_id
//! 3. Call actor_ref.tell(reply_message) with correlation_id
//! 4. tell() checks ReplyWaiterRegistry for waiting ReplyWaiter
//! 5. If found, routes reply to ReplyWaiter (bypasses mailbox)
//! ```
//!
//! **Key Point**: ReplyWaiter is NOT involved in routing. It's only used by `ActorRef::tell()`
//! to wake up the waiting `ask()` caller once the reply arrives.
//!
//! ## Example Usage
//!
//! ```rust,ignore
//! use plexspaces_actor_service::ActorServiceImpl;
//! use plexspaces_object_registry::ObjectRegistry;
//! use plexspaces_keyvalue::InMemoryKVStore;
//!
//! // Create ActorService with object registry
//! let kv = Arc::new(InMemoryKVStore::new());
//! let object_registry = Arc::new(ObjectRegistry::new(kv));
//! let service_locator = Arc::new(ServiceLocator::new());
//! // Register ActorRegistry, ReplyWaiterRegistry in service_locator first
//! let actor_service = ActorServiceImpl::new(service_locator, "node1".to_string());
//!
//! // Start gRPC server
//! let addr = "0.0.0.0:8000".parse()?;
//! tonic::transport::Server::builder()
//!     .add_service(ActorServiceServer::new(actor_service))
//!     .serve(addr)
//!     .await?;
//! ```

#![warn(missing_docs)]
#![warn(clippy::all)]

use async_trait::async_trait;
use futures::future::join_all;
use serde::Deserialize;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::ServiceLocatorImpl;
use plexspaces_actor::parallel::{
    build_collective_message, reduce_values, resolve_timeout, scatter_stats_from_results,
    select_collective_value, shard_group_config, shard_query_responses_from_results,
};
use plexspaces_actor::ActorRef as ActorRefImpl;
use plexspaces_core::{
    monitoring::{
        record_node_shard_groups_created, record_node_shard_messages_received,
        record_node_shard_messages_sent, record_node_shard_operation,
        record_node_shard_operation_failed,
    },
    ActorId, ActorRegistry, RequestContext, ServiceLocator as ServiceLocatorTrait,
};
use plexspaces_proto::common::v1::Message;
use std::collections::HashMap;
use std::time::{Duration, Instant, SystemTime};
use ulid::Ulid;

// Import proto types and gRPC service trait
use plexspaces_proto::actor::v1::{
    // gRPC service trait and server
    actor_service_server::ActorService as ActorServiceTrait,
    ActorDownNotification,
    AllReduceShardGroupRequest,
    AllReduceShardGroupResponse,
    AskReplyRequest,
    AskReplyResponse,
    BarrierShardGroupRequest,
    BarrierShardGroupResponse,
    BroadcastShardGroupRequest,
    BroadcastShardGroupResponse,
    BulkUpdateShardGroupRequest,
    BulkUpdateShardGroupResponse,
    CheckActorExistsRequest,
    CheckActorExistsResponse,
    // ShardGroup types
    CreateShardGroupRequest,
    CreateShardGroupResponse,
    DeleteActorRequest,
    DeleteShardGroupRequest,
    GetActorRequest,
    GetActorResponse,
    GetShardGroupRequest,
    GetShardGroupResponse,
    LinkActorRequest,
    LinkActorResponse,
    ListActorsRequest,
    ListActorsResponse,
    ListShardGroupsRequest,
    ListShardGroupsResponse,
    MapShardGroupRequest,
    MapShardGroupResponse,
    MigrateActorRequest,
    MigrateActorResponse,
    MonitorActorRequest,
    MonitorActorResponse,
    ReduceShardGroupRequest,
    ReduceShardGroupResponse,
    ScaleShardGroupRequest,
    ScaleShardGroupResponse,
    ScatterGatherRequest,
    ScatterGatherResponse,
    ScatterGatherStats,
    // Request/Response types (from proto)
    SendMessageRequest,
    SendMessageResponse,
    SendToShardRequest,
    SendToShardResponse,
    SetActorStateRequest,
    SetActorStateResponse,
    ShardGroup,
    ShardGroupAggregationStrategy,
    ShardGroupState,
    ShardQueryResponse,
    ShardUpdateStats,
    SpawnActorRequest,
    SpawnActorResponse,
    SpawnActorsRequest,
    SpawnActorsResponse,
    StreamMessageRequest,
    StreamMessageResponse,
    UnlinkActorRequest,
    UnlinkActorResponse,
};
use plexspaces_proto::common::v1::Empty;

/// ActorService implementation - gRPC gateway for actor messaging
///
/// ## Responsibilities
/// - Route messages to local or remote actors based on canonical actor IDs
/// - Lookup actors/nodes in ObjectRegistry
/// - Deliver messages to local actors via ActorRef
/// - Forward messages to remote nodes via gRPC
/// - Emit metrics for all operations
pub struct ActorServiceImpl {
    /// ServiceLocator for service access and gRPC client caching
    /// Stored as ServiceLocatorImpl (concrete type) to access ActorFactory inherent methods
    /// ServiceLocatorImpl is the only production implementation, so this is safe and production-grade
    service_locator: Arc<ServiceLocatorImpl>,

    /// Local node ID (for routing decisions)
    local_node_id: String,

    /// ShardGroups registry: group_id -> ShardGroup metadata
    /// Thread-safe HashMap for concurrent access
    shard_groups: Arc<tokio::sync::RwLock<std::collections::HashMap<String, ShardGroup>>>,
}

impl ActorServiceImpl {
    /// Create new ActorService
    ///
    /// # Arguments
    /// * `service_locator` - ServiceLocatorImpl for service access and gRPC client caching
    /// * `local_node_id` - ID of this node
    ///
    /// # Note
    /// Services (ActorRegistry, ReplyWaiterRegistry) should already be registered in ServiceLocator
    /// before creating ActorServiceImpl. They will be retrieved synchronously if runtime is available,
    /// otherwise on first async access.
    ///
    /// # Design
    /// Uses `ServiceLocatorImpl` directly (not trait) to access ActorFactory inherent methods.
    /// This is production-grade because ServiceLocatorImpl is the only production implementation.
    pub fn new(service_locator: Arc<ServiceLocatorImpl>, local_node_id: String) -> Self {
        // Services will be retrieved from ServiceLocator on first use
        // This avoids "Cannot start a runtime from within a runtime" errors
        ActorServiceImpl {
            service_locator,
            local_node_id,
            shard_groups: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    fn build_canonical_actor_id(
        &self,
        name: &str,
        actor_type: &str,
        namespace: &str,
        node_id: &str,
    ) -> Result<ActorId, Status> {
        ActorId::new(name, actor_type, namespace, node_id).map_err(|e| {
            Status::invalid_argument(format!(
                "Invalid actor identity name='{}' actor_type='{}' namespace='{}' node_id='{}': {}",
                name, actor_type, namespace, node_id, e
            ))
        })
    }

    fn parse_canonical_actor_id(&self, actor_id: &str) -> Result<ActorId, Status> {
        ActorId::from_canonical(actor_id).map_err(|e| {
            Status::invalid_argument(format!("Invalid canonical actor id '{}': {}", actor_id, e))
        })
    }

    fn actor_type_from_client_target(&self, requested_actor_type: &str) -> Option<String> {
        if let Ok(actor_id) = ActorId::from_canonical(requested_actor_type) {
            Some(actor_id.actor_type().to_string())
        } else {
            requested_actor_type
                .split_once(':')
                .map(|(actor_type, _)| actor_type.to_string())
        }
    }

    /// Get ActorRegistry from ServiceLocator (lazy initialization)
    async fn get_actor_registry(&self) -> Arc<ActorRegistry> {
        self.service_locator
            .actor_registry()
            .await
            .expect("ActorRegistry must be registered in ServiceLocator")
    }

    /// Resolve an actor id for actor-type-based ask/tell operations when no active local instance is found.
    ///
    /// AskReply and SendMessage are actor-type based, so this helper resolves the canonical target actor id.
    /// Local activation is owned by `ActorRegistry::ask()` / `ActorRegistry::tell()`, while
    /// remote delivery continues to use the existing routing path.
    async fn resolve_target_actor_id_for_type_lookup(
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

        let requested_namespace = ctx.namespace().to_string();
        let (instance_id, simple_actor_type) = if let Some((actor_type_part, instance_id_part)) =
            requested_actor_type.split_once(':')
        {
            (instance_id_part.to_string(), actor_type_part.to_string())
        } else {
            (
                ulid::Ulid::new().to_string(),
                requested_actor_type.to_string(),
            )
        };

        let requested_actor_id = self.build_canonical_actor_id(
            &instance_id,
            &simple_actor_type,
            &requested_namespace,
            &self.local_node_id,
        )?;

        if virtual_actor_manager
            .get_metadata(&requested_actor_id)
            .await
            .is_some()
        {
            return Ok(requested_actor_id.to_string());
        }

        let type_metadata = virtual_actor_manager
            .get_virtual_actor_type(&simple_actor_type)
            .await
            .ok_or_else(|| {
                Status::not_found(format!(
                    "No actors found for type '{}' in tenant '{}', namespace '{}'",
                    requested_actor_type,
                    ctx.tenant_id(),
                    ctx.namespace()
                ))
            })?;

        let target_actor_id = self.build_canonical_actor_id(
            &instance_id,
            &simple_actor_type,
            &type_metadata.namespace,
            &self.local_node_id,
        )?;

        Ok(target_actor_id.to_string())
    }

    async fn canonical_actor_id_from_client_target(
        &self,
        ctx: &RequestContext,
        requested_actor_type: &str,
    ) -> Option<String> {
        if requested_actor_type.contains("//") {
            return Some(requested_actor_type.to_string());
        }

        if !requested_actor_type.contains(':') {
            let resolved = self
                .resolve_actor_target(ctx, requested_actor_type)
                .await
                .ok();
            return resolved;
        }

        let (actor_type, base_actor_id) = requested_actor_type.split_once(':')?;
        if actor_type.is_empty() || base_actor_id.is_empty() {
            return None;
        }

        let namespace = if let Some(virtual_actor_manager) =
            self.service_locator.virtual_actor_manager().await
        {
            virtual_actor_manager
                .get_virtual_actor_type(actor_type)
                .await
                .map(|metadata| metadata.namespace)
                .unwrap_or_else(|| ctx.namespace().to_string())
        } else {
            ctx.namespace().to_string()
        };

        self.build_canonical_actor_id(base_actor_id, actor_type, &namespace, &self.local_node_id)
            .ok()
            .map(|actor_id| actor_id.to_string())
    }

    fn duration_from_proto(duration: Option<prost_types::Duration>) -> Option<Duration> {
        duration
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
    }

    fn build_message_metadata(
        headers: &HashMap<String, String>,
        path: &str,
        subpath: &str,
    ) -> HashMap<String, String> {
        let mut metadata = headers.clone();
        if !path.is_empty() {
            metadata.insert("http_path".to_string(), path.to_string());
        }
        if !subpath.is_empty() {
            metadata.insert("http_subpath".to_string(), subpath.to_string());
        }
        metadata
    }

    fn build_request_payload(
        payload: &[u8],
        query_params: &HashMap<String, String>,
        http_method: &str,
    ) -> Result<Vec<u8>, Status> {
        if http_method.eq_ignore_ascii_case("GET") {
            serde_json::to_vec(query_params)
                .map_err(|e| Status::internal(format!("Failed to serialize query params: {}", e)))
        } else if payload.is_empty() {
            Ok(Vec::new())
        } else {
            // Deserialize only the first JSON value; do not require end-of-input (matches Python
            // `payload_from_request_json`, which uses raw_decode on "Extra data"). Some gateways
            // or clients append a second JSON document or stray bytes after a valid object.
            let mut de = serde_json::Deserializer::from_slice(payload);
            let _: serde_json::Value = Deserialize::deserialize(&mut de)
                .map_err(|e| Status::invalid_argument(format!("Invalid JSON payload: {}", e)))?;
            Ok(payload.to_vec())
        }
    }

    async fn resolve_actor_target(
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

    async fn route_actor_request(
        &self,
        ctx: RequestContext,
        requested_actor_type: &str,
        mut message: Message,
        wait_for_response: bool,
        timeout: Option<Duration>,
    ) -> Result<(String, String, Option<Message>), Status> {
        let requested_target = self
            .canonical_actor_id_from_client_target(&ctx, requested_actor_type)
            .await
            .unwrap_or_else(|| requested_actor_type.to_string());
        Self::set_message_receiver_id(&mut message, &requested_target);

        match self
            .route_message(
                ctx.clone(),
                &requested_target,
                message.clone(),
                wait_for_response,
                timeout,
            )
            .await
        {
            Ok((message_id, reply)) => Ok((requested_target, message_id, reply)),
            Err(status) if status.code() == tonic::Code::NotFound => {
                let mut type_candidates = Vec::new();
                type_candidates.push(requested_actor_type.to_string());

                if requested_actor_type.contains('@') {
                    if let Some(actor_type) = self.actor_type_from_client_target(requested_actor_type) {
                        if actor_type != requested_actor_type {
                            type_candidates.push(actor_type);
                        }
                    }
                } else if let Some((actor_type, _instance_id)) =
                    requested_actor_type.split_once(':')
                {
                    if actor_type != requested_actor_type {
                        type_candidates.push(actor_type.to_string());
                    }
                }

                let mut resolved_actor_id = None;
                for actor_type in type_candidates {
                    match self.resolve_actor_target(&ctx, &actor_type).await {
                        Ok(actor_id) => {
                            resolved_actor_id = Some(actor_id);
                            break;
                        }
                        Err(candidate_err) if candidate_err.code() == tonic::Code::NotFound => {}
                        Err(candidate_err) => return Err(candidate_err),
                    }
                }

                let resolved_actor_id = resolved_actor_id.ok_or(status)?;
                Self::set_message_receiver_id(&mut message, &resolved_actor_id);
                let (message_id, reply) = self
                    .route_message(ctx, &resolved_actor_id, message, wait_for_response, timeout)
                    .await?;
                Ok((resolved_actor_id, message_id, reply))
            }
            Err(status) => Err(status),
        }
    }

    /// Canonicalize receiver_id at the service boundary so actors always observe the
    /// resolved framework-owned actor id rather than a client alias such as `type:id`.
    fn set_message_receiver_id(message: &mut Message, resolved_actor_id: &str) {
        message.receiver_id = resolved_actor_id.to_string();
    }

    /// Check if service should accept requests (not shutting down)
    ///
    /// ## Purpose
    /// Verifies that the node is not shutting down before processing requests.
    /// This ensures graceful shutdown: stop accepting new requests but complete in-progress ones.
    ///
    /// ## Returns
    /// `Ok(())` if service is accepting requests, `Err(Status::unavailable)` if shutting down
    async fn check_accepting_requests(&self) -> Result<(), Status> {
        if self.service_locator.is_shutdown_requested() {
            return Err(Status::unavailable(
                "Service is shutting down and not accepting new requests",
            ));
        }
        Ok(())
    }

    /// Spawn a new actor locally on this node - Public API for ActorContext
    ///
    /// ## Design Principle
    /// ActorService ALWAYS creates actors locally on the node where it's called.
    /// There is no remote spawning - to spawn on a remote node, call that node's ActorService directly.
    ///
    /// ## Arguments
    /// * `actor_id` - Canonical actor ID string at the service boundary. Client-facing builders
    ///   should pass only the actor name and let the runtime construct this value.
    /// * `actor_type` - Type of actor to spawn (must be registered in BehaviorFactory if using factory)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Returns
    /// ActorRef for the spawned actor using the canonical actor ID format.
    ///
    /// ## Implementation
    /// Delegates to Node::spawn_actor() via ServiceLocator. Creates Actor using ActorBuilder
    /// and BehaviorFactory (if available) or a simple default behavior.
    /// Spawn a new actor locally on this node - Public API for ActorContext
    ///
    /// ## Design Principle
    /// ActorService ALWAYS creates actors locally on the node where it's called.
    /// This method delegates to Node::spawn_actor() via ServiceLocator.
    ///
    /// ## Arguments
    /// * `actor_id` - Canonical actor ID string at the service boundary. Client-facing builders
    ///   should pass only the actor name and let the runtime construct this value.
    /// * `actor_type` - Type of actor to spawn (used by BehaviorFactory if available)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Returns
    /// ActorRef for the spawned actor using the canonical actor ID format.
    ///
    /// ## Implementation
    /// Delegates to Node::spawn_actor() via ServiceLocator. The actual implementation
    /// is in Node's CreateActor gRPC handler which has full access to Node.
    ///
    /// ## Note
    /// This method is primarily for ActorContext compatibility. For direct spawning,
    /// use Node::spawn_actor() or the CreateActor gRPC RPC.
    pub async fn spawn_actor(
        &self,
        ctx: &plexspaces_core::RequestContext,
        actor_id: &str,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<ActorRefImpl, Box<dyn std::error::Error + Send + Sync>> {
        // Public/client-facing spawn takes a base actor id.
        // The framework owns canonical actor-id construction so callers do not have to
        // assemble `{id}//{actor_type}::{namespace}@{node_id}` themselves.
        let local_actor_id = if let Ok(parsed) = ActorId::from_canonical(actor_id) {
            if parsed.node_id() != self.local_node_id {
                return Err(format!(
                    "Cannot spawn actor on remote node '{}' via local ActorService. ActorService always creates actors locally. To spawn on '{}', call that node's ActorService directly.",
                    parsed.node_id(), parsed.node_id()
                )
                .into());
            }
            self.build_canonical_actor_id(
                parsed.name(),
                actor_type,
                if parsed.namespace().is_empty() {
                    ctx.namespace()
                } else {
                    parsed.namespace()
                },
                &self.local_node_id,
            )
            .map_err(|e| e.to_string())?
        } else {
            self.build_canonical_actor_id(actor_id, actor_type, ctx.namespace(), &self.local_node_id)
                .map_err(|e| e.to_string())?
        };

        // Use ActorFactory from ServiceLocatorImpl (direct access to inherent method)
        use plexspaces_actor::ActorFactory;
        let actor_factory: Arc<dyn ActorFactory> = self.service_locator.get_actor_factory().await
            .ok_or_else(|| format!(
                "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called and ActorFactory is registered. Actor ID would be: {}",
                local_actor_id
            ))?;

        // Use ActorFactory to spawn actor
        // ActorFactory returns MessageSender, but we need ActorRefImpl
        // The actor is already registered in ActorRegistry, so we can create ActorRefImpl
        // that uses MessageSender internally
        actor_factory
            .spawn_actor(
                &ctx,
                &local_actor_id,
                actor_type,
                initial_state,
                config,
                labels,
                vec![], // facets (empty - facets should be attached via config or separate API)
            )
            .await?;

        // Actor is now spawned and registered - try to create ActorRefImpl::local() if mailbox available
        let registry = self.get_actor_registry().await;
        Ok(Self::create_actor_ref_for_local_actor(
            &ctx,
            &registry,
            &local_actor_id,
            &self.local_node_id,
            self.service_locator.clone(),
        )
        .await)
    }

    /// Send a message to an actor (local or remote) - Public API for ActorContext
    ///
    /// ## Arguments
    /// * `actor_id` - Canonical actor ID in format `name//actor_type::namespace@node_id`
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Message ID if successful
    ///
    /// ## Reply Routing
    /// If the message has a `correlation_id`, it's treated as a reply to an `ask()` request.
    /// For local actors, the reply is routed automatically via ActorRef::tell() using ReplyWaiterRegistry.
    pub async fn send_message(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}",
                message.id, actor_id, message.sender_id, message.receiver_id, message.message_type, message.correlation_id
            );
        }

        // Check if this is a reply (has correlation_id) and route to per-ActorRef reply map if local
        // correlation_id is now String - check if not empty
        if !message.correlation_id.is_empty() {
            let correlation_id = message.correlation_id.clone();
            let actor_id_full = self
                .parse_canonical_actor_id(actor_id)
                .map_err(|e| e.to_string())?;
            let node_id = actor_id_full.node_id().to_string();

            // If local actor, route reply via MessageSender.tell()
            // ActorRef::tell() will automatically check for correlation_id and route to ReplyWaiter
            // if there's a pending ask() call - routing handled by ReplyWaiterRegistry
            if node_id == self.local_node_id {
                // Use MessageSender.tell() - ActorRef::tell() handles reply routing automatically
                // When MessageSender.tell() is called, it eventually calls ActorRef::tell(),
                // which checks ReplyWaiterRegistry for the correlation_id and routes to ReplyWaiter
                // Try lookup with constructed ID first
                let mut sender_opt = self
                    .get_actor_registry()
                    .await
                    .lookup_actor(&actor_id_full)
                    .await;

                if let Some(sender) = sender_opt {
                    // MessageSender exists - use it directly
                    // ActorRef::tell() will check for correlation_id and route to ReplyWaiter if present
                    let message_id = message.id.to_string();
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            "🟪 [ACTOR_SERVICE::send_message] REPLY ROUTING: message_id={}, correlation_id={}, routing via MessageSender.tell()",
                            message_id, correlation_id
                        );
                    }
                    sender
                        .tell(message)
                        .await
                        .map_err(|e| Status::internal(format!("Failed to send reply: {}", e)))?;
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            "🟪 [ACTOR_SERVICE::send_message] REPLY ROUTED: message_id={}, correlation_id={}",
                            message_id, correlation_id
                        );
                    }
                    return Ok(message_id);
                }

                // Temp sender not found in ActorRegistry (may have been cleaned up after
                // ask() timeout).  Try ReplyWaiterRegistry directly using correlation_id
                // so the reply still reaches the waiter if it is still active.
                if actor_id_full.is_temporary_sender() {
                    if let Some(waiter_registry) = self.service_locator.reply_waiter_registry().await
                    {
                        let message_id = message.id.to_string();
                        if waiter_registry.notify(&correlation_id, message).await {
                            tracing::info!(
                                message_id = %message_id,
                                correlation_id = %correlation_id,
                                temp_sender = %actor_id_full,
                                "Reply routed directly via ReplyWaiterRegistry after temporary sender cleanup"
                            );
                            return Ok(message_id);
                        }
                        tracing::warn!(
                            message_id = %message_id,
                            correlation_id = %correlation_id,
                            temp_sender = %actor_id_full,
                            "Reply arrived after ask timeout; temporary sender and ReplyWaiter were already cleaned up"
                        );
                        return Ok(message_id);
                    }
                }
            }
        }

        // Normal message routing (no correlation_id or remote actor)
        // Note: send_message is called from ActorContext which should provide RequestContext
        // For now, create empty context - this should be fixed to pass ctx from caller
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message] NORMAL ROUTING: message_id={}, actor_id={}, calling route_message",
                message.id, actor_id
            );
        }
        let (msg_id, _) = self
            .route_message(ctx, actor_id, message, false, None)
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

    /// Send a message and wait for reply (request-reply pattern) - Public API for ActorContext
    ///
    /// ## Design
    /// Uses ActorRef::ask() directly instead of route_message with wait_for_response=true.
    /// This ensures proper routing, metrics, and virtual actor activation.
    ///
    /// ## Arguments
    /// * `actor_id` - Canonical actor ID in format `name//actor_type::namespace@node_id`
    /// * `message` - Request message
    /// * `timeout` - Optional timeout
    ///
    /// ## Returns
    /// Reply message
    pub async fn send_message_and_wait(
        &self,
        actor_id: &str,
        message: Message,
        timeout: Option<std::time::Duration>,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, timeout={:?}",
                message.id, actor_id, message.sender_id, message.receiver_id, message.message_type, message.correlation_id, timeout
            );
        }

        let target_actor_id = self
            .parse_canonical_actor_id(actor_id)
            .map_err(|e| e.to_string())?;
        let node_id = target_actor_id.node_id().to_string();

        if node_id == self.local_node_id {
            // LOCAL: use ActorRegistry ask() so virtual activation stays inside the registry.
            let registry = self.get_actor_registry().await;
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
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
                .map_err(|e| {
                    use plexspaces_core::ActorRegistryError;
                    match e {
                        ActorRegistryError::ActorNotFound(_) => "Actor not found".into(),
                        ActorRegistryError::Timeout => "Request timed out".into(),
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
                    message.id, actor_id
                );
            }
            // Note: send_message_and_wait should receive RequestContext from caller
            // For now, create empty context - this should be fixed to pass ctx from ActorContext
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            let (_, response) = self
                .route_message(ctx, actor_id, message, true, timeout)
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
    async fn create_actor_ref_for_local_actor(
        ctx: &RequestContext,
        _registry: &Arc<ActorRegistry>,
        actor_id: &ActorId,
        local_node_id: &str,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> ActorRefImpl {
        ActorRefImpl::remote(
            actor_id.clone(),
            ctx.tenant_id().to_string(),
            ctx.namespace().to_string(),
            local_node_id.to_string(),
            service_locator,
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

        // Use unified routing module (returns Future, converts ActorRefError to Status)
        use plexspaces_actor::routing::route_message as routing_route_message;
        let result = routing_route_message(
            ctx,
            self.service_locator.clone(),
            actor_id.to_string(),
            message,
            wait_for_response,
            timeout,
        )
        .await;

        // Convert ActorRefError to Status
        result.map_err(|e| match e {
            plexspaces_actor::ActorRefError::Timeout => {
                Status::deadline_exceeded("No reply received within timeout")
            }
            plexspaces_actor::ActorRefError::ActorNotFound(id) => {
                Status::not_found(format!("Actor not found: {}", id))
            }
            plexspaces_actor::ActorRefError::InvalidActorId(msg) => {
                Status::invalid_argument(msg)
            }
            plexspaces_actor::ActorRefError::SendFailed(msg) => {
                Status::internal(format!("Failed to send message: {}", msg))
            }
            _ => Status::internal(format!("Routing error: {}", e)),
        })
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

/// Implement ActorService trait from core (for ActorContext)
#[async_trait]
impl plexspaces_core::actor_context::ActorService for ActorServiceImpl {
    async fn spawn_actor(
        &self,
        actor_id: &str,
        actor_type: &str,
        initial_state: Vec<u8>,
    ) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        // Create RequestContext - tenant comes from auth, not config
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let actor_ref_impl = self
            .spawn_actor(
                &ctx,
                actor_id,
                actor_type,
                initial_state,
                None,
                std::collections::HashMap::new(),
            )
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        plexspaces_core::ActorRef::new(actor_ref_impl.id().clone())
            .map_err(|e| format!("Failed to create ActorRef: {}", e).into())
    }

    async fn send(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.send_message(actor_id, message).await
    }

    async fn send_and_wait(
        &self,
        actor_id: &str,
        message: Message,
        timeout: Option<std::time::Duration>,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        self.send_message_and_wait(actor_id, message, timeout).await
    }

    async fn create_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::CreateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::CreateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.create_shard_group_internal(ctx, req).await
    }

    async fn bulk_update_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BulkUpdateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.bulk_update_shard_group_internal(ctx, req).await
    }

    async fn map_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::MapShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::MapShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.map_shard_group_internal(ctx, req).await
    }

    async fn scatter_gather(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::ScatterGatherRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ScatterGatherResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.scatter_gather_internal(ctx, req).await
    }

    async fn broadcast_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BroadcastShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BroadcastShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.broadcast_shard_group_internal(ctx, req).await
    }

    async fn reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::ReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.reduce_shard_group_internal(ctx, req).await
    }

    async fn all_reduce_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::AllReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::AllReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.all_reduce_shard_group_internal(ctx, req).await
    }

    async fn barrier_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BarrierShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BarrierShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.barrier_shard_group_internal(ctx, req).await
    }

    async fn spawn_actors(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::SpawnActorsRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::SpawnActorsResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        self.check_accepting_requests()
            .await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;

        let mut results = Vec::new();
        for spawn_req in req.requests {
            let actor_type = spawn_req.actor_type.clone();
            let namespace = if spawn_req.namespace.is_empty() {
                ctx.namespace().to_string()
            } else {
                spawn_req.namespace.clone()
            };
            // instances_count == 0 or 1 means spawn one actor
            let count = if spawn_req.instances_count <= 1 {
                1
            } else {
                spawn_req.instances_count
            };

            for i in 0..count {
                let actor_name = if spawn_req.actor_id.is_empty() {
                    Ulid::new().to_string()
                } else if count > 1 {
                    format!("{}-{}", spawn_req.actor_id, i)
                } else {
                    spawn_req.actor_id.clone()
                };
                let actor_id = self
                    .build_canonical_actor_id(
                        &actor_name,
                        &actor_type,
                        &namespace,
                        &self.local_node_id,
                    )
                    .map_err(|e| e.to_string())?;

                match self
                    .spawn_actor(
                        ctx,
                        actor_id.as_str(),
                        &actor_type,
                        spawn_req.initial_state.clone(),
                        spawn_req.config.clone(),
                        spawn_req.labels.clone(),
                    )
                    .await
                {
                    Ok(_actor_ref) => {
                        results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                            success: true,
                            error: String::new(),
                            response: Some(plexspaces_proto::actor::v1::SpawnActorResponse {
                                actor_ref: actor_id.to_string(),
                                actor: None,
                            }),
                        });
                    }
                    Err(e) => {
                        results.push(plexspaces_proto::actor::v1::SpawnActorResult {
                            success: false,
                            error: e.to_string(),
                            response: None,
                        });
                    }
                }
            }
        }
        Ok(plexspaces_proto::actor::v1::SpawnActorsResponse { results })
    }
}

/// Implement Service trait for ActorServiceImpl (for ServiceLocator registration)
impl plexspaces_core::Service for ActorServiceImpl {
    fn service_name(&self) -> String {
        "ActorServiceImpl".to_string()
    }
}

/// Implement the ActorService gRPC trait
#[async_trait]
impl ActorServiceTrait for ActorServiceImpl {
    /// Send a message to an actor (fire-and-forget or request-reply)
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        self.check_accepting_requests().await?;
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let namespace = if req.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            req.namespace.clone()
        };
        let routing_ctx = RequestContext::new_without_auth(ctx.tenant_id().to_string(), namespace);
        let actor_type = req.actor_type.clone();
        if actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }

        let http_method = if req.http_method.is_empty() {
            "POST".to_string()
        } else {
            req.http_method.to_uppercase()
        };
        if http_method == "GET" {
            return Err(Status::invalid_argument(
                "SendMessage does not support GET; use AskReply",
            ));
        }

        let full_path = if req.path.is_empty() {
            format!("/api/v1/actors/{}/{}", routing_ctx.namespace(), actor_type)
        } else {
            req.path.clone()
        };
        let payload = Self::build_request_payload(&req.payload, &req.query_params, &http_method)?;
        let mut message = Message {
            id: if req.message_id.is_empty() {
                format!("req-{}", Ulid::new())
            } else {
                req.message_id.clone()
            },
            sender_id: req.sender_id.clone(),
            receiver_id: actor_type.clone(),
            message_type: if req.message_type.is_empty() {
                "cast".to_string()
            } else {
                req.message_type.clone()
            },
            payload,
            headers: Self::build_message_metadata(&req.headers, &req.path, &req.subpath),
            correlation_id: req.correlation_id.clone(),
            reply_to: req.reply_to.clone(),
            uri_path: full_path.clone(),
            uri_method: http_method.clone(),
            ..Default::default()
        };
        if message.message_type == "call" {
            message.message_type = "cast".to_string();
        }

        tracing::debug!(
            tenant_id = %routing_ctx.tenant_id(),
            namespace = %routing_ctx.namespace(),
            actor_type = %actor_type,
            method = %http_method,
            path = %full_path,
            "send_message tell request started"
        );

        let start = Instant::now();
        let result = self
            .route_actor_request(routing_ctx, &actor_type, message, false, None)
            .await;

        match result {
            Ok((resolved_actor_id, message_id, _)) => {
                tracing::debug!(
                    actor_type = %actor_type,
                    actor_id = %resolved_actor_id,
                    message_id = %message_id,
                    duration_ms = start.elapsed().as_millis(),
                    "send_message tell request completed"
                );
                Ok(Response::new(SendMessageResponse {
                    success: true,
                    message_id,
                    actor_id: resolved_actor_id,
                    error_message: String::new(),
                }))
            }
            Err(status) => {
                tracing::debug!(
                    actor_type = %actor_type,
                    error = %status,
                    duration_ms = start.elapsed().as_millis(),
                    "send_message tell request failed"
                );
                Err(status)
            }
        }
    }

    // ========================================================================
    // Actor Lifecycle Management RPCs
    // ========================================================================

    async fn spawn_actor(
        &self,
        request: Request<SpawnActorRequest>,
    ) -> Result<Response<SpawnActorResponse>, Status> {
        // Check if service is accepting requests (not shutting down)
        if self.service_locator.is_shutdown_requested() {
            return Err(Status::unavailable(
                "Service is shutting down and not accepting new requests",
            ));
        }

        // This is the gRPC handler - it spawns locally on this node
        // gRPC is already remote, so "remote" in the name was redundant
        // The actor is spawned locally on THIS node (the one receiving the gRPC request)

        // Extract labels from request before consuming it (needed for context creation)
        let labels_for_ctx = request.get_ref().labels.clone();

        // Create RequestContext from request metadata (before consuming request)
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &labels_for_ctx,
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        // Now consume request to get inner data
        let req = request.into_inner();

        // Validate actor_type
        if req.actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }

        // Determine actor ID: client-specified or server-generated; use full actor-id format
        let node_id = self.local_node_id.clone();
        let ctx_namespace = ctx.namespace();
        let effective_namespace = if !req.namespace.is_empty() {
            req.namespace.as_str()
        } else {
            ctx_namespace
        };
        let namespace = if effective_namespace.is_empty() {
            None
        } else {
            Some(effective_namespace)
        };
        let effective_ctx = RequestContext::new_without_auth(
            ctx.tenant_id().to_string(),
            effective_namespace.to_string(),
        );
        let actor_id = if !req.actor_id.is_empty() {
            if let Ok(parsed) = ActorId::from_canonical(&req.actor_id) {
                if parsed.node_id() != node_id {
                    return Err(Status::invalid_argument(format!(
                        "Actor '{}' targets node '{}' but this service only spawns on local node '{}'",
                        req.actor_id,
                        parsed.node_id(),
                        node_id
                    )));
                }
                parsed
            } else {
                self.build_canonical_actor_id(
                    &req.actor_id,
                    &req.actor_type,
                    effective_namespace,
                    &node_id,
                )?
            }
        } else {
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            self.build_canonical_actor_id(
                &format!("actor-{}", timestamp),
                &req.actor_type,
                effective_namespace,
                &node_id,
            )?
        };

        // Clone values before using them (needed for both spawn_actor and proto_actor)
        let actor_type = req.actor_type.clone();
        let initial_state = req.initial_state.clone();
        let config = req.config.clone();
        let labels = req.labels.clone();
        let facets = req.facets.clone();

        // Log facets for observability
        let facet_count = facets.len();
        let facet_types: Vec<&str> = facets.iter().map(|f| f.r#type.as_str()).collect();
        tracing::info!(
            actor_id = %actor_id,
            actor_type = %actor_type,
            facet_count = facet_count,
            facet_types = ?facet_types,
            "Spawning actor with facets via gRPC"
        );

        // Convert proto Facets to Box<dyn Facet> using FacetRegistry
        let mut facet_boxes: Vec<Box<dyn plexspaces_facet::Facet>> = Vec::new();
        if !facets.is_empty() {
            // Get FacetRegistry from ServiceLocator
            let facet_registry_wrapper = self
                .service_locator
                .get_facet_registry()
                .await
                .ok_or_else(|| Status::internal("FacetRegistry not available in ServiceLocator"))?;
            let facet_registry = facet_registry_wrapper.inner();

            for proto_facet in &facets {
                match plexspaces_actor::create_facet_from_proto(proto_facet, facet_registry).await {
                    Ok(facet_box) => {
                        facet_boxes.push(facet_box);
                    }
                    Err(e) => {
                        tracing::warn!(
                            actor_id = %actor_id,
                            facet_type = %proto_facet.r#type,
                            error = %e,
                            "Failed to create facet, skipping"
                        );
                        // Continue with other facets rather than failing entirely
                    }
                }
            }
        }

        // Use ActorFactory to spawn the actor locally
        use plexspaces_actor::ActorFactory;
        let actor_factory_opt: Option<Arc<dyn ActorFactory>> =
            self.service_locator.get_actor_factory().await;

        let spawned_actor_ref = if let Some(factory) = actor_factory_opt {
            // Spawn actor using ActorFactory with facets
            let spawned_actor_ref = factory
                .spawn_actor(
                    &effective_ctx,
                    &actor_id,
                    &actor_type,
                    initial_state.clone(),
                    config.clone(),
                    labels.clone(),
                    facet_boxes, // Pass converted facets
                )
                .await
                .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;

            // Record metrics for facet attachment
            if facet_count > 0 {
                metrics::counter!("plexspaces.actor.spawn.with_facets").increment(1);
                metrics::counter!("plexspaces.actor.facets.attached").increment(facet_count as u64);
            }
            spawned_actor_ref
        } else {
            return Err(Status::internal(
                "ActorFactory not available in ServiceLocator",
            ));
        };

        let spawned_actor_id = spawned_actor_ref
            .actor_id()
            .unwrap_or_else(|| actor_id.to_string());

        // Build proto Actor message for response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: spawned_actor_id.to_string(),
            name: actor_id.name().to_string(),
            actor_type,
            state: ActorState::ActorStateActive as i32,
            node_id: node_id.clone(),
            vm_id: String::new(),
            actor_state: initial_state,
            metadata: None,
            config,
            metrics: None,
            facets, // Return facets in response
            actor_state_schema_version: 0,
            error_message: String::new(),
            namespace: effective_namespace.to_string(),
        };

        // Return response with the canonical actor ID string
        Ok(Response::new(SpawnActorResponse {
            actor_ref: spawned_actor_id.to_string(),
            actor: Some(proto_actor),
        }))
    }

    async fn get_actor(
        &self,
        _request: Request<GetActorRequest>,
    ) -> Result<Response<GetActorResponse>, Status> {
        Err(Status::unimplemented("get_actor not yet implemented"))
    }

    async fn list_actors(
        &self,
        _request: Request<ListActorsRequest>,
    ) -> Result<Response<ListActorsResponse>, Status> {
        Err(Status::unimplemented("list_actors not yet implemented"))
    }

    async fn delete_actor(
        &self,
        _request: Request<DeleteActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("delete_actor not yet implemented"))
    }

    // ========================================================================
    // Actor State Management RPCs
    // ========================================================================

    async fn set_actor_state(
        &self,
        _request: Request<SetActorStateRequest>,
    ) -> Result<Response<SetActorStateResponse>, Status> {
        Err(Status::unimplemented("set_actor_state not yet implemented"))
    }

    async fn migrate_actor(
        &self,
        _request: Request<MigrateActorRequest>,
    ) -> Result<Response<MigrateActorResponse>, Status> {
        Err(Status::unimplemented("migrate_actor not yet implemented"))
    }

    // ========================================================================
    // Streaming & Monitoring RPCs
    // ========================================================================

    /// Stream type for bidirectional message streaming
    type StreamMessagesStream =
        Pin<Box<dyn Stream<Item = Result<StreamMessageResponse, Status>> + Send>>;

    async fn stream_messages(
        &self,
        _request: Request<tonic::Streaming<StreamMessageRequest>>,
    ) -> Result<Response<Self::StreamMessagesStream>, Status> {
        Err(Status::unimplemented("stream_messages not yet implemented"))
    }

    async fn monitor_actor(
        &self,
        _request: Request<MonitorActorRequest>,
    ) -> Result<Response<MonitorActorResponse>, Status> {
        Err(Status::unimplemented("monitor_actor not yet implemented"))
    }

    async fn notify_actor_down(
        &self,
        _request: Request<ActorDownNotification>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented(
            "notify_actor_down not yet implemented",
        ))
    }

    async fn link_actor(
        &self,
        _request: Request<LinkActorRequest>,
    ) -> Result<Response<LinkActorResponse>, Status> {
        Err(Status::unimplemented("link_actor not yet implemented"))
    }

    async fn unlink_actor(
        &self,
        _request: Request<UnlinkActorRequest>,
    ) -> Result<Response<UnlinkActorResponse>, Status> {
        Err(Status::unimplemented("unlink_actor not yet implemented"))
    }

    async fn check_actor_exists(
        &self,
        _request: Request<CheckActorExistsRequest>,
    ) -> Result<Response<CheckActorExistsResponse>, Status> {
        Err(Status::unimplemented(
            "check_actor_exists not yet implemented",
        ))
    }

    async fn ask_reply(
        &self,
        request: Request<AskReplyRequest>,
    ) -> Result<Response<AskReplyResponse>, Status> {
        self.check_accepting_requests().await?;
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let namespace = if req.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            req.namespace.clone()
        };
        let routing_ctx = RequestContext::new_without_auth(ctx.tenant_id().to_string(), namespace);
        let actor_type = req.actor_type.clone();
        if actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }

        let http_method = if req.http_method.is_empty() {
            "GET".to_string()
        } else {
            req.http_method.to_uppercase()
        };
        let full_path = if req.path.is_empty() {
            format!("/api/v1/actors/{}/{}", routing_ctx.namespace(), actor_type)
        } else {
            req.path.clone()
        };
        let payload = Self::build_request_payload(&req.payload, &req.query_params, &http_method)?;
        let message = Message {
            id: if req.message_id.is_empty() {
                format!("req-{}", Ulid::new())
            } else {
                req.message_id.clone()
            },
            sender_id: req.sender_id.clone(),
            receiver_id: actor_type.clone(),
            message_type: if req.message_type.is_empty() {
                "call".to_string()
            } else {
                req.message_type.clone()
            },
            payload,
            headers: Self::build_message_metadata(&req.headers, &req.path, &req.subpath),
            correlation_id: req.correlation_id.clone(),
            reply_to: req.reply_to.clone(),
            uri_path: full_path.clone(),
            uri_method: http_method.clone(),
            ..Default::default()
        };
        let timeout =
            Self::duration_from_proto(req.timeout).or_else(|| Some(Duration::from_secs(5)));

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                tenant_id = %routing_ctx.tenant_id(),
                namespace = %routing_ctx.namespace(),
                actor_type = %actor_type,
                method = %http_method,
                path = %full_path,
                "ask_reply request started"
            );
        }

        let start = Instant::now();
        let result = self
            .route_actor_request(routing_ctx, &actor_type, message, true, timeout)
            .await;

        match result {
            Ok((resolved_actor_id, _message_id, Some(reply))) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(
                        actor_type = %actor_type,
                        actor_id = %resolved_actor_id,
                        duration_ms = start.elapsed().as_millis(),
                        reply_size = reply.payload.len(),
                        "ask_reply request completed"
                    );
                }
                Ok(Response::new(AskReplyResponse {
                    success: true,
                    payload: reply.payload,
                    headers: reply.headers,
                    actor_id: resolved_actor_id,
                    error_message: String::new(),
                }))
            }
            Ok((_resolved_actor_id, _message_id, None)) => {
                Err(Status::internal("No reply received from actor"))
            }
            Err(status) => {
                tracing::debug!(
                    actor_type = %actor_type,
                    error = %status,
                    duration_ms = start.elapsed().as_millis(),
                    "ask_reply request failed"
                );
                Err(status)
            }
        }
    }

    // ========================================================================
    // ShardGroup RPCs (data-parallel sharding)
    // ========================================================================

    async fn create_shard_group(
        &self,
        request: Request<CreateShardGroupRequest>,
    ) -> Result<Response<CreateShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let resp = self
            .create_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to create ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn spawn_actors(
        &self,
        request: Request<SpawnActorsRequest>,
    ) -> Result<Response<SpawnActorsResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = plexspaces_core::actor_context::ActorService::spawn_actors(self, &ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to spawn actors: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn bulk_update_shard_group(
        &self,
        request: Request<BulkUpdateShardGroupRequest>,
    ) -> Result<Response<BulkUpdateShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .bulk_update_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to bulk update ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn map_shard_group(
        &self,
        request: Request<MapShardGroupRequest>,
    ) -> Result<Response<MapShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let group_id = req.group_id.clone();
        let resp = self
            .map_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| {
                let error_msg = format!("Failed to map ShardGroup {}: {}", group_id, e);
                tracing::error!("{}", error_msg);
                Status::internal(error_msg)
            })?;
        Ok(Response::new(resp))
    }

    async fn scatter_gather(
        &self,
        request: Request<ScatterGatherRequest>,
    ) -> Result<Response<ScatterGatherResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .scatter_gather_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to scatter-gather ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn broadcast_shard_group(
        &self,
        request: Request<BroadcastShardGroupRequest>,
    ) -> Result<Response<BroadcastShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .broadcast_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to broadcast ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn reduce_shard_group(
        &self,
        request: Request<ReduceShardGroupRequest>,
    ) -> Result<Response<ReduceShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .reduce_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to reduce ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn all_reduce_shard_group(
        &self,
        request: Request<AllReduceShardGroupRequest>,
    ) -> Result<Response<AllReduceShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .all_reduce_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to all-reduce ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn barrier_shard_group(
        &self,
        request: Request<BarrierShardGroupRequest>,
    ) -> Result<Response<BarrierShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .barrier_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to barrier ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    async fn delete_shard_group(
        &self,
        request: Request<DeleteShardGroupRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups.get(&req.group_id).cloned()
        };

        let group = match group {
            Some(g) => g,
            None => {
                // Idempotent: succeed if group doesn't exist
                return Ok(Response::new(Empty {}));
            }
        };

        // Stop all shard actors
        let actor_factory = self
            .service_locator
            .get_actor_factory()
            .await
            .ok_or_else(|| Status::internal("Actor factory not available"))?;

        for shard_actor_id in &group.shard_actor_ids {
            if let Ok(shard_actor_id) = self.parse_canonical_actor_id(shard_actor_id) {
                let _ = actor_factory.stop_actor(&ctx, &shard_actor_id).await;
            }
        }

        // Remove from registry
        {
            let mut groups = self.shard_groups.write().await;
            groups.remove(&req.group_id);
        }

        // Unregister from TaskRouter (if registered)
        if let Some(task_router) = self.service_locator.get_task_router().await {
            if let Err(e) = task_router.unregister_group(&req.group_id).await {
                tracing::warn!(
                    group_id = %req.group_id,
                    error = %e,
                    "Failed to unregister ShardGroup from TaskRouter (non-fatal)"
                );
            } else {
                tracing::debug!(
                    group_id = %req.group_id,
                    "Unregistered ShardGroup from TaskRouter"
                );
            }
        }

        // Emit metrics
        let _ = metrics::counter!("plexspaces_shard_group_deleted_total", 
            "group_id" => req.group_id.clone());

        tracing::info!(group_id = %req.group_id, "Deleted ShardGroup");

        Ok(Response::new(Empty {}))
    }

    async fn get_shard_group(
        &self,
        request: Request<GetShardGroupRequest>,
    ) -> Result<Response<GetShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        let groups = self.shard_groups.read().await;
        let group = groups
            .get(&req.group_id)
            .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?;

        Ok(Response::new(GetShardGroupResponse {
            group: Some(group.clone()),
        }))
    }

    async fn scale_shard_group(
        &self,
        request: Request<ScaleShardGroupRequest>,
    ) -> Result<Response<ScaleShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let _ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let _req = request.into_inner();

        Err(Status::unimplemented("ScaleShardGroup not yet implemented"))
    }

    async fn list_shard_groups(
        &self,
        request: Request<ListShardGroupsRequest>,
    ) -> Result<Response<ListShardGroupsResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        let groups = self.shard_groups.read().await;
        let filtered: Vec<ShardGroup> = groups
            .values()
            .filter(|g| {
                // Filter by actor_type if specified
                if !req.actor_type.is_empty() && g.actor_type != req.actor_type {
                    return false;
                }
                // Filter by state if specified
                if req.state != ShardGroupState::ShardGroupStateUnspecified as i32
                    && g.state != req.state
                {
                    return false;
                }
                true
            })
            .cloned()
            .collect();

        // Apply pagination
        let page = req.page.unwrap_or_default();
        let offset = page.offset as usize;
        let limit = page.limit as usize;
        let total_size = filtered.len();
        let has_next = offset + limit < total_size;

        let paginated: Vec<ShardGroup> = filtered.into_iter().skip(offset).take(limit).collect();

        Ok(Response::new(ListShardGroupsResponse {
            groups: paginated,
            page: Some(plexspaces_proto::common::v1::PageResponse {
                total_size: total_size as i32,
                offset: offset as i32,
                limit: limit as i32,
                has_next,
            }),
        }))
    }

    async fn send_to_shard(
        &self,
        request: Request<SendToShardRequest>,
    ) -> Result<Response<SendToShardResponse>, Status> {
        self.check_accepting_requests().await?;
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        // Calculate shard_id from partition_key using partition strategy
        use crate::actor_service::partition::calculate_shard_id;
        let shard_id = calculate_shard_id(
            &req.partition_key,
            shard_group_config(&group).partition_strategy,
            shard_group_config(&group).shard_count,
            None, // TODO: Support range boundaries from group metadata
        )
        .map_err(|e| Status::invalid_argument(format!("Partition calculation failed: {}", e)))?;

        let shard_actor_id = group
            .shard_actor_ids
            .get(shard_id as usize)
            .ok_or_else(|| Status::internal(format!("Invalid shard_id {}", shard_id)))?
            .clone();

        // Route message to shard actor
        let mut message = req
            .message
            .ok_or_else(|| Status::invalid_argument("message is required"))?;
        message.receiver_id = shard_actor_id.clone();

        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        let response_message = if req.wait_for_response {
            let (_, response) = self
                .route_message(ctx.clone(), &shard_actor_id, message, true, timeout)
                .await?;
            response
        } else {
            let _ = self
                .route_message(ctx.clone(), &shard_actor_id, message, false, None)
                .await?;
            None
        };

        record_node_shard_messages_sent(self.local_node_id.as_str());

        // Emit metrics
        let _ = metrics::counter!("plexspaces_send_to_shard_total",
            "group_id" => req.group_id.clone(),
            "shard_id" => shard_id.to_string());

        Ok(Response::new(SendToShardResponse {
            shard_id,
            shard_actor_id,
            response: response_message,
        }))
    }
}

impl ActorServiceImpl {
    /// Resolve the target node IDs for shard placement.
    ///
    /// Strategy controls resolution semantics:
    /// - `FROM_REGISTRY` ignores `node_ids` and lists currently known nodes.
    /// - `NODE_IDS` uses only the explicit `node_ids`.
    /// - `SAME_NODE`, `UNSPECIFIED`, or no placement target the local node.
    async fn resolve_shard_group_target_nodes(
        &self,
        ctx: &RequestContext,
        placement: Option<&plexspaces_proto::actor::v1::NodePlacement>,
        local_node_id: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::actor::v1::NodePlacementStrategy;

        let strategy = placement
            .and_then(|p| NodePlacementStrategy::try_from(p.strategy).ok())
            .unwrap_or(NodePlacementStrategy::NodePlacementStrategyUnspecified);

        let target_nodes = match placement {
            Some(placement)
                if strategy == NodePlacementStrategy::NodePlacementStrategyFromRegistry =>
            {
                let node_registry =
                    self.service_locator
                        .get_node_registry()
                        .await
                        .ok_or_else(|| {
                            "NodeRegistry not available for from_registry placement".to_string()
                        })?;
                let local_cluster = self
                    .service_locator
                    .get_node_config()
                    .await
                    .map(|config| config.cluster_name)
                    .unwrap_or_default();
                let cluster = if placement.cluster.is_empty() {
                    if local_cluster.is_empty() {
                        None
                    } else {
                        Some(local_cluster.as_str())
                    }
                } else {
                    Some(placement.cluster.as_str())
                };
                let (registrations, _) = node_registry
                    .list_nodes(ctx, cluster, 1000, "")
                    .await
                    .map_err(|e| format!("list_nodes failed: {}", e))?;
                if registrations.is_empty() {
                    tracing::warn!(
                        local_node_id = %local_node_id,
                        list_cluster_filter = ?cluster,
                        node_config_cluster_name = %local_cluster,
                        placement_cluster_field = %placement.cluster,
                        "from_registry placement: list_nodes returned zero members (SWIM/cache empty or cluster label filter excluded all nodes)"
                    );
                }
                registrations
                    .into_iter()
                    .map(|registration| registration.node_id)
                    .collect()
            }
            Some(placement) if strategy == NodePlacementStrategy::NodePlacementStrategyNodeIds => {
                placement.node_ids.clone()
            }
            _ => vec![local_node_id.to_string()],
        };

        if target_nodes.is_empty() {
            return Err("Placement produced no target nodes for shard group creation".into());
        }

        Ok(target_nodes)
    }

    /// Internal implementation of create_shard_group (used by both gRPC and trait)
    async fn create_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let config = req.config.as_ref().ok_or("config is required")?;
        let group_id = config.group_id.as_str();
        let shard_count = config.shard_count;

        // Proto (buf.validate) defines: group_id min_len:1 max_len:255; shard_count gte:1 lte:1000000000; actor_type min_len:1 max_len:128.
        // Defensive check when proto validation is not invoked at the boundary.
        if shard_count == 0 {
            return Err("config.shard_count must be >= 1".into());
        }
        if shard_count > 1_000_000_000 {
            return Err("config.shard_count must be <= 1000000000".into());
        }
        if config.group_id.is_empty() {
            return Err("config.group_id is required".into());
        }
        if req.actor_type.is_empty() {
            return Err("actor_type is required".into());
        }

        // Check if group already exists
        {
            let groups = self.shard_groups.read().await;
            if groups.contains_key(group_id) {
                return Err(format!("ShardGroup {} already exists", group_id).into());
            }
        }

        let registry = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not available".to_string())?;
        let local_node_id = registry.local_node_id();

        let target_nodes = self
            .resolve_shard_group_target_nodes(ctx, config.placement.as_ref(), &local_node_id)
            .await?;

        let actor_factory = self
            .service_locator
            .get_actor_factory()
            .await
            .ok_or_else(|| "Actor factory not available".to_string())?;

        let mut shard_actor_ids = Vec::with_capacity(shard_count as usize);

        for shard_id in 0..shard_count {
            let actor_id_base = format!("{}-{}", group_id, ulid::Ulid::new());
            let initial_state = req.initial_state.clone();

            let mut shard_config = req.shard_config.clone().unwrap_or_default();
            shard_config.actor_groups.push(config.group_id.clone());
            if config.placement.is_some() {
                use plexspaces_proto::v1::actor::ActorResourceRequirements;
                shard_config.resource_requirements = Some(ActorResourceRequirements {
                    placement: config.placement.clone(),
                });
            }

            let target_node = &target_nodes[shard_id as usize % target_nodes.len()];

            if target_node == local_node_id {
                let full_id = self
                    .build_canonical_actor_id(
                        &actor_id_base,
                        &req.actor_type,
                        ctx.namespace(),
                        target_node,
                    )
                    .map_err(|e| e.to_string())?;
                match actor_factory
                    .spawn_actor(
                        &ctx,
                        &full_id,
                        &req.actor_type,
                        initial_state,
                        Some(shard_config),
                        std::collections::HashMap::new(),
                        vec![],
                    )
                    .await
                {
                    Ok(_sender) => {
                        shard_actor_ids.push(full_id.to_string());
                    }
                    Err(e) => {
                        for spawned_id in &shard_actor_ids {
                            if let Ok(spawned_id) = self.parse_canonical_actor_id(spawned_id) {
                                let _ = actor_factory.stop_actor(ctx, &spawned_id).await;
                            }
                        }
                        return Err(
                            format!("Failed to spawn shard {} (local): {}", shard_id, e).into()
                        );
                    }
                }
            } else {
                // Remote node: resolve channel and call SpawnActor on the target node via gRPC.
                // On failure we stop only locally-spawned shards; remote shards are left running
                // (caller may retry or orchestration can terminate them via TerminateActor).
                let channel = self
                    .service_locator
                    .get_actor_service_client(target_node)
                    .await
                    .map_err(|e| format!("Failed to get client for node {}: {}", target_node, e))?;
                let mut client = plexspaces_proto::ActorServiceClient::new(channel);
                let spawn_req = SpawnActorRequest {
                    actor_type: req.actor_type.clone(),
                    actor_id: actor_id_base.clone(),
                    initial_state,
                    config: Some(shard_config),
                    labels: std::collections::HashMap::new(),
                    facets: vec![],
                    namespace: ctx.namespace().to_string(),
                    instances_count: 1,
                };
                let spawn_response = client
                    .spawn_actor(tonic::Request::new(spawn_req))
                    .await
                    .map_err(|e| format!("Remote spawn to {} failed: {}", target_node, e))?;
                let actor_ref = spawn_response.into_inner().actor_ref;
                if actor_ref.is_empty() {
                    for spawned_id in &shard_actor_ids {
                        if let Ok(spawned_id) = self.parse_canonical_actor_id(spawned_id) {
                            let _ = actor_factory.stop_actor(ctx, &spawned_id).await;
                        }
                    }
                    return Err(format!(
                        "Remote spawn to {} returned empty actor_ref",
                        target_node
                    )
                    .into());
                }
                shard_actor_ids.push(actor_ref);
            }
        }

        // Create ShardGroup metadata (unified config; placement in config carries required_labels)
        let group = ShardGroup {
            config: Some(config.clone()),
            actor_type: req.actor_type.clone(),
            shard_actor_ids: shard_actor_ids.clone(),
            state: ShardGroupState::ShardGroupStateActive as i32,
            created_at: Some(prost_types::Timestamp {
                seconds: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            metadata: req.metadata.clone(),
            rebalance_status: None,
        };

        // Store group
        {
            let mut groups = self.shard_groups.write().await;
            groups.insert(config.group_id.clone(), group.clone());
        }

        // Integrate with TaskRouter for actor-level routing
        // Register ShardGroup with TaskRouter (if available)
        // This enables channel-based routing for ShardGroup operations
        if let Some(task_router) = self.service_locator.get_task_router().await {
            if let Err(e) = task_router.register_group(group.clone()).await {
                tracing::warn!(
                    group_id = %config.group_id,
                    error = %e,
                    "Failed to register ShardGroup in TaskRouter (non-fatal)"
                );
            } else {
                tracing::debug!(
                    group_id = %config.group_id,
                    shard_count = shard_actor_ids.len(),
                    "Registered ShardGroup in TaskRouter"
                );
            }
        }

        record_node_shard_groups_created(self.local_node_id.as_str());

        // Emit metrics
        metrics::counter!("plexspaces_shard_group_created_total", 
            "group_id" => config.group_id.clone(),
            "actor_type" => req.actor_type.clone(),
            "shard_count" => shard_count.to_string());

        tracing::info!(
            group_id = %config.group_id,
            shard_count = shard_count,
            "Created ShardGroup"
        );

        Ok(CreateShardGroupResponse { group: Some(group) })
    }

    /// Internal implementation of bulk_update_shard_group
    async fn bulk_update_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Implementation extracted from gRPC method
        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };

        // Route updates to appropriate shards based on partition_key
        use crate::actor_service::partition::calculate_shard_id;
        use futures::future::join_all;

        let timeout = resolve_timeout(req.timeout.as_ref());

        // Group updates by shard_id
        let total_updates = req.updates.len();
        let mut updates_by_shard: std::collections::HashMap<u32, Vec<(String, Message)>> =
            std::collections::HashMap::new();
        for (partition_key_str, mut message) in req.updates {
            let partition_key = partition_key_str.as_bytes();
            let shard_id = calculate_shard_id(
                partition_key,
                shard_group_config(&group).partition_strategy,
                shard_group_config(&group).shard_count,
                None,
            )
            .map_err(|e| format!("Partition calculation failed: {}", e))?;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .ok_or_else(|| format!("Invalid shard_id {}", shard_id))?
                .clone();

            // Ensure message ID has "req-" prefix for requests
            if message.id.is_empty() {
                message.id = format!("req-{}", ulid::Ulid::new().to_string());
            } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                message.id = format!("req-{}", message.id);
            }

            message.receiver_id = shard_actor_id.clone();
            updates_by_shard
                .entry(shard_id)
                .or_insert_with(Vec::new)
                .push((partition_key_str, message));
        }

        // Send updates to shards in parallel (reuse existing logic from gRPC method)
        // TODO: Extract common parallel update logic
        let mut handles = Vec::new();
        let mut shard_stats_map: std::collections::HashMap<u32, ShardUpdateStats> =
            std::collections::HashMap::new();

        for (shard_id, updates) in updates_by_shard {
            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .unwrap()
                .clone();
            let service_locator = self.service_locator.clone();
            let ctx = ctx.clone();
            let wait_for_responses = req.wait_for_responses;
            let consistency_level = req.consistency_level;

            let handle = tokio::spawn(async move {
                let mut succeeded = 0u32;
                let mut failed = 0u32;

                let updates_clone = updates.clone();
                match consistency_level {
                    x if x
                        == plexspaces_proto::v1::actor::ConsistencyLevel::ConsistencyLevelEventual
                            as i32 =>
                    {
                        for (_key, mut message) in updates_clone {
                            // Ensure message ID has "req-" prefix
                            if message.id.is_empty() {
                                message.id = format!("req-{}", ulid::Ulid::new().to_string());
                            } else if !message.id.starts_with("req-")
                                && !message.id.starts_with("res-")
                            {
                                message.id = format!("req-{}", message.id);
                            }
                            let receiver_id = message.receiver_id.clone();
                            let route_result = plexspaces_actor::routing::route_message(
                                ctx.clone(),
                                service_locator.clone() as Arc<dyn ServiceLocatorTrait>,
                                receiver_id,
                                message,
                                wait_for_responses,
                                if wait_for_responses {
                                    Some(timeout)
                                } else {
                                    None
                                },
                            )
                            .await;
                            if route_result.is_ok() {
                                succeeded += 1;
                            } else {
                                failed += 1;
                            }
                        }
                    }
                    _ => {
                        for (_key, mut message) in updates_clone {
                            // Ensure message ID has "req-" prefix
                            if message.id.is_empty() {
                                message.id = format!("req-{}", ulid::Ulid::new().to_string());
                            } else if !message.id.starts_with("req-")
                                && !message.id.starts_with("res-")
                            {
                                message.id = format!("req-{}", message.id);
                            }
                            let receiver_id = message.receiver_id.clone();
                            let route_result = plexspaces_actor::routing::route_message(
                                ctx.clone(),
                                service_locator.clone() as Arc<dyn ServiceLocatorTrait>,
                                receiver_id,
                                message,
                                wait_for_responses,
                                if wait_for_responses {
                                    Some(timeout)
                                } else {
                                    None
                                },
                            )
                            .await;
                            if route_result.is_ok() {
                                succeeded += 1;
                            } else {
                                failed += 1;
                            }
                        }
                    }
                }

                (shard_id, succeeded, failed, updates.len() as u32)
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        let results = tokio::time::timeout(timeout, join_all(handles))
            .await
            .map_err(|_| "Bulk update timeout")?;

        let mut total_sent = 0u32;
        let mut total_succeeded = 0u32;
        let mut total_failed = 0u32;
        let mut shard_stats = Vec::new();

        for result in results {
            let (shard_id, succeeded, failed, sent) = result.unwrap_or((0, 0, 0, 0));
            total_sent += sent;
            total_succeeded += succeeded;
            total_failed += failed;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .cloned()
                .unwrap_or_default();

            shard_stats.push(ShardUpdateStats {
                shard_id,
                shard_actor_id,
                updates_sent: sent,
                updates_succeeded: succeeded,
                updates_failed: failed,
            });
        }

        Ok(BulkUpdateShardGroupResponse {
            updates_sent: total_sent,
            updates_succeeded: total_succeeded,
            updates_failed: total_failed,
            shard_stats,
            errors: Vec::new(),
        })
    }

    /// Unified parallel operation helper (Erlang pmap pattern)
    ///
    /// ## Design
    /// Uses `routing::ask_helper()` for each shard with one shared temporary sender created by
    /// `ActorFactory::create_temporary_sender`. Each shard still gets its own correlation id and
    /// waiter entry, while local delivery continues to flow through `ActorRegistry::tell()`.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace (CRITICAL: flows from API → ActorBuilder → ActorRef)
    /// * `group_id` - Shard group identifier
    /// * `shard_actor_ids` - List of shard actor IDs to query in parallel
    /// * `query_message` - Message to send to each shard
    /// * `timeout` - Timeout for each shard operation
    /// * `operation_name` - Operation name for logging/metrics
    async fn parallel_operation_unified(
        &self,
        ctx: RequestContext,
        group_id: String,
        shard_actor_ids: Vec<String>,
        query_message: Message,
        timeout: Duration,
        operation_name: &str,
    ) -> Result<
        Vec<(u32, String, Duration, bool, String, Option<Message>)>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let start_time = Instant::now();

        tracing::info!(
            group_id = %group_id,
            shard_count = shard_actor_ids.len(),
            timeout_secs = timeout.as_secs(),
            tenant_id = %ctx.tenant_id(),
            "🔄 [{}] Starting parallel operation (ask_helper)",
            operation_name
        );

        // CRITICAL: Use ONE canonical temporary sender actor for all shards.
        // Each shard gets its own correlation_id (format: "req-shard-{shard_id}-{ulid}" for debugging)
        // All replies go to the same temporary sender ActorRef, but are routed to the correct ReplyWaiter
        // by correlation_id via ReplyWaiterRegistry (which supports multiple correlation_ids)
        use plexspaces_core::TEMP_SENDER_PREFIX;
        let operation_id = Ulid::new().to_string();
        let temp_sender_id = self
            .build_canonical_actor_id(
                &format!("{}_{}", TEMP_SENDER_PREFIX, operation_id),
                plexspaces_core::TEMP_SENDER_ACTOR_TYPE,
                ctx.namespace(),
                &self.local_node_id,
            )
            .map_err(|e| e.to_string())?;
        let expires_at = Instant::now() + (timeout * 2);
        // CRITICAL: Use RequestContext from caller (tenant_id flows from API → ActorBuilder → ActorRef)

        // Create ONE temporary sender for all shards (use operation_id as correlation_id for registration)
        // The actual routing uses ReplyWaiterRegistry keyed by per-shard correlation_ids
        let factory = self
            .service_locator
            .get_actor_factory()
            .await
            .ok_or_else(|| "ActorFactory not found in ServiceLocator".to_string())?;
        factory
            .create_temporary_sender(
                &ctx,
                temp_sender_id.clone(),
                operation_id.clone(),
                expires_at,
            )
            .await
            .map_err(|e| format!("Failed to create temp sender: {}", e))?;

        tracing::info!(
            group_id = %group_id,
            temp_sender_id = %temp_sender_id,
            shard_count = shard_actor_ids.len(),
            "✅ [{}] Created temp sender, sending {} ask_helper requests...",
            operation_name,
            shard_actor_ids.len()
        );

        let service_locator = self.service_locator.clone();
        let mut handles = Vec::with_capacity(shard_actor_ids.len());
        for (shard_id, shard_actor_id) in shard_actor_ids.iter().enumerate() {
            // Each shard gets its own correlation_id (format: "req-shard-{shard_id}-{ulid}" for debugging)
            // This ensures each shard's reply is routed to the correct ReplyWaiter
            let correlation_id = format!("req-shard-{}-{}", shard_id, Ulid::new().to_string());
            let request_start = Instant::now();
            let mut msg = query_message.clone();
            // Ensure message ID has "req-" prefix for requests
            let message_id = if msg.id.is_empty() {
                format!("req-{}", Ulid::new().to_string())
            } else if !msg.id.starts_with("req-") && !msg.id.starts_with("res-") {
                format!("req-{}", msg.id)
            } else {
                msg.id.clone()
            };
            msg.id = message_id.clone();
            msg.receiver_id = shard_actor_id.clone();
            msg.message_type = "call".to_string();

            let sl = service_locator.clone();
            let tid = temp_sender_id.clone();
            let cid = correlation_id.clone();
            let sid = shard_actor_id.clone();
            let mid = message_id.clone();
            let t = timeout;
            // CRITICAL: Clone RequestContext for each task (tenant_id flows from API → ActorBuilder → ActorRef)
            let ctx_task = ctx.clone();

            let handle = tokio::spawn(async move {
                use plexspaces_actor::routing::ask_helper;
                let result =
                    ask_helper(ctx_task, sl, sid.clone(), msg, tid.to_string(), cid.clone(), t)
                        .await;
                (shard_id as u32, sid, request_start, result)
            });
            handles.push(handle);
        }

        // Await all handles in parallel using join_all (true parallel map/reduce)
        // This enables all asks to be sent asynchronously, then all replies collected in parallel
        // All ask_helper() calls return Futures, so we can await them all together
        let join_results = join_all(handles).await;

        let mut results = Vec::with_capacity(join_results.len());
        for join_result in join_results {
            let (shard_id, shard_actor_id, request_start, result) =
                join_result.map_err(|e| format!("Task join error: {}", e))?;
            let latency = request_start.elapsed();
            match result {
                Ok(reply) => {
                    tracing::debug!(
                        group_id = %group_id,
                        shard_id = shard_id,
                        actor_id = %shard_actor_id,
                        latency_ms = latency.as_millis(),
                        "✅ [{}] Received reply",
                        operation_name
                    );
                    results.push((
                        shard_id,
                        shard_actor_id,
                        latency,
                        true,
                        String::new(),
                        Some(reply),
                    ));
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    tracing::warn!(
                        group_id = %group_id,
                        shard_id = shard_id,
                        actor_id = %shard_actor_id,
                        latency_ms = latency.as_millis(),
                        error = %error_msg,
                        "❌ [{}] Shard failed",
                        operation_name
                    );
                    results.push((shard_id, shard_actor_id, latency, false, error_msg, None));
                }
            }
        }

        // Sort by shard_id for consistent ordering
        results.sort_by_key(|r| r.0);

        let received_count = results.iter().filter(|r| r.3).count();
        let failed_count = results.len() - received_count;

        // Track shard messages received (for all successful replies)
        for result in &results {
            if result.3 {
                record_node_shard_messages_received(self.local_node_id.as_str());
            }
        }

        if failed_count > 0 {
            let errors: Vec<String> = results
                .iter()
                .filter_map(|r| {
                    if !r.3 {
                        Some(format!("Shard {} ({}): {}", r.0, r.1, r.4))
                    } else {
                        None
                    }
                })
                .collect();
            tracing::warn!(
                group_id = %group_id,
                total_duration_ms = start_time.elapsed().as_millis(),
                received = received_count,
                failed = failed_count,
                total = results.len(),
                errors = ?errors,
                "⚠️  [{}] Collected replies: {}/{} succeeded, {} failed",
                operation_name,
                received_count,
                results.len(),
                failed_count
            );
        } else {
            tracing::info!(
                group_id = %group_id,
                total_duration_ms = start_time.elapsed().as_millis(),
                received = received_count,
                total = results.len(),
                "✅ [{}] Collected replies: {}/{} succeeded",
                operation_name,
                received_count,
                results.len()
            );
        }

        // Cleanup: Remove the single shared temporary sender after all per-correlation waiters
        // created by ask_helper() have been cleaned up.
        if let Some(registry) = self.service_locator.actor_registry().await {
            registry.remove_temporary_sender(&temp_sender_id).await;
        }

        Ok(results)
    }

    /// Internal implementation of map_shard_group
    /// Uses unified parallel operation helper (Erlang pmap pattern)
    async fn map_shard_group_internal(
        &self,
        _ctx: &RequestContext,
        req: MapShardGroupRequest,
    ) -> Result<MapShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();
        let group_id = req.group_id.clone();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(10));

        let query_proto = req
            .map_function
            .ok_or_else(|| "map_function is required".to_string())?;

        // Use unified parallel operation helper
        // CRITICAL: Pass RequestContext with tenant_id (flows from API → ActorBuilder → ActorRef)
        let results = self
            .parallel_operation_unified(
                _ctx.clone(),
                group_id.clone(),
                group.shard_actor_ids.clone(),
                query_proto,
                timeout,
                "MAP_SHARD_GROUP",
            )
            .await?;

        // Convert results to response format
        let mut shard_responses = Vec::new();
        let mut shards_responded = 0;
        let mut shards_failed = 0;
        let mut max_latency = Duration::ZERO;
        let mut min_latency = Duration::MAX;

        for (shard_id, shard_actor_id, latency, success, error, proto_response) in results {
            if success {
                shards_responded += 1;
                if latency < min_latency {
                    min_latency = latency;
                }
            } else {
                shards_failed += 1;
            }
            if latency > max_latency {
                max_latency = latency;
            }

            shard_responses.push(ShardQueryResponse {
                shard_id,
                shard_actor_id,
                response: proto_response,
                latency: Some(prost_types::Duration {
                    seconds: latency.as_secs() as i64,
                    nanos: latency.subsec_nanos() as i32,
                }),
                success,
                error,
            });
        }

        let total_duration = start_time.elapsed();

        record_node_shard_operation(self.local_node_id.as_str());
        if shards_failed > 0 {
            record_node_shard_operation_failed(self.local_node_id.as_str());
        }

        if shards_failed > 0 {
            let failed_shards: Vec<String> = shard_responses
                .iter()
                .filter_map(|r| {
                    if !r.success {
                        Some(format!(
                            "Shard {} ({}): {}",
                            r.shard_id, r.shard_actor_id, r.error
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            tracing::warn!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = shard_group_config(&group).shard_count,
                shards_responded,
                shards_failed,
                failed_shards = ?failed_shards,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "⚠️  [MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded, {} failed",
                shards_responded,
                shard_group_config(&group).shard_count,
                shards_failed
            );
        } else {
            tracing::info!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = shard_group_config(&group).shard_count,
                shards_responded,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "✅ [MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded successfully",
                shards_responded,
                shard_group_config(&group).shard_count
            );
        }

        use plexspaces_proto::actor::v1::ScatterGatherStats;
        Ok(MapShardGroupResponse {
            shard_results: shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: shard_group_config(&group).shard_count,
                shards_responded,
                shards_failed,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        })
    }

    /// Internal implementation of scatter_gather
    /// Uses unified parallel operation helper (Erlang pmap pattern)
    async fn scatter_gather_internal(
        &self,
        _ctx: &RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();
        let group_id = req.group_id.clone();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));

        let query = req.query.ok_or_else(|| "query is required".to_string())?;

        // Use unified parallel operation helper
        // CRITICAL: Pass RequestContext with tenant_id (flows from API → ActorBuilder → ActorRef)
        let results = self
            .parallel_operation_unified(
                _ctx.clone(),
                group_id.clone(),
                group.shard_actor_ids.clone(),
                query,
                timeout,
                "SCATTER_GATHER",
            )
            .await?;

        // Convert results to response format and aggregate
        let mut shard_responses = Vec::new();
        let mut shards_responded = 0;
        let mut shards_failed = 0;
        let mut max_latency = Duration::ZERO;
        let mut min_latency = Duration::MAX;
        let mut successful_responses = Vec::new();

        for (shard_id, shard_actor_id, latency, success, error, proto_response) in results {
            if success {
                shards_responded += 1;
                if latency < min_latency {
                    min_latency = latency;
                }
                if let Some(ref resp) = proto_response {
                    successful_responses.push((shard_id, resp.clone()));
                }
            } else {
                shards_failed += 1;
            }
            if latency > max_latency {
                max_latency = latency;
            }

            shard_responses.push(ShardQueryResponse {
                shard_id,
                shard_actor_id,
                response: proto_response,
                latency: Some(prost_types::Duration {
                    seconds: latency.as_secs() as i64,
                    nanos: latency.subsec_nanos() as i32,
                }),
                success,
                error,
            });
        }

        // Check minimum responses requirement
        if shards_responded < req.min_responses as usize {
            let error_msg = format!(
                "Scatter-gather failed: only {} shards responded, minimum required: {}",
                shards_responded, req.min_responses
            );
            tracing::error!(
                group_id = %group_id,
                shards_responded,
                min_required = req.min_responses,
                "❌ [SCATTER_GATHER] {}",
                error_msg
            );
            return Err(error_msg.into());
        }

        // Aggregate results based on strategy
        let result = match req.aggregation {
            x if x == ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32 => {
                // Concatenate all successful responses
                let mut aggregated_payloads = Vec::new();
                for (_shard_id, resp) in successful_responses {
                    aggregated_payloads.push(resp.payload);
                }
                Some(Message {
                    id: format!("scatter-gather-{}", Ulid::new()),
                    sender_id: "scatter-gather".to_string(),
                    receiver_id: String::new(),
                    channel: String::new(),
                    message_type: "aggregated".to_string(),
                    payload: aggregated_payloads.concat(),
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    headers: std::collections::HashMap::new(),
                    priority: 0,
                    ttl: None,
                    delivery_count: 0,
                    idempotency_key: String::new(),
                    correlation_id: String::new(),
                    reply_to: String::new(),
                    partition_key: String::new(),
                    uri_path: String::new(),
                    uri_method: String::new(),
                })
            }
            x if x == ShardGroupAggregationStrategy::ShardGroupAggregationMerge as i32 => {
                // Sum numeric values from all responses
                let mut sum: i64 = 0;
                for (_shard_id, resp) in successful_responses {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&resp.payload) {
                        if let Some(num) = value.as_i64() {
                            sum += num;
                        } else if let Some(num) = value.as_f64() {
                            sum += num as i64;
                        }
                    }
                }
                Some(Message {
                    id: format!("scatter-gather-{}", Ulid::new()),
                    sender_id: "scatter-gather".to_string(),
                    receiver_id: String::new(),
                    channel: String::new(),
                    message_type: "aggregated".to_string(),
                    payload: serde_json::json!({ "sum": sum }).to_string().into_bytes(),
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    headers: std::collections::HashMap::new(),
                    priority: 0,
                    ttl: None,
                    delivery_count: 0,
                    idempotency_key: String::new(),
                    correlation_id: String::new(),
                    reply_to: String::new(),
                    partition_key: String::new(),
                    uri_path: String::new(),
                    uri_method: String::new(),
                })
            }
            _ => {
                // Default: return first successful response or None
                successful_responses
                    .first()
                    .map(|(_shard_id, resp)| resp.clone())
            }
        };

        let total_duration = start_time.elapsed();

        record_node_shard_operation(self.local_node_id.as_str());
        if shards_failed > 0 {
            record_node_shard_operation_failed(self.local_node_id.as_str());
        }

        tracing::info!(
            group_id = %group_id,
            total_duration_ms = total_duration.as_millis(),
            shards_queried = shard_group_config(&group).shard_count,
            shards_responded,
            shards_failed,
            min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
            max_latency_ms = max_latency.as_millis(),
            has_aggregated_result = result.is_some(),
            "✅ [SCATTER_GATHER] Scatter-gather operation completed: {}/{} shards responded successfully",
            shards_responded,
            shard_group_config(&group).shard_count
        );

        Ok(ScatterGatherResponse {
            result,
            shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: shard_group_config(&group).shard_count,
                shards_responded: shards_responded as u32,
                shards_failed: shards_failed as u32,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        })
    }

    async fn broadcast_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };
        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));
        let message = req.message.ok_or("message is required")?;
        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                req.group_id.clone(),
                group.shard_actor_ids.clone(),
                message,
                timeout,
                "BROADCAST_SHARD_GROUP",
            )
            .await?;
        let stats = scatter_stats_from_results(shard_group_config(&group).shard_count, &results);
        if stats.shards_responded < req.min_acks {
            return Err(format!(
                "Broadcast failed: only {} shards acknowledged, minimum required: {}",
                stats.shards_responded, req.min_acks
            )
            .into());
        }
        Ok(BroadcastShardGroupResponse {
            shard_responses: shard_query_responses_from_results(results),
            stats: Some(stats),
        })
    }

    async fn reduce_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };
        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));
        let map_function = req.map_function.ok_or("map_function is required")?;
        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                req.group_id.clone(),
                group.shard_actor_ids.clone(),
                map_function,
                timeout,
                "REDUCE_SHARD_GROUP",
            )
            .await?;
        let stats = scatter_stats_from_results(shard_group_config(&group).shard_count, &results);
        if stats.shards_responded < req.min_responses {
            return Err(format!(
                "Reduce failed: only {} shards responded, minimum required: {}",
                stats.shards_responded, req.min_responses
            )
            .into());
        }
        let mut values = Vec::new();
        for (_shard_id, _actor_id, _latency, success, _error, response) in &results {
            if *success {
                let response = response
                    .as_ref()
                    .ok_or("Missing shard response for successful reduction")?;
                values.push(select_collective_value(response, req.target.as_ref())?);
            }
        }
        let reduced_value = reduce_values(values, req.reduction as i32)?;
        let result = build_collective_message(
            "collective",
            serde_json::to_vec(&reduced_value)?,
            HashMap::from([
                ("plexspaces-collective-op".to_string(), "reduce".to_string()),
                ("plexspaces-group-id".to_string(), req.group_id.clone()),
            ]),
        );
        Ok(ReduceShardGroupResponse {
            result: Some(result),
            shard_responses: shard_query_responses_from_results(results),
            stats: Some(stats),
        })
    }

    async fn all_reduce_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let reduce_req = ReduceShardGroupRequest {
            group_id: req.group_id.clone(),
            map_function: req.map_function.clone(),
            timeout: req.timeout.clone(),
            min_responses: req.min_responses,
            reduction: req.reduction,
            target: req.target.clone(),
        };
        let reduce_resp = self.reduce_shard_group_internal(ctx, reduce_req).await?;
        let reduced_message = reduce_resp
            .result
            .clone()
            .ok_or("AllReduce failed to produce reduced result")?;
        let mut broadcast_message = reduced_message.clone();
        broadcast_message.message_type = "event".to_string();
        broadcast_message.headers.insert(
            "plexspaces-collective-op".to_string(),
            "all-reduce-result".to_string(),
        );
        let broadcast_resp = self
            .broadcast_shard_group_internal(
                ctx,
                BroadcastShardGroupRequest {
                    group_id: req.group_id,
                    message: Some(broadcast_message),
                    timeout: req.timeout,
                    min_acks: req.min_responses,
                },
            )
            .await?;
        Ok(AllReduceShardGroupResponse {
            result: Some(reduced_message),
            shard_responses: broadcast_resp.shard_responses,
            stats: broadcast_resp.stats,
        })
    }

    async fn barrier_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let payload = serde_json::json!({
            "barrier_id": req.barrier_id,
            "round": req.round,
        });
        let message = build_collective_message(
            "info",
            serde_json::to_vec(&payload)?,
            HashMap::from([(
                "plexspaces-collective-op".to_string(),
                "barrier".to_string(),
            )]),
        );
        let response = self
            .broadcast_shard_group_internal(
                ctx,
                BroadcastShardGroupRequest {
                    group_id: req.group_id,
                    message: Some(message),
                    timeout: req.timeout,
                    min_acks: req.min_acks,
                },
            )
            .await?;
        Ok(BarrierShardGroupResponse {
            shard_responses: response.shard_responses,
            stats: response.stats,
        })
    }
}

impl ActorServiceImpl {
    async fn bulk_update_shard_group(
        &self,
        request: Request<BulkUpdateShardGroupRequest>,
    ) -> Result<Response<BulkUpdateShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &(self.service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self
            .bulk_update_shard_group_internal(&ctx, req)
            .await
            .map_err(|e| Status::internal(format!("Failed to bulk update ShardGroup: {}", e)))?;
        Ok(Response::new(resp))
    }

    #[allow(dead_code)]
    async fn bulk_update_shard_group_old(
        &self,
        request: Request<BulkUpdateShardGroupRequest>,
    ) -> Result<Response<BulkUpdateShardGroupResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        // Route updates to appropriate shards based on partition_key
        use crate::actor_service::partition::calculate_shard_id;
        use futures::future::join_all;

        let timeout = resolve_timeout(req.timeout.as_ref());

        // Group updates by shard_id
        let mut updates_by_shard: std::collections::HashMap<u32, Vec<(String, Message)>> =
            std::collections::HashMap::new();
        for (partition_key_str, mut message) in req.updates {
            let partition_key = partition_key_str.as_bytes();
            let shard_id = calculate_shard_id(
                partition_key,
                shard_group_config(&group).partition_strategy,
                shard_group_config(&group).shard_count,
                None,
            )
            .map_err(|e| {
                Status::invalid_argument(format!("Partition calculation failed: {}", e))
            })?;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .ok_or_else(|| Status::internal(format!("Invalid shard_id {}", shard_id)))?
                .clone();

            message.receiver_id = shard_actor_id.clone();
            updates_by_shard
                .entry(shard_id)
                .or_insert_with(Vec::new)
                .push((partition_key_str, message));
        }

        // Send updates to shards in parallel
        let mut handles = Vec::new();
        let mut shard_stats_map: std::collections::HashMap<u32, ShardUpdateStats> =
            std::collections::HashMap::new();

        for (shard_id, updates) in updates_by_shard {
            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .unwrap()
                .clone();
            let service_locator = self.service_locator.clone();
            let wait_for_responses = req.wait_for_responses;
            let consistency_level = req.consistency_level;

            let handle = tokio::spawn(async move {
                let mut succeeded = 0u32;
                let mut failed = 0u32;

                // Send updates based on consistency level
                // Clone updates for iteration (needed because updates is moved in first match arm)
                let updates_clone = updates.clone();
                match consistency_level {
                    x if x
                        == plexspaces_proto::v1::actor::ConsistencyLevel::ConsistencyLevelEventual
                            as i32 =>
                    {
                        // Eventual consistency: send all updates, don't wait
                        for (_key, message) in updates_clone {
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> =
                                service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                if let Ok(receiver_id) =
                                    ActorId::from_canonical(&message.receiver_id)
                                {
                                    if registry.tell(&receiver_id, message).await.is_ok() {
                                        succeeded += 1;
                                    } else {
                                        failed += 1;
                                    }
                                } else {
                                    failed += 1;
                                }
                            } else {
                                failed += 1;
                            }
                        }
                    }
                    _ => {
                        // Stronger consistency: send sequentially or with coordination
                        // For now, send sequentially (can be optimized later)
                        for (_key, message) in updates_clone {
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> =
                                service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                let receiver_id = match ActorId::from_canonical(&message.receiver_id)
                                {
                                    Ok(receiver_id) => receiver_id,
                                    Err(_) => {
                                        failed += 1;
                                        continue;
                                    }
                                };
                                if wait_for_responses {
                                    let ask_ctx = RequestContext::new_without_auth(
                                        String::new(),
                                        String::new(),
                                    );
                                    if registry
                                        .ask(
                                            &ask_ctx,
                                            &receiver_id,
                                            message,
                                            Duration::from_secs(5),
                                        )
                                        .await
                                        .is_ok()
                                    {
                                        succeeded += 1;
                                    } else {
                                        failed += 1;
                                    }
                                } else {
                                    if registry.tell(&receiver_id, message).await.is_ok() {
                                        succeeded += 1;
                                    } else {
                                        failed += 1;
                                    }
                                }
                            } else {
                                failed += 1;
                            }
                        }
                    }
                }

                (shard_id, succeeded, failed, updates.len() as u32)
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        let results = tokio::time::timeout(timeout, join_all(handles))
            .await
            .map_err(|_| Status::deadline_exceeded("Bulk update timeout"))?;

        let mut total_sent = 0u32;
        let mut total_succeeded = 0u32;
        let mut total_failed = 0u32;
        let mut shard_stats = Vec::new();

        for result in results {
            let (shard_id, succeeded, failed, sent) = result.unwrap_or((0, 0, 0, 0));
            total_sent += sent;
            total_succeeded += succeeded;
            total_failed += failed;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .cloned()
                .unwrap_or_default();

            shard_stats.push(ShardUpdateStats {
                shard_id,
                shard_actor_id,
                updates_sent: sent,
                updates_succeeded: succeeded,
                updates_failed: failed,
            });
        }

        // Emit metrics
        metrics::counter!("plexspaces_bulk_update_shard_group_total",
            "group_id" => req.group_id.clone());
        metrics::histogram!("plexspaces_bulk_update_shard_group_updates",
            "group_id" => req.group_id.clone())
        .record(total_sent as f64);

        Ok(Response::new(BulkUpdateShardGroupResponse {
            updates_sent: total_sent,
            updates_succeeded: total_succeeded,
            updates_failed: total_failed,
            shard_stats,
            errors: Vec::new(), // TODO: Collect actual errors
        }))
    }
}

impl ActorServiceImpl {
    /// Extract tenant_id from JWT claims in request metadata (x-tenant-id header set by JWT middleware).
    fn extract_tenant_id_from_jwt(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
        // Try to get from x-tenant-id header (set by JWT middleware)
        metadata
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }
}

/// Newtype wrapper for Arc<ActorServiceImpl> to implement ActorServiceTrait
///
/// This wrapper exists to satisfy Rust's orphan rules - we can't implement
/// a foreign trait (ActorServiceTrait) for a foreign type (Arc<T>), but we
/// can implement it for our own newtype.
///
/// ## Why This Exists
/// Tonic's ActorServiceServer requires a type that implements ActorService.
/// By wrapping Arc<ActorServiceImpl> in a newtype, we can:
/// 1. Clone the Arc for the gRPC server (which needs ownership)
/// 2. Keep references for other uses (registration, local routing)
/// 3. Follow the Node pattern for lifecycle management
#[derive(Clone)]
pub struct ActorServiceWrapper(Arc<ActorServiceImpl>);

impl ActorServiceWrapper {
    /// Create a new wrapper around ActorServiceImpl
    pub fn new(inner: Arc<ActorServiceImpl>) -> Self {
        Self(inner)
    }

    /// Get a reference to the inner ActorServiceImpl
    pub fn inner(&self) -> &Arc<ActorServiceImpl> {
        &self.0
    }
}

impl From<Arc<ActorServiceImpl>> for ActorServiceWrapper {
    fn from(inner: Arc<ActorServiceImpl>) -> Self {
        Self(inner)
    }
}

/// Implement ActorService trait for ActorServiceWrapper
///
/// All methods delegate to the inner ActorServiceImpl.
#[async_trait]
impl ActorServiceTrait for ActorServiceWrapper {
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        // Use fully qualified syntax to call the trait method, not the public method
        ActorServiceTrait::send_message(&*self.0, request).await
    }

    async fn spawn_actor(
        &self,
        request: Request<SpawnActorRequest>,
    ) -> Result<Response<SpawnActorResponse>, Status> {
        // Use fully qualified syntax to call the trait method
        ActorServiceTrait::spawn_actor(&*self.0, request).await
    }

    async fn get_actor(
        &self,
        request: Request<GetActorRequest>,
    ) -> Result<Response<GetActorResponse>, Status> {
        self.0.get_actor(request).await
    }

    async fn list_actors(
        &self,
        request: Request<ListActorsRequest>,
    ) -> Result<Response<ListActorsResponse>, Status> {
        self.0.list_actors(request).await
    }

    async fn delete_actor(
        &self,
        request: Request<DeleteActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.0.delete_actor(request).await
    }

    async fn set_actor_state(
        &self,
        request: Request<SetActorStateRequest>,
    ) -> Result<Response<SetActorStateResponse>, Status> {
        self.0.set_actor_state(request).await
    }

    async fn migrate_actor(
        &self,
        request: Request<MigrateActorRequest>,
    ) -> Result<Response<MigrateActorResponse>, Status> {
        self.0.migrate_actor(request).await
    }

    type StreamMessagesStream =
        Pin<Box<dyn Stream<Item = Result<StreamMessageResponse, Status>> + Send>>;

    async fn stream_messages(
        &self,
        request: Request<tonic::Streaming<StreamMessageRequest>>,
    ) -> Result<Response<Self::StreamMessagesStream>, Status> {
        self.0.stream_messages(request).await
    }

    async fn monitor_actor(
        &self,
        request: Request<MonitorActorRequest>,
    ) -> Result<Response<MonitorActorResponse>, Status> {
        self.0.monitor_actor(request).await
    }

    async fn notify_actor_down(
        &self,
        request: Request<ActorDownNotification>,
    ) -> Result<Response<Empty>, Status> {
        self.0.notify_actor_down(request).await
    }

    async fn link_actor(
        &self,
        request: Request<LinkActorRequest>,
    ) -> Result<Response<LinkActorResponse>, Status> {
        self.0.link_actor(request).await
    }

    async fn unlink_actor(
        &self,
        request: Request<UnlinkActorRequest>,
    ) -> Result<Response<UnlinkActorResponse>, Status> {
        self.0.unlink_actor(request).await
    }

    async fn check_actor_exists(
        &self,
        request: Request<CheckActorExistsRequest>,
    ) -> Result<Response<CheckActorExistsResponse>, Status> {
        self.0.check_actor_exists(request).await
    }

    async fn ask_reply(
        &self,
        request: Request<AskReplyRequest>,
    ) -> Result<Response<AskReplyResponse>, Status> {
        self.0.ask_reply(request).await
    }

    async fn create_shard_group(
        &self,
        request: Request<CreateShardGroupRequest>,
    ) -> Result<Response<CreateShardGroupResponse>, Status> {
        self.0.create_shard_group(request).await
    }

    async fn delete_shard_group(
        &self,
        request: Request<DeleteShardGroupRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.0.delete_shard_group(request).await
    }

    async fn get_shard_group(
        &self,
        request: Request<GetShardGroupRequest>,
    ) -> Result<Response<GetShardGroupResponse>, Status> {
        self.0.get_shard_group(request).await
    }

    async fn scale_shard_group(
        &self,
        request: Request<ScaleShardGroupRequest>,
    ) -> Result<Response<ScaleShardGroupResponse>, Status> {
        self.0.scale_shard_group(request).await
    }

    async fn list_shard_groups(
        &self,
        request: Request<ListShardGroupsRequest>,
    ) -> Result<Response<ListShardGroupsResponse>, Status> {
        self.0.list_shard_groups(request).await
    }

    async fn send_to_shard(
        &self,
        request: Request<SendToShardRequest>,
    ) -> Result<Response<SendToShardResponse>, Status> {
        self.0.send_to_shard(request).await
    }

    async fn broadcast_shard_group(
        &self,
        request: Request<BroadcastShardGroupRequest>,
    ) -> Result<Response<BroadcastShardGroupResponse>, Status> {
        self.0.broadcast_shard_group(request).await
    }

    async fn reduce_shard_group(
        &self,
        request: Request<ReduceShardGroupRequest>,
    ) -> Result<Response<ReduceShardGroupResponse>, Status> {
        self.0.reduce_shard_group(request).await
    }

    async fn all_reduce_shard_group(
        &self,
        request: Request<AllReduceShardGroupRequest>,
    ) -> Result<Response<AllReduceShardGroupResponse>, Status> {
        self.0.all_reduce_shard_group(request).await
    }

    async fn barrier_shard_group(
        &self,
        request: Request<BarrierShardGroupRequest>,
    ) -> Result<Response<BarrierShardGroupResponse>, Status> {
        self.0.barrier_shard_group(request).await
    }

    async fn scatter_gather(
        &self,
        request: Request<ScatterGatherRequest>,
    ) -> Result<Response<ScatterGatherResponse>, Status> {
        self.0.scatter_gather(request).await
    }

    async fn bulk_update_shard_group(
        &self,
        request: Request<BulkUpdateShardGroupRequest>,
    ) -> Result<Response<BulkUpdateShardGroupResponse>, Status> {
        self.0.bulk_update_shard_group(request).await
    }

    async fn map_shard_group(
        &self,
        request: Request<MapShardGroupRequest>,
    ) -> Result<Response<MapShardGroupResponse>, Status> {
        self.0.map_shard_group(request).await
    }

    async fn spawn_actors(
        &self,
        request: Request<SpawnActorsRequest>,
    ) -> Result<Response<SpawnActorsResponse>, Status> {
        self.0.spawn_actors(request).await
    }
}

pub mod partition;

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::{MessageSender, ObjectRegistry as ObjectRegistryTrait};
    use plexspaces_mailbox::{mailbox_config_default, Mailbox};
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy};
    use plexspaces_proto::node::v1::{NodeCapacity, NodeRegistration};
    use plexspaces_proto::object_registry::v1::ObjectRegistration;
    use plexspaces_proto::object_registry::v1::ObjectType;
    use std::time::Duration as StdDuration;
    use ulid::Ulid;

    /// Helper to create a test message with proto Message type
    fn create_test_message(payload: Vec<u8>) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    fn create_test_send_message_request(message: Message) -> SendMessageRequest {
        SendMessageRequest {
            namespace: String::new(),
            actor_type: message.receiver_id.clone(),
            http_method: "POST".to_string(),
            payload: message.payload,
            headers: message.headers,
            query_params: HashMap::new(),
            path: String::new(),
            subpath: String::new(),
            sender_id: message.sender_id,
            message_type: if message.message_type.is_empty() {
                "cast".to_string()
            } else {
                message.message_type
            },
            correlation_id: message.correlation_id,
            reply_to: message.reply_to,
            message_id: message.id,
        }
    }

    /// Simple wrapper to adapt ObjectRegistryImpl to ObjectRegistryTrait
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistryImpl>,
    }

    struct MockNodeRegistry {
        nodes: Vec<NodeRegistration>,
    }

    #[async_trait::async_trait]
    impl plexspaces_core::NodeRegistryTrait for MockNodeRegistry {
        async fn lookup_node(
            &self,
            _ctx: &RequestContext,
            node_id: &str,
        ) -> Result<Option<NodeRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(self
                .nodes
                .iter()
                .find(|node| node.node_id == node_id)
                .cloned())
        }

        async fn register_node(
            &self,
            _ctx: &RequestContext,
            _registration: NodeRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn unregister_node(
            &self,
            _ctx: &RequestContext,
            _node_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn list_nodes(
            &self,
            _ctx: &RequestContext,
            cluster: Option<&str>,
            _page_size: u32,
            _page_token: &str,
        ) -> Result<(Vec<NodeRegistration>, String), Box<dyn std::error::Error + Send + Sync>>
        {
            let registrations = self
                .nodes
                .iter()
                .filter(|node| {
                    cluster.is_none_or(|cluster_name| {
                        node.capabilities
                            .get("cluster")
                            .map(|value| value == cluster_name)
                            .unwrap_or(false)
                    })
                })
                .cloned()
                .collect();
            Ok((registrations, String::new()))
        }

        async fn send_heartbeat(
            &self,
            _ctx: &RequestContext,
            _node_id: &str,
            _capacity: Option<NodeCapacity>,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        fn start_gossip_protocol(&self) {}

        fn stop_gossip_protocol(&self) {}

        fn is_gossip_running(&self) -> bool {
            false
        }

        async fn kickoff_seed_reconcile_ping(
            &self,
            _node_id: String,
            _node_address: String,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }

        async fn cache_stats(&self) -> (usize, usize, StdDuration) {
            (self.nodes.len(), 0, StdDuration::from_secs(0))
        }
    }

    #[async_trait::async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            );
            self.inner.lookup(ctx, obj_type, object_id).await.map_err(
                |e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                },
            )
        }

        async fn lookup_full(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .lookup_full(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }

        async fn register(
            &self,
            ctx: &plexspaces_core::RequestContext,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner.register(ctx, registration).await.map_err(
                |e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                },
            )
        }

        async fn discover(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            object_category: Option<String>,
            capabilities: Option<Vec<String>>,
            labels: Option<Vec<String>>,
            health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            offset: usize,
            limit: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .discover(
                    ctx,
                    object_type,
                    object_category,
                    capabilities,
                    labels,
                    health_status,
                    offset,
                    limit,
                )
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }

        async fn unregister(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .unregister(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }

        async fn heartbeat(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .heartbeat(ctx, object_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    ))
                })
        }
    }

    /// Helper to create a test ActorRegistry (async because SqliteObjectRegistryRepository::new is async)
    async fn create_test_registry(local_node_id: &str) -> Arc<ActorRegistry> {
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        Arc::new(ActorRegistry::new(
            object_registry,
            local_node_id.to_string(),
        ))
    }

    /// Helper to create ActorServiceImpl with proper ServiceLocator setup for tests
    async fn create_test_actor_service(
        actor_registry: Arc<ActorRegistry>,
        node_id: String,
    ) -> ActorServiceImpl {
        use crate::service_locator::ServiceLocatorImpl;
        use plexspaces_core::ServiceLocator as ServiceLocatorTrait;
        // Create ServiceLocatorImpl directly
        let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
        // Disable auth so tests can call gRPC methods without JWT
        service_locator_impl
            .register_security_config(plexspaces_proto::node::v1::SecurityConfig {
                disable_auth: true,
                ..Default::default()
            })
            .await;
        // Initialize services first — this registers GrpcConnectionManager and other
        // services.  The idempotency guard in initialize_services triggers on
        // actor_registry being present, so we must NOT register actor_registry before
        // this call.
        service_locator_impl
            .initialize_services(Some(plexspaces_proto::node::v1::ReleaseSpec {
                node: Some(plexspaces_proto::node::v1::NodeConfig {
                    id: node_id.clone(),
                    listen_addr: "127.0.0.1:0".to_string(),
                    grpc_connection_pool_size: 2,
                    max_connections: 100,
                    heartbeat_interval_ms: 5000,
                    clustering_enabled: true,
                    ..Default::default()
                }),
                ..Default::default()
            }))
            .await;
        // Override actor_registry with the test-specific one (which has test actors).
        // VirtualActorManager must always be present in the actor-registry; set it here.
        {
            use plexspaces_core::VirtualActorManager;
            let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
            actor_registry
                .set_virtual_actor_manager(virtual_actor_manager)
                .await;
        }
        service_locator_impl
            .register_actor_registry(actor_registry.clone())
            .await;
        ActorServiceImpl::new(service_locator_impl, node_id)
    }

    async fn create_test_actor_service_impl_with_node_registry(
        nodes: Vec<NodeRegistration>,
    ) -> ActorServiceImpl {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
        let mock_registry: Arc<dyn plexspaces_core::NodeRegistryTrait> =
            Arc::new(MockNodeRegistry { nodes });
        service
            .service_locator
            .register_node_registry(mock_registry)
            .await;
        service
    }

    /// Helper to register an actor with ActorRegistry for tests
    async fn register_test_actor(
        actor_registry: Arc<ActorRegistry>,
        actor_id: ActorId,
        mailbox: Arc<Mailbox>,
        service_locator: Arc<dyn ServiceLocatorTrait>,
        actor_type: &str,
        namespace: &str,
    ) {
        // CRITICAL: Pass tenant_id from RequestContext to ActorRef (empty for tests)
        let sender: Arc<dyn MessageSender> = Arc::new(plexspaces_actor::ActorRef::local(
            actor_id.clone(),
            String::new(), // Test context uses empty tenant_id
            namespace.to_string(),
            mailbox,
            service_locator,
        ));
        // Tenant comes from auth, not config - use empty strings for test actor registration
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(String::new(), namespace.to_string());
        actor_registry
            .register_actor(
                &ctx,
                actor_id,
                sender,
                actor_type.to_string(),
                None,
                None,
                None,
            )
            .await;
    }

    fn test_actor_id(name: &str, actor_type: &str, namespace: &str, node_id: &str) -> ActorId {
        ActorId::new(name, actor_type, namespace, node_id).expect("test actor id should be valid")
    }

    /// Helper to create a test ActorRegistry with a node registration
    async fn create_test_registry_with_node(
        local_node_id: &str,
        node_id: &str,
        node_address: &str,
    ) -> Arc<ActorRegistry> {
        let object_repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));

        // Register node using ObjectTypeNode
        // Use internal context for system operations (node registration is system-level)
        let ctx = plexspaces_core::RequestContext::new_without_auth(String::new(), String::new())
            .with_admin(true);
        let registration = ObjectRegistration {
            object_id: node_id.to_string(),
            object_type: ObjectType::ObjectTypeNode as i32,
            object_category: "Node".to_string(),
            grpc_address: node_address.to_string(),
            ..Default::default()
        };

        object_registry_impl
            .register(&ctx, registration)
            .await
            .unwrap();

        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        Arc::new(ActorRegistry::new(
            object_registry,
            local_node_id.to_string(),
        ))
    }

    #[tokio::test]
    async fn test_resolve_shard_group_target_nodes_from_registry_ignores_node_ids() {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let mock_registry: Arc<dyn plexspaces_core::NodeRegistryTrait> =
            Arc::new(MockNodeRegistry {
                nodes: vec![
                    NodeRegistration {
                        node_id: "node1".to_string(),
                        node_address: "127.0.0.1:9001".to_string(),
                        capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                        ..Default::default()
                    },
                    NodeRegistration {
                        node_id: "node2".to_string(),
                        node_address: "127.0.0.1:9002".to_string(),
                        capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                        ..Default::default()
                    },
                ],
            });
        service
            .service_locator
            .register_node_registry(mock_registry)
            .await;

        let placement = NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
            cluster: "heat".to_string(),
            node_ids: vec!["stale-node".to_string()],
            required_labels: HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: None,
            affinity_labels: HashMap::new(),
        };

        let target_nodes = service
            .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
            .await
            .expect("target nodes should resolve from registry");

        assert_eq!(target_nodes, vec!["node1".to_string(), "node2".to_string()]);
    }

    #[tokio::test]
    async fn test_resolve_shard_group_target_nodes_from_registry_defaults_to_local_cluster() {
        let service = create_test_actor_service_impl_with_node_registry(vec![
            NodeRegistration {
                node_id: "node1".to_string(),
                node_address: "127.0.0.1:9001".to_string(),
                capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                ..Default::default()
            },
            NodeRegistration {
                node_id: "node2".to_string(),
                node_address: "127.0.0.1:9002".to_string(),
                capabilities: HashMap::from([("cluster".to_string(), "heat".to_string())]),
                ..Default::default()
            },
            NodeRegistration {
                node_id: "node3".to_string(),
                node_address: "127.0.0.1:9003".to_string(),
                capabilities: HashMap::from([("cluster".to_string(), "other".to_string())]),
                ..Default::default()
            },
        ])
        .await;

        service
            .service_locator
            .register_node_config(plexspaces_proto::node::v1::NodeConfig {
                id: "node1".to_string(),
                cluster_name: "heat".to_string(),
                ..Default::default()
            })
            .await;

        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let placement = plexspaces_proto::actor::v1::NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
            cluster: String::new(),
            node_ids: vec![],
            required_labels: HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: None,
            affinity_labels: HashMap::new(),
        };

        let target_nodes = service
            .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
            .await
            .unwrap();

        assert_eq!(target_nodes, vec!["node1".to_string(), "node2".to_string()]);
    }

    #[tokio::test]
    async fn test_resolve_shard_group_target_nodes_from_registry_errors_when_empty() {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry, "node1".to_string()).await;
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        let mock_registry: Arc<dyn plexspaces_core::NodeRegistryTrait> =
            Arc::new(MockNodeRegistry { nodes: Vec::new() });
        service
            .service_locator
            .register_node_registry(mock_registry)
            .await;

        let placement = NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
            cluster: String::new(),
            node_ids: vec!["stale-node".to_string()],
            required_labels: HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: None,
            affinity_labels: HashMap::new(),
        };

        let error = service
            .resolve_shard_group_target_nodes(&ctx, Some(&placement), "node1")
            .await
            .expect_err("empty registry placement should fail");

        assert!(error
            .to_string()
            .contains("Placement produced no target nodes for shard group creation"));
    }

    #[tokio::test]
    async fn test_register_and_unregister_local_actor() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        let actor_id = test_actor_id("test", "TestActor", "default", "node1");

        let mailbox = Arc::new(
            Mailbox::new(mailbox_config_default(), actor_id.to_string())
                .await
                .expect("Failed to create mailbox"),
        );
        let _actor_ref = plexspaces_core::ActorRef::new(actor_id.clone()).unwrap();

        // ACT: Register actor
        register_test_actor(
            actor_registry.clone(),
            actor_id.clone(),
            Arc::clone(&mailbox),
            service.service_locator.clone(),
            "TestActor",
            "default",
        )
        .await;

        // ASSERT: Actor is in cache
        {
            let is_activated = service
                .get_actor_registry()
                .await
                .is_actor_activated(&actor_id)
                .await;
            assert!(is_activated);
        }

        // ACT: Unregister actor
        actor_registry
            .unregister(&actor_id)
            .await
            .unwrap();

        // ASSERT: Actor is removed from cache
        {
            let is_activated = service
                .get_actor_registry()
                .await
                .is_actor_activated(&actor_id)
                .await;
            assert!(!is_activated);
        }
    }

    #[tokio::test]
    async fn test_parse_actor_id() {
        let actor_id = ActorId::from_canonical("counter//worker::default@node1")
            .expect("canonical actor id should parse");
        assert_eq!(actor_id.name(), "counter");
        assert_eq!(actor_id.actor_type(), "worker");
        assert_eq!(actor_id.namespace(), "default");
        assert_eq!(actor_id.node_id(), "node1");
        assert_eq!(actor_id.to_string(), "counter//worker::default@node1");

        assert!(ActorId::from_canonical("counter@node1").is_err());
        assert!(ActorId::from_canonical("invalid_no_node_separator").is_err());
    }

    #[tokio::test]
    async fn test_canonical_actor_id_from_client_target_resolves_bare_live_actor_type() {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(
            Mailbox::new(
                mailbox_config_default(),
                "controller//controller::app-ns@node1".to_string(),
            )
            .await
            .expect("Failed to create mailbox"),
        );
        let actor_id = test_actor_id("controller", "controller", "app-ns", "node1");
        let ctx = RequestContext::new_without_auth(String::new(), "app-ns".to_string());

        register_test_actor(
            actor_registry,
            actor_id.clone(),
            mailbox,
            service.service_locator.clone(),
            "controller",
            "app-ns",
        )
        .await;

        let resolved = service
            .canonical_actor_id_from_client_target(&ctx, "controller")
            .await;

        assert_eq!(resolved, Some(actor_id.to_string()));
    }

    #[test]
    fn test_set_message_receiver_id_uses_canonical_actor_id() {
        let mut message = create_test_message(b"test".to_vec());
        message.receiver_id = "controller:cart-1".to_string();

        ActorServiceImpl::set_message_receiver_id(&mut message, "cart-1//controller::app-ns@node1");

        assert_eq!(message.receiver_id, "cart-1//controller::app-ns@node1");
    }

    // ========================================================================
    // COVERAGE TESTS - route_message()
    // ========================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_route_message_invalid_actor_id() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route with a non-canonical actor ID. Parsing should fail before lookup.
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_message(ctx, "invalid_no_node", message, false, None)
            .await;

        // ASSERT: Should fail because the actor ID is not canonical
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("Invalid actor ID"));
    }

    #[tokio::test]
    async fn test_route_message_local_routing() {
        // ARRANGE: Create actor and register it locally
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        let actor_id = test_actor_id("test", "TestActor", "default", "node1");

        let mailbox = Arc::new(
            Mailbox::new(mailbox_config_default(), actor_id.to_string())
                .await
                .expect("Failed to create mailbox"),
        );
        let _actor_ref = plexspaces_core::ActorRef::new(actor_id.clone()).unwrap();
        register_test_actor(
            actor_registry.clone(),
            actor_id.clone(),
            Arc::clone(&mailbox),
            service.service_locator.clone(),
            "TestActor",
            "default",
        )
        .await;

        let message = create_test_message(b"hello".to_vec());
        let message_id = message.id.to_string();

        // ACT: Route message via route_message() entry point
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_message(ctx, actor_id.as_str(), message, false, None)
            .await;

        // ASSERT: Should route locally
        assert!(result.is_ok());
        let (returned_id, response) = result.unwrap();
        assert_eq!(returned_id, message_id);
        assert!(response.is_none());

        // Verify message delivered (poll immediately - no sleep needed)
        let delivered = mailbox.dequeue().await;
        assert!(
            delivered.is_some(),
            "Message should be delivered immediately"
        );
        assert_eq!(delivered.unwrap().payload, b"hello");
    }

    // ========================================================================
    // COVERAGE TESTS - send_message() gRPC Handler
    // ========================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_message_missing_actor_type() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let request = tonic::Request::new(SendMessageRequest {
            namespace: String::new(),
            actor_type: String::new(),
            http_method: "POST".to_string(),
            payload: Vec::new(),
            headers: HashMap::new(),
            query_params: HashMap::new(),
            path: String::new(),
            subpath: String::new(),
            sender_id: String::new(),
            message_type: "cast".to_string(),
            correlation_id: String::new(),
            reply_to: String::new(),
            message_id: String::new(),
        });

        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should fail with InvalidArgument
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("actor_type"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_message_missing_receiver() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mut proto_message = create_test_message(b"test".to_vec());
        proto_message.receiver_id = String::new();
        let request = tonic::Request::new(create_test_send_message_request(proto_message));

        // ACT
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should fail with InvalidArgument
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("actor_type"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_message_success() {
        // ARRANGE: Create actor and register it
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        let actor_id = test_actor_id("test", "TestActor", "default", "node1");

        let mailbox = Arc::new(
            Mailbox::new(mailbox_config_default(), actor_id.to_string())
                .await
                .expect("Failed to create mailbox"),
        );
        let _actor_ref = plexspaces_core::ActorRef::new(actor_id.clone()).unwrap();
        register_test_actor(
            actor_registry.clone(),
            actor_id.clone(),
            Arc::clone(&mailbox),
            service.service_locator.clone(),
            "TestActor",
            "default",
        )
        .await;

        // Create proto message with a valid JSON payload (send_message validates JSON)
        let json_payload = b"{\"msg\":\"hello\"}".to_vec();
        let mut message = create_test_message(json_payload.clone());
        message.receiver_id = actor_id.to_string();
        let proto_message = message.clone();
        let expected_message_id = proto_message.id.clone();

        let request = tonic::Request::new(create_test_send_message_request(proto_message));

        // ACT: Send via gRPC handler
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should succeed
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        assert_eq!(response.message_id, expected_message_id);
        assert!(response.success);

        // Verify delivery (poll immediately - no sleep needed)
        let delivered = mailbox.dequeue().await;
        assert!(
            delivered.is_some(),
            "Message should be delivered immediately"
        );
        assert_eq!(delivered.unwrap().payload, json_payload);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_message_with_timeout() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        let actor_id = test_actor_id("test", "TestActor", "default", "node1");

        let mailbox = Arc::new(
            Mailbox::new(mailbox_config_default(), actor_id.to_string())
                .await
                .expect("Failed to create mailbox"),
        );
        let _actor_ref = plexspaces_core::ActorRef::new(actor_id.clone()).unwrap();
        register_test_actor(
            actor_registry.clone(),
            actor_id.clone(),
            Arc::clone(&mailbox),
            service.service_locator.clone(),
            "TestActor",
            "default",
        )
        .await;

        // Create message with timeout and valid JSON payload (send_message validates JSON)
        let mut message = create_test_message(b"{}".to_vec());
        message.receiver_id = actor_id.to_string();
        let proto_message = message.clone();

        let request = tonic::Request::new(create_test_send_message_request(proto_message));

        // ACT: Send with timeout (though fire-and-forget ignores it)
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should succeed (timeout parsed but not used for fire-and-forget)
        assert!(result.is_ok());
    }

    // ========================================================================
    // Connection Manager
    // ========================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_connection_manager_available() {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        // Test that connection manager is available
        let conn_manager = service.service_locator.get_grpc_connection_manager().await;
        assert!(
            conn_manager.is_some(),
            "GrpcConnectionManager should be available"
        );
    }

    // ========================================================================
    // COVERAGE TESTS - send_message() timeout conversion
    // ========================================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_send_message_converts_timeout_correctly() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        let actor_id = test_actor_id("test", "TestActor", "default", "node1");

        let mailbox = Arc::new(
            Mailbox::new(mailbox_config_default(), actor_id.to_string())
                .await
                .expect("Failed to create mailbox"),
        );
        let _actor_ref = plexspaces_core::ActorRef::new(actor_id.clone()).unwrap();
        register_test_actor(
            actor_registry.clone(),
            actor_id.clone(),
            Arc::clone(&mailbox),
            service.service_locator.clone(),
            "TestActor",
            "default",
        )
        .await;

        // Create message with fractional seconds timeout and valid JSON payload
        let mut message = create_test_message(b"{}".to_vec());
        message.receiver_id = actor_id.to_string();
        let proto_message = message.clone();

        let request = tonic::Request::new(create_test_send_message_request(proto_message));

        // ACT: Send with fractional timeout
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should succeed (timeout converted correctly)
        assert!(
            result.is_ok(),
            "Expected success, got error: {:?}",
            result.err()
        );
    }

    // ========================================================================
    // INTEGRATION TEST - route_remote() Success Path with Real gRPC Server
    // ========================================================================

    // Integration tests cover real gRPC server scenarios; unit tests focus on local routing and simulated remote.
    // - test_route_remote_connection_pooling
    // - test_route_remote_with_timeout
    // - test_route_remote_multi_node_scenario
    // These tests used real gRPC servers with sleep calls, making them slow and flaky.
    // Integration tests in crates/actor-service/tests/integration/ cover these scenarios.

    #[tokio::test]
    async fn test_route_remote_error_handling() {
        // Test: Error handling for network failures and invalid addresses
        // ARRANGE: Create service with invalid node address
        let invalid_address = "127.0.0.1:1"; // Port 1 is typically not in use
        let registry = create_test_registry_with_node("node1", "node2", invalid_address).await;
        let service = create_test_actor_service(registry.clone(), "node1".to_string()).await;

        // Service registration is synchronous - verify immediately
        use plexspaces_core::service_names;
        assert!(
            service.service_locator.actor_registry().await.is_some(),
            "ActorRegistry should be registered synchronously"
        );

        // ACT: Try to route to unreachable node
        let message = create_test_message(b"test".to_vec());
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_message(ctx, "actor//worker::default@node2", message, false, None)
            .await;

        // ASSERT: Should fail with appropriate error
        assert!(result.is_err(), "Should fail when node is unreachable");
        let err = result.unwrap_err();
        // Error should indicate connection failure
        assert!(
            err.code() == tonic::Code::Unavailable || err.code() == tonic::Code::Internal,
            "Expected Unavailable or Internal error, got {:?}: {}",
            err.code(),
            err.message()
        );
    }
}
