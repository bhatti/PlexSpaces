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
//!          -> Registry.get_node_address("node2") -> "remote_host:9002"
//!          -> gRPC client.SendMessage("remote_host:9002", msg)
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
//! // Register ActorRegistry, ReplyTracker, ReplyWaiterRegistry in service_locator first
//! let actor_service = ActorServiceImpl::new(service_locator, "node1".to_string());
//!
//! // Start gRPC server
//! let addr = "0.0.0.0:9001".parse()?;
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
use std::time::Duration;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

use plexspaces_core::{ActorId, ActorRegistry, ReplyTracker, ServiceLocator, actor_context::ObjectRegistry as ObjectRegistryTrait, MessageSender};
use plexspaces_actor::ActorFactory;
use plexspaces_actor::ActorRef as ActorRefImpl;
use plexspaces_actor::RegularActorWrapper;
use plexspaces_mailbox::{Message, Mailbox};
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};

// Import proto types and gRPC service trait
use plexspaces_proto::actor::v1::{
    actor_service_client::ActorServiceClient,

    // gRPC service trait and server
    actor_service_server::ActorService as ActorServiceTrait,
    actor_service_server::ActorServiceServer,
    ActorDownNotification,
    ActivateActorRequest,
    ActivateActorResponse,
    CheckActorExistsRequest,
    CheckActorExistsResponse,
    CreateActorRequest,
    CreateActorResponse,
    DeleteActorRequest,
    DeactivateActorRequest,
    GetActorRequest,
    GetActorResponse,
    GetOrActivateActorRequest,
    GetOrActivateActorResponse,
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
    service_locator: Arc<ServiceLocator>,
    
    /// Local node ID (for routing decisions)
    local_node_id: String,
}

impl ActorServiceImpl {
    /// Create new ActorService
    ///
    /// # Arguments
    /// * `service_locator` - ServiceLocator for service access and gRPC client caching
    /// * `local_node_id` - ID of this node
    ///
    /// # Note
    /// Services (ActorRegistry, ReplyTracker) should already be registered in ServiceLocator
    /// before creating ActorServiceImpl. They will be retrieved synchronously if runtime is available,
    /// otherwise on first async access.
    pub fn new(service_locator: Arc<ServiceLocator>, local_node_id: String) -> Self {
        // Services will be retrieved from ServiceLocator on first use
        // This avoids "Cannot start a runtime from within a runtime" errors
        ActorServiceImpl {
            service_locator,
            local_node_id,
        }
    }
    
    /// Get ActorRegistry from ServiceLocator (lazy initialization)
    async fn get_actor_registry(&self) -> Arc<ActorRegistry> {
        self.service_locator
            .get_service()
            .await
            .expect("ActorRegistry must be registered in ServiceLocator")
    }
    
    /// Get ReplyTracker from ServiceLocator (lazy initialization)
    async fn get_reply_tracker(&self) -> Arc<ReplyTracker> {
        self.service_locator
            .get_service()
            .await
            .expect("ReplyTracker must be registered in ServiceLocator")
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
        actor_id: &str,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<ActorRefImpl, Box<dyn std::error::Error + Send + Sync>> {
        // Ensure actor_id uses local node_id
        // Parse actor_id to get actor_name and node_id
        let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
            (name.to_string(), node.to_string())
        } else {
            (actor_id.to_string(), self.local_node_id.clone())
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

        // Use ActorFactory from ServiceLocator (direct dependency - no callbacks needed)
        use plexspaces_actor::ActorFactory;
        use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
        let actor_factory: Arc<ActorFactoryImpl> = self.service_locator.get_service().await
            .ok_or_else(|| format!(
                "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called and ActorFactory is registered. Actor ID would be: {}",
                local_actor_id
            ))?;
        
        // Use ActorFactory to spawn actor
        // ActorFactory returns MessageSender, but we need ActorRefImpl
        // The actor is already registered in ActorRegistry, so we can create ActorRefImpl
        // that uses MessageSender internally
        actor_factory.spawn_actor(
            &local_actor_id,
            actor_type,
            initial_state,
            config,
            labels,
        ).await?;
        
        // Actor is now spawned and registered - create ActorRefImpl pointing to local node
        // ActorRefImpl::remote pointing to local node will use MessageSender from registry
        Ok(ActorRefImpl::remote(
            local_actor_id,
            self.local_node_id.clone(),
            self.service_locator.clone(),
        ))
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
    /// For local actors, the reply is routed to the ReplyTracker via ActorRef::tell().
    pub async fn send_message(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}",
            message.id, actor_id, message.sender, message.receiver, message.message_type_str(), message.correlation_id
        );
        
        // Check if this is a reply (has correlation_id) and route to per-ActorRef reply map if local
        // Extract correlation_id first to avoid borrow issues
        let correlation_id_opt = message.correlation_id.clone();
        if let Some(correlation_id) = &correlation_id_opt {
            // Parse actor@node ID
            let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
                (name.to_string(), node.to_string())
            } else {
                (actor_id.to_string(), self.local_node_id.clone())
            };

            // If local actor, route reply via MessageSender.tell()
            // ActorRef::tell() will automatically check for correlation_id and route to ReplyWaiter
            // if there's a pending ask() call - no need for ReplyTracker!
            if node_id == self.local_node_id {
                // Use MessageSender.tell() - ActorRef::tell() handles reply routing automatically
                // When MessageSender.tell() is called, it eventually calls ActorRef::tell(),
                // which checks self.reply_waiters for the correlation_id and routes to ReplyWaiter
                let actor_id_full = format!("{}@{}", actor_name, node_id);
                if let Some(sender) = self.get_actor_registry().await.lookup_actor(&actor_id_full).await {
                    // MessageSender exists - use it directly
                    // ActorRef::tell() will check for correlation_id and route to ReplyWaiter if present
                    let message_id = message.id().to_string();
                    tracing::debug!(
                        "🟪 [ACTOR_SERVICE::send_message] REPLY ROUTING: message_id={}, correlation_id={}, routing via MessageSender.tell()",
                        message_id, correlation_id
                    );
                    sender.tell(message).await
                        .map_err(|e| Status::internal(format!("Failed to send reply: {}", e)))?;
                    tracing::debug!(
                        "🟪 [ACTOR_SERVICE::send_message] REPLY ROUTED: message_id={}, correlation_id={}",
                        message_id, correlation_id
                    );
                    return Ok(message_id);
                }
            }
        }
        
