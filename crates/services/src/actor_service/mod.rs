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
//! Replies are routed via `PendingAsks` — a ULID-keyed DashMap of oneshot channels.
//! No temporary actor object or mailbox is created per ask.
//!
//! ### Local ask() flow
//! ```
//! 1. ask() registers a oneshot channel in PendingAsks keyed by ULID correlation_id.
//! 2. Request dispatched to target actor's mailbox.
//! 3. Actor calls ctx.send_reply(); reply addressed to temporary-sender ActorId.
//! 4. dispatch_local_message intercepts: ActorId::is_temporary_sender() == true.
//! 5. PendingAsks::resolve(correlation_id, reply) fires the oneshot channel.
//! 6. ask() rx.await returns the reply.
//! ```
//!
//! ### Remote ask() flow
//! ```
//! 1. ask() detects remote node (ActorId::is_on_node() == false).
//! 2. Delegates directly to gRPC AskReply — no PendingAsks entry created.
//! 3. gRPC transport handles request-reply inline.
//! ```
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
//! // Register ActorRegistry (with PendingAsks) in service_locator first
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
#![allow(clippy::result_large_err)]

mod activation;
pub mod partition;
mod routing;
mod supervision;

use async_trait::async_trait;

use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use crate::ServiceLocatorImpl;
use plexspaces_actor::parallel::shard_group_config;
use plexspaces_actor::{
    monitoring::record_node_shard_messages_sent, ActorId, ActorRegistry, RequestContext,
    RequestContextExt, ServiceLocator as ServiceLocatorTrait,
};
use plexspaces_proto::common::v1::Message;
use plexspaces_service_traits::ServiceLocatorBase;
use std::collections::HashMap;
use std::time::{Duration, Instant};
use ulid::Ulid;

