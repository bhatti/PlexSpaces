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

//! ProcessGroupService - Erlang pg/pg2-Inspired Distributed Pub/Sub
//!
//! ## Design Principle
//!
//! This module provides two implementations:
//! 1. **ProcessGroupServiceImpl** - Core trait implementation for ActorContext access
//! 2. **ProcessGroupServiceGrpc** - gRPC service implementation for remote access
//!
//! ### Industry Standard (Erlang pg/pg2)
//!
//! Process groups follow Erlang pg/pg2 semantics:
//! - **Named Groups**: Actors join named groups for coordination
//! - **Multiple Joins**: Actor can join same group multiple times (join_count tracked)
//! - **Local vs Global**: Fast local queries, distributed global queries
//! - **Topic Filtering**: Actors subscribe to topics within groups
//!
//! ### State Management
//!
//! Uses ObjectRegistry with OBJECT_TYPE_PROCESS_GROUP:
//! - Each node maintains local membership view
//! - ObjectRegistry provides distributed coordination
//! - Local operations are fast (no network)
//! - Global operations use ObjectRegistry discovery
//!
//! ### Key Format in ObjectRegistry
//!
//! - Group metadata: `{tenant}:{namespace}:pg:{group_name}`
//! - Membership: `{tenant}:{namespace}:pg:{group_name}:member:{actor_id}@{node_id}`
//!
//! ### Authentication & Multi-Tenancy
//!
//! For gRPC requests, tenant_id is extracted from auth token (x-tenant-id header)
//! using `request_context_from_grpc_request` - consistent with ActorService pattern.

#![warn(missing_docs)]
#![warn(clippy::all)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::{Request, Response, Status};
use tracing::{debug, info, trace, warn};

use plexspaces_core::actor_context::{ObjectRegistry, ProcessGroupService};
use plexspaces_core::{RequestContext, ServiceLocator as ServiceLocatorTrait};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};

// Import proto types and gRPC service trait
use plexspaces_proto::common::v1::Empty;
use plexspaces_proto::processgroups::v1::{
    process_group_service_server::ProcessGroupService as ProcessGroupServiceTrait,
    CreateGroupRequest, CreateGroupResponse, DeleteGroupRequest, GetLocalMembersRequest,
    GetLocalMembersResponse, GetMembersRequest, GetMembersResponse, JoinGroupRequest,
    LeaveGroupRequest, ListGroupsRequest, ListGroupsResponse, ProcessGroup, PublishToGroupRequest,
    PublishToGroupResponse,
};

use crate::ServiceLocatorImpl;

/// Local membership entry (per-node in-memory cache)
///
/// ## Purpose
/// Tracks local membership for fast get_local_members queries.
/// Follows industry standard: each node maintains local view, syncs via ObjectRegistry.
#[derive(Clone, Debug)]
struct LocalMembership {
    /// Actor ID
    _actor_id: String,
    /// Join count (Erlang pg2 semantics)
    join_count: i32,
    /// Topics subscribed to
    topics: Vec<String>,
    /// Timestamp when joined
    _joined_at: i64,
}

/// Local group state (per-node in-memory cache)
#[derive(Clone, Debug, Default)]
struct LocalGroupState {
    /// Local members (actor_id -> membership)
    members: HashMap<String, LocalMembership>,
}

/// ProcessGroupServiceImpl - Core implementation
///
/// ## Architecture
///
/// ```text
/// ┌─────────────────────────────────────────────────────────────┐
/// │                    ProcessGroupServiceImpl                   │
/// │  ┌─────────────────┐  ┌──────────────────────────────────┐ │
/// │  │ Local Cache     │  │ ObjectRegistry                   │ │
/// │  │ (fast queries)  │  │ (distributed state)              │ │
/// │  │                 │  │                                  │ │
/// │  │ get_local_      │  │ - Group metadata                 │ │
/// │  │ members()       │  │ - Membership (cluster-wide)      │ │
/// │  │                 │  │ - OBJECT_TYPE_PROCESS_GROUP      │ │
/// │  └─────────────────┘  └──────────────────────────────────┘ │
/// └─────────────────────────────────────────────────────────────┘
/// ```
///
/// ## Design Decisions
///
/// 1. **Local Cache**: Each node caches its local memberships for O(1) get_local_members
/// 2. **ObjectRegistry**: Distributed state via OBJECT_TYPE_PROCESS_GROUP
/// 3. **Eventual Consistency**: Local cache may lag slightly behind ObjectRegistry
/// 4. **Join Count**: Tracked locally, synced to ObjectRegistry on changes
pub struct ProcessGroupServiceImpl {
    /// ServiceLocator for accessing ObjectRegistry and other services
    service_locator: Arc<dyn ServiceLocatorTrait>,

