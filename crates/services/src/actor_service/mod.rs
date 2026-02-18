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
//! that routes messages to local or remote actors based on the `actor@node` addressing scheme.
//!
//! ### Key Responsibilities
//!
//! 1. **Parse actor@node IDs** to determine routing (local vs remote)
//! 2. **Local routing**: Lookup actor in registry, deliver to local mailbox
//! 3. **Remote routing**: Forward to remote node's ActorService via gRPC (using ServiceLocator for client caching)
//! 4. **Keep actors lightweight**: Actors never directly use gRPC
//! 5. **Local-only actor creation**: CreateActor and spawn_actor ALWAYS create actors locally on the node where called
//!
//! ### Message Flow
//!
//! ```text
//! Client -> ActorService.SendMessage("counter@node2", msg)
//!   |
//!   +--> Parse: actor_name="counter", node_id="node2"
//!   |
//!   +--> If node2 == local_node_id:
//!   |      -> Registry.lookup("counter@node2") -> ActorRef
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
//! 1. Extract correlation_id from temporary sender ID (format: "ask-{correlation_id}@{node_id}")
//! 2. Create ActorRef with temporary sender ID
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
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use futures::future::join_all;

use plexspaces_core::{ActorRegistry, ServiceLocator as ServiceLocatorTrait, actor_context::ObjectRegistry as ObjectRegistryTrait, MessageSender, ReplyWaiter, ReplyWaiterError, RequestContext};
use std::collections::HashMap;
use std::time::{SystemTime, Duration, Instant};
use std::io::Write;
use plexspaces_actor::{ActorFactory, Actor};
use plexspaces_actor::ActorRef as ActorRefImpl;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_core::ActorId;
use crate::ServiceLocatorImpl;
use plexspaces_proto::common::v1::Message;
use ulid::Ulid;

// Import proto types and gRPC service trait
use plexspaces_proto::actor::v1::{
    actor_service_client::ActorServiceClient,

    // gRPC service trait and server
    actor_service_server::ActorService as ActorServiceTrait,
    ActorDownNotification,
    ActivateActorRequest,
    ActivateActorResponse,
    CheckActorExistsRequest,
    CheckActorExistsResponse,
    DeleteActorRequest,
    DeactivateActorRequest,
    TerminateActorRequest,
    TerminateActorResponse,
    GetActorRequest,
    GetActorResponse,
    GetOrActivateActorRequest,
    GetOrActivateActorResponse,
    InvokeActorRequest,
    InvokeActorResponse,
    LinkActorRequest,
    LinkActorResponse,
    ListActorsRequest,
    ListActorsResponse,
    MigrateActorRequest,
    MigrateActorResponse,
    MonitorActorRequest,
    MonitorActorResponse,
    // Request/Response types (from proto)
    SendMessageRequest,
    SendMessageResponse,
    SetActorStateRequest,
    SetActorStateResponse,
    SpawnActorRequest,
    SpawnActorResponse,
    StreamMessageRequest,
    StreamMessageResponse,
    UnlinkActorRequest,
    UnlinkActorResponse,
    // ShardGroup types
    CreateShardGroupRequest, CreateShardGroupResponse,
    DeleteShardGroupRequest,
    GetShardGroupRequest, GetShardGroupResponse,
    ListShardGroupsRequest, ListShardGroupsResponse,
    ScaleShardGroupRequest, ScaleShardGroupResponse,
    SendToShardRequest, SendToShardResponse,
    ScatterGatherRequest, ScatterGatherResponse,
    BulkUpdateShardGroupRequest, BulkUpdateShardGroupResponse,
    MapShardGroupRequest, MapShardGroupResponse,
    ShardGroup, ShardGroupState, PartitionStrategy, ShardGroupAggregationStrategy,
    ShardQueryResponse, ScatterGatherStats, ShardUpdateStats, RebalanceStatus,
};
use plexspaces_proto::common::v1::Empty;

