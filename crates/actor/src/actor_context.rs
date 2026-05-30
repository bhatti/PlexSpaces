// SPDX-License-Identifier: AGPL-3.0-or-later
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

use crate::{ActorId, RequestContext, RequestContextExt, ServiceLocator};
// self_ref and parent_ref use the lightweight service-traits ActorRef (no mailbox required).
use plexspaces_proto::common::v1::Message;
use plexspaces_service_traits::ActorRef;

// ObjectRegistry, TupleSpaceProvider, ObjectRegistration live in service-traits.
pub use plexspaces_service_traits::object_registry::{
    HealthStatus as ObjectRegistryHealthStatus, ObjectRegistration, ObjectRegistry, RegisterResult,
};
pub use plexspaces_service_traits::tuplespace_provider::TupleSpaceProvider;

// ChannelService is defined in plexspaces-service-traits to break the
// actor → journaling → mailbox → channel → actor cycle.
pub use plexspaces_service_traits::ChannelService;

/// Trait for process group operations (Erlang pg/pg2-style pub/sub)
///
/// ## Purpose
/// Provides unified interface for distributed pub/sub and broadcast messaging.
/// This is a Rust trait (not in proto), following proto-first principle.
///
/// ## Proto-First Principle
/// - Proto defines: ProcessGroup, GroupMembership, PublishToGroupRequest (structs)
/// - Rust defines: ProcessGroupService trait (implementation detail, flexible)
///
/// ## Design (Industry Standard - Erlang pg/pg2)
/// Process groups provide distributed pub/sub with:
/// - **Named Groups**: Actors join named groups for coordination
/// - **Topic Filtering**: Actors subscribe to specific topics within groups
/// - **Multiple Joins**: Actors can join same group multiple times (join_count tracked)
/// - **Local vs Global**: Fast local member queries, distributed global queries
///
/// ## State Management
/// Uses ObjectRegistry (OBJECT_TYPE_PROCESS_GROUP) for distributed state:
/// - Each node maintains local membership view
/// - ObjectRegistry provides distributed coordination (shared DB, etc.)
/// - Local operations (get_local_members) are fast (no network)
/// - Global operations may require coordination
// ProcessGroupService is defined in plexspaces-service-traits to break the
// actor → journaling → mailbox → channel → actor cycle.
pub use plexspaces_service_traits::ProcessGroupService;

// ActorService is re-exported from plexspaces-service-traits.
// The canonical definition lives there so that plexspaces-journaling can depend
// on it without pulling in the full plexspaces-core crate.
pub use plexspaces_service_traits::ActorService;

/// Trait for providing link semantics (bidirectional death propagation)
///
/// ## Purpose
/// Allows components to link actors for cascading failure handling.
/// ActorRegistry implements this trait to provide link/unlink functionality for local actors.
///
/// ## Erlang Philosophy
/// Supervision uses links internally (Erlang/OTP pattern). When a supervisor
/// adds a child, it links to the child so cascading failures work correctly.
///
/// ## Design
/// - [`ActorRegistry`] routes `link`/`unlink` locally or via [`ActorService`] like messaging.
/// - Follows Erlang/OTP link semantics (bidirectional death propagation)
#[async_trait]
pub trait LinkProvider: Send + Sync {
    /// Link two actors (bidirectional death propagation)
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant/namespace isolation
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link
    ///
    /// ## Returns
    /// Success or error
    async fn link(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), String>;