    /// Local node ID
    local_node_id: String,

    /// Local group state cache (group_name -> LocalGroupState)
    /// Key format: "{tenant_id}:{namespace}:{group_name}"
    local_state: RwLock<HashMap<String, LocalGroupState>>,
}

impl ProcessGroupServiceImpl {
    /// Create new ProcessGroupServiceImpl
    ///
    /// # Arguments
    /// * `service_locator` - ServiceLocator for accessing ObjectRegistry
    /// * `local_node_id` - ID of this node
    pub fn new(service_locator: Arc<dyn ServiceLocatorTrait>, local_node_id: String) -> Self {
        ProcessGroupServiceImpl {
            service_locator,
            local_node_id,
            local_state: RwLock::new(HashMap::new()),
        }
    }

    /// Generate cache key for local state
    fn cache_key(tenant_id: &str, namespace: &str, group_name: &str) -> String {
        format!("{}:{}:{}", tenant_id, namespace, group_name)
    }

    /// Generate ObjectRegistry object_id for group metadata
    fn group_object_id(tenant_id: &str, namespace: &str, group_name: &str) -> String {
        format!("pg:{}:{}:{}", tenant_id, namespace, group_name)
    }

    /// Generate ObjectRegistry object_id for membership
    fn membership_object_id(
        tenant_id: &str,
        namespace: &str,
        group_name: &str,
        actor_id: &str,
        node_id: &str,
    ) -> String {
        format!(
            "pg:{}:{}:{}:member:{}@{}",
            tenant_id, namespace, group_name, actor_id, node_id
        )
    }

    /// Get ObjectRegistry from ServiceLocator
    async fn get_object_registry(
        &self,
    ) -> Result<Arc<dyn ObjectRegistry>, Box<dyn std::error::Error + Send + Sync>> {
        self.service_locator
            .get_object_registry()
            .await
            .ok_or_else(|| "ObjectRegistry not found in ServiceLocator".into())
    }

    /// Check if service should accept requests (not shutting down)
    fn check_accepting_requests(&self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.service_locator.is_shutdown_requested() {
            return Err("Service is shutting down".into());
        }
        Ok(())
    }