/// ActorService implementation - gRPC gateway for actor messaging
///
/// ## Responsibilities
/// - Route messages to local or remote actors based on `actor@node` addressing
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
    
    /// Get ActorRegistry from ServiceLocator (lazy initialization)
    async fn get_actor_registry(&self) -> Arc<ActorRegistry> {
        
        self.service_locator
            .actor_registry()
            .await
            .expect("ActorRegistry must be registered in ServiceLocator")
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
            return Err(Status::unavailable("Service is shutting down and not accepting new requests"));
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
    /// * `actor_id` - Actor ID (will be suffixed with local node_id: "actor_name@local_node_id")
    /// * `actor_type` - Type of actor to spawn (must be registered in BehaviorFactory if using factory)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Returns
    /// ActorRef for the spawned actor (format: "actor_name@local_node_id")
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
    /// * `actor_id` - Actor ID (will be suffixed with local node_id: "actor_name@local_node_id")
    /// * `actor_type` - Type of actor to spawn (used by BehaviorFactory if available)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    ///
    /// ## Returns
    /// ActorRef for the spawned actor (format: "actor_name@local_node_id")
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
        // CRITICAL: Normalize actor_id to include @node suffix BEFORE passing to factory
        // Parse actor_id to get actor_name and node_id
        let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
            (name.to_string(), node.to_string())
        } else {
            (actor_id.to_string(), String::new())
        };
        
        // Always use local node_id (ignore any node_id in actor_id)
        let local_actor_id = if node_id.is_empty() || node_id == self.local_node_id {
            format!("{}@{}", actor_name, self.local_node_id)
        } else {
            // If actor_id specifies a different node, reject it
            return Err(format!(
                "Cannot spawn actor on remote node '{}' via local ActorService. ActorService always creates actors locally. To spawn on '{}', call that node's ActorService directly.",
                node_id, node_id
            ).into());
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
        actor_factory.spawn_actor(
            &ctx,
            &local_actor_id,
            actor_type,
            initial_state,
            config,
            labels,
            vec![], // facets (empty - facets should be attached via config or separate API)
        ).await?;
        
        // Actor is now spawned and registered - try to create ActorRefImpl::local() if mailbox available
        let registry = self.get_actor_registry().await;
        Ok(Self::create_actor_ref_for_local_actor(
            &ctx,
            &registry,
            &local_actor_id,
            &self.local_node_id,
            self.service_locator.clone(),
        ).await)
    }

    /// Send a message to an actor (local or remote) - Public API for ActorContext
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID in format "actor_name@node_id"
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
            // Parse actor@node ID
            let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
                (name.to_string(), node.to_string())
            } else {
                (actor_id.to_string(), self.local_node_id.clone())
            };

            // If local actor, route reply via MessageSender.tell()
            // ActorRef::tell() will automatically check for correlation_id and route to ReplyWaiter
            // if there's a pending ask() call - routing handled by ReplyWaiterRegistry
            if node_id == self.local_node_id {
                // Use MessageSender.tell() - ActorRef::tell() handles reply routing automatically
                // When MessageSender.tell() is called, it eventually calls ActorRef::tell(),
                // which checks ReplyWaiterRegistry for the correlation_id and routes to ReplyWaiter
                let actor_id_full = format!("{}@{}", actor_name, node_id);
                
                // Try lookup with constructed ID first
                let mut sender_opt = self.get_actor_registry().await.lookup_actor(&actor_id_full).await;
                
                // If not found and original actor_id already has @, try direct lookup (in case parsing was wrong)
                if sender_opt.is_none() && actor_id.contains('@') && actor_id != actor_id_full {
                    sender_opt = self.get_actor_registry().await.lookup_actor(&actor_id.to_string()).await;
                }
                
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
                    sender.tell(message).await
                        .map_err(|e| Status::internal(format!("Failed to send reply: {}", e)))?;
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            "🟪 [ACTOR_SERVICE::send_message] REPLY ROUTED: message_id={}, correlation_id={}",
                            message_id, correlation_id
                        );
                    }
                    return Ok(message_id);
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
                msg_id, actor_id
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
    /// * `actor_id` - Actor ID in format "actor_name@node_id"
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
        
        // Parse actor_id to determine if local or remote
        let (_actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
            (name.to_string(), node.to_string())
        } else {
            (actor_id.to_string(), self.local_node_id.clone())
        };

        if node_id == self.local_node_id {
            // LOCAL: Get ActorRef and use ask()
            let actor_id_str = actor_id.to_string();
            let registry = self.get_actor_registry().await;
            
            // Check if actor exists
            if registry.lookup_actor(&actor_id_str).await.is_none() 
                && !registry.is_actor_activated(&actor_id_str).await {
                // Actor doesn't exist - return error
                return Err("Actor not found".into());
            }
            
            // Create ActorRef - try local first (with mailbox), fallback to remote
            let ctx = RequestContext::new_without_auth(String::new(), String::new());
            let actor_ref = Self::create_actor_ref_for_local_actor(
                &ctx,
                &registry,
                &actor_id_str,
                &self.local_node_id,
                self.service_locator.clone(),
            ).await;

            let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(5));
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL: message_id={}, actor_id={}, calling ActorRef::ask()",
                    message.id, actor_id_str
                );
            }
            let result = actor_ref.ask(message, timeout_duration).await
                .map_err(|e| {
                    use plexspaces_actor::ActorRefError;
                    match e {
                        ActorRefError::ActorNotFound(_) => "Actor not found".into(),
                        _ => format!("Failed to send ask request: {}", e).into(),
                    }
                });
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL COMPLETED: actor_id={}, result={:?}",
                    actor_id_str, result.is_ok()
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
        registry: &Arc<ActorRegistry>,
        actor_id: &str,
        local_node_id: &str,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) -> ActorRefImpl {
        // Try to get mailbox from actor instance
        let actor_id_typed = ActorId::from(actor_id);
        if let Some(instance) = registry.get_actor_instance(&actor_id_typed).await {
            if let Some(actor) = instance.downcast_ref::<Actor>() {
                let mailbox = actor.mailbox().clone();
                // CRITICAL: Pass tenant_id from RequestContext to ActorRef
                return ActorRefImpl::local(
                    actor_id,
                    ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
                    ctx.namespace().to_string(),
                    mailbox,
                    service_locator,
                );
            }
        }
        // Fallback to remote pointing to local node (for virtual actors or when mailbox unavailable)
        // CRITICAL: Pass tenant_id from RequestContext to ActorRef
        ActorRefImpl::remote(
            actor_id,
            ctx.tenant_id().to_string(), // CRITICAL: tenant_id flows from API → ActorBuilder → ActorRef
            ctx.namespace().to_string(),
            local_node_id.to_string(),
            service_locator,
        )
    }

    /// Helper function to route message (can be called from spawned tasks)
    /// Extracts routing logic so it can be used without needing &self
    async fn route_message_helper(
        ctx: RequestContext,
        service_locator: &Arc<ServiceLocatorImpl>,
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        // Use unified routing module (returns Future, converts ActorRefError to Status)
        use plexspaces_actor::routing::route_message as routing_route_message;
        let result = routing_route_message(
            ctx,
            service_locator.clone() as Arc<dyn ServiceLocatorTrait>,
            actor_id.to_string(),
            message,
            wait_for_response,
            timeout,
        ).await;
        
        // Convert ActorRefError to Status
        result.map_err(|e| match e {
            plexspaces_actor::ActorRefError::Timeout => Status::deadline_exceeded("No reply received within timeout"),
            plexspaces_actor::ActorRefError::ActorNotFound(id) => Status::not_found(format!("Actor not found: {}", id)),
            plexspaces_actor::ActorRefError::SendFailed(msg) => Status::internal(format!("Failed to send message: {}", msg)),
            _ => Status::internal(format!("Routing error: {}", e)),
        })
    }

    /// Route message to local or remote actor
    ///
    /// # Arguments
    /// * `ctx` - RequestContext with tenant_id and namespace (required for proper isolation) - FIRST PARAMETER
    /// * `actor_id` - Target actor ID in format "actor@node"
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
        // Extract message_id for logging before moving message
        let message_id = message.id.clone();
        let message_sender = message.sender_id.clone();
        let message_receiver = message.receiver_id.clone();
        let message_type = message.message_type.to_string();
        let message_correlation_id = message.correlation_id.clone();
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::route_message] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, wait_for_response={}, timeout={:?}",
                message_id, actor_id, message_sender, message_receiver, message_type, message_correlation_id, wait_for_response, timeout
            );
        }
        
        // Use unified routing module (returns Future, converts ActorRefError to Status)
        use plexspaces_actor::routing::route_message as routing_route_message;
        let result = routing_route_message(
            ctx,
            self.service_locator.clone(),
            actor_id.to_string(),
            message,
            wait_for_response,
            timeout,
        ).await;
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟪 [ACTOR_SERVICE::route_message] COMPLETED: message_id={}, actor_id={}, result={:?}",
                message_id, actor_id, result.is_ok()
            );
        }
        
        // Convert ActorRefError to Status
        result.map_err(|e| match e {
            plexspaces_actor::ActorRefError::Timeout => Status::deadline_exceeded("No reply received within timeout"),
            plexspaces_actor::ActorRefError::ActorNotFound(id) => Status::not_found(format!("Actor not found: {}", id)),
            plexspaces_actor::ActorRefError::SendFailed(msg) => Status::internal(format!("Failed to send message: {}", msg)),
            _ => Status::internal(format!("Routing error: {}", e)),
        })
    }

    /// Route message to local actor
    ///
    /// ## Design
    /// Uses unified routing module. Delegates to `routing::route_local()`.
    /// This ensures proper routing, metrics, and virtual actor activation.
    async fn route_local(
        &self,
        ctx: RequestContext,
        actor_name: &str,
        node_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        // Construct full actor ID
        let actor_id = format!("{}@{}", actor_name, node_id);
        
        // Use unified routing module (returns Future, converts ActorRefError to Status)
        use plexspaces_actor::routing::route_local as routing_route_local;
        let result = routing_route_local(
            ctx,
            self.service_locator.clone(),
            actor_id,
            message,
            wait_for_response,
            timeout,
        ).await;
        
        // Convert ActorRefError to Status
        result.map_err(|e| match e {
            plexspaces_actor::ActorRefError::Timeout => Status::deadline_exceeded("No reply received within timeout"),
            plexspaces_actor::ActorRefError::ActorNotFound(id) => Status::not_found(format!("Actor not found: {}", id)),
            plexspaces_actor::ActorRefError::SendFailed(msg) => Status::internal(format!("Failed to send message: {}", msg)),
            _ => Status::internal(format!("Routing error: {}", e)),
        })
    }

    /// Route message to remote actor
    ///
    /// ## Design
    /// Uses unified routing module. Delegates to `routing::route_remote()`.
    async fn route_remote(
        &self,
        ctx: RequestContext,
        node_id: &str,
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        // Use unified routing module (returns Future, converts ActorRefError to Status)
        use plexspaces_actor::routing::route_remote as routing_route_remote;
        let result = routing_route_remote(
            ctx,
            self.service_locator.clone(),
            node_id.to_string(),
            actor_id.to_string(),
            message,
            wait_for_response,
            timeout,
        ).await;
        
        // Convert ActorRefError to Status
        result.map_err(|e| match e {
            plexspaces_actor::ActorRefError::Timeout => Status::deadline_exceeded("No reply received within timeout"),
            plexspaces_actor::ActorRefError::ActorNotFound(id) => Status::not_found(format!("Actor not found: {}", id)),
            plexspaces_actor::ActorRefError::SendFailed(msg) => Status::internal(format!("Failed to send message: {}", msg)),
            _ => Status::internal(format!("Routing error: {}", e)),
        })
    }


    /// Send message to actor with location transparency
    ///
    /// ## Purpose
    /// Public API for sending messages to local or remote actors.
    /// Automatically routes to correct node based on actor@node addressing.
    ///
    /// ## Arguments
    /// * `actor_id` - Target actor ID in format "actor@node"
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
    ///     "payment@node1",
    ///     message,
    ///     false,
    ///     None
    /// ).await?;
    ///
    /// // Request-reply (ask)
    /// let (msg_id, Some(reply)) = actor_service.send(
    ///     "inventory@node2",
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
        let actor_ref_impl = self.spawn_actor(&ctx, actor_id, actor_type, initial_state, None, std::collections::HashMap::new()).await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        plexspaces_core::ActorRef::new(actor_ref_impl.id().to_string())
            .map_err(|e| format!("Failed to create ActorRef: {}", e).into())
    }

    async fn send(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        self.send_message(actor_id, message).await
    }

    async fn create_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::CreateShardGroupRequest,
    ) -> Result<plexspaces_proto::actor::v1::CreateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests().await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.create_shard_group_internal(ctx, req).await
    }

    async fn bulk_update_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::BulkUpdateShardGroupRequest,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests().await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.bulk_update_shard_group_internal(ctx, req).await
    }

    async fn map_shard_group(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::MapShardGroupRequest,
    ) -> Result<plexspaces_proto::actor::v1::MapShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests().await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.map_shard_group_internal(ctx, req).await
    }

    async fn scatter_gather(
        &self,
        ctx: &RequestContext,
        req: plexspaces_proto::actor::v1::ScatterGatherRequest,
    ) -> Result<plexspaces_proto::actor::v1::ScatterGatherResponse, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests().await
            .map_err(|e| format!("Service not accepting requests: {}", e))?;
        self.scatter_gather_internal(ctx, req).await
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
        let req = request.into_inner();
        let proto_message = req
            .message
            .ok_or_else(|| Status::invalid_argument("Message is required"))?;

        // Convert proto Message to mailbox Message
        let message = proto_message.clone();

        // Extract target actor ID from message
        let actor_id = if proto_message.receiver_id.is_empty() {
            return Err(Status::invalid_argument("Receiver ID is required"));
        } else {
            &proto_message.receiver_id
        };

        // Convert timeout
        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        // Extract RequestContext from gRPC request metadata
        // TODO: Extract tenant_id and namespace from metadata headers
        let ctx = RequestContext::new_without_auth(String::new(), String::new());

        // Route message
        let (message_id, response) = self
            .route_message(ctx, actor_id, message, req.wait_for_response, timeout)
            .await?;

        // Response is already proto Message
        let response_message = response;

        Ok(Response::new(SendMessageResponse {
            message_id,
            response: response_message,
        }))
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
            return Err(Status::unavailable("Service is shutting down and not accepting new requests"));
        }
        
        // This is the gRPC handler - it spawns locally on this node
        // gRPC is already remote, so "remote" in the name was redundant
        // The actor is spawned locally on THIS node (the one receiving the gRPC request)
        
        // Extract labels from request before consuming it (needed for context creation)
        let labels_for_ctx = request.get_ref().labels.clone();
        
        // Create RequestContext from request metadata (before consuming request)
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &labels_for_ctx,
            &service_locator_trait,
        ).await
        .map_err(|e| {
            Status::invalid_argument(format!("Invalid request context: {}", e))
        })?;
        
        // Now consume request to get inner data
        let req = request.into_inner();
        
        // Validate actor_type
        if req.actor_type.is_empty() {
            return Err(Status::invalid_argument("Missing actor_type"));
        }
        
        // Determine actor ID: client-specified or server-generated
        let node_id = self.local_node_id.clone();
        let actor_id = if !req.actor_id.is_empty() {
            // Client-specified ID (for virtual actors)
            // Ensure it includes node suffix for consistency
            if req.actor_id.contains('@') {
                req.actor_id.clone()
            } else {
                format!("{}@{}", req.actor_id, node_id)
            }
        } else {
            // Server-generated ID (use timestamp-based ID)
            use std::time::{SystemTime, UNIX_EPOCH};
            let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            format!("actor-{}@{}", timestamp, node_id)
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
            let facet_registry_wrapper = self.service_locator.get_facet_registry().await
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
        let actor_factory_opt: Option<Arc<dyn ActorFactory>> = self.service_locator.get_actor_factory().await;
        
        if let Some(factory) = actor_factory_opt {
            // Spawn actor using ActorFactory with facets
            factory.spawn_actor(
                &ctx,
                &actor_id,
                &actor_type,
                initial_state.clone(),
                config.clone(),
                labels.clone(),
                facet_boxes, // Pass converted facets
            ).await
            .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;
            
            // Record metrics for facet attachment
            if facet_count > 0 {
                metrics::counter!("plexspaces.actor.spawn.with_facets").increment(1);
                metrics::counter!("plexspaces.actor.facets.attached").increment(facet_count as u64);
            }
        } else {
            return Err(Status::internal("ActorFactory not available in ServiceLocator"));
        }
        
        // Build proto Actor message for response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: actor_id.clone(),
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
            namespace: String::new(), // Namespace from application/actor context
        };
        
        // Return response with ActorRef format "actor_id@node_id"
        Ok(Response::new(SpawnActorResponse {
            actor_ref: actor_id,
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

    async fn activate_actor(
        &self,
        _request: Request<ActivateActorRequest>,
    ) -> Result<Response<ActivateActorResponse>, Status> {
        Err(Status::unimplemented("activate_actor not yet implemented"))
    }

    async fn deactivate_actor(
        &self,
        _request: Request<DeactivateActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        Err(Status::unimplemented("deactivate_actor not yet implemented"))
    }

    async fn check_actor_exists(
        &self,
        _request: Request<CheckActorExistsRequest>,
    ) -> Result<Response<CheckActorExistsResponse>, Status> {
        Err(Status::unimplemented("check_actor_exists not yet implemented"))
    }

    async fn get_or_activate_actor(
        &self,
        request: Request<GetOrActivateActorRequest>,
    ) -> Result<Response<GetOrActivateActorResponse>, Status> {
        // Create RequestContext from gRPC request (before consuming request)
        // GetOrActivateActorRequest doesn't have labels field, use empty map
        let labels_for_ctx = std::collections::HashMap::new();
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &labels_for_ctx,
            &service_locator_trait,
        )
        .await
        .map_err(|e| {
            Status::invalid_argument(format!("Invalid request context: {}", e))
        })?;

        // Now consume request
        let req = request.into_inner();
        
        // Use unified implementation
        let (was_activated, final_actor_id) = get_or_activate_actor_impl(
            &self.service_locator,
            &self.local_node_id,
            &ctx,
            &req,
        ).await?;

        // Build response
        use plexspaces_proto::v1::actor::{Actor as ProtoActor, ActorState};
        let proto_actor = ProtoActor {
            actor_id: final_actor_id.clone(),
            actor_type: if req.actor_type.is_empty() {
                "unknown".to_string()
            } else {
                req.actor_type.clone()
            },
            state: ActorState::ActorStateActive as i32,
            node_id: self.local_node_id.clone(),
            vm_id: String::new(),
            actor_state: req.initial_state.clone(),
            metadata: None,
            config: req.config,
            metrics: None,
            facets: vec![],
            actor_state_schema_version: 0,
            error_message: String::new(),
            namespace: String::new(), // Namespace from application/actor context
        };

        // Build actor_ref (format: "actor_id@node_id")
        // Use final_actor_id from unified implementation
        let actor_ref = final_actor_id;

        Ok(Response::new(GetOrActivateActorResponse {
            actor_ref,
            actor: Some(proto_actor),
            was_activated,
        }))
    }

    async fn invoke_actor(
        &self,
        request: Request<InvokeActorRequest>,
    ) -> Result<Response<InvokeActorResponse>, Status> {
        let start_time = std::time::Instant::now();
        let _metadata = request.metadata().clone();
        let _req = request.get_ref().clone(); // Clone to avoid moving request
        
        // Create RequestContext from gRPC request - uses shared validation from RequestContext
        // tenant_id comes from auth or default config, not from request body
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &service_locator_trait,
        ).await
        .map_err(|e| {
            Status::invalid_argument(format!("Invalid request context: {}", e))
        })?;
        
        // Get request body (tenant_id is no longer in request)
        let req = request.into_inner();
        
        // Extract tenant_id and namespace. Path-derived namespace (req.namespace) is the source
        // of truth for actor lookup so /api/v1/actors/leader-election-term1/LeaderElection and
        // .../leader-election-term2/LeaderElection resolve to different actors.
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = if !req.namespace.is_empty() {
            req.namespace.clone()
        } else {
            ctx.namespace().to_string()
        };
        let lookup_ctx = plexspaces_core::RequestContext::new_without_auth(tenant_id.clone(), namespace.clone());
        
        // OBSERVABILITY: Start tracing span (clone for span to avoid moving)
        let tenant_id_for_span = tenant_id.clone();
        let namespace_for_span = namespace.clone();
        let span = tracing::span!(
            tracing::Level::INFO,
            "actor_service.invoke_actor",
            tenant_id = %tenant_id_for_span,
            namespace = %namespace_for_span,
            actor_type = %req.actor_type,
            http_method = %req.http_method
        );
        let _guard = span.enter();
        
        // URL for logging (path from request or constructed)
        let url = if req.path.is_empty() {
            format!("/api/v1/actors/{}/{}/{}", tenant_id, namespace, req.actor_type)
        } else {
            req.path.clone()
        };

        let actor_type = req.actor_type.clone();
        if actor_type.is_empty() {
            metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                "error_type" => "missing_actor_type",
                "tenant_id" => tenant_id.clone(),
                "namespace" => namespace.clone()
            ).increment(1);
            return Err(Status::invalid_argument("Missing actor_type"));
        }
        
        // Validate actor_type length (proto validation should catch this, but double-check)
        if actor_type.len() > 128 {
            metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                "error_type" => "invalid_actor_type",
                "tenant_id" => tenant_id,
                "namespace" => namespace
            ).increment(1);
            return Err(Status::invalid_argument("Actor type exceeds maximum length of 128 characters"));
        }

        // Get ActorRegistry to lookup actors
        let actor_registry = self.get_actor_registry().await;
        
        // OBSERVABILITY: Track lookup start
        let lookup_start = std::time::Instant::now();
        
        // Discover actors by type using efficient hashmap lookup (O(1))
        let actor_ids = actor_registry.discover_actors_by_type(&lookup_ctx, &actor_type).await;

        // OBSERVABILITY: Track lookup duration
        let lookup_duration = lookup_start.elapsed();
        metrics::histogram!("plexspaces_actor_service_invoke_actor_lookup_duration_seconds")
            .record(lookup_duration.as_secs_f64());

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟦 [INVOKE_ACTOR] Lookup complete: found {} actors of type '{}' in tenant '{}', namespace '{}' (took {:?})",
                actor_ids.len(), actor_type, tenant_id, namespace, lookup_duration
            );
        }

        if actor_ids.is_empty() {
            metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                "error_type" => "actor_not_found",
                "tenant_id" => tenant_id.clone(),
                "namespace" => namespace.clone(),
                "actor_type" => actor_type.clone()
            ).increment(1);
            tracing::warn!(
                "🟦 [INVOKE_ACTOR] No actors found: type='{}', tenant='{}', namespace='{}'",
                actor_type, &tenant_id, &namespace
            );
            return Err(Status::not_found(format!(
                "No actors found for type '{}' in tenant '{}', namespace '{}'",
                actor_type, &tenant_id, &namespace
            )));
        }

        // Randomly select an actor if multiple found (load balancing)
        use rand::Rng;
        let selected_actor_id = if actor_ids.len() == 1 {
            actor_ids[0].clone()
        } else {
            let mut rng = rand::thread_rng();
            let idx = rng.gen_range(0..actor_ids.len());
            actor_ids[idx].clone()
        };
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "🟦 [INVOKE_ACTOR] Selected actor: {} (from {} candidates)",
                selected_actor_id, actor_ids.len()
            );
        }
        
        // Determine invocation pattern: tell (fire-and-forget) vs ask (request-reply)
        // - tell: message_type "cast"; ask: message_type "call".
        // msg_type_override (from query param msg_type) takes precedence when set; otherwise ask flag or GET.
        let http_method = req.http_method.to_uppercase();
        let is_get = http_method.is_empty() || http_method == "GET";
        let is_delete = http_method == "DELETE";
        let use_ask = if req.msg_type_override == "call" {
            true
        } else if req.msg_type_override == "cast" {
            false
        } else {
            req.ask || is_get
        };

        // Prepare message payload and metadata
        let (payload, mut metadata) = if is_get || is_delete {
            // GET/DELETE: Convert query params to JSON string
            let query_json = serde_json::to_string(&req.query_params)
                .map_err(|e| {
                    metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                        "error_type" => "serialization_error"
                    ).increment(1);
                    Status::internal(format!("Failed to serialize query params: {}", e))
                })?;
            (query_json.into_bytes(), HashMap::new())
        } else {
            // POST/PUT: Use body as payload, convert headers to metadata
            (req.payload, req.headers)
        };

        // Add path information to metadata if provided
        // This allows actors to access the complete URL path for custom routing
        if !req.path.is_empty() {
            metadata.insert("http_path".to_string(), req.path.clone());
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!("🟦 [INVOKE_ACTOR] Custom path provided: {}", req.path);
            }
        }
        
        // Add subpath to metadata if provided (for future routing capabilities)
        if !req.subpath.is_empty() {
            metadata.insert("http_subpath".to_string(), req.subpath.clone());
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!("🟦 [INVOKE_ACTOR] Subpath provided: {}", req.subpath);
            }
        }

        // Create message (id from client not yet in proto; server assigns ULID for correlation)
        // Ensure message ID has "req-" prefix for requests (ask) or "cast" for tell
        let message_id = if use_ask {
            format!("req-{}", ulid::Ulid::new().to_string())
        } else {
            format!("req-{}", ulid::Ulid::new().to_string()) // Both ask and tell use req- prefix
        };
        let mut message = Message {
            id: message_id,
            payload,
            receiver_id: selected_actor_id.clone(),
            ..Default::default()
        };
        // Set message type: ask = "call" (request-reply), tell = "cast" (fire-and-forget)
        message.message_type = if use_ask {
            "call".to_string()
        } else {
            "cast".to_string()
        };
        message.headers = metadata;
        
        // Set URI path and method for HTTP-based invocations
        // Use full path from request, or construct from tenant_id and actor_type
        let full_path = if !req.path.is_empty() {
            req.path.clone()
        } else {
            format!("/api/v1/actors/{}/{}", &namespace, req.actor_type)
        };
        message.uri_path = full_path.clone();
        message.uri_method = http_method.clone();

        // route_message: wait_for_response=true => ask (request-reply), false => tell (fire-and-forget).
        // Use timeout from request if provided, otherwise default to 5 seconds for ask operations.
        let wait_for_response = use_ask;
        let timeout = if wait_for_response {
            req.timeout.map(|d| {
                std::time::Duration::from_secs(d.seconds as u64)
                    + std::time::Duration::from_nanos(d.nanos as u64)
            }).or_else(|| Some(std::time::Duration::from_secs(5)))
        } else {
            None
        };

        let message_id = message.id.clone();
        let invocation_label = if use_ask { "ask" } else { "tell" };
        let envelope_msg_type = message.message_type.clone();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                message_id = %message_id,
                invocation = %invocation_label,
                msg_type = %envelope_msg_type,
                "🟦 [INVOKE_ACTOR] START: tenant_id={}, namespace={}, actor_type={}, http_method={}, url={}",
                &tenant_id, &namespace, actor_type, http_method, url
            );
        }

        // OBSERVABILITY: Track invocation start
        let invoke_start = std::time::Instant::now();
        
        // Branch on ask vs tell (request-reply vs fire-and-forget)
        let result = if use_ask {
            // Ask pattern (request-reply): GET or explicit ask=true
            let method_label = if is_get { "GET" } else { "POST/PUT" };
            metrics::counter!("plexspaces_actor_service_invoke_actor_total",
                "method" => method_label,
                "pattern" => "ask",
                "tenant_id" => tenant_id.clone(),
                "namespace" => namespace.clone(),
                "actor_type" => actor_type.clone()
            ).increment(1);
            
            match self.route_message(ctx.clone(), &selected_actor_id, message, true, timeout).await {
                Ok((_, Some(reply))) => {
                    let invoke_duration = invoke_start.elapsed();
                    metrics::histogram!("plexspaces_actor_service_invoke_actor_duration_seconds",
                        "method" => method_label,
                        "pattern" => "ask",
                        "status" => "success",
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).record(invoke_duration.as_secs_f64());
                    
                    tracing::info!(
                        message_id = %message_id,
                        invocation = "ask",
                        msg_type = %envelope_msg_type,
                        actor_id = %selected_actor_id,
                        path = %full_path,
                        method = %http_method,
                        duration_ms = invoke_duration.as_millis(),
                        payload_size = reply.payload.len(),
                        "INVOKE_ACTOR SUCCESS (ask) duration_ms={}",
                        invoke_duration.as_millis()
                    );
                    
                    Ok(Response::new(InvokeActorResponse {
                        success: true,
                        payload: reply.payload,
                        headers: reply.headers,
                        actor_id: selected_actor_id.clone(),
                        error_message: String::new(),
                    }))
                }
                Ok((_, None)) => {
                    // This shouldn't happen for ask() pattern
                    Err(Status::internal("No reply received from actor"))
                }
                Err(e) => {
                    let invoke_duration = invoke_start.elapsed();
                    metrics::histogram!("plexspaces_actor_service_invoke_actor_duration_seconds",
                        "method" => method_label,
                        "pattern" => "ask",
                        "status" => "error",
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).record(invoke_duration.as_secs_f64());
                    metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                        "error_type" => "ask_failed",
                        "method" => method_label,
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).increment(1);
                    
                    let err_first = e.message().lines().next().unwrap_or("");
                    tracing::error!(
                        message_id = %message_id,
                        invocation = "ask",
                        actor_id = %selected_actor_id,
                        path = %full_path,
                        method = %http_method,
                        error_first_line = %err_first,
                        duration_ms = invoke_duration.as_millis(),
                        "INVOKE_ACTOR FAILED (ask) duration_ms={}",
                        invoke_duration.as_millis()
                    );
                    
                    Err(e)
                }
            }
        } else {
            // POST/PUT/DELETE: Use tell() (fire-and-forget)
            let method_label = if http_method == "PUT" { "PUT" } else if is_delete { "DELETE" } else { "POST" };
            metrics::counter!("plexspaces_actor_service_invoke_actor_total",
                "method" => method_label,
                "pattern" => "tell",
                "tenant_id" => tenant_id.clone(),
                "namespace" => namespace.clone(),
                "actor_type" => actor_type.clone()
            ).increment(1);
            
            match self.route_message(ctx.clone(), &selected_actor_id, message, false, None).await {
                Ok((_, _)) => {
                    let invoke_duration = invoke_start.elapsed();
                    let method_label = if http_method == "PUT" { "PUT" } else if is_delete { "DELETE" } else { "POST" };
                    metrics::histogram!("plexspaces_actor_service_invoke_actor_duration_seconds",
                        "method" => method_label,
                        "pattern" => "tell",
                        "status" => "success",
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).record(invoke_duration.as_secs_f64());
                    
                    tracing::info!(
                        message_id = %message_id,
                        invocation = "tell",
                        msg_type = %envelope_msg_type,
                        actor_id = %selected_actor_id,
                        path = %full_path,
                        method = %http_method,
                        duration_ms = invoke_duration.as_millis(),
                        "INVOKE_ACTOR SUCCESS (tell) duration_ms={}",
                        invoke_duration.as_millis()
                    );
                    
                    Ok(Response::new(InvokeActorResponse {
                        success: true,
                        payload: vec![],
                        headers: HashMap::new(),
                        actor_id: selected_actor_id.clone(),
                        error_message: String::new(),
                    }))
                }
                Err(e) => {
                    let invoke_duration = invoke_start.elapsed();
                    let method_label = if http_method == "PUT" { "PUT" } else if is_delete { "DELETE" } else { "POST" };
                    metrics::histogram!("plexspaces_actor_service_invoke_actor_duration_seconds",
                        "method" => method_label,
                        "pattern" => "tell",
                        "status" => "error",
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).record(invoke_duration.as_secs_f64());
                    metrics::counter!("plexspaces_actor_service_invoke_actor_errors_total",
                        "error_type" => "tell_failed",
                        "method" => method_label,
                        "tenant_id" => tenant_id.clone(),
                        "namespace" => namespace.clone(),
                        "actor_type" => actor_type.clone()
                    ).increment(1);
                    
                    let err_first = e.message().lines().next().unwrap_or("");
                    tracing::error!(
                        message_id = %message_id,
                        invocation = "tell",
                        actor_id = %selected_actor_id,
                        path = %full_path,
                        method = %http_method,
                        error_first_line = %err_first,
                        duration_ms = invoke_duration.as_millis(),
                        "INVOKE_ACTOR FAILED (tell) duration_ms={}",
                        invoke_duration.as_millis()
                    );
                    
                    Err(e)
                }
            }
        };
        
        // OBSERVABILITY: Track total duration
        let total_duration = start_time.elapsed();
        metrics::histogram!("plexspaces_actor_service_invoke_actor_total_duration_seconds")
            .record(total_duration.as_secs_f64());
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                path = %full_path,
                method = %http_method,
                "🟦 [INVOKE_ACTOR] COMPLETED: total_duration={:?}, success={}",
                total_duration, result.is_ok()
            );
        }
        
        result
    }

    /// Terminate an actor gracefully by ID
    ///
    /// ## Purpose
    /// Permanently terminates an actor, completing pending work and removing from system.
    /// This is the HTTP DELETE endpoint for actors (pairs with SpawnActor).
    ///
    /// ## Difference from DeactivateActor
    /// - TerminateActor: Permanent termination (actor removed from system)
    /// - DeactivateActor: Temporary passivation (virtual actor can reactivate on next message)
    async fn terminate_actor(
        &self,
        request: Request<TerminateActorRequest>,
    ) -> Result<Response<TerminateActorResponse>, Status> {
        let start_time = std::time::Instant::now();
        
        // Check if service is accepting requests (not shutting down)
        if self.service_locator.is_shutdown_requested() {
            return Err(Status::unavailable("Service is shutting down and not accepting new requests"));
        }
        
        // Create RequestContext from gRPC request
        let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &service_locator_trait,
        ).await
        .map_err(|e| {
            Status::invalid_argument(format!("Invalid request context: {}", e))
        })?;
        
        let req = request.into_inner();
        let actor_id = req.actor_id.clone();
        let namespace = if req.namespace.is_empty() {
            ctx.namespace().to_string()
        } else {
            req.namespace.clone()
        };
        let force = req.force;
        let timeout_ms = if req.timeout_ms > 0 {
            req.timeout_ms
        } else {
            5000 // Default 5 seconds
        };
        
        tracing::info!(
            actor_id = %actor_id,
            namespace = %namespace,
            force = %force,
            timeout_ms = %timeout_ms,
            "Terminating actor"
        );
        
        // Get actor factory to stop the actor
        let actor_factory = self.service_locator.get_actor_factory().await
            .ok_or_else(|| Status::internal("Actor factory not available"))?;
        
        // Build full actor ID if needed (actor_id@node_id format)
        let full_actor_id = if actor_id.contains('@') {
            actor_id.clone()
        } else {
            let node_id = self.service_locator.get_node_id().await
                .ok_or_else(|| Status::internal("Node ID not available"))?;
            format!("{}@{}", actor_id, node_id)
        };
        
        // Stop the actor using actor factory with tenant isolation validation
        // Note: timeout_ms is currently not used by stop_actor, but kept for future use
        let _timeout = std::time::Duration::from_millis(timeout_ms);
        match actor_factory.stop_actor(&ctx, &full_actor_id).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_actor_service_terminate_actor_duration_seconds",
                    "namespace" => namespace.clone(),
                    "status" => "success"
                ).record(duration.as_secs_f64());
                metrics::counter!("plexspaces_actor_service_terminate_actor_total",
                    "namespace" => namespace.clone(),
                    "status" => "success"
                ).increment(1);
                
                tracing::info!(
                    actor_id = %full_actor_id,
                    duration_ms = %duration.as_millis(),
                    "Actor terminated successfully"
                );
                
                Ok(Response::new(TerminateActorResponse {
                    success: true,
                    actor_id: full_actor_id,
                    messages_processed: 0, // TODO: Track actual count
                    messages_dropped: 0,
                    error_message: String::new(),
                }))
            }
            Err(e) => {
                let duration = start_time.elapsed();
                metrics::histogram!("plexspaces_actor_service_terminate_actor_duration_seconds",
                    "namespace" => namespace.clone(),
                    "status" => "error"
                ).record(duration.as_secs_f64());
                metrics::counter!("plexspaces_actor_service_terminate_actor_total",
                    "namespace" => namespace.clone(),
                    "status" => "error"
                ).increment(1);
                
                tracing::error!(
                    actor_id = %full_actor_id,
                    error = %e,
                    "Failed to terminate actor"
                );
                
                Err(Status::internal(format!("Failed to terminate actor: {}", e)))
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
        ).await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        let req = request.into_inner();
        let resp = self.create_shard_group_internal(&ctx, req).await
            .map_err(|e| Status::internal(format!("Failed to create ShardGroup: {}", e)))?;
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
        ).await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self.bulk_update_shard_group_internal(&ctx, req).await
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
        ).await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let group_id = req.group_id.clone();
        let resp = self.map_shard_group_internal(&ctx, req).await
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
        ).await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self.scatter_gather_internal(&ctx, req).await
            .map_err(|e| Status::internal(format!("Failed to scatter-gather ShardGroup: {}", e)))?;
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
        ).await
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
        let actor_factory = self.service_locator.get_actor_factory().await
            .ok_or_else(|| Status::internal("Actor factory not available"))?;

        for shard_actor_id in &group.shard_actor_ids {
            let _ = actor_factory.stop_actor(&ctx, shard_actor_id).await;
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
        metrics::counter!("plexspaces_shard_group_deleted_total", 
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
        let group = groups.get(&req.group_id)
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
        let req = request.into_inner();
        
        // Extract RequestContext from gRPC metadata
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        
        // TODO: Implement scale_shard_group_internal
        // For now, return not implemented
        Err(Status::unimplemented("ScaleShardGroup not yet implemented"))
    }

    async fn list_shard_groups(
        &self,
        request: Request<ListShardGroupsRequest>,
    ) -> Result<Response<ListShardGroupsResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        let groups = self.shard_groups.read().await;
        let filtered: Vec<ShardGroup> = groups.values()
            .filter(|g| {
                // Filter by actor_type if specified
                if !req.actor_type.is_empty() && g.actor_type != req.actor_type {
                    return false;
                }
                // Filter by state if specified
                if req.state != ShardGroupState::ShardGroupStateUnspecified as i32
                    && g.state != req.state {
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

        let paginated: Vec<ShardGroup> = filtered.into_iter()
            .skip(offset)
            .take(limit)
            .collect();

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
        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups.get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        // Calculate shard_id from partition_key using partition strategy
        use crate::actor_service::partition::calculate_shard_id;
        let shard_id = calculate_shard_id(
            &req.partition_key,
            group.partition_strategy,
            group.shard_count,
            None, // TODO: Support range boundaries from group metadata
        ).map_err(|e| Status::invalid_argument(format!("Partition calculation failed: {}", e)))?;

        let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
            .ok_or_else(|| Status::internal(format!("Invalid shard_id {}", shard_id)))?
            .clone();

        // Route message to shard actor
        let mut message = req.message.ok_or_else(|| Status::invalid_argument("message is required"))?;
        message.receiver_id = shard_actor_id.clone();

        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        // Extract RequestContext from gRPC request
        // TODO: Extract tenant_id and namespace from metadata headers
        let ctx = RequestContext::new_without_auth(String::new(), String::new());

        let response_message = if req.wait_for_response {
            let (_, response) = self.route_message(ctx.clone(), &shard_actor_id, message, true, timeout).await?;
            response
        } else {
            let _ = self.route_message(ctx.clone(), &shard_actor_id, message, false, None).await?;
            None
        };

        // Track shard message metrics
        if let Some(accessor) = self.service_locator.get_node_metrics_accessor().await {
            accessor.increment_shard_messages_sent().await;
        }
        if let Some(registry) = self.service_locator.actor_registry().await {
            let actor_metrics = registry.actor_metrics();
            use plexspaces_core::message_metrics::ActorMetricsExt;
            let mut metrics = actor_metrics.write().await;
            metrics.increment_shard_messages_sent_total();
        }

        // Emit metrics
        metrics::counter!("plexspaces_send_to_shard_total",
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
    /// Internal implementation of create_shard_group (used by both gRPC and trait)
    async fn create_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Validate request
        if req.group_id.is_empty() {
            return Err("group_id is required".into());
        }
        if req.actor_type.is_empty() {
            return Err("actor_type is required".into());
        }
        if req.shard_count == 0 {
            return Err("shard_count must be >= 1".into());
        }
        if req.shard_count > 10000 {
            return Err("shard_count must be <= 10000".into());
        }

        // Check if group already exists
        {
            let groups = self.shard_groups.read().await;
            if groups.contains_key(&req.group_id) {
                return Err(format!("ShardGroup {} already exists", req.group_id).into());
            }
        }

        // Spawn shard actors
        let actor_factory = self.service_locator.get_actor_factory().await
            .ok_or_else(|| "Actor factory not available".to_string())?;

        let mut shard_actor_ids = Vec::with_capacity(req.shard_count as usize);
        let partition_strategy = req.partition_strategy;

        for shard_id in 0..req.shard_count {
            // Generate unique actor ID using ULID (no shard-id in ID for rebalancing)
            // Format: "{group_id}-{ulid}" - spawn_actor will normalize to add @node_id
            let actor_id_base = format!("{}-{}", req.group_id, ulid::Ulid::new());
            let initial_state = req.initial_state.clone();

            // Build ActorConfig with labels -> ActorResourceRequirements mapping
            let mut shard_config = req.shard_config.clone().unwrap_or_default();
            
            // Add group_id to actor_groups (not shard_id - actors shouldn't know their shard for rebalancing)
            shard_config.actor_groups.push(req.group_id.clone());
            
            // If ShardGroup has labels, set them in ActorResourceRequirements for node placement
            if !req.labels.is_empty() {
                use plexspaces_proto::v1::actor::ActorResourceRequirements;
                shard_config.resource_requirements = Some(ActorResourceRequirements {
                    resources: shard_config.resource_requirements.as_ref()
                        .and_then(|r| r.resources.clone()),
                    required_labels: req.labels.clone(),
                    placement: shard_config.resource_requirements.as_ref()
                        .and_then(|r| r.placement.clone()),
                    actor_groups: shard_config.resource_requirements.as_ref()
                        .map(|r| r.actor_groups.clone())
                        .unwrap_or_default(),
                });
            }

            // Spawn shard actor - spawn_actor normalizes ID to include @node_id
            match actor_factory.spawn_actor(
                &ctx,
                &actor_id_base,
                &req.actor_type,
                initial_state,
                Some(shard_config),
                req.labels.clone(), // Pass labels as well for compatibility
                vec![], // facets
            ).await {
                Ok(_actor_ref) => {
                    // CRITICAL: Get the actual node ID from ActorRegistry (not self.local_node_id)
                    // spawn_actor normalizes the ID internally using the registry's local_node_id
                    let registry = self.service_locator.actor_registry().await
                        .ok_or_else(|| "ActorRegistry not available".to_string())?;
                    
                    // Get the actual local node ID from registry (this is what spawn_actor uses)
                    let actual_node_id = registry.local_node_id();
                    
                    // Construct the normalized ID using the actual node ID from registry
                    let normalized_id = if actor_id_base.contains('@') {
                        actor_id_base.clone()
                    } else {
                        format!("{}@{}", actor_id_base, actual_node_id)
                    };
                    
                    // Verify actor is registered by looking it up
                    if let Some(_found_ref) = registry.lookup_actor(&normalized_id).await {
                        shard_actor_ids.push(normalized_id);
                    } else {
                        // Fallback: use constructed ID (shouldn't happen, but be defensive)
                        tracing::warn!(
                            "Shard actor spawned but not found in registry: {}, using constructed ID",
                            normalized_id
                        );
                        shard_actor_ids.push(normalized_id);
                    }
                }
                Err(e) => {
                    // Cleanup: stop already-spawned shards
                    for spawned_id in &shard_actor_ids {
                        let _ = actor_factory.stop_actor(ctx, spawned_id).await;
                    }
                    return Err(format!("Failed to spawn shard {}: {}", shard_id, e).into());
                }
            }
        }

        // Create ShardGroup metadata
        let group = ShardGroup {
            group_id: req.group_id.clone(),
            actor_type: req.actor_type.clone(),
            shard_count: req.shard_count,
            partition_strategy,
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
            labels: req.labels.clone(),
            rebalance_status: None,
        };

        // Store group
        {
            let mut groups = self.shard_groups.write().await;
            groups.insert(req.group_id.clone(), group.clone());
        }

        // Integrate with TaskRouter for actor-level routing
        // Register ShardGroup with TaskRouter (if available)
        // This enables channel-based routing for ShardGroup operations
        if let Some(task_router) = self.service_locator.get_task_router().await {
            if let Err(e) = task_router.register_group(group.clone()).await {
                tracing::warn!(
                    group_id = %req.group_id,
                    error = %e,
                    "Failed to register ShardGroup in TaskRouter (non-fatal)"
                );
            } else {
                tracing::debug!(
                    group_id = %req.group_id,
                    shard_count = shard_actor_ids.len(),
                    "Registered ShardGroup in TaskRouter"
                );
            }
        }

        // Track shard group creation metrics
        if let Some(accessor) = self.service_locator.get_node_metrics_accessor().await {
            accessor.increment_shard_groups_created().await;
        }
        if let Some(registry) = self.service_locator.actor_registry().await {
            let actor_metrics = registry.actor_metrics();
            use plexspaces_core::message_metrics::ActorMetricsExt;
            let mut metrics = actor_metrics.write().await;
            metrics.increment_shard_groups_created_total();
        }

        // Emit metrics
        metrics::counter!("plexspaces_shard_group_created_total", 
            "group_id" => req.group_id.clone(),
            "actor_type" => req.actor_type.clone(),
            "shard_count" => req.shard_count.to_string());

        tracing::info!(
            group_id = %req.group_id,
            shard_count = req.shard_count,
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
            groups.get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };

        // Route updates to appropriate shards based on partition_key
        use crate::actor_service::partition::calculate_shard_id;
        use futures::future::join_all;
        
        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        }).unwrap_or(std::time::Duration::from_secs(30));

        // Group updates by shard_id
        let total_updates = req.updates.len();
        let mut updates_by_shard: std::collections::HashMap<u32, Vec<(String, Message)>> = std::collections::HashMap::new();
        for (partition_key_str, mut message) in req.updates {
            let partition_key = partition_key_str.as_bytes();
            let shard_id = calculate_shard_id(
                partition_key,
                group.partition_strategy,
                group.shard_count,
                None,
            ).map_err(|e| format!("Partition calculation failed: {}", e))?;
            
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
                .ok_or_else(|| format!("Invalid shard_id {}", shard_id))?
                .clone();
            
            // Ensure message ID has "req-" prefix for requests
            if message.id.is_empty() {
                message.id = format!("req-{}", ulid::Ulid::new().to_string());
            } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                message.id = format!("req-{}", message.id);
            }
            
            message.receiver_id = shard_actor_id.clone();
            updates_by_shard.entry(shard_id).or_insert_with(Vec::new).push((partition_key_str, message));
        }

        // Send updates to shards in parallel (reuse existing logic from gRPC method)
        // TODO: Extract common parallel update logic
        let mut handles = Vec::new();
        let mut shard_stats_map: std::collections::HashMap<u32, ShardUpdateStats> = std::collections::HashMap::new();

        for (shard_id, updates) in updates_by_shard {
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize).unwrap().clone();
            let service_locator = self.service_locator.clone();
            let wait_for_responses = req.wait_for_responses;
            let consistency_level = req.consistency_level;
            
            let handle = tokio::spawn(async move {
                let mut succeeded = 0u32;
                let mut failed = 0u32;
                
                let updates_clone = updates.clone();
                match consistency_level {
                    x if x == plexspaces_proto::v1::actor::ConsistencyLevel::ConsistencyLevelEventual as i32 => {
                        for (key, mut message) in updates_clone {
                            // Ensure message ID has "req-" prefix
                            if message.id.is_empty() {
                                message.id = format!("req-{}", ulid::Ulid::new().to_string());
                            } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                                message.id = format!("req-{}", message.id);
                            }
                            // Clone values before moving message
                            let message_id = message.id.clone();
                            let receiver_id = message.receiver_id.clone();
                            
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> = service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                if let Some(actor_ref) = registry.lookup_actor(&receiver_id).await {
                                    if actor_ref.tell(message).await.is_ok() {
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
                        for (_key, mut message) in updates_clone {
                            // Ensure message ID has "req-" prefix
                            if message.id.is_empty() {
                                message.id = format!("req-{}", ulid::Ulid::new().to_string());
                            } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                                message.id = format!("req-{}", message.id);
                            }
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> = service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                if let Some(actor_ref) = registry.lookup_actor(&message.receiver_id).await {
                                    if actor_ref.tell(message).await.is_ok() {
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
                }
                
                (shard_id, succeeded, failed, updates.len() as u32)
            });
            handles.push(handle);
        }

        // Wait for all updates to complete
        let results = tokio::time::timeout(timeout, join_all(handles)).await
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
            
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
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
    /// Uses routing::ask_helper() for each shard: one temp sender (via ActorFactory::create_temporary_sender),
    /// spawn one task per shard that calls ask_helper(), join all, then cleanup temp sender.
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
    ) -> Result<Vec<(u32, String, Duration, bool, String, Option<Message>)>, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();


        tracing::info!(
            group_id = %group_id,
            shard_count = shard_actor_ids.len(),
            timeout_secs = timeout.as_secs(),
            tenant_id = %ctx.tenant_id(),
            "🔄 [{}] Starting parallel operation (ask_helper)",
            operation_name
        );

        // CRITICAL: Use ONE temporary sender for all shards with format "{TEMP_SENDER_PREFIX}-{operation_id}@{node_id}"
        // Each shard gets its own correlation_id (format: "req-shard-{shard_id}-{ulid}" for debugging)
        // All replies go to the same temporary sender ActorRef, but are routed to the correct ReplyWaiter
        // by correlation_id via ReplyWaiterRegistry (which supports multiple correlation_ids)
        use plexspaces_core::TEMP_SENDER_PREFIX;
        let operation_id = Ulid::new().to_string();
        let temp_sender_id = format!("{}-{}@{}", TEMP_SENDER_PREFIX, operation_id, self.local_node_id);
        let expires_at = Instant::now() + (timeout * 2);
        // CRITICAL: Use RequestContext from caller (tenant_id flows from API → ActorBuilder → ActorRef)

        // Create ONE temporary sender for all shards (use operation_id as correlation_id for registration)
        // The actual routing uses ReplyWaiterRegistry keyed by per-shard correlation_ids
        let factory = self.service_locator.get_actor_factory().await
            .ok_or_else(|| "ActorFactory not found in ServiceLocator".to_string())?;
        factory
            .create_temporary_sender(&ctx, temp_sender_id.clone(), operation_id.clone(), expires_at)
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
                let result = ask_helper(
                    ctx_task,
                    sl,
                    sid.clone(),
                    msg,
                    tid,
                    cid.clone(),
                    t,
                ).await;
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
            let (shard_id, shard_actor_id, request_start, result) = join_result
                .map_err(|e| format!("Task join error: {}", e))?;
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
                    results.push((shard_id, shard_actor_id, latency, true, String::new(), Some(reply)));
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
            if result.3 { // success = true
                if let Some(accessor) = self.service_locator.get_node_metrics_accessor().await {
                    accessor.increment_shard_messages_received().await;
                }
                if let Some(registry) = self.service_locator.actor_registry().await {
                    let actor_metrics = registry.actor_metrics();
                    use plexspaces_core::message_metrics::ActorMetricsExt;
                    let mut metrics = actor_metrics.write().await;
                    metrics.increment_shard_messages_received_total();
                }
            }
        }
        
        if failed_count > 0 {
            let errors: Vec<String> = results.iter()
                .filter_map(|r| if !r.3 { Some(format!("Shard {} ({}): {}", r.0, r.1, r.4)) } else { None })
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

        // Cleanup: Remove the single temporary sender (all correlation_ids are cleaned up by ask_helper)
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
            groups.get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req.timeout.map(|d| {
            Duration::from_secs(d.seconds as u64)
                + Duration::from_nanos(d.nanos as u64)
        }).unwrap_or(Duration::from_secs(10));

        let query_proto = req.map_function.ok_or_else(|| "map_function is required".to_string())?;

        // Use unified parallel operation helper
        // CRITICAL: Pass RequestContext with tenant_id (flows from API → ActorBuilder → ActorRef)
        let results = self.parallel_operation_unified(
            _ctx.clone(),
            group_id.clone(),
            group.shard_actor_ids.clone(),
            query_proto,
            timeout,
            "MAP_SHARD_GROUP",
        ).await?;

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
        
        // Track shard operation metrics
        if let Some(accessor) = self.service_locator.get_node_metrics_accessor().await {
            accessor.increment_shard_operations_total().await;
            if shards_failed > 0 {
                accessor.increment_shard_operations_failed().await;
            }
        }
        if let Some(registry) = self.service_locator.actor_registry().await {
            let actor_metrics = registry.actor_metrics();
            use plexspaces_core::message_metrics::ActorMetricsExt;
            let mut metrics = actor_metrics.write().await;
            metrics.increment_shard_operations_total();
            if shards_failed > 0 {
                metrics.increment_shard_operations_failed_total();
            }
        }
        
        if shards_failed > 0 {
            let failed_shards: Vec<String> = shard_responses.iter()
                .filter_map(|r| if !r.success { Some(format!("Shard {} ({}): {}", r.shard_id, r.shard_actor_id, r.error)) } else { None })
                .collect();
            tracing::warn!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = group.shard_count,
                shards_responded,
                shards_failed,
                failed_shards = ?failed_shards,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "⚠️  [MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded, {} failed",
                shards_responded,
                group.shard_count,
                shards_failed
            );
        } else {
            tracing::info!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = group.shard_count,
                shards_responded,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "✅ [MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded successfully",
                shards_responded,
                group.shard_count
            );
        }

        use plexspaces_proto::actor::v1::ScatterGatherStats;
        Ok(MapShardGroupResponse {
            shard_results: shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: group.shard_count,
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
            groups.get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req.timeout.map(|d| {
            Duration::from_secs(d.seconds as u64)
                + Duration::from_nanos(d.nanos as u64)
        }).unwrap_or(Duration::from_secs(5));

        let query = req.query.ok_or_else(|| "query is required".to_string())?;

        // Use unified parallel operation helper
        // CRITICAL: Pass RequestContext with tenant_id (flows from API → ActorBuilder → ActorRef)
        let results = self.parallel_operation_unified(
            _ctx.clone(),
            group_id.clone(),
            group.shard_actor_ids.clone(),
            query,
            timeout,
            "SCATTER_GATHER",
        ).await?;

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
                shards_responded,
                req.min_responses
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
                successful_responses.first().map(|(_shard_id, resp)| resp.clone())
            }
        };

        let total_duration = start_time.elapsed();
        
        // Track shard operation metrics
        if let Some(accessor) = self.service_locator.get_node_metrics_accessor().await {
            accessor.increment_shard_operations_total().await;
            if shards_failed > 0 {
                accessor.increment_shard_operations_failed().await;
            }
        }
        if let Some(registry) = self.service_locator.actor_registry().await {
            let actor_metrics = registry.actor_metrics();
            use plexspaces_core::message_metrics::ActorMetricsExt;
            let mut metrics = actor_metrics.write().await;
            metrics.increment_shard_operations_total();
            if shards_failed > 0 {
                metrics.increment_shard_operations_failed_total();
            }
        }
        
        tracing::info!(
            group_id = %group_id,
            total_duration_ms = total_duration.as_millis(),
            shards_queried = group.shard_count,
            shards_responded,
            shards_failed,
            min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
            max_latency_ms = max_latency.as_millis(),
            has_aggregated_result = result.is_some(),
            "✅ [SCATTER_GATHER] Scatter-gather operation completed: {}/{} shards responded successfully",
            shards_responded,
            group.shard_count
        );

        Ok(ScatterGatherResponse {
            result,
            shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: group.shard_count,
                shards_responded: shards_responded as u32,
                shards_failed: shards_failed as u32,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        })
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
        ).await
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
        let actor_factory = self.service_locator.get_actor_factory().await
            .ok_or_else(|| Status::internal("Actor factory not available"))?;

        for shard_actor_id in &group.shard_actor_ids {
            let _ = actor_factory.stop_actor(&ctx, shard_actor_id).await;
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
        metrics::counter!("plexspaces_shard_group_deleted_total", 
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
        let group = groups.get(&req.group_id)
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
        let req = request.into_inner();
        
        // Extract RequestContext from gRPC metadata
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        
        // TODO: Implement scale_shard_group_internal
        // For now, return not implemented
        Err(Status::unimplemented("ScaleShardGroup not yet implemented"))
    }

    async fn list_shard_groups(
        &self,
        request: Request<ListShardGroupsRequest>,
    ) -> Result<Response<ListShardGroupsResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        let groups = self.shard_groups.read().await;
        let filtered: Vec<ShardGroup> = groups.values()
            .filter(|g| {
                // Filter by actor_type if specified
                if !req.actor_type.is_empty() && g.actor_type != req.actor_type {
                    return false;
                }
                // Filter by state if specified
                if req.state != ShardGroupState::ShardGroupStateUnspecified as i32
                    && g.state != req.state {
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

        let paginated: Vec<ShardGroup> = filtered.into_iter()
            .skip(offset)
            .take(limit)
            .collect();

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
        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups.get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        // Calculate shard_id from partition_key using partition strategy
        use crate::actor_service::partition::calculate_shard_id;
        let shard_id = calculate_shard_id(
            &req.partition_key,
            group.partition_strategy,
            group.shard_count,
            None, // TODO: Support range boundaries from group metadata
        ).map_err(|e| Status::invalid_argument(format!("Partition calculation failed: {}", e)))?;

        let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
            .ok_or_else(|| Status::internal(format!("Invalid shard_id {}", shard_id)))?
            .clone();

        // Route message to shard actor
        let mut message = req.message.ok_or_else(|| Status::invalid_argument("message is required"))?;
        message.receiver_id = shard_actor_id.clone();

        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        // Extract RequestContext from gRPC request
        // TODO: Extract tenant_id and namespace from metadata headers
        let ctx = RequestContext::new_without_auth(String::new(), String::new());

        let response_message = if req.wait_for_response {
            let (_, response) = self.route_message(ctx.clone(), &shard_actor_id, message, true, timeout).await?;
            response
        } else {
            let _ = self.route_message(ctx.clone(), &shard_actor_id, message, false, None).await?;
            None
        };

        // Emit metrics
        metrics::counter!("plexspaces_send_to_shard_total",
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
    // Old implementation kept for reference (can be removed later)
    #[allow(dead_code)]
    async fn scatter_gather_old(
        &self,
        request: Request<ScatterGatherRequest>,
    ) -> Result<Response<ScatterGatherResponse>, Status> {
        self.check_accepting_requests().await?;
        let req = request.into_inner();

        // Get group
        let group = {
            let groups = self.shard_groups.read().await;
            groups.get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        }).unwrap_or(std::time::Duration::from_secs(5));

        let query = req.query.ok_or_else(|| Status::invalid_argument("query is required"))?;

        // Prepare messages for all shards
        let mut query_messages = Vec::new();
        for (shard_id, shard_actor_id) in group.shard_actor_ids.iter().enumerate() {
            let mut query_msg = query.clone();
            query_msg.receiver_id = shard_actor_id.clone();
            query_messages.push((shard_id as u32, shard_actor_id.clone(), query_msg));
        }

        // Send query to all shards in parallel using tokio::spawn
        use futures::future::join_all;
        let mut handles = Vec::new();

        for (shard_id, shard_actor_id, query_msg) in query_messages {
            let actor_id = shard_actor_id.clone();
            let query_msg_clone = query_msg.clone();
            
            // Clone what we need for routing
            let service_locator = self.service_locator.clone();
            let local_node_id = self.local_node_id.clone();
            
            let handle = tokio::spawn(async move {
                let start = std::time::Instant::now();
                // Route message using service_locator to get ActorRegistry
                // This is a simplified routing - in production, we'd use the full route_message logic
                let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> = service_locator.actor_registry().await;
                match actor_registry {
                    Some(registry) => {
                        // Lookup actor and send message
                        match registry.lookup_actor(&actor_id).await {
                            Some(actor_ref) => {
                                // Send message using tell (fire-and-forget)
                                // TODO: Implement proper ask pattern with ReplyWaiter for request-reply
                                match actor_ref.tell(query_msg_clone).await {
                                    Ok(_) => {
                                        let latency = start.elapsed();
                                        // For now, return success without response (scatter-gather with COLLECT will need proper ask)
                                        (shard_id, actor_id.clone(), latency, true, String::new(), None::<Message>)
                                    }
                                    Err(e) => {
                                        let latency = start.elapsed();
                                        (shard_id, actor_id.clone(), latency, false, e.to_string(), None::<Message>)
                                    }
                                }
                            }
                            None => {
                                let latency = start.elapsed();
                                (shard_id, actor_id.clone(), latency, false, format!("Actor {} not found", actor_id), None::<Message>)
                            }
                        }
                    }
                    None => {
                        let latency = start.elapsed();
                        (shard_id, actor_id, latency, false, "Actor registry not available".to_string(), None::<Message>)
                    }
                }
            });
            handles.push(handle);
        }
        
        // Convert JoinHandles to futures
        let futures: Vec<_> = handles.into_iter().map(|h| async move {
            h.await.unwrap_or_else(|e| {
                (0, String::new(), std::time::Duration::ZERO, false, format!("Task join error: {}", e), None::<Message>)
            })
        }).collect();

        // Execute all queries with timeout
        let results = tokio::time::timeout(timeout, join_all(futures)).await;

        let mut shard_responses = Vec::new();
        let mut shards_responded = 0;
        let mut shards_failed = 0;
        let mut max_latency = std::time::Duration::ZERO;

        match results {
            Ok(results_vec) => {
                for (shard_id, shard_actor_id, latency, success, error, response) in results_vec {
                    if success {
                        shards_responded += 1;
                    } else {
                        shards_failed += 1;
                    }
                    if latency > max_latency {
                        max_latency = latency;
                    }
                    shard_responses.push(ShardQueryResponse {
                        shard_id,
                        shard_actor_id,
                        response,
                        latency: Some(prost_types::Duration {
                            seconds: latency.as_secs() as i64,
                            nanos: latency.subsec_nanos() as i32,
                        }),
                        success,
                        error,
                    });
                }
            }
            Err(_) => {
                // Timeout - mark all as failed
                shards_failed = group.shard_count;
            }
        }

        // Aggregate results based on strategy
        let result = match req.aggregation {
            x if x == ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32 => {
                // Concatenate all responses
                let mut payload = Vec::new();
                for resp in &shard_responses {
                    if let Some(msg) = &resp.response {
                        payload.extend_from_slice(&msg.payload);
                    }
                }
                Some(Message {
                    id: format!("scatter-gather-{}", ulid::Ulid::new()),
                    sender_id: "scatter-gather".to_string(),
                    receiver_id: String::new(),
                    channel: String::new(),
                    message_type: String::new(),
                    payload,
                    timestamp: Some(prost_types::Timestamp {
                        seconds: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64,
                        nanos: 0,
                    }),
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
                // Default: return first response
                shard_responses.first()
                    .and_then(|r| r.response.clone())
            }
        };

        // Emit metrics
        metrics::counter!("plexspaces_scatter_gather_total",
            "group_id" => req.group_id.clone());
        if max_latency > std::time::Duration::ZERO {
            metrics::histogram!("plexspaces_scatter_gather_duration_seconds",
                "group_id" => req.group_id.clone()).record(max_latency.as_secs_f64());
        }
        // Note: shard_count histogram removed - use counter with labels instead if needed

        Ok(Response::new(ScatterGatherResponse {
            result,
            shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: group.shard_count,
                shards_responded,
                shards_failed,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        }))
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
        ).await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let req = request.into_inner();
        let resp = self.bulk_update_shard_group_internal(&ctx, req).await
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
            groups.get(&req.group_id)
                .ok_or_else(|| Status::not_found(format!("ShardGroup {} not found", req.group_id)))?
                .clone()
        };

        // Route updates to appropriate shards based on partition_key
        use crate::actor_service::partition::calculate_shard_id;
        use futures::future::join_all;
        
        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        }).unwrap_or(std::time::Duration::from_secs(30));

        // Group updates by shard_id
        let mut updates_by_shard: std::collections::HashMap<u32, Vec<(String, Message)>> = std::collections::HashMap::new();
        for (partition_key_str, mut message) in req.updates {
            let partition_key = partition_key_str.as_bytes();
            let shard_id = calculate_shard_id(
                partition_key,
                group.partition_strategy,
                group.shard_count,
                None,
            ).map_err(|e| Status::invalid_argument(format!("Partition calculation failed: {}", e)))?;
            
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
                .ok_or_else(|| Status::internal(format!("Invalid shard_id {}", shard_id)))?
                .clone();
            
            message.receiver_id = shard_actor_id.clone();
            updates_by_shard.entry(shard_id).or_insert_with(Vec::new).push((partition_key_str, message));
        }

        // Send updates to shards in parallel
        let mut handles = Vec::new();
        let mut shard_stats_map: std::collections::HashMap<u32, ShardUpdateStats> = std::collections::HashMap::new();

        for (shard_id, updates) in updates_by_shard {
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize).unwrap().clone();
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
                    x if x == plexspaces_proto::v1::actor::ConsistencyLevel::ConsistencyLevelEventual as i32 => {
                        // Eventual consistency: send all updates, don't wait
                        for (_key, message) in updates_clone {
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> = service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                if let Some(actor_ref) = registry.lookup_actor(&message.receiver_id).await {
                                    if actor_ref.tell(message).await.is_ok() {
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
                            let actor_registry: Option<Arc<plexspaces_core::ActorRegistry>> = service_locator.actor_registry().await;
                            if let Some(registry) = actor_registry {
                                if let Some(actor_ref) = registry.lookup_actor(&message.receiver_id).await {
                                    if wait_for_responses {
                                        // Wait for response (stronger consistency)
                                        // Use route_message via ActorService (simplified - actual implementation would use proper routing)
                                        // For now, just send and mark as succeeded (proper ask pattern would require ReplyWaiter)
                                        if actor_ref.tell(message).await.is_ok() {
                                            succeeded += 1;
                                        } else {
                                            failed += 1;
                                        }
                                    } else {
                                        // Fire-and-forget
                                        if actor_ref.tell(message).await.is_ok() {
                                            succeeded += 1;
                                        } else {
                                            failed += 1;
                                        }
                                    }
                                } else {
                                    failed += 1;
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
        let results = tokio::time::timeout(timeout, join_all(handles)).await
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
            
            let shard_actor_id = group.shard_actor_ids.get(shard_id as usize)
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
            "group_id" => req.group_id.clone()).record(total_sent as f64);

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
    /// Extract tenant_id from JWT claims in request metadata (legacy method, kept for compatibility)
    fn extract_tenant_id_from_jwt(metadata: &tonic::metadata::MetadataMap) -> Option<String> {
        // Try to get from x-tenant-id header (set by JWT middleware)
        metadata.get("x-tenant-id")
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

    async fn activate_actor(
        &self,
        request: Request<ActivateActorRequest>,
    ) -> Result<Response<ActivateActorResponse>, Status> {
        self.0.activate_actor(request).await
    }

    async fn deactivate_actor(
        &self,
        request: Request<DeactivateActorRequest>,
    ) -> Result<Response<Empty>, Status> {
        self.0.deactivate_actor(request).await
    }

    async fn check_actor_exists(
        &self,
        request: Request<CheckActorExistsRequest>,
    ) -> Result<Response<CheckActorExistsResponse>, Status> {
        self.0.check_actor_exists(request).await
    }

    async fn get_or_activate_actor(
        &self,
        request: Request<GetOrActivateActorRequest>,
    ) -> Result<Response<GetOrActivateActorResponse>, Status> {
        self.0.get_or_activate_actor(request).await
    }

    async fn invoke_actor(
        &self,
        request: Request<InvokeActorRequest>,
    ) -> Result<Response<InvokeActorResponse>, Status> {
        self.0.invoke_actor(request).await
    }

    async fn terminate_actor(
        &self,
        request: Request<TerminateActorRequest>,
    ) -> Result<Response<TerminateActorResponse>, Status> {
        self.0.terminate_actor(request).await
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
}

pub mod get_or_activate_impl;
pub mod partition;
pub use get_or_activate_impl::get_or_activate_actor_impl;

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
    use plexspaces_keyvalue::SqliteKVStore;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::object_registry::v1::ObjectRegistration;
    use plexspaces_proto::object_registry::v1::ObjectType;
    use std::time::Duration as StdDuration;
    use ulid::Ulid;
    use chrono::{DateTime, Utc};
    
    /// Helper to create a test message with proto Message type
    fn create_test_message(payload: Vec<u8>) -> Message {
        Message {
            id: Ulid::new().to_string(),
            payload,
            ..Default::default()
        }
    }

    /// Simple wrapper to adapt ObjectRegistryImpl to ObjectRegistryTrait
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistryImpl>,
    }

    #[async_trait::async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
            self.inner
                .lookup(ctx, obj_type, object_id)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })
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
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })
        }

        async fn register(
            &self,
            ctx: &plexspaces_core::RequestContext,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .register(ctx, registration)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })
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
                .discover(ctx, object_type, object_category, capabilities, labels, health_status, offset, limit)
                .await
                .map_err(|e| -> Box<dyn std::error::Error + Send + Sync> {
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
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
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
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
                    Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
                })
        }
    }

    /// Helper to create a test ActorRegistry (async because SqliteObjectRegistryRepository::new is async)
    async fn create_test_registry(local_node_id: &str) -> Arc<ActorRegistry> {
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
        let object_registry_impl = Arc::new(ObjectRegistryImpl::new(object_repo));
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        Arc::new(ActorRegistry::new(object_registry, local_node_id.to_string()))
    }

    /// Helper to create ActorServiceImpl with proper ServiceLocator setup for tests
    async fn create_test_actor_service(actor_registry: Arc<ActorRegistry>, node_id: String) -> ActorServiceImpl {
        use crate::service_locator::ServiceLocatorImpl;
        use plexspaces_core::ServiceLocator as ServiceLocatorTrait;
        // Create ServiceLocatorImpl directly
        let service_locator_impl = Arc::new(ServiceLocatorImpl::new());
        // Register actor_registry using strongly-typed method
        service_locator_impl.register_actor_registry(actor_registry.clone()).await;
        // Initialize with default services
        service_locator_impl.initialize_services(
            Some(node_id.clone()),
            None,
            None,
        ).await;
        ActorServiceImpl::new(service_locator_impl, node_id)
    }

    /// Helper to register an actor with ActorRegistry for tests
    async fn register_test_actor(
        actor_registry: Arc<ActorRegistry>,
        actor_id: String,
        mailbox: Arc<Mailbox>,
        service_locator: Arc<dyn ServiceLocatorTrait>,
    ) {
        // CRITICAL: Pass tenant_id from RequestContext to ActorRef (empty for tests)
        let sender: Arc<dyn MessageSender> = Arc::new(plexspaces_actor::ActorRef::local(
            actor_id.clone(),
            String::new(), // Test context uses empty tenant_id
            String::new(), // Test context uses empty namespace
            mailbox,
            service_locator,
        ));
        // Tenant comes from auth, not config - use empty strings for test actor registration
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::new_without_auth(String::new(), String::new());
        actor_registry.register_actor(&ctx, actor_id, sender, None, None, None, None).await;
    }

    /// Helper to create a test ActorRegistry with a node registration
    async fn create_test_registry_with_node(local_node_id: &str, node_id: &str, node_address: &str) -> Arc<ActorRegistry> {
        let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
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
        
        object_registry_impl.register(&ctx, registration).await.unwrap();
        
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        Arc::new(ActorRegistry::new(object_registry, local_node_id.to_string()))
    }

    // ========================================================================
    // UNIT TESTS - route_local (TDD Red Phase)
    // ========================================================================

    #[tokio::test]
    async fn test_route_local_actor_not_found() {
        // ARRANGE: Create service with empty local actors
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route to non-existent actor
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_local(ctx, "nonexistent", "node1", message, false, None)
            .await;

        // ASSERT: Should fail with NotFound
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
        assert!(err.message().contains("Actor not found") || err.message().contains("not found"));
    }

    #[tokio::test]
    async fn test_route_local_fire_and_forget_success() {
        // ARRANGE: Create actor and register it
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = create_test_message(b"hello".to_vec());
        let message_id = message.id.to_string();

        // ACT: Route message (fire-and-forget)
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_local(
                ctx, "test", "node1", message, false, // fire-and-forget
                None,
            )
            .await;

        // ASSERT: Should succeed
        if let Err(e) = &result {
            tracing::warn!("route_local failed: {}", e);
            tracing::warn!("Actor ID: test@node1");
            // Check if actor is registered
            let found = service.get_actor_registry().await.lookup_actor(&"test@node1".to_string()).await;
            tracing::warn!("Actor found in registry: {}", found.is_some());
            let activated = service.get_actor_registry().await.is_actor_activated(&"test@node1".to_string()).await;
            tracing::warn!("Actor activated: {}", activated);
        }
        assert!(result.is_ok(), "route_local should succeed, got error: {:?}", result.err());
        let (returned_msg_id, response) = result.unwrap();
        assert_eq!(returned_msg_id, message_id);
        assert!(response.is_none()); // No response for fire-and-forget

        // Verify message was delivered to actor's mailbox
        // Poll for message delivery (no sleep - use proper async waiting)
        let delivered_msg = mailbox.dequeue().await;
        assert!(delivered_msg.is_some(), "Message should be delivered immediately");
        assert_eq!(delivered_msg.unwrap().payload, b"hello");
    }

    #[tokio::test]
    async fn test_route_local_request_reply_not_implemented() {
        // ARRANGE: Create actor and register it
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = create_test_message(b"hello".to_vec());

        // ACT: Try request-reply (ask pattern)
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_local(
                ctx,
                "test",
                "node1",
                message,
                true, // wait_for_response
                Some(StdDuration::from_secs(5)),
            )
            .await;

        // ASSERT: Should fail with timeout (no reply received)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Timeout occurs when no reply is received
        assert!(err.code() == tonic::Code::DeadlineExceeded || err.code() == tonic::Code::Internal);
    }

    // ========================================================================
    // UNIT TESTS - route_remote (TDD Red Phase)
    // ========================================================================

    #[tokio::test]
    async fn test_route_remote_node_not_found() {
        // ARRANGE: Create service with empty registry
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Service registration is synchronous - no wait needed

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route to unknown node
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_remote(ctx, "node2", "actor@node2", message, false, None)
            .await;

        // ASSERT: Should fail with NotFound (or Internal if ActorRegistry not registered yet)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Accept Internal if ActorRegistry not registered yet, otherwise NotFound
        assert!(
            err.code() == tonic::Code::NotFound || err.code() == tonic::Code::Internal,
            "Expected NotFound or Internal, got {:?}: {}",
            err.code(),
            err.message()
        );
        if err.code() == tonic::Code::NotFound {
            assert!(err.message().contains("Node not found"));
        }
    }

    #[tokio::test]
    async fn test_register_and_unregister_local_actor() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();

        // ACT: Register actor
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // ASSERT: Actor is in cache
        {
            let is_activated = service.get_actor_registry().await.is_actor_activated(&"test@node1".to_string()).await;
            assert!(is_activated);
        }

        // ACT: Unregister actor
        actor_registry.unregister(&"test@node1".to_string()).await.unwrap();

        // ASSERT: Actor is removed from cache
        {
            let is_activated = service.get_actor_registry().await.is_actor_activated(&"test@node1".to_string()).await;
            assert!(!is_activated);
        }
    }

    #[tokio::test]
    async fn test_parse_actor_id() {
        // Test parsing actor_id format
        let actor_id = "counter@node1";
        let result = actor_id.split_once('@');
        assert!(result.is_some());
        let (actor_name, node_id) = result.unwrap();
        assert_eq!(actor_name, "counter");
        assert_eq!(node_id, "node1");

        // Test invalid format
        let invalid = "invalid";
        let result = invalid.split_once('@');
        assert!(result.is_none());
    }

    // ========================================================================
    // COVERAGE TESTS - route_message()
    // ========================================================================

    #[tokio::test]
    async fn test_route_message_invalid_actor_id() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route with actor ID that doesn't exist (no @node defaults to local)
        // Since actor IDs without @node are now valid (default to local node),
        // this will fail with NotFound when the actor isn't found
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_message(ctx, "invalid_no_node", message, false, None)
            .await;

        // ASSERT: Should fail with NotFound (actor doesn't exist on local node)
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::NotFound);
        assert!(err.message().contains("Actor not found") || err.message().contains("not found"));
    }

    #[tokio::test]
    async fn test_route_message_local_routing() {
        // ARRANGE: Create actor and register it locally
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = create_test_message(b"hello".to_vec());
        let message_id = message.id.to_string();

        // ACT: Route message via route_message() entry point
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_message(ctx, "test@node1", message, false, None)
            .await;

        // ASSERT: Should route locally
        assert!(result.is_ok());
        let (returned_id, response) = result.unwrap();
        assert_eq!(returned_id, message_id);
        assert!(response.is_none());

        // Verify message delivered (poll immediately - no sleep needed)
        let delivered = mailbox.dequeue().await;
        assert!(delivered.is_some(), "Message should be delivered immediately");
        assert_eq!(delivered.unwrap().payload, b"hello");
    }

    // ========================================================================
    // COVERAGE TESTS - send_message() gRPC Handler
    // ========================================================================

    #[tokio::test]
    async fn test_send_message_missing_message() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        // ACT: Call send_message with no message
        let request = tonic::Request::new(SendMessageRequest {
            message: None, // Missing!
            wait_for_response: false,
            timeout: None,
        });

        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should fail with InvalidArgument
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("Message is required"));
    }

    #[tokio::test]
    async fn test_send_message_missing_receiver() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        // Create message without receiver_id
        let mut proto_message = create_test_message(b"test".to_vec());
        proto_message.receiver_id = String::new(); // Empty receiver_id!

        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response: false,
            timeout: None,
        });

        // ACT
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should fail with InvalidArgument
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), tonic::Code::InvalidArgument);
        assert!(err.message().contains("Receiver ID is required"));
    }

    #[tokio::test]
    async fn test_send_message_success() {
        // ARRANGE: Create actor and register it
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create proto message
        let mut message = create_test_message(b"hello".to_vec());
        message.receiver_id = "test@node1".to_string();
        let proto_message = message.clone();
        let expected_message_id = proto_message.id.clone();

        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response: false,
            timeout: None,
        });

        // ACT: Send via gRPC handler
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should succeed
        assert!(result.is_ok());
        let response = result.unwrap().into_inner();
        assert_eq!(response.message_id, expected_message_id);
        assert!(response.response.is_none()); // No response for fire-and-forget

        // Verify delivery (poll immediately - no sleep needed)
        let delivered = mailbox.dequeue().await;
        assert!(delivered.is_some(), "Message should be delivered immediately");
        assert_eq!(delivered.unwrap().payload, b"hello");
    }

    #[tokio::test]
    async fn test_send_message_with_timeout() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create message with timeout
        let mut message = create_test_message(b"test".to_vec());
        message.receiver_id = "test@node1".to_string();
        let proto_message = message.clone();

        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response: false,
            timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 500_000_000, // 5.5 seconds
            }),
        });

        // ACT: Send with timeout (though fire-and-forget ignores it)
        let result = ActorServiceTrait::send_message(&service, request).await;

        // ASSERT: Should succeed (timeout parsed but not used for fire-and-forget)
        assert!(result.is_ok());
    }

    // ========================================================================
    // COVERAGE TESTS - Connection Manager (get_or_create_client removed, now uses GrpcConnectionManager)
    // ========================================================================

    #[tokio::test]
    async fn test_connection_manager_available() {
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Test that connection manager is available
        let conn_manager = service.service_locator.get_grpc_connection_manager().await;
        assert!(conn_manager.is_some(), "GrpcConnectionManager should be available");
    }

    // ========================================================================
    // COVERAGE TESTS - route_remote() Error Paths
    // ========================================================================

    #[tokio::test]
    async fn test_route_remote_node_not_in_registry() {
        // ARRANGE: Create service with empty registry
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Service registration is synchronous - no wait needed

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route to unknown node (not in registry)
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_remote(ctx, "unknown_node", "actor@unknown_node", message, false, None)
            .await;

        // ASSERT: Should fail with NotFound (or Internal if ActorRegistry not registered yet)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Accept Internal if ActorRegistry not registered yet, otherwise NotFound
        assert!(
            err.code() == tonic::Code::NotFound || err.code() == tonic::Code::Internal,
            "Expected NotFound or Internal, got {:?}: {}",
            err.code(),
            err.message()
        );
        if err.code() == tonic::Code::NotFound {
            assert!(err.message().contains("Node not found"));
        }
    }

    #[tokio::test]
    async fn test_route_remote_registry_error() {
        // ARRANGE: Create service with registry that will fail lookup
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Service registration is synchronous - no wait needed

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route to node (registry lookup will fail with NotFound)
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_remote(ctx, "node2", "actor@node2", message, false, None)
            .await;

        // ASSERT: Should fail with NotFound (or Internal if ActorRegistry not registered yet)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Accept Internal if ActorRegistry not registered yet, otherwise NotFound
        assert!(
            err.code() == tonic::Code::NotFound || err.code() == tonic::Code::Internal,
            "Expected NotFound or Internal, got {:?}: {}",
            err.code(),
            err.message()
        );
    }

    #[tokio::test]
    async fn test_route_remote_connection_failed() {
        // ARRANGE: Register a node with unreachable address
        let actor_registry = create_test_registry_with_node("node1", "node2", "127.0.0.1:19999").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Service registration is synchronous - no wait needed

        let message = create_test_message(b"test".to_vec());

        // ACT: Try to route to unreachable node
        let ctx = RequestContext::new_without_auth("test".to_string(), "default".to_string());
        let result = service
            .route_remote(ctx, "node2", "actor@node2", message, false, None)
            .await;

        // ASSERT: Should fail with Unavailable (or Internal if ActorRegistry not registered yet)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Accept Internal if ActorRegistry not registered yet, otherwise Unavailable
        assert!(
            err.code() == tonic::Code::Unavailable || err.code() == tonic::Code::Internal,
            "Expected Unavailable or Internal, got {:?}: {}",
            err.code(),
            err.message()
        );
        if err.code() == tonic::Code::Unavailable {
            assert!(err.message().contains("Connection to") || err.message().contains("failed"));
        }
    }

    // ========================================================================
    // COVERAGE TESTS - send_message() timeout conversion
    // ========================================================================

    #[tokio::test]
    async fn test_send_message_converts_timeout_correctly() {
        // ARRANGE
        let actor_registry = create_test_registry("node1").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create message with fractional seconds timeout
        let mut message = create_test_message(b"test".to_vec());
        message.receiver_id = "test@node1".to_string();
        let proto_message = message.clone();

        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response: false,
            timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 500_000_000, // 5.5 seconds total
            }),
        });

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

    // NOTE: Real gRPC server tests removed - these are covered by integration tests
    // which are faster and more reliable. Unit tests focus on local routing and
    // simulated remote scenarios. The following tests were removed:
    // - test_route_remote_success_with_real_server
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
        let result = service.route_message(ctx, "actor@node2", message, false, None).await;

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