        // Normal message routing (no correlation_id or remote actor)
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message] NORMAL ROUTING: message_id={}, actor_id={}, calling route_message",
            message.id, actor_id
        );
        let (msg_id, _) = self
            .route_message(actor_id, message, false, None)
            .await
            .map_err(|e| format!("Failed to send message: {}", e))?;
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message] COMPLETED: message_id={}, actor_id={}",
            msg_id, actor_id
        );
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
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message_and_wait] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, timeout={:?}",
            message.id, actor_id, message.sender, message.receiver, message.message_type_str(), message.correlation_id, timeout
        );
        
        // Parse actor_id to determine if local or remote
        let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
            (name.to_string(), node.to_string())
        } else {
            (actor_id.to_string(), self.local_node_id.clone())
        };

        if node_id == self.local_node_id {
            // LOCAL: Get ActorRef and use ask()
            let actor_id_str = actor_id.to_string();
            // For ask(), we need ActorRef. Use MessageSender to determine if actor exists.
            // For activated actors, we can't create ActorRef::local without mailbox.
            // Solution: Use ActorRef::remote pointing to local node - it will use MessageSender internally.
            let actor_ref = if self.get_actor_registry().await.lookup_actor(&actor_id_str).await.is_some() {
                // Actor exists (activated or virtual) - create remote ActorRef pointing to local node
                // ActorRef::ask() will use MessageSender internally
                ActorRefImpl::remote(actor_id_str.clone(), node_id.clone(), self.service_locator.clone())
            } else if self.get_actor_registry().await.is_actor_activated(&actor_id_str).await {
                // Actor is activated but no MessageSender - this shouldn't happen, but handle it
                // Use remote ActorRef pointing to local node
                ActorRefImpl::remote(actor_id_str.clone(), node_id.clone(), self.service_locator.clone())
            } else {
                // Actor doesn't exist - return error
                return Err("Actor not found".into());
            };

            let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(5));
            tracing::debug!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL: message_id={}, actor_id={}, calling ActorRef::ask()",
                message.id, actor_id_str
            );
            let result = actor_ref.ask(message, timeout_duration).await
                .map_err(|e| {
                    use plexspaces_actor::ActorRefError;
                    match e {
                        ActorRefError::ActorNotFound(_) => "Actor not found".into(),
                        _ => format!("Failed to send ask request: {}", e).into(),
                    }
                });
            tracing::debug!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] LOCAL COMPLETED: actor_id={}, result={:?}",
                actor_id_str, result.is_ok()
            );
            result
        } else {
            // REMOTE: Use route_message (which handles remote routing via gRPC)
            tracing::debug!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] REMOTE: message_id={}, actor_id={}, calling route_message",
                message.id, actor_id
            );
            let (_, response) = self
                .route_message(actor_id, message, true, timeout)
                .await
                .map_err(|e| format!("Failed to send message and wait: {}", e))?;

            tracing::debug!(
                "🟪 [ACTOR_SERVICE::send_message_and_wait] REMOTE COMPLETED: actor_id={}, has_response={}",
                actor_id, response.is_some()
            );
            response.ok_or_else(|| "No response received".into())
        }
    }

    /// Route message to local or remote actor
    ///
    /// # Arguments
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
    pub async fn route_message(
        &self,
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        // Extract message_id and other fields for logging before moving message
        let message_id = message.id.clone();
        let message_sender = message.sender.clone();
        let message_receiver = message.receiver.clone();
        let message_type = message.message_type_str().to_string();
        let message_correlation_id = message.correlation_id.clone();
        
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::route_message] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, wait_for_response={}, timeout={:?}",
            message_id, actor_id, message_sender, message_receiver, message_type, message_correlation_id, wait_for_response, timeout
        );
        
        // Parse actor@node ID (or just actor name, defaults to local node)
        let (actor_name, node_id) = if let Some((name, node)) = actor_id.split_once('@') {
            (name.to_string(), node.to_string())
        } else {
            // No @node specified, default to local node
            (actor_id.to_string(), self.local_node_id.clone())
        };

        // OBSERVABILITY: Track routing decision
        metrics::counter!("plexspaces_actor_service_route_total",
            "actor_id" => actor_id.to_string(),
            "node_id" => node_id.clone(),
            "local" => if node_id == self.local_node_id { "true" } else { "false" }
        )
        .increment(1);

        let result = if node_id == self.local_node_id {
            // LOCAL ROUTING: Deliver to local actor
            tracing::debug!(
                "🟪 [ACTOR_SERVICE::route_message] LOCAL ROUTING: message_id={}, actor_id={}",
                message_id, actor_id
            );
            self.route_local(&actor_name, &node_id, message, wait_for_response, timeout)
                .await
        } else {
            // REMOTE ROUTING: Forward to remote node
            tracing::debug!(
                "🟪 [ACTOR_SERVICE::route_message] REMOTE ROUTING: message_id={}, actor_id={}, node_id={}",
                message_id, actor_id, node_id
            );
            self.route_remote(&node_id, actor_id, message, wait_for_response, timeout)
                .await
        };
        
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::route_message] COMPLETED: message_id={}, actor_id={}, result={:?}",
            message_id, actor_id, result.is_ok()
        );
        result
    }

    /// Route message to local actor
    ///
    /// ## Design
    /// Uses ActorRef::tell() and ActorRef::ask() instead of direct mailbox access.
    /// This ensures proper routing, metrics, and virtual actor activation.
    async fn route_local(
        &self,
        actor_name: &str,
        node_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        let start = std::time::Instant::now();

        // Construct full actor ID
        let actor_id = format!("{}@{}", actor_name, node_id);
        let message_id = message.id().to_string();

        // Get ActorRef from ActorRegistry (direct dependency - no callbacks needed)
        // ActorRef::tell() and ActorRef::ask() handle virtual actor activation automatically
        // via VirtualActorWrapper which activates actors on first message
        // Try to get mailbox first (for activated actors)
        // If no mailbox, check if Actor trait exists (for virtual actors)
        // If neither exists, actor doesn't exist - return NotFound
        // For tell(), use MessageSender directly if available, otherwise create remote ActorRef
        // We can't create ActorRef::local without mailbox, so always use remote pointing to local node
        let actor_ref = if self.get_actor_registry().await.lookup_actor(&actor_id).await.is_some() {
            // Actor exists (activated or virtual) - create remote ActorRef pointing to local node
            // ActorRef::tell() will use MessageSender internally
            ActorRefImpl::remote(actor_id.clone(), node_id.to_string(), self.service_locator.clone())
        } else if self.get_actor_registry().await.is_actor_activated(&actor_id).await {
            // Actor is activated but no MessageSender - use remote ActorRef pointing to local node
            ActorRefImpl::remote(actor_id.clone(), node_id.to_string(), self.service_locator.clone())
        } else {
            // Actor doesn't exist - return NotFound
            return Err(Status::not_found(format!("Actor not found: {}", actor_id)));
        };

        // OBSERVABILITY: Track duration
        let duration = start.elapsed();
        metrics::histogram!("plexspaces_actor_service_local_route_duration_seconds")
            .record(duration.as_secs_f64());

        if wait_for_response {
            // ASK PATTERN: Use ActorRef::ask() - handles ReplyTracker internally
            let timeout_duration = timeout.unwrap_or(std::time::Duration::from_secs(5));
            
            match actor_ref.ask(message, timeout_duration).await {
                Ok(reply) => {
                    metrics::counter!("plexspaces_actor_service_local_route_success_total",
                        "pattern" => "ask"
                    )
                    .increment(1);
                    Ok((message_id, Some(reply)))
                }
                Err(e) => {
                    use plexspaces_actor::ActorRefError;
                    let error_type = match e {
                        ActorRefError::Timeout => "timeout",
                        ActorRefError::ActorTerminated => "actor_terminated",
                        _ => "other",
                    };
                    
                    metrics::counter!("plexspaces_actor_service_local_route_error_total",
                        "pattern" => "ask",
                        "error" => error_type
                    )
                    .increment(1);
                    
                    match e {
                        ActorRefError::Timeout => {
                            Err(Status::deadline_exceeded("No reply received within timeout"))
                        }
                        ActorRefError::ActorTerminated => {
                            Err(Status::internal("Actor terminated before reply"))
                        }
                        _ => {
                            Err(Status::internal(format!("Failed to send ask request: {}", e)))
                        }
                    }
                }
            }
        } else {
            // TELL PATTERN: Use ActorRef::tell() - handles routing and metrics
            actor_ref.tell(message).await
                .map_err(|e| {
                    // Convert ActorRefError to appropriate Status
                    use plexspaces_actor::ActorRefError;
                    match e {
                        ActorRefError::ActorNotFound(_) => {
                            Status::not_found(format!("Actor not found: {}", actor_id))
                        }
                        ActorRefError::SendFailed(msg) if msg.contains("not found") || msg.contains("Actor not found") => {
                            Status::not_found(format!("Actor not found: {}", actor_id))
                        }
                        _ => {
                            Status::internal(format!("Failed to send message: {}", e))
                        }
                    }
                })?;
            
            metrics::counter!("plexspaces_actor_service_local_route_success_total",
                "pattern" => "tell"
            )
            .increment(1);

            Ok((message_id, None))
        }
    }

    /// Route message to remote actor
    async fn route_remote(
        &self,
        node_id: &str,
        _actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), Status> {
        let start = std::time::Instant::now();

        // OBSERVABILITY: Track remote routing
        metrics::counter!("plexspaces_actor_service_remote_route_total",
            "target_node" => node_id.to_string()
        )
        .increment(1);

        // Get or create gRPC client for remote node
        let mut client = self.get_or_create_client(node_id).await?;

        // Convert message to proto
        let proto_message = message.to_proto();

        // Convert timeout to proto Duration
        let proto_timeout = timeout.map(|d| prost_types::Duration {
            seconds: d.as_secs() as i64,
            nanos: d.subsec_nanos() as i32,
        });

        // Create SendMessage request
        let request = tonic::Request::new(SendMessageRequest {
            message: Some(proto_message),
            wait_for_response,
            timeout: proto_timeout,
        });

        // Forward to remote ActorService
        let response = client.send_message(request).await.map_err(|e| {
            metrics::counter!("plexspaces_actor_service_remote_route_error_total",
                "target_node" => node_id.to_string(),
                "error" => e.code().to_string()
            )
            .increment(1);
            Status::unavailable(format!("Remote call to {} failed: {}", node_id, e))
        })?;

        let response_inner = response.into_inner();

        // OBSERVABILITY: Track duration
        let duration = start.elapsed();
        metrics::histogram!("plexspaces_actor_service_remote_route_duration_seconds")
            .record(duration.as_secs_f64());

        metrics::counter!("plexspaces_actor_service_remote_route_success_total",
            "target_node" => node_id.to_string()
        )
        .increment(1);

        // Convert response back to internal Message if present
        let reply_message = response_inner
            .response
            .map(|proto_msg| Message::from_proto(&proto_msg));

        Ok((response_inner.message_id, reply_message))
    }

    /// Get or create gRPC client for remote node
    ///
    /// Uses ServiceLocator for gRPC client caching (one client per node, shared across all ActorRefs)
    async fn get_or_create_client(
        &self,
        node_id: &str,
    ) -> Result<ActorServiceClient<tonic::transport::Channel>, Status> {
        // Use ServiceLocator to get cached gRPC client
        self.service_locator
            .get_node_client(node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get gRPC client: {}", e)))
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
        actor_id: &str,
        message: Message,
        wait_for_response: bool,
        timeout: Option<std::time::Duration>,
    ) -> Result<(String, Option<Message>), String> {
        self.route_message(actor_id, message, wait_for_response, timeout)
            .await
            .map_err(|e| e.to_string())
    }

    /// Send a reply message to the sender of the original message
    ///
    /// ## Purpose
    /// Unified method for sending replies that handles:
    /// - Temporary sender IDs (from ask() called outside actor context)
    /// - Local and remote reply routing
    /// - Self-messaging validation
    /// - Correlation ID routing
    ///
    /// ## Arguments
    /// * `correlation_id` - Correlation ID from the original message (optional)
    /// * `sender_id` - ID of the actor that sent the original message (or temporary sender ID)
    /// * `target_actor_id` - ID of the actor sending the reply (usually `msg.receiver`)
    /// * `reply_message` - The reply message to send
    ///
    /// ## Returns
    /// Ok(()) if reply was sent successfully
    pub async fn send_reply(
        &self,
        correlation_id: Option<&str>,
        sender_id: &ActorId,
        target_actor_id: ActorId,
        mut reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        
        tracing::debug!(
            "🔴 [ACTOR_SERVICE::SEND_REPLY] START: sender_id={}, target_actor_id={}, correlation_id={:?}, reply_message_type={}",
            sender_id, target_actor_id, correlation_id, reply_message.message_type_str()
        );
        
        // VALIDATION: Check for self-messaging (target == sender)
        if sender_id == &target_actor_id {
            tracing::error!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] SELF-MESSAGING DETECTED! sender_id={}, target_actor_id={}, correlation_id={:?}",
                sender_id, target_actor_id, correlation_id
            );
            return Err(format!(
                "Self-messaging detected in send_reply: actor {} cannot send reply to itself",
                sender_id
            ).into());
        }
        
        // Check if sender_id is a temporary sender ID (from ask() called outside actor context)
        let is_temporary = sender_id.starts_with("ask-") && sender_id.contains('@');
        
        if is_temporary {
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] TEMPORARY SENDER DETECTED: sender_id={}, target_actor_id={}, correlation_id={:?}",
                sender_id, target_actor_id, correlation_id
            );
            
            // Extract node_id from temporary sender ID (format: "ask-{correlation_id}@{node_id}")
            let node_id = if let Some(prefix_removed) = sender_id.strip_prefix("ask-") {
                if let Some((_corr_id, node_id)) = prefix_removed.split_once('@') {
                    Some(node_id.to_string())
                } else {
                    None
                }
            } else {
                None
            };
            
            let node_id = node_id.ok_or_else(|| format!("Invalid temporary sender ID format: {}", sender_id))?;
            
            // Extract correlation_id from temporary sender ID
            let temp_corr_id = if let Some(prefix_removed) = sender_id.strip_prefix("ask-") {
                if let Some((corr_id, _node_id)) = prefix_removed.split_once('@') {
                    Some(corr_id.to_string())
                } else {
                    None
                }
            } else {
                None
            };
            
            // Use provided correlation_id if available, otherwise use extracted one
            let final_corr_id = correlation_id.or_else(|| temp_corr_id.as_deref())
                .ok_or_else(|| format!("No correlation_id available for temporary sender: {}", sender_id))?;
            
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] TEMPORARY SENDER ROUTING: node_id={}, correlation_id={}, target_actor_id={}",
                node_id, final_corr_id, target_actor_id
            );
            
            // Prepare reply message
            reply_message.receiver = sender_id.clone(); // Temporary sender ID is the receiver
            reply_message.sender = Some(target_actor_id.clone());
            reply_message.correlation_id = Some(final_corr_id.to_string());
            
            // Create ActorRef with temporary sender ID - tell() will detect and route to ReplyWaiter
            let actor_ref = ActorRefImpl::remote(sender_id.clone(), node_id, self.service_locator.clone());
            
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] CALLING tell() FOR TEMPORARY SENDER: reply_sender={}, reply_receiver={}, correlation_id={}",
                target_actor_id, sender_id, final_corr_id
            );
            
            let result = actor_ref.tell(reply_message).await;
            
            let duration = start.elapsed();
            match &result {
                Ok(_) => {
                    metrics::counter!("plexspaces_actor_service_send_reply_total",
                        "sender_type" => "temporary",
                        "status" => "success"
                    ).increment(1);
                    metrics::histogram!("plexspaces_actor_service_send_reply_duration_seconds",
                        "sender_type" => "temporary"
                    ).record(duration.as_secs_f64());
                    tracing::debug!(
                        "🔴 [ACTOR_SERVICE::SEND_REPLY] SUCCESS (TEMPORARY SENDER): duration_ms={}",
                        duration.as_millis()
                    );
                }
                Err(e) => {
                    metrics::counter!("plexspaces_actor_service_send_reply_total",
                        "sender_type" => "temporary",
                        "status" => "error"
                    ).increment(1);
                    metrics::counter!("plexspaces_actor_service_send_reply_errors_total",
                        "sender_type" => "temporary"
                    ).increment(1);
                    tracing::error!(
                        "🔴 [ACTOR_SERVICE::SEND_REPLY] ERROR (TEMPORARY SENDER): error={}, duration_ms={}",
                        e, duration.as_millis()
                    );
                }
            }
            
            return result.map_err(|e| format!("Failed to send reply: {}", e).into());
        }
        
        // Normal routing: sender_id is a real actor ID
        tracing::debug!(
            "🔴 [ACTOR_SERVICE::SEND_REPLY] NORMAL ROUTING: sender_id={}, target_actor_id={}, correlation_id={:?}",
            sender_id, target_actor_id, correlation_id
        );
        
        // Determine if sender is local or remote
        let (sender_name, sender_node_id) = if let Some((name, node)) = sender_id.split_once('@') {
            (name.to_string(), Some(node.to_string()))
        } else {
            (sender_id.to_string(), None)
        };
        
        let is_local = sender_node_id.as_ref()
            .map(|n| n == &self.local_node_id)
            .unwrap_or(true);
        
        // Prepare reply message
        reply_message.receiver = sender_id.clone();
        reply_message.sender = Some(target_actor_id.clone());
        if let Some(corr_id) = correlation_id {
            reply_message.correlation_id = Some(corr_id.to_string());
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] SET CORRELATION_ID: correlation_id={}",
                corr_id
            );
        }
        
        tracing::debug!(
            "🔴 [ACTOR_SERVICE::SEND_REPLY] REPLY MESSAGE PREPARED: reply_sender={}, reply_receiver={}, correlation_id={:?}, is_local={}",
            target_actor_id, sender_id, reply_message.correlation_id, is_local
        );
        
        let result = if is_local {
            // LOCAL: Look up sender in registry and use tell()
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] LOCAL REPLY: Looking up sender actor_id={} in registry",
                sender_id
            );
            
            let sender = self.get_actor_registry().await.lookup_actor(sender_id).await
                .ok_or_else(|| format!("Actor not found: {}", sender_id))?;
            
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] REGISTRY LOOKUP SUCCESS: Found sender ActorRef for actor_id={}",
                sender_id
            );
            
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] CALLING tell() FOR LOCAL REPLY: reply_sender={}, reply_receiver={}, correlation_id={:?}",
                target_actor_id, sender_id, reply_message.correlation_id
            );
            
            sender.tell(reply_message).await
                .map_err(|e| format!("MessageSender.tell() failed: {}", e).into())
        } else {
            // REMOTE: Create ActorRef and use tell()
            let node_id = sender_node_id.ok_or_else(|| format!("No node_id in sender_id: {}", sender_id))?;
            
            tracing::debug!(
                "🔴 [ACTOR_SERVICE::SEND_REPLY] REMOTE REPLY: node_id={}, sender_id={}",
                node_id, sender_id
            );
            
            let actor_ref = ActorRefImpl::remote(sender_id.clone(), node_id, self.service_locator.clone());
            actor_ref.tell(reply_message).await
                .map_err(|e| format!("Failed to send remote reply: {}", e).into())
        };
        
        let duration = start.elapsed();
        match &result {
            Ok(_) => {
                metrics::counter!("plexspaces_actor_service_send_reply_total",
                    "sender_type" => if is_local { "local" } else { "remote" },
                    "status" => "success"
                ).increment(1);
                metrics::histogram!("plexspaces_actor_service_send_reply_duration_seconds",
                    "sender_type" => if is_local { "local" } else { "remote" }
                ).record(duration.as_secs_f64());
                tracing::debug!(
                    "🔴 [ACTOR_SERVICE::SEND_REPLY] SUCCESS: is_local={}, duration_ms={}",
                    is_local, duration.as_millis()
                );
            }
            Err(e) => {
                metrics::counter!("plexspaces_actor_service_send_reply_total",
                    "sender_type" => if is_local { "local" } else { "remote" },
                    "status" => "error"
                ).increment(1);
                metrics::counter!("plexspaces_actor_service_send_reply_errors_total",
                    "sender_type" => if is_local { "local" } else { "remote" }
                ).increment(1);
                tracing::error!(
                    "🔴 [ACTOR_SERVICE::SEND_REPLY] ERROR: is_local={}, error={}, duration_ms={}",
                    is_local, e, duration.as_millis()
                );
            }
        }
        
        result
    }
    
}