    /// Create ObjectRegistration for group metadata
    fn create_group_registration(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> ObjectRegistration {
        ObjectRegistration {
            object_id: Self::group_object_id(ctx.tenant_id(), ctx.namespace(), group_name),
            object_name: group_name.to_string(),
            object_type: ObjectType::ObjectTypeProcessGroup as i32,
            version: "1.0".to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
            node_id: self.local_node_id.clone(),
            grpc_address: String::new(), // Will be set by node registration
            object_category: "pubsub".to_string(),
            capabilities: vec!["topic-filtering".to_string(), "multiple-joins".to_string()],
            metadata: None,
            health_status: HealthStatus::HealthStatusHealthy as i32,
            labels: vec![],
            metrics: HashMap::new(),
            last_heartbeat: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            created_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            updated_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
        }
    }

    /// Create ObjectRegistration for membership
    fn create_membership_registration(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: &[String],
        join_count: i32,
    ) -> ObjectRegistration {
        let mut labels = topics.to_vec();
        labels.push(format!("join_count:{}", join_count));

        ObjectRegistration {
            object_id: Self::membership_object_id(
                ctx.tenant_id(),
                ctx.namespace(),
                group_name,
                actor_id,
                &self.local_node_id,
            ),
            object_name: format!("{}@{}", actor_id, group_name),
            object_type: ObjectType::ObjectTypeProcessGroup as i32,
            version: "1.0".to_string(),
            tenant_id: ctx.tenant_id().to_string(),
            namespace: ctx.namespace().to_string(),
            node_id: self.local_node_id.clone(),
            grpc_address: String::new(),
            object_category: "membership".to_string(),
            capabilities: topics.to_vec(),
            metadata: None,
            health_status: HealthStatus::HealthStatusHealthy as i32,
            labels,
            metrics: HashMap::new(),
            last_heartbeat: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            created_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
            updated_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
        }
    }
}

/// Re-export the server type for convenience
pub use plexspaces_proto::processgroups::v1::process_group_service_server::ProcessGroupServiceServer as ProcessGroupServiceGrpcServer;

// ============================================================================
// CORE TRAIT IMPLEMENTATION
// ============================================================================

#[async_trait]
impl ProcessGroupService for ProcessGroupServiceImpl {
    async fn create_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let span = tracing::span!(
            tracing::Level::DEBUG,
            "process_group.create",
            group_name = %group_name,
            tenant_id = %ctx.tenant_id()
        );
        let _guard = span.enter();
        trace!("Creating process group");

        let object_registry = self.get_object_registry().await?;

        // Check if group already exists
        let group_id = Self::group_object_id(ctx.tenant_id(), ctx.namespace(), group_name);
        if object_registry
            .lookup(ctx, &group_id, Some(ObjectType::ObjectTypeProcessGroup))
            .await?
            .is_some()
        {
            return Err(format!("Group already exists: {}", group_name).into());
        }

        // Register group in ObjectRegistry
        let registration = self.create_group_registration(ctx, group_name);
        object_registry.register(ctx, registration).await?;

        // Initialize local state
        let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
        let mut state = self.local_state.write().await;
        state.insert(cache_key, LocalGroupState::default());

        metrics::counter!("plexspaces_process_groups_created_total", "tenant" => ctx.tenant_id().to_string()).increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Process group created: {}", group_name);
        }

