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

//! Enhanced ActorContext with service access
//!
//! ## Purpose
//! Provides actors with access to all system services they need:
//! - ActorService: Spawn and communicate with actors (local and remote)
//! - ObjectRegistry: Service discovery
//! - TupleSpaceProvider: Coordination
//! - Node: Node-level operations
//!
//! ## Design (Option C: Actor as Container)
//! Actors receive this context in all their methods, giving them full access
//! to the system without needing to pass services around manually.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

use crate::{ActorId, ActorRef, RequestContext, ServiceLocator};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleSpaceError};
use futures::stream::BoxStream;

// Re-export proto ObjectRegistration type as type alias
pub type ObjectRegistration = plexspaces_proto::object_registry::v1::ObjectRegistration;

/// Trait for channel operations (queue and topic patterns)
///
/// ## Purpose
/// Provides unified interface for channel operations (queue and topic patterns).
/// This is a Rust trait (not in proto), following proto-first principle.
///
/// ## Proto-First Principle
/// - Proto defines: ChannelConfig, ChannelMessage, ChannelBackend (structs/enums)
/// - Rust defines: ChannelService trait (implementation detail, flexible)
///
/// ## Design
/// Channels provide two main patterns:
/// - **Queue**: Load-balanced to one consumer (work distribution)
/// - **Topic**: All subscribers receive (pub/sub)
#[async_trait]
pub trait ChannelService: Send + Sync {
    /// Send message to queue (load-balanced to one consumer)
    ///
    /// ## Arguments
    /// * `queue_name` - Name of the queue
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Message ID if successful
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    /// Publish message to topic (all subscribers receive)
    ///
    /// ## Arguments
    /// * `topic_name` - Name of the topic
    /// * `message` - Message to publish
    ///
    /// ## Returns
    /// Message ID if successful
    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    /// Subscribe to topic (returns stream of messages)
    ///
    /// ## Arguments
    /// * `topic_name` - Name of the topic
    ///
    /// ## Returns
    /// Stream of messages that can be consumed asynchronously
    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>>;