// Import proto types and gRPC service trait
use plexspaces_proto::actor::v1::{
    // gRPC service trait and server
    actor_service_server::ActorService as ActorServiceTrait,
    ActorDownNotification,
    ActorState,
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
    DemonitorActorRequest,
    GetActorRequest,
    GetActorResponse,
    GetActorStatesRequest,
    GetActorStatesResponse,
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
    MonitorActorRequest,
    MonitorActorResponse,
    ReduceShardGroupRequest,
    ReduceShardGroupResponse,
    ScaleShardGroupRequest,
    ScaleShardGroupResponse,
    ScatterGatherRequest,
    ScatterGatherResponse,
    // Request/Response types (from proto)
    SendMessageRequest,
    SendMessageResponse,
    SendToShardRequest,
    SendToShardResponse,
    ShardGroup,
    ShardGroupState,
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
    /// Services (ActorRegistry with PendingAsks) should already be registered in ServiceLocator
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

    /// Returns the service locator for this actor service
    pub fn service_locator(&self) -> Arc<ServiceLocatorImpl> {
        self.service_locator.clone()
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

    /// Get ActorRegistry from ServiceLocator (lazy initialization)
    async fn get_actor_registry(&self) -> Arc<ActorRegistry> {
        self.service_locator
            .actor_registry()
            .await
            .expect("ActorRegistry must be registered in ServiceLocator")
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
        } else {
            Ok(payload.to_vec())
        }
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
}

/// Implement Service trait for ActorServiceImpl (for ServiceLocator registration)
impl plexspaces_actor::Service for ActorServiceImpl {
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
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let request_id = req.request_id.clone();
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

        // If actor_name is provided, route through the full resolution pipeline using
        // "actor_name:actor_type" format so Steps 1-3 (registry, virtual type, definition slot)
        // all apply. This ensures definition-slot patterns (e.g. "session-1:ephemeral" where
        // "ephemeral" is a named definition) work the same as URL-based routing.
        let actor_target = if !req.actor_name.is_empty() {
            format!("{}:{}", req.actor_name, actor_type)
        } else {
            actor_type.clone()
        };

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
            receiver_id: actor_target.clone(),
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
            actor_target = %actor_target,
            method = %http_method,
            path = %full_path,
            "send_message tell request started"
        );

        let start = Instant::now();
        let result = self
            .route_actor_request(routing_ctx, &actor_target, message, false, None)
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
                    request_id,
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
        if self.service_locator.is_shutting_down().await {
            return Err(Status::unavailable(
                "Service is shutting down and not accepting new requests",
            ));
        }

        // This is the gRPC handler - it spawns locally on this node
        // gRPC is already remote, so "remote" in the name was redundant
        // The actor is spawned locally on THIS node (the one receiving the gRPC request)

        // Labels for JWT/context derivation live on spec.labels (proto-first spawn contract).
        let labels_for_ctx = request
            .get_ref()
            .spec
            .as_ref()
            .map(|s| s.labels.clone())
            .unwrap_or_default();

        // Create RequestContext from request metadata (before consuming request)
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &labels_for_ctx,
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();

        if req.instances_count > 1 {
            return Err(Status::invalid_argument(
                "instances_count > 1 is not supported on SpawnActor; use SpawnActorsRequest",
            ));
        }

        let mut spec = req
            .spec
            .ok_or_else(|| Status::invalid_argument("spec is required"))?;
        if !req.namespace.is_empty() {
            spec.namespace = req.namespace.clone();
        }

        let identity = spec
            .identity
            .as_ref()
            .ok_or_else(|| Status::invalid_argument("spec.identity is required"))?;
        if identity.actor_type.is_empty() {
            return Err(Status::invalid_argument(
                "spec.identity.actor_type is required",
            ));
        }
        let actor_type = identity.actor_type.clone();

        let node_id = self.local_node_id.clone();
        let effective_namespace = if spec.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            spec.namespace.clone()
        };
        let effective_ctx = RequestContext::new_without_auth(
            ctx.tenant_id().to_string(),
            effective_namespace.clone(),
        );

        let actor_id = if !identity.name.is_empty() {
            if let Ok(parsed) = ActorId::from_canonical(&identity.name) {
                if parsed.node_id() != node_id.as_str() {
                    return Err(Status::invalid_argument(format!(
                        "Actor '{}' targets node '{}' but this service only spawns on local node '{}'",
                        identity.name,
                        parsed.node_id(),
                        node_id
                    )));
                }
                parsed
            } else {
                self.build_canonical_actor_id(
                    &identity.name,
                    &actor_type,
                    &effective_namespace,
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
                &actor_type,
                &effective_namespace,
                &node_id,
            )?
        };

        let mut init_args = spec.args.clone();
        let effective_role = spec.role.clone();
        if init_args.is_empty() && !effective_role.is_empty() {
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                if let Some(definition) = manager
                    .get_virtual_actor_definition(effective_ctx.namespace(), &effective_role)
                    .await
                {
                    init_args = definition.spec.args;
                }
            }
        }

        let config = spec.config.clone();
        let labels = spec.labels.clone();
        let facets = spec.facets.clone();

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
        use plexspaces_actor::ActorSpawnSpec;
        use plexspaces_proto::common::v1::ActorIdentity;

        let spawn_spec = ActorSpawnSpec {
            identity: Some(ActorIdentity {
                name: actor_id.name().to_string(),
                actor_type: actor_type.clone(),
            }),
            role: effective_role,
            namespace: effective_ctx.namespace().to_string(),
            tenant_id: effective_ctx.tenant_id().to_string(),
            visibility: spec.visibility,
            behavior_kind: spec.behavior_kind.clone(),
            args: init_args,
            facets: facets.clone(),
            config: config.clone(),
            labels: labels.clone(),
            register_in_object_registry: spec.register_in_object_registry,
            enforce_unique_placement: spec.enforce_unique_placement,
            placement_strategy: spec.placement_strategy,
        };

        let actor_factory_opt: Option<Arc<dyn ActorFactory>> =
            self.service_locator.get_actor_factory().await;

        let spawned_actor_ref = if let Some(factory) = actor_factory_opt {
            let spawned_actor_ref = factory
                .spawn_actor(&effective_ctx, &spawn_spec, facet_boxes)
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

        let actor_state_bytes = plexspaces_actor::wasm_init_payload(&spawn_spec, &actor_id);

        // Build proto Actor message for response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: spawned_actor_id.to_string(),
            name: actor_id.name().to_string(),
            actor_type,
            state: ActorState::ActorStateActive as i32,
            node_id: node_id.clone(),
            vm_id: String::new(),
            actor_state: actor_state_bytes,
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
            request_id: req.request_id.clone(),
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
        request: Request<DeleteActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.check_accepting_requests().await?;
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
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

        if req.actor_id.is_empty() {
            return Err(Status::invalid_argument("actor_id is required"));
        }

        // Resolve name:type shorthand or canonical ID to a fully-qualified canonical ID.
        let canonical_id = self
            .canonical_actor_id_from_client_target(&routing_ctx, &req.actor_id)
            .await
            .unwrap_or_else(|| req.actor_id.clone());

        let actor_id = plexspaces_actor::ActorId::from_canonical(&canonical_id).map_err(|e| {
            Status::invalid_argument(format!("Invalid actor_id '{}': {}", req.actor_id, e))
        })?;

        let factory = self
            .service_locator
            .get_actor_factory()
            .await
            .ok_or_else(|| Status::internal("ActorFactory not available"))?;

        factory
            .stop_actor(&routing_ctx, &actor_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to stop actor: {}", e)))?;

        Ok(Response::new(Empty {}))
    }

    // ========================================================================
    // Actor State Management RPCs
    // ========================================================================

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
        request: Request<MonitorActorRequest>,
    ) -> Result<Response<MonitorActorResponse>, Status> {
        self.check_accepting_requests().await?;
        let metadata = request.metadata().clone();
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let routing_ctx = crate::request_context_from_grpc_request(
            &metadata,
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();

        let target_id = self.parse_canonical_actor_id(&req.actor_id)?;
        let supervisor_id = self
            .parse_canonical_actor_id(&req.supervisor_id)
            .map_err(|_| Status::invalid_argument("Invalid supervisor_id"))?;

        let registry = self.get_actor_registry().await;

        let monitor_ref = registry
            .monitor(&routing_ctx, &target_id, &supervisor_id)
            .await
            .map_err(|e| match e {
                plexspaces_actor::ActorRegistryError::LinkMonitorDenied(m) => {
                    Status::permission_denied(m)
                }
                plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                    Status::permission_denied(m)
                }
                other => Status::internal(format!("monitor failed: {}", other)),
            })?;

        Ok(Response::new(MonitorActorResponse {
            request_id: req.request_id.clone(),
            monitor_ref,
        }))
    }

    async fn demonitor_actor(
        &self,
        request: Request<DemonitorActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.check_accepting_requests().await?;
        let metadata = request.metadata().clone();
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let routing_ctx = crate::request_context_from_grpc_request(
            &metadata,
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();

        let target_id = self.parse_canonical_actor_id(&req.actor_id)?;
        let supervisor_id = self
            .parse_canonical_actor_id(&req.supervisor_id)
            .map_err(|_| Status::invalid_argument("Invalid supervisor_id"))?;

        let registry = self.get_actor_registry().await;
        let monitors = registry.actor_monitor().get_monitors(&target_id).await;
        if let Some(link) = monitors.iter().find(|l| l.monitor_ref == req.monitor_ref) {
            if link.monitoring_actor_id != supervisor_id {
                return Err(Status::permission_denied(
                    "monitor_ref is not owned by supervisor_id",
                ));
            }
        }

        registry
            .demonitor(&routing_ctx, &target_id, &supervisor_id, &req.monitor_ref)
            .await
            .map_err(|e| match e {
                plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                    Status::permission_denied(m)
                }
                plexspaces_actor::ActorRegistryError::LinkMonitorDenied(m) => {
                    Status::permission_denied(m)
                }
                other => Status::internal(format!("demonitor failed: {}", other)),
            })?;

        Ok(Response::new(Empty {}))
    }

    async fn notify_actor_down(
        &self,
        request: Request<ActorDownNotification>,
    ) -> Result<Response<Empty>, Status> {
        self.check_accepting_requests().await?;
        let metadata = request.metadata().clone();
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let req = request.into_inner();

        let supervisor_id = self
            .parse_canonical_actor_id(&req.supervisor_id)
            .map_err(|_| Status::invalid_argument("Invalid supervisor_id"))?;
        let terminated_id = self
            .parse_canonical_actor_id(&req.actor_id)
            .map_err(|_| Status::invalid_argument("Invalid actor_id"))?;

        let routing_ctx = crate::request_context_from_grpc_request(
            &metadata,
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .unwrap_or_else(|_| {
            plexspaces_actor::RequestContext::new_without_auth(
                "system".into(),
                supervisor_id.namespace().to_string(),
            )
            .with_internal(true)
        });

        let registry = self.get_actor_registry().await;

        if req.is_link_signal && req.reason != "normal" && req.reason != "shutdown" {
            // Link EXIT signal: kill the linked actor on this node.
            let linked_reason = plexspaces_actor::ExitReason::Linked {
                actor_id: terminated_id,
                reason: Box::new(plexspaces_actor::ExitReason::Error(req.reason)),
            };
            registry
                .handle_actor_termination(&supervisor_id, linked_reason)
                .await;
        } else {
            // Monitor DOWN: deliver __DOWN__ message to the supervisor's mailbox.
            let down_msg = plexspaces_actor::actor_monitor::create_down_message(
                &terminated_id,
                &req.monitor_ref,
                &req.reason,
            );
            registry
                .tell(&routing_ctx, &supervisor_id, down_msg)
                .await
                .map_err(|e| match e {
                    plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                        Status::permission_denied(m)
                    }
                    other => Status::internal(format!("tell failed: {}", other)),
                })?;
        }

        Ok(Response::new(Empty {}))
    }

    async fn link_actor(
        &self,
        request: Request<LinkActorRequest>,
    ) -> Result<Response<LinkActorResponse>, Status> {
        self.check_accepting_requests().await?;
        let metadata = request.metadata().clone();
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let routing_ctx = crate::request_context_from_grpc_request(
            &metadata,
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();

        let a = self.parse_canonical_actor_id(&req.actor_id)?;
        let b = self.parse_canonical_actor_id(&req.linked_actor_id)?;

        self.get_actor_registry()
            .await
            .link(&routing_ctx, &a, &b)
            .await
            .map_err(|e| match e {
                plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                    Status::permission_denied(m)
                }
                plexspaces_actor::ActorRegistryError::LinkMonitorDenied(m) => {
                    Status::permission_denied(m)
                }
                other => Status::internal(format!("link failed: {}", other)),
            })?;

        Ok(Response::new(LinkActorResponse {
            request_id: req.request_id.clone(),
            success: true,
        }))
    }

    async fn unlink_actor(
        &self,
        request: Request<UnlinkActorRequest>,
    ) -> Result<Response<UnlinkActorResponse>, Status> {
        self.check_accepting_requests().await?;
        let metadata = request.metadata().clone();
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let routing_ctx = crate::request_context_from_grpc_request(
            &metadata,
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();

        let a = self.parse_canonical_actor_id(&req.actor_id)?;
        let b = self.parse_canonical_actor_id(&req.linked_actor_id)?;

        self.get_actor_registry()
            .await
            .unlink(&routing_ctx, &a, &b)
            .await
            .map_err(|e| match e {
                plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                    Status::permission_denied(m)
                }
                plexspaces_actor::ActorRegistryError::LinkMonitorDenied(m) => {
                    Status::permission_denied(m)
                }
                other => Status::internal(format!("unlink failed: {}", other)),
            })?;

        Ok(Response::new(UnlinkActorResponse {
            request_id: req.request_id.clone(),
            success: true,
        }))
    }

    async fn get_actor_states(
        &self,
        request: Request<GetActorStatesRequest>,
    ) -> Result<Response<GetActorStatesResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();
        let registry = self.get_actor_registry().await;

        let mut states = HashMap::new();
        for actor_id_str in &req.actor_ids {
            let actor_id = self.parse_canonical_actor_id(actor_id_str)?;
            // Use the actor's local_state_handle (via ActorRegistry::get_actor_state) for
            // authoritative state, falling back to registration status.
            let state = if let Some(proto_state) = registry.get_actor_state(&actor_id).await {
                proto_state as i32
            } else if registry.is_actor_registered(&actor_id).await {
                ActorState::ActorStateInactive as i32
            } else {
                ActorState::ActorStateTerminated as i32
            };
            states.insert(actor_id_str.clone(), state);
        }

        Ok(Response::new(GetActorStatesResponse {
            request_id: req.request_id.clone(),
            states,
        }))
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
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let request_id = req.request_id.clone();
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

        // If actor_name is provided, route through the full resolution pipeline using
        // "actor_name:actor_type" format so Steps 1-3 (registry, virtual type, definition slot)
        // all apply. This ensures definition-slot patterns (e.g. "session-1:ephemeral" where
        // "ephemeral" is a named definition) work the same as URL-based routing.
        let actor_target = if !req.actor_name.is_empty() {
            format!("{}:{}", req.actor_name, actor_type)
        } else {
            actor_type.clone()
        };

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
            receiver_id: actor_target.clone(),
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
                actor_target = %actor_target,
                method = %http_method,
                path = %full_path,
                "ask_reply request started"
            );
        }

        let start = Instant::now();
        let result = self
            .route_actor_request(routing_ctx, &actor_target, message, true, timeout)
            .await;

        match result {
            Ok((resolved_actor_id, _message_id, Some(reply))) => {
                let is_error = reply.message_type == "error_reply";
                let error_message = if is_error {
                    serde_json::from_slice::<serde_json::Value>(&reply.payload)
                        .ok()
                        .and_then(|v| v.get("error")?.as_str().map(String::from))
                        .unwrap_or_else(|| "Actor handler failed".to_string())
                } else {
                    String::new()
                };
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(
                        actor_type = %actor_type,
                        actor_id = %resolved_actor_id,
                        duration_ms = start.elapsed().as_millis(),
                        reply_size = reply.payload.len(),
                        is_error = is_error,
                        "ask_reply request completed"
                    );
                }
                Ok(Response::new(AskReplyResponse {
                    request_id,
                    success: !is_error,
                    payload: reply.payload,
                    headers: reply.headers,
                    actor_id: resolved_actor_id,
                    error_message,
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = plexspaces_actor::actor_context::ActorService::spawn_actors(self, &ctx, req)
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            request_id: req.request_id.clone(),
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
            &(self.service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>),
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
            request_id: req.request_id.clone(),
            groups: paginated,
            page: Some(plexspaces_proto::common::v1::PageResponse {
                request_id: ulid::Ulid::new().to_string(),
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
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
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
            request_id: req.request_id.clone(),
            shard_id,
            shard_actor_id,
            response: response_message,
        }))
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

    async fn demonitor_actor(
        &self,
        request: Request<DemonitorActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.0.demonitor_actor(request).await
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

    async fn get_actor_states(
        &self,
        request: Request<GetActorStatesRequest>,
    ) -> Result<Response<GetActorStatesResponse>, Status> {
        self.0.get_actor_states(request).await
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

#[cfg(test)]
mod tests;