        Ok(())
    }

    async fn delete_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let span = tracing::span!(
            tracing::Level::DEBUG,
            "process_group.delete",
            group_name = %group_name,
            tenant_id = %ctx.tenant_id()
        );
        let _guard = span.enter();
        trace!("Deleting process group");

        let object_registry = self.get_object_registry().await?;

        // Unregister group from ObjectRegistry (idempotent)
        let group_id = Self::group_object_id(ctx.tenant_id(), ctx.namespace(), group_name);
        let _ = object_registry
            .unregister(ctx, ObjectType::ObjectTypeProcessGroup, &group_id)
            .await;

        // TODO: Also unregister all memberships (would need prefix scan in ObjectRegistry)
        // For now, memberships become orphaned - could be cleaned up by background job

        // Remove from local state
        let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
        let mut state = self.local_state.write().await;
        state.remove(&cache_key);

        metrics::counter!("plexspaces_process_groups_deleted_total", "tenant" => ctx.tenant_id().to_string()).increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Process group deleted: {}", group_name);
        }

        Ok(())
    }

    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let span = tracing::span!(
            tracing::Level::DEBUG,
            "process_group.join",
            group_name = %group_name,
            actor_id = %actor_id,
            tenant_id = %ctx.tenant_id()
        );
        let _guard = span.enter();
        trace!("Joining process group");

        let object_registry = self.get_object_registry().await?;

        // Verify group exists
        let group_id = Self::group_object_id(ctx.tenant_id(), ctx.namespace(), group_name);
        if object_registry
            .lookup(ctx, &group_id, Some(ObjectType::ObjectTypeProcessGroup))
            .await?
            .is_none()
        {
            return Err(format!("Group not found: {}", group_name).into());
        }

        // Update local state first (fast path)
        let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
        let join_count = {
            let mut state = self.local_state.write().await;
            let group_state = state.entry(cache_key.clone()).or_default();

            let membership = group_state
                .members
                .entry(actor_id.to_string())
                .or_insert_with(|| LocalMembership {
                    _actor_id: actor_id.to_string(),
                    join_count: 0,
                    topics: vec![],
                    _joined_at: chrono::Utc::now().timestamp(),
                });

            // Increment join count (Erlang pg2 semantics)
            membership.join_count += 1;

            // Merge topics
            for topic in &topics {
                if !membership.topics.contains(topic) {
                    membership.topics.push(topic.clone());
                }
            }

            membership.join_count
        };

        // Sync to ObjectRegistry
        let registration =
            self.create_membership_registration(ctx, group_name, actor_id, &topics, join_count);
        object_registry.register(ctx, registration).await?;

        metrics::counter!("plexspaces_process_groups_joins_total", "group" => group_name.to_string(), "tenant" => ctx.tenant_id().to_string()).increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Actor {} joined group {}", actor_id, group_name);
        }

        Ok(())
    }

    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let span = tracing::span!(
            tracing::Level::DEBUG,
            "process_group.leave",
            group_name = %group_name,
            actor_id = %actor_id,
            tenant_id = %ctx.tenant_id()
        );
        let _guard = span.enter();
        trace!("Leaving process group");

        let object_registry = self.get_object_registry().await?;

        // Update local state
        let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
        let (should_unregister, new_join_count, topics) = {
            let mut state = self.local_state.write().await;

            if let Some(group_state) = state.get_mut(&cache_key) {
                if let Some(membership) = group_state.members.get_mut(actor_id) {
                    membership.join_count = membership.join_count.saturating_sub(1);

                    if membership.join_count == 0 {
                        let topics = membership.topics.clone();
                        group_state.members.remove(actor_id);
                        (true, 0, topics)
                    } else {
                        (false, membership.join_count, membership.topics.clone())
                    }
                } else {
                    return Err(format!("Actor {} not in group {}", actor_id, group_name).into());
                }
            } else {
                return Err(format!("Group not found: {}", group_name).into());
            }
        };

        // Sync to ObjectRegistry
        let membership_id = Self::membership_object_id(
            ctx.tenant_id(),
            ctx.namespace(),
            group_name,
            actor_id,
            &self.local_node_id,
        );

        if should_unregister {
            object_registry
                .unregister(ctx, ObjectType::ObjectTypeProcessGroup, &membership_id)
                .await?;
        } else {
            // Update with new join count
            let registration = self.create_membership_registration(
                ctx,
                group_name,
                actor_id,
                &topics,
                new_join_count,
            );
            object_registry.register(ctx, registration).await?;
        }

        metrics::counter!("plexspaces_process_groups_leaves_total", "group" => group_name.to_string(), "tenant" => ctx.tenant_id().to_string()).increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            debug!("Actor {} left group {}", actor_id, group_name);
        }

        Ok(())
    }

    async fn get_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let object_registry = self.get_object_registry().await?;

        // Verify group exists
        let group_id = Self::group_object_id(ctx.tenant_id(), ctx.namespace(), group_name);
        if object_registry
            .lookup(ctx, &group_id, Some(ObjectType::ObjectTypeProcessGroup))
            .await?
            .is_none()
        {
            return Err(format!("Group not found: {}", group_name).into());
        }

        // Discover all memberships from ObjectRegistry
        // Filter by object_type=ProcessGroup and object_category=membership
        let registrations = object_registry
            .discover(
                ctx,
                Some(ObjectType::ObjectTypeProcessGroup),
                Some("membership".to_string()),
                None,  // no capability filter
                None,  // no label filter
                None,  // no health filter
                0,     // offset
                10000, // limit (large enough for all members)
            )
            .await?;

        // Extract actor IDs from membership registrations for this group
        let prefix = format!(
            "pg:{}:{}:{}:member:",
            ctx.tenant_id(),
            ctx.namespace(),
            group_name
        );

        let members: Vec<String> = registrations
            .into_iter()
            .filter(|r| r.object_id.starts_with(&prefix))
            .map(|r| {
                // Extract actor_id from object_id: "pg:tenant:ns:group:member:actor@node"
                r.object_id
                    .strip_prefix(&prefix)
                    .and_then(|s| s.split('@').next())
                    .unwrap_or("")
                    .to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        // Deduplicate (same actor might be registered from multiple nodes)
        let mut unique_members: Vec<String> = members
            .into_iter()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();
        unique_members.sort();

        Ok(unique_members)
    }

    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        // Fast path: query local cache only
        let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
        let state = self.local_state.read().await;

        if let Some(group_state) = state.get(&cache_key) {
            let members: Vec<String> = group_state.members.keys().cloned().collect();
            Ok(members)
        } else {
            // Group doesn't exist locally - might exist on other nodes
            // Return empty rather than error (consistent with Erlang pg2)
            Ok(vec![])
        }
    }

    async fn list_groups(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let object_registry = self.get_object_registry().await?;

        // Discover all groups from ObjectRegistry
        let registrations = object_registry
            .discover(
                ctx,
                Some(ObjectType::ObjectTypeProcessGroup),
                Some("pubsub".to_string()), // Groups have category "pubsub"
                None,                       // no capability filter
                None,                       // no label filter
                None,                       // no health filter
                0,                          // offset
                10000,                      // limit
            )
            .await?;

        // Extract group names from object_ids
        let prefix = format!("pg:{}:{}:", ctx.tenant_id(), ctx.namespace());

        let groups: Vec<String> = registrations
            .into_iter()
            .filter(|r| r.object_id.starts_with(&prefix) && r.object_category == "pubsub")
            .map(|r| {
                // Extract group_name from object_id: "pg:tenant:ns:group_name"
                r.object_id.strip_prefix(&prefix).unwrap_or("").to_string()
            })
            .filter(|s| !s.is_empty())
            .collect();

        Ok(groups)
    }

    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        topic: Option<&str>,
        _message: Message,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        self.check_accepting_requests()?;

        let span = tracing::span!(
            tracing::Level::DEBUG,
            "process_group.publish",
            group_name = %group_name,
            topic = ?topic,
            tenant_id = %ctx.tenant_id()
        );
        let _guard = span.enter();
        trace!("Publishing to process group");

        // Get all members (cluster-wide)
        let all_members = self.get_members(ctx, group_name).await?;

        // Filter by topic if specified
        let recipients = if let Some(topic_filter) = topic {
            // Get memberships to check topics
            let cache_key = Self::cache_key(ctx.tenant_id(), ctx.namespace(), group_name);
            let state = self.local_state.read().await;

            if let Some(group_state) = state.get(&cache_key) {
                all_members
                    .into_iter()
                    .filter(|actor_id| {
                        if let Some(membership) = group_state.members.get(actor_id) {
                            // Include if subscribed to topic OR has no topic filter (receives all)
                            membership.topics.is_empty()
                                || membership.topics.contains(&topic_filter.to_string())
                        } else {
                            // Remote member - include by default (can't check topics locally)
                            true
                        }
                    })
                    .collect()
            } else {
                all_members
            }
        } else {
            all_members
        };

        let recipient_count = recipients.len() as u32;

        // TODO: Actually deliver messages to recipients via ActorService
        // For now, just log and return count
        // In production, would:
        // 1. Get ActorService from ServiceLocator
        // 2. For each recipient, send message via ActorService.send()
        // 3. Track failures

        debug!(
            "Published message to {} recipients in group {}",
            recipient_count, group_name
        );

        metrics::counter!("plexspaces_process_groups_publish_total", "group" => group_name.to_string(), "tenant" => ctx.tenant_id().to_string()).increment(1);
        metrics::histogram!("plexspaces_process_groups_fanout_size").record(recipient_count as f64);

        Ok(recipient_count)
    }
}