    /// Unlink two actors
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant/namespace isolation
    /// * `actor_id` - First actor in the link
    /// * `linked_actor_id` - Second actor in the link
    ///
    /// ## Returns
    /// Success or error
    async fn unlink(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        linked_actor_id: &ActorId,
    ) -> Result<(), String>;
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
    ) -> Result<
        std::sync::Arc<tokio::sync::RwLock<Box<dyn plexspaces_facet::Facet>>>,
        Box<dyn std::error::Error + Send + Sync>,
    >;
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
///             .actor_registry()
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
///             .actor_registry()
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
    pub service_locator: Arc<dyn ServiceLocator>,

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
        service_locator: Arc<dyn ServiceLocator>,
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

    /// Attach the actor's own reference to the context before runtime startup.
    ///
    /// The runtime must set this before `init()`, facet lifecycle hooks, or message
    /// handling run so `ctx.actor_id()` and `ctx.self_ref()` are always available.
    pub fn with_self_ref(mut self, self_ref: ActorRef) -> Self {
        self.self_ref = Some(self_ref);
        self
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

    /// Get this actor's canonical typed ID.
    ///
    /// ## Purpose
    /// Keeps runtime code and generated SDK handlers on structured `ActorId`
    /// instead of reparsing `receiver_id` strings from individual messages.
    ///
    /// ## Panics
    /// Panics if called before the actor's self reference has been set during spawn.
    /// That indicates a runtime initialization bug.
    pub fn actor_id(&self) -> &ActorId {
        self.self_ref
            .as_ref()
            .expect("ActorContext self_ref must be set before message handling")
            .id()
    }

    /// Get parent ActorRef (supervisor)
    pub fn parent_ref(&self) -> Option<&ActorRef> {
        self.parent_ref.as_ref()
    }

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
    /// * `target_actor_id` - Typed ID of the actor sending the reply (usually `ctx.actor_id().clone()`)
    /// * `reply_message` - The reply message to send
    ///
    /// ## Returns
    /// Ok(()) if reply was sent successfully
    ///
    /// ## Example
    /// ```rust,ignore
    /// // In actor's handle_message or handle_request:
    /// if !msg.sender_id.is_empty() {
    ///     let reply = Message { payload: b"response".to_vec(), ..Default::default() };
    ///     ctx.send_reply(
    ///         Some(&msg.correlation_id),
    ///         &msg.sender_id,
    ///         ctx.actor_id().clone(),
    ///         reply,
    ///     ).await?;
    /// }
    /// ```
    pub async fn send_reply(
        &self,
        correlation_id: Option<&str>,
        sender_id: &str,
        target_actor_id: ActorId,
        mut reply_message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let target_actor_id_clone = target_actor_id.clone();
        let reply_message_id = reply_message.id.clone();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("[ACTOR_CONTEXT::send_reply] START: sender_id={}, target_actor_id={}, correlation_id={:?}, reply_message_id={}", 
                sender_id, target_actor_id_clone, correlation_id, reply_message_id);
        }

        // SIMPLIFIED: Use send() method - temporary sender behaves like normal actor
        // Set message fields: receiver_id=sender_id (where reply goes TO), sender_id=target_actor_id (where reply comes FROM), correlation_id
        reply_message.receiver_id = sender_id.to_string(); // Reply goes TO the sender (temporary sender for ask pattern)
        reply_message.sender_id = target_actor_id.to_string(); // Reply comes FROM the current actor
        if let Some(corr_id) = correlation_id {
            reply_message.correlation_id = corr_id.to_string();
        }

        // Ensure reply message has an ID with "res-" prefix for tracking
        if reply_message.id.is_empty() {
            use ulid::Ulid;
            reply_message.id = format!("res-{}", Ulid::new().to_string());
        } else if !reply_message.id.starts_with("res-") && !reply_message.id.starts_with("req-") {
            // If ID exists but doesn't have prefix, add res- prefix for replies
            reply_message.id = format!("res-{}", reply_message.id);
        }

        // Use send() method - it will route to temporary sender just like any other actor
        // ActorRef::tell() will detect temporary sender and route to ReplyWaiter automatically
        let actor_service = self
            .get_actor_service()
            .await
            .ok_or_else(|| "ActorService not available in ServiceLocator".to_string())?;

        let ctx = RequestContext::new_without_auth(self.tenant_id.clone(), self.namespace.clone());
        let result = actor_service
            .send(&ctx, sender_id, reply_message)
            .await
            .map(|_| ()); // Ignore message_id return value

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                "[ACTOR_CONTEXT::send_reply] END: sender_id={}, target_actor_id={}, result={:?}",
                sender_id,
                target_actor_id_clone,
                result.is_ok()
            );
        }
        result
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
    /// ## Returns
    /// `Some(Arc<dyn ChannelService>)` if registered, `None` otherwise
    ///
    /// ## Note
    /// Uses ServiceLocator's generic service lookup. The service must be registered
    /// via `service_locator.register_service(channel_service).await`.
    ///
    /// ## Implementation
    /// Since ChannelService is a trait, we need to look it up by type name.
    /// The service must be registered as a concrete type that implements ChannelService.
    pub async fn get_channel_service(&self) -> Option<Arc<dyn ChannelService>> {
        self.service_locator.get_channel_service().await
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
