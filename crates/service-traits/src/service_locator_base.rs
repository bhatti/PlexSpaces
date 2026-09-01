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

//! Service locator base trait plus `ActorService` and `ActorFactory` traits.
//!
//! # Purpose
//! `ServiceLocatorBase` contains only the service-accessor methods that
//! `plexspaces-journaling` needs. It lives in `plexspaces-service-traits` so
//! that journaling does not have to pull in the full `plexspaces-core` crate.
//!
//! `plexspaces-core`'s `ServiceLocator` trait extends `ServiceLocatorBase` and
//! adds the remaining methods (those that return core-specific types like
//! `ActorRegistry`).

use async_trait::async_trait;
use plexspaces_common::RequestContext;
use plexspaces_proto::common::v1::Message;
use std::sync::Arc;

use crate::{ActorId, ActorRef, ActorStateChecker, JournalStorage, MessageSender};

/// Readonly base service locator trait.
///
/// # Purpose
/// Contains only the service-getter methods that `plexspaces-journaling` (and
/// other leaf crates) need. `plexspaces-core`'s full `ServiceLocator` trait
/// extends this with the remaining methods.
///
/// # Design
/// All methods are `async` and return `Option` because services may not be
/// registered yet at query time (e.g. during early node bootstrap).
#[async_trait]
pub trait ServiceLocatorBase: Send + Sync {
    /// Get the actor messaging service.
    async fn get_actor_service(&self) -> Option<Arc<dyn ActorService>>;

    /// Get journal storage backend.
    async fn get_journal_storage(&self) -> Option<Arc<dyn JournalStorage + Send + Sync>>;

    /// Get key-value store.
    async fn get_keyvalue_store(&self) -> Option<Arc<dyn plexspaces_common::KeyValueStore>>;

    /// Get distributed lock manager.
    async fn get_lock_manager(
        &self,
    ) -> Option<Arc<dyn plexspaces_locks::LockManager + Send + Sync>>;

    /// Get process group service for Erlang pg/pg2-style pub/sub.
    async fn get_process_group_service(&self) -> Option<Arc<dyn crate::ProcessGroupService>> {
        None
    }

    /// Get the actor liveness checker (thin façade over ActorRegistry).
    ///
    /// Used by `ReminderFacet` to determine whether a target actor is running
    /// before firing a reminder, without depending on `ActorRegistry` directly.
    async fn get_actor_state_checker(&self) -> Option<Arc<dyn ActorStateChecker>> {
        None
    }

    /// Get actor factory for spawning and activating actors.
    async fn get_actor_factory(&self) -> Option<Arc<dyn ActorFactory>>;

    /// Returns `true` if a node-level shutdown has been requested.
    fn is_shutdown_requested(&self) -> bool;

    /// Build a `RequestContext` for system-initiated operations (node heartbeat, etc.).
    ///
    /// Tenant and namespace are taken from `NodeConfig.default_tenant_id` /
    /// `NodeConfig.default_namespace` when available, otherwise empty strings.
    async fn request_context_for_system_operations(&self) -> RequestContext;

    /// Same as [`request_context_for_system_operations`] but with an explicit namespace.
    async fn request_context_for_system_operations_with_namespace(
        &self,
        namespace: String,
    ) -> RequestContext;
}

// ─────────────────────────────────────────────────────────────────────────────
// ActorService trait
// ─────────────────────────────────────────────────────────────────────────────

/// Trait for actor service operations (spawning, messaging).
///
/// # Purpose
/// Provides a unified interface for actor operations, whether local or remote.
/// Supports canonical `ActorId` strings for location-transparent operations.
///
/// # Architecture
/// This trait lives in `plexspaces-service-traits` so that `plexspaces-journaling`
/// can use it without depending on `plexspaces-core`. `plexspaces-core` re-exports
/// this trait unchanged.
#[async_trait]
pub trait ActorService: Send + Sync {
    /// Send a message to an actor (fire-and-forget).
    ///
    /// Returns the message ID on success.
    async fn send(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message to an actor and wait for a reply.
    async fn send_and_wait(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _message: Message,
        _timeout: Option<std::time::Duration>,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        Err("send_and_wait is not implemented".into())
    }

    /// Spawn a new actor (local or remote).
    ///
    /// Returns a lightweight [`ActorRef`] for the spawned actor.
    async fn spawn_actor(
        &self,
        ctx: &RequestContext,
        spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
    ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>>;

    /// Create a ShardGroup (data-parallel worker pool).
    async fn create_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::CreateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::CreateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("create_shard_group is not implemented".into())
    }

    /// Bulk update ShardGroup.
    async fn bulk_update_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::BulkUpdateShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("bulk_update_shard_group is not implemented".into())
    }