// ============================================================================
// gRPC SERVICE IMPLEMENTATION
// ============================================================================

/// gRPC wrapper for ProcessGroupServiceImpl
///
/// ## Authentication & Multi-Tenancy
///
/// This implementation follows the ActorService pattern for authentication:
/// - Uses `request_context_from_grpc_request` to extract tenant_id from auth token
/// - Validates tenant context before processing requests
/// - Ensures proper multi-tenant isolation
///
/// ## Design
///
/// Delegates all operations to the core ProcessGroupService trait implementation.
pub struct ProcessGroupServiceGrpc {
    /// Core implementation
    inner: Arc<ProcessGroupServiceImpl>,
    /// ServiceLocator for auth context extraction
    service_locator: Arc<ServiceLocatorImpl>,
}

impl ProcessGroupServiceGrpc {
    /// Create new gRPC service wrapper
    pub fn new(
        inner: Arc<ProcessGroupServiceImpl>,
        service_locator: Arc<ServiceLocatorImpl>,
    ) -> Self {
        ProcessGroupServiceGrpc {
            inner,
            service_locator,
        }
    }

    /// Extract RequestContext from gRPC request metadata
    ///
    /// Uses shared validation from `request_context_from_grpc_request` which:
    /// - Extracts tenant_id from x-tenant-id header (set by auth middleware)
    /// - Falls back to NodeConfig defaults if auth disabled
    /// - Validates tenant context for multi-tenancy
    async fn extract_context(
        &self,
        metadata: &tonic::metadata::MetadataMap,
    ) -> Result<RequestContext, Status> {
        let sl: Arc<dyn plexspaces_core::ServiceLocator> = self.service_locator.clone();
        crate::request_context_from_grpc_request(metadata, &std::collections::HashMap::new(), &sl)
            .await
            .map_err(|e| Status::unauthenticated(format!("Invalid request context: {}", e)))
    }

