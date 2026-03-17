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

//! Actor Factory trait - for spawning and activating actors
//!
//! ## Purpose
//! Provides a trait for spawning actors without depending on Node directly.
//! This allows VirtualActorManager, ActorService, and other components to spawn actors
//! without tight coupling to Node.
//!
//! ## Design
//! - Trait defined in core crate (avoids circular dependencies)
//! - Returns MessageSender (ActorRef implements MessageSender, avoiding circular dependency)
//! - Implementation (ActorFactoryImpl) lives in actor crate
//! - ServiceLocator stores `Arc<dyn ActorFactory>` directly
//!
//! ## Note on spawn_built_actor
//! The `spawn_built_actor` method is NOT part of this trait because it requires
//! the concrete Actor type. Instead, it's available as a
//! method on `ActorFactoryImpl` directly. Use `get_actor_factory_impl()` helper
//! if you need to call `spawn_built_actor`.
//!
//! ## Note on Return Type
//! Returns `Arc<dyn MessageSender>` which `ActorRef` implements. This allows the trait
//! to stay in core crate while implementations can return `ActorRef` wrapped as `Arc<dyn MessageSender>`.

use crate::{ActorId, MessageSender, RequestContext};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for spawning and activating actors
///
/// ## Purpose
/// Allows components like VirtualActorManager and ActorService to spawn actors without
/// depending on Node directly. ActorFactory implementations should use ServiceLocator
/// to access ActorRegistry and other services needed for spawning.
///
/// ## Implementation
/// The main implementation is `ActorFactoryImpl` in the actor crate.
///
/// ## Note
/// This trait does NOT include `spawn_built_actor` because that method requires
/// the concrete Actor type. Use `ActorFactoryImpl` directly for that method.
#[async_trait]
pub trait ActorFactory: Send + Sync {
    /// Activate a virtual actor (start it if not already started)
    ///
    /// ## Arguments
    /// * `actor_id` - The actor ID to activate
    ///
    /// ## Returns
    /// Ok(()) if activation successful, error otherwise
    async fn activate_virtual_actor(
        &self,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Spawn a new actor locally
    ///
    /// ## Purpose
    /// Creates and starts a new actor on the local node. The actor will be registered
    /// in ActorRegistry automatically.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (first parameter)
    /// * `actor_id` - Actor ID (format: "actor_name@node_id")
    /// * `actor_type` - Type of actor to spawn (used by BehaviorFactory if available)
    /// * `initial_state` - Initial state bytes (passed to BehaviorFactory if available)
    /// * `config` - Optional actor configuration
    /// * `labels` - Optional labels for the actor
    /// * `facets` - Optional facets to attach to the actor
    ///
    /// ## Returns
    /// `Arc<dyn MessageSender>` for the spawned actor. `ActorRef` implements `MessageSender`,
    /// so implementations can return `ActorRef` wrapped as `Arc<dyn MessageSender>`.
    ///
    /// ## Note
    /// ActorFactory implementations should use ActorRegistry to register the actor
    /// after spawning. Returns `Arc<dyn MessageSender>` which `ActorRef` implements.
    async fn spawn_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
        actor_type: &str,
        initial_state: Vec<u8>,
        config: Option<plexspaces_proto::v1::actor::ActorConfig>,
        labels: HashMap<String, String>,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>>;

    /// Stop an actor
    ///
    /// ## Purpose
    /// Stops and unregisters an actor from the ActorRegistry.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext for tenant isolation (validates caller has permission)
    /// * `actor_id` - Actor ID to stop
    ///
    /// ## Returns
    /// Ok(()) on success, error otherwise
    ///
    /// ## Tenant Isolation
    /// The caller's tenant_id and namespace from `ctx` must match the actor's stored
    /// tenant_id and namespace. This prevents cross-tenant access.
    ///
    /// ## Note
    /// This method unregisters the actor from ActorRegistry and performs cleanup.
    /// The actor will be garbage collected after unregistration.
    async fn stop_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Create a temporary sender for ask() pattern
    ///
    /// ## Purpose
    /// Creates and registers a temporary sender ActorRef used for request-reply (ask) pattern.
    /// The temporary sender receives replies and routes them to the ReplyWaiter by correlation_id.
    ///
    /// ## Arguments
    /// * `ctx` - RequestContext with proper tenant/namespace
    /// * `temp_sender_id` - Temporary sender ID (format: "ask-{correlation_id}@{node_id}")
    /// * `correlation_id` - Correlation ID for matching replies
    /// * `expires_at` - Expiration time for the temporary sender
    ///
    /// ## Returns
    /// `Arc<dyn MessageSender>` - The temporary sender ActorRef
    async fn create_temporary_sender(
        &self,
        ctx: &RequestContext,
        temp_sender_id: String,
        correlation_id: String,
        expires_at: std::time::Instant,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>>;

    /// Returns self as Any for downcasting to concrete implementation
    ///
    /// ## Purpose
    /// Enables downcasting `Arc<dyn ActorFactory>` to concrete types like `ActorFactoryImpl`
    /// when access to implementation-specific methods is needed (e.g., typed spawn methods).
    ///
    /// ## Example
    /// ```ignore
    /// let factory: Arc<dyn ActorFactory> = service_locator.get_actor_factory().await?;
    /// let factory_impl = factory.as_any()
    ///     .downcast_ref::<ActorFactoryImpl>()
    ///     .ok_or("Expected ActorFactoryImpl")?;
    /// factory_impl.spawn_workflow(ctx, id, behavior, facets).await?;
    /// ```
    fn as_any(&self) -> &dyn std::any::Any;
}