    /// Map over ShardGroup.
    async fn map_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::MapShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::MapShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("map_shard_group is not implemented".into())
    }

    /// Scatter-gather query.
    async fn scatter_gather(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::ScatterGatherRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ScatterGatherResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("scatter_gather is not implemented".into())
    }

    /// Broadcast a message to all shards in a group.
    async fn broadcast_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::BroadcastShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BroadcastShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("broadcast_shard_group is not implemented".into())
    }

    /// Reduce shard responses.
    async fn reduce_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::ReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::ReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("reduce_shard_group is not implemented".into())
    }

    /// All-reduce shard responses and fan the reduced value back out.
    async fn all_reduce_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::AllReduceShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::AllReduceShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("all_reduce_shard_group is not implemented".into())
    }

    /// Synchronize a shard group at a framework barrier round.
    async fn barrier_shard_group(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::BarrierShardGroupRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::BarrierShardGroupResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("barrier_shard_group is not implemented".into())
    }

    /// Spawn multiple actors.
    async fn spawn_actors(
        &self,
        _ctx: &RequestContext,
        _req: plexspaces_proto::actor::v1::SpawnActorsRequest,
    ) -> Result<
        plexspaces_proto::actor::v1::SpawnActorsResponse,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Err("spawn_actors is not implemented".into())
    }

    /// Establish a monitor on the node that hosts `actor_id`.
    async fn monitor_actor(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _supervisor_id: &str,
        _supervisor_callback: &str,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        Err("monitor_actor is not implemented".into())
    }

    /// Cancel a monitor.
    async fn demonitor_actor(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _supervisor_id: &str,
        _monitor_ref: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("demonitor_actor is not implemented".into())
    }

    /// Register a link toward `linked_actor_id`.
    async fn link_actor(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _linked_actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("link_actor is not implemented".into())
    }

    /// Remove a link toward `linked_actor_id`.
    async fn unlink_actor(
        &self,
        _ctx: &RequestContext,
        _actor_id: &str,
        _linked_actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        Err("unlink_actor is not implemented".into())
    }

    /// Remove all shard groups whose shard actors belong to the given namespace.
    ///
    /// Called during application undeploy to prevent stale group registrations
    /// from blocking re-deployment. Shard actor IDs embed the namespace in their
    /// canonical form (`{name}//{type}::{namespace}@{node}`), so we match on that.
    /// Shard actors are stopped best-effort — they may already be down.
    async fn purge_shard_groups_for_namespace(
        &self,
        _ctx: &RequestContext,
        _namespace: &str,
    ) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
        Ok(0)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// ActorFactory trait
// ─────────────────────────────────────────────────────────────────────────────

/// Trait for spawning and activating actors.
///
/// # Purpose
/// Allows components like `VirtualActorManager` and `ReminderFacet` to spawn
/// actors without depending on Node directly. Implementations live in
/// `plexspaces-actor`.
///
/// # Architecture
/// This trait lives in `plexspaces-service-traits` so that `plexspaces-journaling`
/// can use it without depending on `plexspaces-core`. `plexspaces-core` re-exports
/// this trait unchanged.
#[async_trait]
pub trait ActorFactory: Send + Sync {
    /// Activate a virtual actor (start it if not already started).
    async fn activate_virtual_actor(
        &self,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Spawn a new actor locally.
    ///
    /// Returns `Arc<dyn MessageSender>` for the spawned actor.
    async fn spawn_actor(
        &self,
        ctx: &RequestContext,
        spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
        facets: Vec<Box<dyn plexspaces_facet::Facet>>,
    ) -> Result<Arc<dyn MessageSender>, Box<dyn std::error::Error + Send + Sync>>;

    /// Stop an actor.
    async fn stop_actor(
        &self,
        ctx: &RequestContext,
        actor_id: &ActorId,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Returns self as Any for downcasting to concrete implementation.
    fn as_any(&self) -> &dyn std::any::Any;
}