    /// Convert error to gRPC Status with appropriate codes
    fn error_to_status(err: Box<dyn std::error::Error + Send + Sync>) -> Status {
        let msg = err.to_string();
        if msg.contains("not found") || msg.contains("not in group") {
            Status::not_found(msg)
        } else if msg.contains("already exists") {
            Status::already_exists(msg)
        } else if msg.contains("shutting down") {
            Status::unavailable(msg)
        } else if msg.contains("permission") || msg.contains("unauthorized") {
            Status::permission_denied(msg)
        } else {
            Status::internal(msg)
        }
    }
}

#[async_trait]
impl ProcessGroupServiceTrait for ProcessGroupServiceGrpc {
    async fn create_group(
        &self,
        request: Request<CreateGroupRequest>,
    ) -> Result<Response<CreateGroupResponse>, Status> {
        let start_time = std::time::Instant::now();
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        // Extract RequestContext from auth - tenant_id comes from auth token, not request body
        let ctx = self.extract_context(&metadata).await?;

        // Validate required fields
        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }

        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();

        // OBSERVABILITY: Start tracing span
        let span = tracing::span!(
            tracing::Level::INFO,
            "process_group_service.create_group",
            tenant_id = %tenant_id,
            namespace = %namespace,
            group_name = %req.group_name
        );
        let _guard = span.enter();

        info!(
            "🟦 [CREATE_GROUP] START: tenant_id={}, namespace={}, group_name={}",
            &tenant_id, &namespace, req.group_name
        );

        self.inner
            .create_group(&ctx, &req.group_name)
            .await
            .map_err(Self::error_to_status)?;

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_process_group_service_create_duration_seconds")
            .record(duration.as_secs_f64());

        // Return created group
        let group = ProcessGroup {
            group_name: req.group_name,
            tenant_id,
            namespace,
            member_ids: vec![],
            member_count: 0,
            metadata: req.metadata,
            created_at: Some(prost_types::Timestamp {
                seconds: chrono::Utc::now().timestamp(),
                nanos: 0,
            }),
        };