    /// Receive from queue (blocking until message available)
    ///
    /// ## Arguments
    /// * `queue_name` - Name of the queue
    /// * `timeout` - Optional timeout for receiving
    ///
    /// ## Returns
    /// Optional message (None if timeout or queue empty)
    async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Trait for actor service operations (spawning, messaging)
///
/// ## Purpose
/// Provides unified interface for actor operations, whether local or remote.
/// Supports `actor@node1` syntax for location-transparent operations.
#[async_trait]
pub trait ActorService: Send + Sync {
    /// Spawn a new actor (local or remote)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID in format "actor_name@node_id" (or just "actor_name" for local)
    /// * `actor_type` - Type of actor to spawn
    /// * `initial_state` - Initial state bytes
    ///
    /// ## Returns
    /// ActorRef for the spawned actor
    async fn spawn_actor(
        &self,
        actor_id: &str,
        actor_type: &str,
        initial_state: Vec<u8>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message to an actor (local or remote)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID in format "actor_name@node_id"
    /// * `message` - Message to send
    ///
    /// ## Returns
    /// Message ID if successful
    async fn send(
        &self,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

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
    ///
    /// ## Example
    /// ```rust,ignore
    /// // In actor's handle_message or handle_request:
    /// if let Some(sender_id) = &msg.sender {
    ///     let reply = Message::new(b"response".to_vec());
    ///     actor_service.send_reply(
    ///         msg.correlation_id.as_deref(),
    ///         sender_id,
    ///         msg.receiver.clone(),
    ///         reply,
    ///     ).await?;
    /// }
    /// ```
    async fn send_reply(
        &self,
        correlation_id: Option<&str>,
        sender_id: &ActorId,
        target_actor_id: ActorId,
        reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
}

/// Trait for facet service operations (accessing facets from actors)
///
/// ## Purpose
/// Provides unified interface for accessing facets attached to actors.
/// Enables explicit facet access from ActorContext (Option B).
///
/// ## Design
/// - Explicit facet access (A3: explicit)
/// - Type-safe via generics
/// - Works with local and remote actors (future)
#[async_trait]
pub trait FacetService: Send + Sync {
    /// Get a facet from an actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `facet_type` - Facet type identifier (e.g., "timer", "reminder")
    ///
    /// ## Returns
    /// Arc to the facet (if found and type matches) or error
    ///
    /// ## Note
    /// This is a simplified version that returns Arc<dyn Facet>.
    /// For type-safe access, use `get_facet_typed` (requires downcasting).
    async fn get_facet(
        &self,
        actor_id: &ActorId,
        facet_type: &str,
    ) -> Result<std::sync::Arc<tokio::sync::RwLock<Box<dyn plexspaces_facet::Facet>>>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Trait for object registry (service discovery)
///
/// ## Purpose
/// Provides unified registration and discovery for actors, tuplespaces, and services.
///
/// ## Design Decision (Option A)
/// Uses actor's context (namespace, tenant) automatically for simplicity.
/// Advanced cases can use `lookup_full()` with explicit parameters.
#[async_trait]
pub trait ObjectRegistry: Send + Sync {
    /// Lookup an object by ID (uses actor's context automatically)
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (tenant_id and namespace come from here)
    /// * `object_id` - Object ID to lookup
    /// * `object_type` - Type of object (defaults to Actor if None)
    ///
    /// ## Returns
    /// Object registration with gRPC address and metadata
    ///
    /// ## Design
    /// Uses RequestContext for tenant and namespace. For advanced cases, use `lookup_full()`.
    async fn lookup(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;

    /// Lookup an object with full parameters (advanced)
    ///
    /// ## Arguments
    /// * `tenant_id` - Tenant identifier
    /// * `namespace` - Namespace
    /// * `object_type` - Type of object
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `object_type` - Object type
    /// * `object_id` - Object ID
    ///
    /// ## Returns
    /// Object registration if found
    ///
    /// ## Note
    /// If ctx.is_admin() is true, tenant filtering is bypassed for admin operations.
    async fn lookup_full(
        &self,
        ctx: &RequestContext,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
        object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;

    /// Register an object
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (tenant_id comes from here)
    /// * `registration` - Object registration details (tenant_id/namespace must match ctx if provided)
    async fn register(
        &self,
        ctx: &RequestContext,
        registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Discover objects by type and filters
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation
    /// * `object_type` - Optional object type filter
    /// * `object_category` - Optional category filter (e.g., application name, workflow definition ID)
    /// * `capabilities` - Optional capabilities filter
    /// * `labels` - Optional labels filter
    /// * `health_status` - Optional health status filter
    /// * `limit` - Maximum number of results to return
    ///
    /// ## Returns
    /// Vector of ObjectRegistration matching the filters
    ///
    /// ## Note
    /// If ctx.is_admin() is true, tenant filtering is bypassed for admin operations.
    async fn discover(
        &self,
        ctx: &RequestContext,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        object_category: Option<String>,
        capabilities: Option<Vec<String>>,
        labels: Option<Vec<String>>,
        health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>>;
}

// ObjectRegistration is re-exported from proto (see use statement above)
// Convenience methods are provided via extension trait below

/// Trait for TupleSpace provider
///
/// ## Purpose
/// Provides access to TupleSpace for coordination.
/// This is a simplified interface - full TupleSpaceProvider trait is in tuplespace crate.
#[async_trait]
pub trait TupleSpaceProvider: Send + Sync {
    /// Write a tuple to the space
    async fn write(&self, tuple: Tuple) -> Result<(), TupleSpaceError>;

    /// Read tuples matching pattern
    async fn read(&self, pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError>;

    /// Take a tuple matching pattern (blocking)
    async fn take(&self, pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError>;

    /// Count tuples matching pattern
    async fn count(&self, pattern: &Pattern) -> Result<usize, TupleSpaceError>;
}


/// Trait for process group operations (pub/sub for actors)
///
/// ## Purpose
/// Provides unified interface for process group operations (Erlang pg/pg2-inspired).
/// This is a Rust trait (not in proto), following proto-first principle.
///
/// ## Proto-First Principle
/// - Proto defines: ProcessGroup, GroupMembership (structs/enums)
/// - Rust defines: ProcessGroupService trait (implementation detail, flexible)
///
/// ## Design
/// Process groups provide actor-level pub/sub for coordination:
/// - **Join/Leave**: Actors subscribe to groups
/// - **Publish**: Broadcast messages to all group members
/// - **Get Members**: Query group membership
#[async_trait]
pub trait ProcessGroupService: Send + Sync {
    /// Join a process group
    ///
    /// ## Arguments
    /// * `group_name` - Name of the group
    /// * `tenant_id` - Tenant for multi-tenancy
    /// * `namespace` - Namespace within tenant
    /// * `actor_id` - Actor ID to add to group
    ///
    /// ## Returns
    /// Ok(()) on success
    async fn join_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Leave a process group
    ///
    /// ## Arguments
    /// * `group_name` - Name of the group
    /// * `tenant_id` - Tenant for multi-tenancy
    /// * `namespace` - Namespace within tenant
    /// * `actor_id` - Actor ID to remove from group
    ///
    /// ## Returns
    /// Ok(()) on success
    async fn leave_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Publish message to all group members
    ///
    /// ## Arguments
    /// * `group_name` - Name of the group
    /// * `tenant_id` - Tenant for multi-tenancy
    /// * `namespace` - Namespace within tenant
    /// * `message` - Message to broadcast
    ///
    /// ## Returns
    /// List of actor IDs that received the message
    async fn publish_to_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
        message: Message,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;

    /// Get all group members
    ///
    /// ## Arguments
    /// * `group_name` - Name of the group
    /// * `tenant_id` - Tenant for multi-tenancy
    /// * `namespace` - Namespace within tenant
    ///
    /// ## Returns
    /// List of actor IDs in the group
    async fn get_members(
        &self,
        group_name: &str,
        tenant_id: &str,
        namespace: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Enhanced ActorContext with service locator access
///
/// ## Purpose
/// Provides actors with access to system services via ServiceLocator.
/// This is the unified context passed to all actor methods.
///
/// ## Design (Akka-Inspired)
/// Following Akka's pattern where ActorContext provides access to ActorSystem (service locator):
/// - Proto defines structure (Actor message)
/// - Rust defines behavior (ActorBehavior trait - hidden from users)
/// - Context provides ServiceLocator for on-demand service access
/// - Users create Actor directly via ActorBuilder
///
/// ## Example
/// ```rust,ignore
/// impl ActorBehavior for MyActor {
///     async fn handle_message(&mut self, ctx: &ActorContext) -> Result<(), BehaviorError> {
///         // Get services via ServiceLocator (on-demand)
///         let actor_service: Arc<dyn ActorService> = ctx.service_locator
///             .get_service()
///             .await
///             .ok_or("ActorService not registered")?;
///         
///         // Spawn remote actor
///         let remote = actor_service
///             .spawn_actor("worker@node2", "Worker", vec![])
///             .await?;
///
///         // Get tuplespace
///         let tuplespace: Arc<dyn TupleSpaceProvider> = ctx.service_locator
///             .get_service()
///             .await
///             .ok_or("TupleSpaceProvider not registered")?;
///         tuplespace.write(tuple).await?;
///
///         Ok(())
///     }
/// }
/// ```
/// Actor context - static, reusable context for actors
///
/// - **Static**: No transient fields (actor_id, sender_id, correlation_id moved out)
/// - **Reusable**: Can be reused across multiple message processing calls
/// - **Message-specific data**: Now in Envelope (sender_id, correlation_id, target_id/actor_id)
/// - **Actor ID**: Removed - actors should get their ID from Envelope.target_id or Actor.id field
#[derive(Clone)]
pub struct ActorContext {
    /// Reference to the node for distribution (static, set once)
    pub node_id: String,
    /// Tenant ID for multi-tenancy (static, set once)
    /// Empty string if auth is disabled
    pub tenant_id: String,
    /// Namespace for isolation (static, set once)
    pub namespace: String,
    /// Metadata (static)
    pub metadata: HashMap<String, String>,
    /// Actor configuration (static)
    pub config: Option<plexspaces_proto::v1::actor::ActorConfig>,

    /// Service locator for accessing system services (Akka-style)
    /// Actors can get services on-demand via service_locator.get_service::<T>().await
    pub service_locator: Arc<ServiceLocator>,

    /// Trap exit flag (Erlang process_flag(trap_exit, true))
    ///
    /// ## Purpose
    /// When true, EXIT signals from linked actors are delivered as messages
    /// to handle_exit() instead of causing this actor to die.
    ///
    /// ## Default
    /// false - linked actor death causes this actor to die (Erlang default)
    pub trap_exit: bool,

    /// Self reference (set after actor is spawned)
    ///
    /// ## Purpose
    /// Provides actors with a reference to themselves for:
    /// - Sending messages to self
    /// - Linking/monitoring other actors
    /// - Getting actor ID
    pub self_ref: Option<ActorRef>,

    /// Parent reference (set by supervisor)
    ///
    /// ## Purpose
    /// Reference to the supervisor that manages this actor.
    /// Used for:
    /// - Understanding supervision hierarchy
    /// - Reporting to parent supervisor
    pub parent_ref: Option<ActorRef>,
}

impl ActorContext {
    /// Create a new ActorContext with ServiceLocator
    ///
    /// ## Purpose
    /// Creates a context with ServiceLocator for on-demand service access.
    /// Services should be registered in ServiceLocator before creating actors.
    ///
    /// ## Arguments
    /// * `node_id` - Node identifier
    /// * `namespace` - Namespace for isolation
    /// * `service_locator` - Service locator for accessing system services
    /// * `config` - Optional actor configuration
    ///
    /// ## Note
    /// Actor ID is no longer stored in context. Actors should get their ID from:
    /// - `Envelope.target_id` when handling messages
    /// - `Actor.id` field for operations outside message handling
    pub fn new(
        node_id: String,
        tenant_id: String,
        namespace: String,
        service_locator: Arc<ServiceLocator>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
    ) -> Self {
        Self {
            node_id,
            tenant_id,
            namespace,
            metadata: HashMap::new(),
            config,
            service_locator,
            trap_exit: false, // Default: linked actor death causes this actor to die
            self_ref: None,   // Set after actor is spawned
            parent_ref: None, // Set by supervisor when starting child
        }
    }

    /// Set trap_exit flag
    ///
    /// ## Erlang Equivalent
    /// process_flag(trap_exit, true)
    ///
    /// ## Effect
    /// When true, EXIT signals from linked actors are delivered as
    /// messages to handle_exit() instead of causing this actor to die.
    pub fn set_trap_exit(&mut self, trap: bool) {
        self.trap_exit = trap;
    }

    /// Check if trapping exits
    pub fn is_trapping_exits(&self) -> bool {
        self.trap_exit
    }

    /// Get self ActorRef
    pub fn self_ref(&self) -> Option<&ActorRef> {
        self.self_ref.as_ref()
    }

    /// Get parent ActorRef (supervisor)
    pub fn parent_ref(&self) -> Option<&ActorRef> {
        self.parent_ref.as_ref()
    }

    /// Create a minimal ActorContext (for testing/backward compatibility)
    ///
    /// ## Note
    /// This creates a context with an empty ServiceLocator. Use `new()` for production.
    ///
    /// ## Deprecated
    /// This is for backward compatibility. New code should use `new()` with real ServiceLocator.
    ///
    /// ## Note on actor_id
    /// Actor ID parameter is deprecated and ignored. Actors should get their ID from
    /// `Envelope.target_id` or `Actor.id` field.

    /// Send a reply message to the sender of the original message
    ///
    /// ## Purpose
    /// Convenience method for sending replies from actors. This is a simplified wrapper
    /// around `ActorRef::send_reply()` that uses the context's service_locator and
    /// the target actor ID (the actor sending the reply).
    ///
    /// ## Arguments
    /// * `correlation_id` - Correlation ID from the original message (optional)
    /// * `sender_id` - ID of the actor that sent the original message (or temporary sender ID)
    /// * `target_actor_id` - ID of the actor sending the reply (usually `msg.receiver`)
    /// * `reply_message` - The reply message to send
    ///
    /// ## Returns
    /// Ok(()) if reply was sent successfully
    ///
    /// ## Example
    /// ```rust,ignore
    /// // In actor's handle_message or handle_request:
    /// if let Some(sender_id) = &msg.sender {
    ///     let reply = Message::new(b"response".to_vec());
    ///     ctx.send_reply(
    ///         msg.correlation_id.as_deref(),
    ///         sender_id,
    ///         msg.receiver.clone(),
    ///         reply,
    ///     ).await?;
    /// }
    /// ```
    pub async fn send_reply(
        &self,
        correlation_id: Option<&str>,
        sender_id: &ActorId,
        target_actor_id: ActorId,
        reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Use unified ActorService::send_reply() which handles all routing logic
        let actor_service = self.get_actor_service().await
            .ok_or_else(|| "ActorService not available in ServiceLocator".to_string())?;
        
        actor_service.send_reply(correlation_id, sender_id, target_actor_id, reply_message).await
    }


    /// Get ActorService from ServiceLocator
    ///
    /// ## Returns
    /// `Some(Arc<dyn ActorService>)` if registered, `None` otherwise
    ///
    /// ## Implementation
    /// Uses ServiceLocator's trait-based storage to retrieve ActorService.
    /// Node registers ActorServiceImpl both as concrete type and as trait object,
    /// allowing this method to work without circular dependencies.
    pub async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>> {
        // ServiceLocator stores it as Arc<dyn ActorService + Send + Sync>
        // Since ActorService already has Send + Sync bounds, this is equivalent
        self.service_locator.get_actor_service().await
    }

    /// Get ChannelService from ServiceLocator
    ///
    /// ## Note
    /// See `get_actor_service()` for limitations and workarounds.
    pub async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        None
    }

    /// Get ObjectRegistry from ServiceLocator
    ///
    /// ## Note
    /// See `get_actor_service()` for limitations and workarounds.
    pub async fn get_object_registry(&self) -> Option<Arc<dyn ObjectRegistry>> {
        None
    }

    /// Get TupleSpaceProvider from ServiceLocator
    ///
    /// ## Note
    /// See `get_actor_service()` for limitations and workarounds.
    pub async fn get_tuplespace(&self) -> Option<Arc<dyn TupleSpaceProvider>> {
        // Get TupleSpaceProvider from ServiceLocator (registered as trait object)
        self.service_locator.get_tuplespace_provider().await
    }

    /// Get ProcessGroupService from ServiceLocator
    ///
    /// ## Note
    /// See `get_actor_service()` for limitations and workarounds.
    pub async fn get_process_group_service(&self) -> Option<Arc<dyn ProcessGroupService>> {
        None
    }


    /// Get FacetService from ServiceLocator
    ///
    /// ## Note
    /// See `get_actor_service()` for limitations and workarounds.
    pub async fn get_facet_service(&self) -> Option<Arc<dyn FacetService>> {
        None
    }


}

// Stub implementations for testing/backward compatibility
struct StubChannelService;

#[async_trait]
impl ChannelService for StubChannelService {
    async fn send_to_queue(
        &self,
        _queue_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubChannelService: send_to_queue not implemented".into())
    }

    async fn publish_to_topic(
        &self,
        _topic_name: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubChannelService: publish_to_topic not implemented".into())
    }

    async fn subscribe_to_topic(
        &self,
        _topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
        use futures::stream;
        Ok(Box::pin(stream::empty()))
    }

    async fn receive_from_queue(
        &self,
        _queue_name: &str,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubChannelService: receive_from_queue not implemented".into())
    }
}

struct StubActorService;

#[async_trait]
impl ActorService for StubActorService {
    async fn spawn_actor(
        &self,
        _actor_id: &str,
        _actor_type: &str,
        _initial_state: Vec<u8>,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubActorService: spawn_actor not implemented".into())
    }

    async fn send(
        &self,
        _actor_id: &str,
        _message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubActorService: send not implemented".into())
    }

    async fn send_reply(
        &self,
        _correlation_id: Option<&str>,
        _sender_id: &ActorId,
        _target_actor_id: ActorId,
        _reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("StubActorService: send_reply not implemented".into())
    }

}

struct StubObjectRegistry;

#[async_trait]
impl ObjectRegistry for StubObjectRegistry {
    async fn lookup(
        &self,
        _ctx: &RequestContext,
        _object_id: &str,
        _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubObjectRegistry: lookup not implemented".into())
    }

    async fn lookup_full(
        &self,
        _ctx: &RequestContext,
        _object_type: plexspaces_proto::object_registry::v1::ObjectType,
        _object_id: &str,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubObjectRegistry: lookup_full not implemented".into())
    }

    async fn register(
        &self,
        _ctx: &RequestContext,
        _registration: ObjectRegistration,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("StubObjectRegistry: register not implemented".into())
    }

    async fn discover(
        &self,
        _ctx: &RequestContext,
        _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        _object_category: Option<String>,
        _capabilities: Option<Vec<String>>,
        _labels: Option<Vec<String>>,
        _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
        _offset: usize,
        _limit: usize,
    ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubObjectRegistry: discover not implemented".into())
    }
}

struct StubTupleSpaceProvider;

#[async_trait]
impl TupleSpaceProvider for StubTupleSpaceProvider {
    async fn write(&self, _tuple: Tuple) -> Result<(), TupleSpaceError> {
        Err(TupleSpaceError::BackendError("StubTupleSpaceProvider: write not implemented".to_string()))
    }

    async fn read(&self, _pattern: &Pattern) -> Result<Vec<Tuple>, TupleSpaceError> {
        Err(TupleSpaceError::BackendError("StubTupleSpaceProvider: read not implemented".to_string()))
    }

    async fn take(&self, _pattern: &Pattern) -> Result<Option<Tuple>, TupleSpaceError> {
        Err(TupleSpaceError::BackendError("StubTupleSpaceProvider: take not implemented".to_string()))
    }

    async fn count(&self, _pattern: &Pattern) -> Result<usize, TupleSpaceError> {
        Err(TupleSpaceError::BackendError("StubTupleSpaceProvider: count not implemented".to_string()))
    }
}

struct StubProcessGroupService;

#[async_trait]
impl ProcessGroupService for StubProcessGroupService {
    async fn join_group(
        &self,
        _group_name: &str,
        _tenant_id: &str,
        _namespace: &str,
        _actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("StubProcessGroupService: join_group not implemented".into())
    }

    async fn leave_group(
        &self,
        _group_name: &str,
        _tenant_id: &str,
        _namespace: &str,
        _actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("StubProcessGroupService: leave_group not implemented".into())
    }

    async fn publish_to_group(
        &self,
        _group_name: &str,
        _tenant_id: &str,
        _namespace: &str,
        _message: Message,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubProcessGroupService: publish_to_group not implemented".into())
    }

    async fn get_members(
        &self,
        _group_name: &str,
        _tenant_id: &str,
        _namespace: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubProcessGroupService: get_members not implemented".into())
    }
}


struct StubFacetService;

#[async_trait]
impl FacetService for StubFacetService {
    async fn get_facet(
        &self,
        _actor_id: &ActorId,
        _facet_type: &str,
    ) -> Result<std::sync::Arc<tokio::sync::RwLock<Box<dyn plexspaces_facet::Facet>>>, Box<dyn std::error::Error + Send + Sync>> {
        Err("StubFacetService: get_facet not implemented".into())
    }
}