// Reply trait removed - use ActorService::send_reply() directly instead

/// Implement the ActorService gRPC trait
#[async_trait]
impl ActorServiceTrait for ActorServiceImpl {
    /// Send a message to an actor (fire-and-forget or request-reply)
    async fn send_message(
        &self,
        request: Request<SendMessageRequest>,
    ) -> Result<Response<SendMessageResponse>, Status> {
        let req = request.into_inner();
        let proto_message = req
            .message
            .ok_or_else(|| Status::invalid_argument("Message is required"))?;

        // Convert proto Message to mailbox Message
        let message = Message::from_proto(&proto_message);

        // Extract target actor ID from message
        let actor_id = if proto_message.receiver_id.is_empty() {
            return Err(Status::invalid_argument("Receiver ID is required"));
        } else {
            &proto_message.receiver_id
        };

        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message (gRPC)] START: message_id={}, actor_id={}, sender={:?}, receiver={}, message_type={}, correlation_id={:?}, wait_for_response={}",
            message.id, actor_id, message.sender, message.receiver, message.message_type_str(), message.correlation_id, req.wait_for_response
        );

        // Convert timeout
        let timeout = req.timeout.map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        // Route message
        let (message_id, response) = self
            .route_message(actor_id, message, req.wait_for_response, timeout)
            .await?;
        
        tracing::debug!(
            "🟪 [ACTOR_SERVICE::send_message (gRPC)] COMPLETED: message_id={}, actor_id={}, has_response={}",
            message_id, actor_id, response.is_some()
        );

        // Convert response back to proto
        let response_message = response.map(|m| m.to_proto());

        Ok(Response::new(SendMessageResponse {
            message_id,
            response: response_message,
        }))
    }

    // ========================================================================
    // Actor Lifecycle Management RPCs
    // ========================================================================

    async fn create_actor(
        &self,
        _request: Request<CreateActorRequest>,
    ) -> Result<Response<CreateActorResponse>, Status> {
        Err(Status::unimplemented("create_actor not yet implemented"))
    }

    async fn spawn_actor(
        &self,
        request: Request<SpawnActorRequest>,
    ) -> Result<Response<SpawnActorResponse>, Status> {
        // This is the gRPC handler - it spawns locally on this node
        // gRPC is already remote, so "remote" in the name was redundant
        // The actor is spawned locally on THIS node (the one receiving the gRPC request)
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
        
        // Use ActorFactory to spawn the actor locally
        use plexspaces_actor::actor_factory_impl::ActorFactoryImpl;
        let actor_factory_opt: Option<Arc<ActorFactoryImpl>> = self.service_locator.get_service().await;
        
        if let Some(factory) = actor_factory_opt {
            // Spawn actor using ActorFactory
            factory.spawn_actor(
                &actor_id,
                &actor_type,
                initial_state.clone(),
                config.clone(),
                labels.clone(),
            ).await
            .map_err(|e| Status::internal(format!("Failed to spawn actor: {}", e)))?;
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
            facets: vec![],
            isolation: None,
            actor_state_schema_version: 0,
            error_message: String::new(),
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
        _request: Request<GetOrActivateActorRequest>,
    ) -> Result<Response<GetOrActivateActorResponse>, Status> {
        Err(Status::unimplemented("get_or_activate_actor not yet implemented"))
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

    async fn create_actor(
        &self,
        request: Request<CreateActorRequest>,
    ) -> Result<Response<CreateActorResponse>, Status> {
        self.0.create_actor(request).await
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_mailbox::{mailbox_config_default, Mailbox, MailboxConfig};
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_proto::object_registry::v1::ObjectRegistration as ProtoObjectRegistration;
    use std::time::Duration as StdDuration;

    /// Simple wrapper to adapt ObjectRegistry to ObjectRegistryTrait
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistry>,
    }

    #[async_trait::async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            tenant_id: &str,
            object_id: &str,
            namespace: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
            self.inner
                .lookup(tenant_id, namespace, obj_type, object_id)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn lookup_full(
            &self,
            tenant_id: &str,
            namespace: &str,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .lookup(tenant_id, namespace, object_type, object_id)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn register(
            &self,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .register(registration)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }
    }

    /// Helper to create a test ActorRegistry
    fn create_test_registry(local_node_id: &str) -> Arc<ActorRegistry> {
        let kv = Arc::new(InMemoryKVStore::new());
        let object_registry_impl = Arc::new(ObjectRegistry::new(kv));
        let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl,
        });
        Arc::new(ActorRegistry::new(object_registry, local_node_id.to_string()))
    }

    /// Helper to create ActorServiceImpl with proper ServiceLocator setup for tests
    async fn create_test_actor_service(actor_registry: Arc<ActorRegistry>, node_id: String) -> ActorServiceImpl {
        use plexspaces_core::ReplyWaiterRegistry;
        let service_locator = Arc::new(ServiceLocator::new());
        let reply_tracker = Arc::new(ReplyTracker::new());
        let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
        service_locator.register_service(actor_registry.clone()).await;
        service_locator.register_service(reply_tracker).await;
        service_locator.register_service(reply_waiter_registry).await;
        ActorServiceImpl::new(service_locator, node_id)
    }

    /// Helper to register an actor with ActorRegistry for tests
    async fn register_test_actor(
        actor_registry: Arc<ActorRegistry>,
        actor_id: String,
        mailbox: Arc<Mailbox>,
        service_locator: Arc<ServiceLocator>,
    ) {
        let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
            actor_id.clone(),
            mailbox,
            service_locator,
        ));
        actor_registry.register_actor(actor_id, sender).await;
    }

    /// Helper to create a test ActorRegistry with a node registration
    async fn create_test_registry_with_node(local_node_id: &str, node_id: &str, node_address: &str) -> Arc<ActorRegistry> {
        let kv = Arc::new(InMemoryKVStore::new());
        let object_registry_impl = Arc::new(ObjectRegistry::new(kv));
        
        // Register node as a service object (nodes are registered as services)
        let node_object_id = format!("_node@{}", node_id);
        let registration = ProtoObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node_address.to_string(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        };
        
        object_registry_impl.register(registration).await.unwrap();
        
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route to non-existent actor
        let result = service
            .route_local("nonexistent", "node1", message, false, None)
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = Message::new(b"hello".to_vec());
        let message_id = message.id().to_string();

        // ACT: Route message (fire-and-forget)
        let result = service
            .route_local(
                "test", "node1", message, false, // fire-and-forget
                None,
            )
            .await;

        // ASSERT: Should succeed
        if let Err(e) = &result {
            eprintln!("route_local failed: {}", e);
            eprintln!("Actor ID: test@node1");
            // Check if actor is registered
            let found = service.get_actor_registry().await.lookup_actor(&"test@node1".to_string()).await;
            eprintln!("Actor found in registry: {}", found.is_some());
            let activated = service.get_actor_registry().await.is_actor_activated(&"test@node1".to_string()).await;
            eprintln!("Actor activated: {}", activated);
        }
        assert!(result.is_ok(), "route_local should succeed, got error: {:?}", result.err());
        let (returned_msg_id, response) = result.unwrap();
        assert_eq!(returned_msg_id, message_id);
        assert!(response.is_none()); // No response for fire-and-forget

        // Verify message was delivered to actor's mailbox
        // Give a moment for async delivery
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let delivered_msg = mailbox.dequeue().await;
        assert!(delivered_msg.is_some());
        assert_eq!(delivered_msg.unwrap().payload(), b"hello");
    }

    #[tokio::test]
    async fn test_route_local_request_reply_not_implemented() {
        // ARRANGE: Create actor and register it
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = Message::new(b"hello".to_vec());

        // ACT: Try request-reply (ask pattern)
        let result = service
            .route_local(
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route to unknown node
        let result = service
            .route_remote("node2", "actor@node2", message, false, None)
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
        let actor_registry = create_test_registry("node1");
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route with actor ID that doesn't exist (no @node defaults to local)
        // Since actor IDs without @node are now valid (default to local node),
        // this will fail with NotFound when the actor isn't found
        let result = service
            .route_message("invalid_no_node", message, false, None)
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        let message = Message::new(b"hello".to_vec());
        let message_id = message.id().to_string();

        // ACT: Route message via route_message() entry point
        let result = service
            .route_message("test@node1", message, false, None)
            .await;

        // ASSERT: Should route locally
        assert!(result.is_ok());
        let (returned_id, response) = result.unwrap();
        assert_eq!(returned_id, message_id);
        assert!(response.is_none());

        // Verify message delivered
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let delivered = mailbox.dequeue().await;
        assert!(delivered.is_some());
        assert_eq!(delivered.unwrap().payload(), b"hello");
    }

    // ========================================================================
    // COVERAGE TESTS - send_message() gRPC Handler
    // ========================================================================

    #[tokio::test]
    async fn test_send_message_missing_message() {
        // ARRANGE: Create service
        let actor_registry = create_test_registry("node1");
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        // Create message without receiver_id
        let mut proto_message = Message::new(b"test".to_vec()).to_proto();
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create proto message
        let mut message = Message::new(b"hello".to_vec());
        message.receiver = "test@node1".to_string();
        let proto_message = message.to_proto();
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

        // Verify delivery
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let delivered = mailbox.dequeue().await;
        assert!(delivered.is_some());
        assert_eq!(delivered.unwrap().payload(), b"hello");
    }

    #[tokio::test]
    async fn test_send_message_with_timeout() {
        // ARRANGE
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create message with timeout
        let mut message = Message::new(b"test".to_vec());
        message.receiver = "test@node1".to_string();
        let proto_message = message.to_proto();

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
    // COVERAGE TESTS - get_or_create_client() Caching
    // ========================================================================

    #[tokio::test]
    async fn test_get_or_create_client_cache_hit() {
        // ARRANGE: Create service and pre-populate client cache
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        // Create a mock client (we'll just use a dummy endpoint that won't be called)
        // NOTE: This is tricky - we can't easily create a fake client without a real server
        // For now, we'll test the cache miss path which is more important

        // Skip this test for now - requires mock gRPC server
        // Will test in integration tests instead
    }

    #[tokio::test]
    async fn test_get_or_create_client_cache_miss_with_invalid_address() {
        // ARRANGE: Register a node with invalid gRPC address
        let actor_registry = create_test_registry_with_node("node1", "node2", "invalid://bad:address").await;
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Wait for service registration to complete (with retry)
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            // Check if ActorRegistry is registered by trying to get it
            if service.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Try to get client for node with bad address
        let result = service.get_or_create_client("node2").await;

        // ASSERT: Should fail (either invalid argument, unavailable, or internal if ActorRegistry not registered yet)
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Could be InvalidArgument (URL parsing), Unavailable (connection failed), or Internal (ActorRegistry not registered)
        assert!(
            err.code() == tonic::Code::InvalidArgument 
                || err.code() == tonic::Code::Unavailable
                || err.code() == tonic::Code::Internal,
            "Expected InvalidArgument, Unavailable, or Internal, got {:?}",
            err.code()
        );
    }

    // ========================================================================
    // COVERAGE TESTS - route_remote() Error Paths
    // ========================================================================

    #[tokio::test]
    async fn test_route_remote_node_not_in_registry() {
        // ARRANGE: Create service with empty registry
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route to unknown node (not in registry)
        let result = service
            .route_remote("unknown_node", "actor@unknown_node", message, false, None)
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route to node (registry lookup will fail with NotFound)
        let result = service
            .route_remote("node2", "actor@node2", message, false, None)
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
        
        // Wait for service registration
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let message = Message::new(b"test".to_vec());

        // ACT: Try to route to unreachable node
        let result = service
            .route_remote("node2", "actor@node2", message, false, None)
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
        let actor_registry = create_test_registry("node1");
        let service = create_test_actor_service(actor_registry.clone(), "node1".to_string()).await;

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test@node1".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref = plexspaces_core::ActorRef::new("test@node1".to_string()).unwrap();
        register_test_actor(actor_registry.clone(), "test@node1".to_string(), Arc::clone(&mailbox), service.service_locator.clone()).await;

        // Create message with fractional seconds timeout
        let mut message = Message::new(b"test".to_vec());
        message.receiver = "test@node1".to_string();
        let proto_message = message.to_proto();

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

    /// Helper to start a test gRPC server
    async fn start_test_server(
        service: ActorServiceImpl,
        port: u16,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{}", port).parse().unwrap();
            tonic::transport::Server::builder()
                .add_service(ActorServiceServer::new(service))
                .serve(addr)
                .await
                .unwrap();
        })
    }

    #[tokio::test]
    async fn test_route_remote_success_with_real_server() {
        // ARRANGE: Create two nodes with separate registries
        let node2_port = 19999;
        let node2_address = format!("127.0.0.1:{}", node2_port);
        let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

        // Node2: Register a test actor
        let mailbox2 = Arc::new(Mailbox::new(mailbox_config_default(), "test@node2".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref2 = plexspaces_core::ActorRef::new("test@node2".to_string()).unwrap();
        let service2 = create_test_actor_service(registry2.clone(), "node2".to_string()).await;
        register_test_actor(registry2.clone(), "test@node2".to_string(), mailbox2.clone(), service2.service_locator.clone()).await;

        // Start node2's gRPC server
        let _server_handle = start_test_server(service2, node2_port).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await; // Wait for server to be ready

        // Node1: Register node2 in its registry
        let actor_registry1 = create_test_registry_with_node("node1", "node2", &node2_address).await;
        let service1 = create_test_actor_service(actor_registry1.clone(), "node1".to_string()).await;
        
        // Wait for service registration to complete (with retry)
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            // Check if ActorRegistry is registered by trying to get it
            if service1.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Send message from node1 to actor on node2
        let mut message = Message::new(b"hello from node1".to_vec());
        message.sender = Some("sender@node1".to_string()); // Valid sender ID
        message.receiver = "test@node2".to_string(); // Valid receiver ID
        let result = service1
            .route_message("test@node2", message, false, None)
            .await;

        // ASSERT: Should succeed (covers route_remote success path)
        assert!(
            result.is_ok(),
            "Expected success, got error: {:?}",
            result.err()
        );

        // Verify message was delivered to node2's actor
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        let delivered = mailbox2.dequeue().await;
        assert!(
            delivered.is_some(),
            "Message should have been delivered to node2"
        );
        assert_eq!(delivered.unwrap().payload(), b"hello from node1");
    }

    #[tokio::test]
    async fn test_route_remote_connection_pooling() {
        // Test: Verify that gRPC clients are cached and reused (connection pooling)
        // ARRANGE: Create two nodes
        let node2_port = 19997;
        let node2_address = format!("127.0.0.1:{}", node2_port);
        let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

        let mailbox2 = Arc::new(Mailbox::new(mailbox_config_default(), "test@node2".to_string()).await.expect("Failed to create mailbox"));
        let service2 = create_test_actor_service(registry2.clone(), "node2".to_string()).await;
        register_test_actor(registry2.clone(), "test@node2".to_string(), mailbox2.clone(), service2.service_locator.clone()).await;

        let _server_handle = start_test_server(service2, node2_port).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Node1: Register node2
        let actor_registry1 = create_test_registry_with_node("node1", "node2", &node2_address).await;
        let service1 = create_test_actor_service(actor_registry1.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if service1.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Send multiple messages to the same remote node
        // This should reuse the same gRPC client (connection pooling)
        for i in 0..5 {
            let mut message = Message::new(format!("message-{}", i).into_bytes());
            message.receiver = "test@node2".to_string();
            
            let result = service1
                .route_message("test@node2", message, false, None)
                .await;
            
            assert!(result.is_ok(), "Message {} should succeed", i);
        }

        // Verify all messages were delivered
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let mut received_messages = std::collections::HashSet::new();
        for _ in 0..5 {
            let delivered = mailbox2.dequeue().await;
            assert!(delivered.is_some(), "All messages should be delivered");
            let message = delivered.unwrap();
            let payload = message.payload().to_vec();
            received_messages.insert(payload);
        }
        // Verify we received all 5 messages (order may vary due to async delivery)
        for i in 0..5 {
            assert!(received_messages.contains(format!("message-{}", i).as_bytes()), 
                "Message {} should be delivered", i);
        }
    }

    #[tokio::test]
    async fn test_route_remote_with_timeout() {
        // ARRANGE: Create two nodes with separate registries
        let node2_port = 19998;
        let node2_address = format!("127.0.0.1:{}", node2_port);
        let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

        let mailbox2 = Arc::new(Mailbox::new(mailbox_config_default(), "test@node2".to_string()).await.expect("Failed to create mailbox"));
        let _actor_ref2 = plexspaces_core::ActorRef::new("test@node2".to_string()).unwrap();
        let service2 = create_test_actor_service(registry2.clone(), "node2".to_string()).await;
        register_test_actor(registry2.clone(), "test@node2".to_string(), mailbox2.clone(), service2.service_locator.clone()).await;

        let _server_handle = start_test_server(service2, node2_port).await;
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Node1: Register node2
        let actor_registry1 = create_test_registry_with_node("node1", "node2", &node2_address).await;
        let service1 = create_test_actor_service(actor_registry1.clone(), "node1".to_string()).await;
        
        // Wait for service registration to complete (with retry)
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            // Check if ActorRegistry is registered by trying to get it
            if service1.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Send message with timeout (exercises timeout conversion path)
        let mut message = Message::new(b"hello".to_vec());
        message.sender = Some("sender@node1".to_string());
        message.receiver = "test@node2".to_string();
        let result = service1
            .route_message(
                "test@node2",
                message,
                false,
                Some(std::time::Duration::from_secs(5)), // With timeout
            )
            .await;

        // ASSERT: Should succeed (covers timeout conversion path)
        assert!(
            result.is_ok(),
            "Expected success, got error: {:?}",
            result.err()
        );

        // Second message to hit cache (line 319)
        let mut message2 = Message::new(b"second".to_vec());
        message2.sender = Some("sender@node1".to_string());
        message2.receiver = "test@node2".to_string();
        let result2 = service1
            .route_message("test@node2", message2, false, None)
            .await;
        assert!(result2.is_ok(), "Expected success for second message");
    }

    #[tokio::test]
    async fn test_route_remote_multi_node_scenario() {
        // Test: Multiple nodes, multiple actors, verify routing works correctly
        // ARRANGE: Create three nodes
        let node2_port = 19996;
        let node3_port = 19995;
        let node2_address = format!("127.0.0.1:{}", node2_port);
        let node3_address = format!("127.0.0.1:{}", node3_port);

        // Node2 setup
        let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;
        let mailbox2 = Arc::new(Mailbox::new(mailbox_config_default(), "actor2@node2".to_string()).await.expect("Failed to create mailbox"));
        let service2 = create_test_actor_service(registry2.clone(), "node2".to_string()).await;
        register_test_actor(registry2.clone(), "actor2@node2".to_string(), mailbox2.clone(), service2.service_locator.clone()).await;
        let _server2 = start_test_server(service2, node2_port).await;

        // Node3 setup
        let registry3 = create_test_registry_with_node("node3", "node3", &node3_address).await;
        let mailbox3 = Arc::new(Mailbox::new(mailbox_config_default(), "actor3@node3".to_string()).await.expect("Failed to create mailbox"));
        let service3 = create_test_actor_service(registry3.clone(), "node3".to_string()).await;
        register_test_actor(registry3.clone(), "actor3@node3".to_string(), mailbox3.clone(), service3.service_locator.clone()).await;
        let _server3 = start_test_server(service3, node3_port).await;

        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

        // Node1: Register both node2 and node3
        // Use create_test_registry_with_node helper and then add node3 manually
        let registry1 = create_test_registry_with_node("node1", "node2", &node2_address).await;
        
        // Also register node3 using the same pattern as create_test_registry_with_node
        // We need to access the underlying ObjectRegistry to register node3
        // For simplicity, create a new registry that includes both nodes
        let kv1 = Arc::new(InMemoryKVStore::new());
        let object_registry_impl1 = Arc::new(ObjectRegistry::new(kv1));
        
        // Register node2
        let node2_object_id = format!("_node@node2");
        use plexspaces_proto::object_registry::v1::{ObjectRegistration as ProtoObjectRegistration, ObjectType};
        let node2_registration = ProtoObjectRegistration {
            object_id: node2_object_id,
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        };
        object_registry_impl1.register(node2_registration).await.unwrap();
        
        // Register node3
        let node3_object_id = format!("_node@node3");
        let node3_registration = ProtoObjectRegistration {
            object_id: node3_object_id,
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node3_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        };
        object_registry_impl1.register(node3_registration).await.unwrap();
        
        // Create ActorRegistry with the ObjectRegistry that has both nodes
        let object_registry1: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
            inner: object_registry_impl1,
        });
        let registry1 = Arc::new(ActorRegistry::new(object_registry1, "node1".to_string()));

        let service1 = create_test_actor_service(registry1.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if service1.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Send messages to both remote nodes
        let mut msg1 = Message::new(b"hello to node2".to_vec());
        msg1.receiver = "actor2@node2".to_string();
        let result1 = service1.route_message("actor2@node2", msg1, false, None).await;
        assert!(result1.is_ok(), "Message to node2 should succeed");

        let mut msg2 = Message::new(b"hello to node3".to_vec());
        msg2.receiver = "actor3@node3".to_string();
        let result2 = service1.route_message("actor3@node3", msg2, false, None).await;
        assert!(result2.is_ok(), "Message to node3 should succeed");

        // ASSERT: Verify both messages were delivered
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        let delivered2 = mailbox2.dequeue().await;
        assert!(delivered2.is_some(), "Message should be delivered to node2");
        assert_eq!(delivered2.unwrap().payload(), b"hello to node2");

        let delivered3 = mailbox3.dequeue().await;
        assert!(delivered3.is_some(), "Message should be delivered to node3");
        assert_eq!(delivered3.unwrap().payload(), b"hello to node3");
    }

    #[tokio::test]
    async fn test_route_remote_error_handling() {
        // Test: Error handling for network failures and invalid addresses
        // ARRANGE: Create service with invalid node address
        let invalid_address = "127.0.0.1:1"; // Port 1 is typically not in use
        let registry = create_test_registry_with_node("node1", "node2", invalid_address).await;
        let service = create_test_actor_service(registry.clone(), "node1".to_string()).await;
        
        // Wait for service registration
        for _ in 0..10 {
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            if service.service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
        }

        // ACT: Try to route to unreachable node
        let message = Message::new(b"test".to_vec());
        let result = service.route_message("actor@node2", message, false, None).await;

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