        Ok(Response::new(CreateGroupResponse { group: Some(group) }))
    }

    async fn delete_group(
        &self,
        request: Request<DeleteGroupRequest>,
    ) -> Result<Response<Empty>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }

        self.inner
            .delete_group(&ctx, &req.group_name)
            .await
            .map_err(Self::error_to_status)?;

        Ok(Response::new(Empty {}))
    }

    async fn join_group(
        &self,
        request: Request<JoinGroupRequest>,
    ) -> Result<Response<Empty>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }
        if req.actor_id.is_empty() {
            return Err(Status::invalid_argument("actor_id is required"));
        }

        self.inner
            .join_group(&ctx, &req.group_name, &req.actor_id, req.topics)
            .await
            .map_err(Self::error_to_status)?;

        Ok(Response::new(Empty {}))
    }

    async fn leave_group(
        &self,
        request: Request<LeaveGroupRequest>,
    ) -> Result<Response<Empty>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }
        if req.actor_id.is_empty() {
            return Err(Status::invalid_argument("actor_id is required"));
        }

        self.inner
            .leave_group(&ctx, &req.group_name, &req.actor_id)
            .await
            .map_err(Self::error_to_status)?;

        Ok(Response::new(Empty {}))
    }

    async fn get_members(
        &self,
        request: Request<GetMembersRequest>,
    ) -> Result<Response<GetMembersResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }

        let members = self
            .inner
            .get_members(&ctx, &req.group_name)
            .await
            .map_err(Self::error_to_status)?;

        let total_count = members.len() as i32;

        Ok(Response::new(GetMembersResponse {
            member_ids: members,
            next_page_token: String::new(),
            total_count,
        }))
    }

    async fn get_local_members(
        &self,
        request: Request<GetLocalMembersRequest>,
    ) -> Result<Response<GetLocalMembersResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }

        let members = self
            .inner
            .get_local_members(&ctx, &req.group_name)
            .await
            .map_err(Self::error_to_status)?;

        let local_count = members.len() as i32;

        Ok(Response::new(GetLocalMembersResponse {
            member_ids: members,
            local_count,
        }))
    }

    async fn list_groups(
        &self,
        request: Request<ListGroupsRequest>,
    ) -> Result<Response<ListGroupsResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        let group_names = self
            .inner
            .list_groups(&ctx)
            .await
            .map_err(Self::error_to_status)?;

        // Filter by pattern if provided
        let filtered_names: Vec<String> = if req.name_pattern.is_empty() {
            group_names
        } else {
            let pattern = req.name_pattern.replace('*', ".*");
            if let Ok(regex) = regex::Regex::new(&format!("^{}$", pattern)) {
                group_names
                    .into_iter()
                    .filter(|name| regex.is_match(name))
                    .collect()
            } else {
                group_names
            }
        };

        let total_count = filtered_names.len() as i32;
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();

        let groups: Vec<ProcessGroup> = filtered_names
            .into_iter()
            .map(|name| ProcessGroup {
                group_name: name,
                tenant_id: tenant_id.clone(),
                namespace: namespace.clone(),
                member_ids: vec![],
                member_count: 0,
                metadata: None,
                created_at: None,
            })
            .collect();

        Ok(Response::new(ListGroupsResponse {
            groups,
            next_page_token: String::new(),
            total_count,
        }))
    }

    async fn publish_to_group(
        &self,
        request: Request<PublishToGroupRequest>,
    ) -> Result<Response<PublishToGroupResponse>, Status> {
        let metadata = request.metadata().clone();
        let req = request.into_inner();

        let ctx = self.extract_context(&metadata).await?;

        if req.group_name.is_empty() {
            return Err(Status::invalid_argument("group_name is required"));
        }
        if req.message.is_none() {
            return Err(Status::invalid_argument("message is required"));
        }

        let message = req.message.unwrap();

        let topic = if req.topic.is_empty() {
            None
        } else {
            Some(req.topic.as_str())
        };

        let recipients_count = self
            .inner
            .publish_to_group(&ctx, &req.group_name, topic, message)
            .await
            .map_err(Self::error_to_status)?;

        Ok(Response::new(PublishToGroupResponse {
            recipients_count,
            failures_count: 0,
            recipients_per_node: HashMap::new(),
        }))
    }
}
