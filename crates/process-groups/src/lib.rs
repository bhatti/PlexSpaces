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

//! # PlexSpaces Process Groups
//!
//! ## Purpose
//! Distributed pub/sub and broadcast messaging for actor coordination (Erlang pg/pg2-inspired).
//! Enables named groups where actors subscribe to topics and receive broadcast messages.
//!
//! ## Architecture Context
//! Process groups integrate with PlexSpaces core components:
//! - **ActorRegistry**: Validates actor IDs, uses same KeyValueStore backend
//! - **Multi-Tenancy**: Groups scoped by tenant_id + namespace
//! - **Distributed Storage**: Group membership in KeyValueStore (Redis, PostgreSQL, etc.)
//! - **Messaging**: PublishToGroup broadcasts using Message from actor_runtime.proto
//!
//! ## Key Components
//! - [`ProcessGroupRegistry`]: Main service for group operations
//! - [`ProcessGroupError`]: Error types for group operations
//!
//! ## Use Cases
//!
//! ### Configuration Updates (Broadcast Pattern)
//! ```rust,ignore
//! use plexspaces_process_groups::*;
//!
//! // All actors subscribe to "config-updates" group
//! registry.join_group("config-updates", "tenant-1", "actor-123").await?;
//!
//! // Admin publishes new config
//! registry.publish_to_group("config-updates", "tenant-1", message).await?;
//! ```
//!
//! ### Event Notification (Pub/Sub Pattern)
//! ```rust,ignore
//! // Actors interested in user events join group
//! registry.join_group("user-events", "tenant-1", "observer-actor").await?;
//!
//! // When user action occurs, publish to group
//! registry.publish_to_group("user-events", "tenant-1", event_msg).await?;
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All data models defined in `process_groups.proto` first, Rust implements the contract.
//!
//! ### Erlang pg2 Compatibility
//! Matches pg2 semantics: multiple joins, get_members, get_local_members.
//!
//! ### Test-Driven Development
//! All features developed with tests first (RED → GREEN → REFACTOR).

#![warn(missing_docs)]
#![warn(clippy::all)]

use plexspaces_core::ActorId;
use plexspaces_keyvalue::{KVError, KeyValueStore};
use plexspaces_proto::processgroups::v1::{GroupMembership, ProcessGroup};
use prost::Message as ProstMessage;
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, trace, warn};

/// Error types for process group operations
#[derive(Debug, Error)]
pub enum ProcessGroupError {
    /// Group not found
    #[error("Group not found: {0}")]
    GroupNotFound(String),

    /// Group already exists
    #[error("Group already exists: {0}")]
    GroupAlreadyExists(String),

    /// Actor not found in group
    #[error("Actor not found in group: {actor_id} (group: {group_name})")]
    ActorNotInGroup {
        /// Group name
        group_name: String,
        /// Actor ID
        actor_id: ActorId,
    },

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Permission denied (cross-tenant access)
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl From<KVError> for ProcessGroupError {
    fn from(err: KVError) -> Self {
        ProcessGroupError::StorageError(err.to_string())
    }
}

/// Process group registry for distributed pub/sub
///
/// ## Purpose
/// Implements Erlang pg/pg2-style process groups for actor coordination.
///
/// ## Architecture
/// Uses KeyValueStore backend for distributed membership tracking:
/// - Group metadata: `tenant:{tenant_id}:group:{group_name}`
/// - Membership: `tenant:{tenant_id}:group:{group_name}:member:{actor_id}`
///
/// ## Thread Safety
/// ProcessGroupRegistry is `Send + Sync`, safe to share across threads via Arc.
#[derive(Clone)]
pub struct ProcessGroupRegistry {
    /// Distributed storage backend
    storage: Arc<dyn KeyValueStore>,

    /// This node's identifier
    node_id: String,
}

impl ProcessGroupRegistry {
    /// Create a new process group registry
    ///
    /// ## Arguments
    /// - `node_id`: Unique identifier for this node
    /// - `storage`: KeyValueStore backend for distributed storage
    ///
    /// ## Examples
    /// ```rust,ignore
    /// use plexspaces_keyvalue::InMemoryKVStore;
    /// use plexspaces_process_groups::ProcessGroupRegistry;
    ///
    /// let kv = Arc::new(InMemoryKVStore::new());
    /// let registry = ProcessGroupRegistry::new("node-1", kv);
    /// ```
    pub fn new(node_id: impl Into<String>, storage: Arc<dyn KeyValueStore>) -> Self {
        Self {
            storage,
            node_id: node_id.into(),
        }
    }

    /// Group metadata key format
    fn group_key(tenant_id: &str, group_name: &str) -> String {
        format!("tenant:{}:group:{}", tenant_id, group_name)
    }

    /// Group membership key format
    fn membership_key(tenant_id: &str, group_name: &str, actor_id: &ActorId) -> String {
        format!(
            "tenant:{}:group:{}:member:{}",
            tenant_id, group_name, actor_id
        )
    }

    /// Membership prefix for listing all members
    fn membership_prefix(tenant_id: &str, group_name: &str) -> String {
        format!("tenant:{}:group:{}:member:", tenant_id, group_name)
    }

    /// Create a new process group
    ///
    /// ## Arguments
    /// - `group_name`: Unique group identifier within tenant
    /// - `tenant_id`: Tenant for multi-tenancy isolation
    /// - `namespace`: Namespace within tenant (optional)
    ///
    /// ## Returns
    /// `Ok(ProcessGroup)` on success
    ///
    /// ## Errors
    /// - `GroupAlreadyExists`: Group with this name already exists
    /// - `StorageError`: Backend storage failure
    pub async fn create_group(
        &self,
        group_name: impl Into<String>,
        tenant_id: impl Into<String>,
        namespace: impl Into<String>,
    ) -> Result<ProcessGroup, ProcessGroupError> {
        let start = std::time::Instant::now();
        let group_name = group_name.into();
        let tenant_id = tenant_id.into();
        let namespace = namespace.into();
        let span = tracing::span!(tracing::Level::DEBUG, "process_group.create", group_name = %group_name, tenant_id = %tenant_id);
        let _guard = span.enter();
        trace!("Creating process group");

        let key = Self::group_key(&tenant_id, &group_name);

        // Check if group already exists
        if self.storage.get(&key).await?.is_some() {
            metrics::counter!("plexspaces_process_groups_create_errors_total", "error" => "already_exists").increment(1);
            warn!("Group already exists: {}", group_name);
            return Err(ProcessGroupError::GroupAlreadyExists(group_name));
        }

        // Create group metadata
        let now = chrono::Utc::now();
        let group = ProcessGroup {
            group_name: group_name.clone(),
            tenant_id: tenant_id.clone(),
            namespace: namespace.clone(),
            member_ids: vec![],
            member_count: 0,
            metadata: None,
            created_at: Some(prost_types::Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
        };

        // Store in KeyValueStore
        let value = group.encode_to_vec();
        self.storage.put(&key, value).await?;

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_process_groups_create_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_process_groups_created_total", "tenant" => tenant_id.to_string()).increment(1);
        debug!(duration_ms = duration.as_millis(), "Process group created");

        Ok(group)
    }

    /// Delete a process group
    ///
    /// ## Arguments
    /// - `group_name`: Group to delete
    /// - `tenant_id`: Tenant for security validation
    ///
    /// ## Semantics
    /// - All members automatically removed
    /// - All published messages automatically removed
    /// - Idempotent (deleting non-existent group is no-op)
    ///
    /// ## Errors
    /// - `StorageError`: Backend storage failure
    pub async fn delete_group(
        &self,
        group_name: &str,
        tenant_id: &str,
    ) -> Result<(), ProcessGroupError> {
        let start = std::time::Instant::now();
        let _span = tracing::span!(tracing::Level::DEBUG, "process_group.delete", group_name = %group_name, tenant_id = %tenant_id);
        trace!("Deleting process group");

        let group_key = Self::group_key(tenant_id, group_name);

        // Delete group metadata (idempotent - no error if doesn't exist)
        let _ = self.storage.delete(&group_key).await;

        // Delete all memberships (prefix scan - optimized batch delete)
        let membership_prefix = Self::membership_prefix(tenant_id, group_name);
        let keys = self.storage.list(&membership_prefix).await?;
        let member_count = keys.len();
        
        // Batch delete memberships (sequential for simplicity - KeyValueStore handles concurrency)
        for key in keys {
            let _ = self.storage.delete(&key).await;
        }

        // Delete all published messages (prefix scan)
        // Format: tenant:{tenant_id}:group:{group_name}:message:
        let message_prefix = format!("tenant:{}:group:{}:message:", tenant_id, group_name);
        let message_keys = self.storage.list(&message_prefix).await?;
        let message_count = message_keys.len();
        
        // Batch delete messages (sequential for simplicity)
        for key in message_keys {
            let _ = self.storage.delete(&key).await;
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_process_groups_delete_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_process_groups_deleted_total", "tenant" => tenant_id.to_string()).increment(1);
        debug!(
            duration_ms = duration.as_millis(),
            members_deleted = member_count,
            messages_deleted = message_count,
            "Process group deleted"
        );

        Ok(())
    }

    /// Join a process group
    ///
    /// ## Arguments
    /// - `group_name`: Group to join
    /// - `tenant_id`: Tenant for isolation
    /// - `actor_id`: Actor to add to group
    /// - `topics`: Optional topics to subscribe to (empty = all topics)
    ///
    /// ## Semantics (Erlang pg2-compatible)
    /// - Actor can join multiple groups
    /// - Actor can join same group multiple times (join_count tracked)
    /// - Must call leave() equal number of times to fully remove
    /// - Topics: If specified, actor only receives messages for subscribed topics
    ///
    /// ## Errors
    /// - `GroupNotFound`: Group doesn't exist (call create_group first)
    /// - `StorageError`: Backend storage failure
    pub async fn join_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        actor_id: &ActorId,
        topics: Vec<String>,
    ) -> Result<(), ProcessGroupError> {
        let start = std::time::Instant::now();
        let _span = tracing::span!(tracing::Level::DEBUG, "process_group.join", group_name = %group_name, tenant_id = %tenant_id, actor_id = %actor_id, topics = ?topics);
        trace!("Joining process group");

        // Verify group exists
        let group_key = Self::group_key(tenant_id, group_name);
        if self.storage.get(&group_key).await?.is_none() {
            metrics::counter!("plexspaces_process_groups_join_errors_total", "error" => "group_not_found").increment(1);
            warn!("Group not found: {}", group_name);
            return Err(ProcessGroupError::GroupNotFound(group_name.to_string()));
        }

        let membership_key = Self::membership_key(tenant_id, group_name, actor_id);

        // Get existing membership or create new
        let mut membership = match self.storage.get(&membership_key).await? {
            Some(bytes) => GroupMembership::decode(&bytes[..])
                .map_err(|e| {
                    metrics::counter!("plexspaces_process_groups_join_errors_total", "error" => "deserialization").increment(1);
                    ProcessGroupError::SerializationError(e.to_string())
                })?,
            None => {
                // New membership
                GroupMembership {
                    group_name: group_name.to_string(),
                    tenant_id: tenant_id.to_string(),
                    actor_id: actor_id.clone(),
                    node_id: self.node_id.clone(),
                    join_count: 0,
                    topics: topics.clone(),
                    joined_at: Some(prost_types::Timestamp {
                        seconds: chrono::Utc::now().timestamp(),
                        nanos: chrono::Utc::now().timestamp_subsec_nanos() as i32,
                    }),
                    last_heartbeat: None,
                }
            }
        };

        // Merge topics if actor already has membership (union of topics)
        if !topics.is_empty() {
            let mut existing_topics: std::collections::HashSet<String> = membership.topics.iter().cloned().collect();
            for topic in topics {
                existing_topics.insert(topic);
            }
            membership.topics = existing_topics.into_iter().collect();
        }

        // Increment join_count (Erlang pg2 semantics)
        membership.join_count += 1;

        // Store updated membership
        let value = membership.encode_to_vec();
        self.storage.put(&membership_key, value).await?;

        // Update group member_count (optimized: only if new member)
        if membership.join_count == 1 {
            self.update_member_count(tenant_id, group_name).await?;
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_process_groups_join_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_process_groups_joins_total", "group" => group_name.to_string(), "tenant" => tenant_id.to_string()).increment(1);
        debug!(duration_ms = duration.as_millis(), "Actor joined process group");

        Ok(())
    }

    /// Leave a process group
    ///
    /// ## Arguments
    /// - `group_name`: Group to leave
    /// - `tenant_id`: Tenant for isolation
    /// - `actor_id`: Actor to remove from group
    ///
    /// ## Semantics (Erlang pg2-compatible)
    /// - Decrements join_count
    /// - Actor fully removed when join_count reaches 0
    /// - Idempotent if join_count already 0
    ///
    /// ## Errors
    /// - `GroupNotFound`: Group doesn't exist
    /// - `ActorNotInGroup`: Actor not a member of this group
    pub async fn leave_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        actor_id: &ActorId,
    ) -> Result<(), ProcessGroupError> {
        let start = std::time::Instant::now();
        let _span = tracing::span!(tracing::Level::DEBUG, "process_group.leave", group_name = %group_name, tenant_id = %tenant_id, actor_id = %actor_id);
        trace!("Leaving process group");

        let membership_key = Self::membership_key(tenant_id, group_name, actor_id);

        // Get existing membership
        let bytes = self.storage.get(&membership_key).await?.ok_or_else(|| {
            metrics::counter!("plexspaces_process_groups_leave_errors_total", "error" => "actor_not_in_group").increment(1);
            warn!("Actor not in group: {} (group: {})", actor_id, group_name);
            ProcessGroupError::ActorNotInGroup {
                group_name: group_name.to_string(),
                actor_id: actor_id.clone(),
            }
        })?;

        let mut membership = GroupMembership::decode(&bytes[..])
            .map_err(|e| {
                metrics::counter!("plexspaces_process_groups_leave_errors_total", "error" => "deserialization").increment(1);
                ProcessGroupError::SerializationError(e.to_string())
            })?;

        // Decrement join_count
        let was_removed = membership.join_count == 1;
        membership.join_count = membership.join_count.saturating_sub(1);

        if membership.join_count == 0 {
            // Fully remove membership
            self.storage.delete(&membership_key).await?;
        } else {
            // Update membership with decremented count
            let value = membership.encode_to_vec();
            self.storage.put(&membership_key, value).await?;
        }

        // Update group member_count (optimized: only if member was fully removed)
        if was_removed {
            self.update_member_count(tenant_id, group_name).await?;
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_process_groups_leave_duration_seconds").record(duration.as_secs_f64());
        metrics::counter!("plexspaces_process_groups_leaves_total", "group" => group_name.to_string(), "tenant" => tenant_id.to_string()).increment(1);
        debug!(duration_ms = duration.as_millis(), "Actor left process group");

        Ok(())
    }

    /// Get all group members (cluster-wide)
    ///
    /// ## Arguments
    /// - `group_name`: Group to query
    /// - `tenant_id`: Tenant for isolation
    ///
    /// ## Returns
    /// List of actor IDs in this group across all nodes
    ///
    /// ## Performance
    /// O(n) where n = total members across cluster
    ///
    /// ## Errors
    /// - `GroupNotFound`: Group doesn't exist
    pub async fn get_members(
        &self,
        group_name: &str,
        tenant_id: &str,
    ) -> Result<Vec<ActorId>, ProcessGroupError> {
        // Verify group exists
        let group_key = Self::group_key(tenant_id, group_name);
        if self.storage.get(&group_key).await?.is_none() {
            return Err(ProcessGroupError::GroupNotFound(group_name.to_string()));
        }

        // List all memberships
        let membership_prefix = Self::membership_prefix(tenant_id, group_name);
        let keys = self.storage.list(&membership_prefix).await?;

        let mut members = Vec::new();
        for key in keys {
            if let Some(bytes) = self.storage.get(&key).await? {
                if let Ok(membership) = GroupMembership::decode(&bytes[..]) {
                    members.push(membership.actor_id);
                }
            }
        }

        Ok(members)
    }

    /// Get local group members (this node only)
    ///
    /// ## Arguments
    /// - `group_name`: Group to query
    /// - `tenant_id`: Tenant for isolation
    ///
    /// ## Returns
    /// List of actor IDs in this group on current node
    ///
    /// ## Performance
    /// O(n) where n = local members only (faster than get_members)
    ///
    /// ## Errors
    /// - `GroupNotFound`: Group doesn't exist
    pub async fn get_local_members(
        &self,
        group_name: &str,
        tenant_id: &str,
    ) -> Result<Vec<ActorId>, ProcessGroupError> {
        // Verify group exists
        let group_key = Self::group_key(tenant_id, group_name);
        if self.storage.get(&group_key).await?.is_none() {
            return Err(ProcessGroupError::GroupNotFound(group_name.to_string()));
        }

        // List all memberships and filter by node_id
        let membership_prefix = Self::membership_prefix(tenant_id, group_name);
        let keys = self.storage.list(&membership_prefix).await?;

        let mut local_members = Vec::new();
        for key in keys {
            if let Some(bytes) = self.storage.get(&key).await? {
                if let Ok(membership) = GroupMembership::decode(&bytes[..]) {
                    if membership.node_id == self.node_id {
                        local_members.push(membership.actor_id);
                    }
                }
            }
        }

        Ok(local_members)
    }

    /// List all groups (optionally filtered)
    ///
    /// ## Arguments
    /// - `tenant_id`: Tenant to filter by
    ///
    /// ## Returns
    /// List of group names
    pub async fn list_groups(&self, tenant_id: &str) -> Result<Vec<String>, ProcessGroupError> {
        let prefix = format!("tenant:{}:group:", tenant_id);
        let keys = self.storage.list(&prefix).await?;

        let mut groups = Vec::new();
        for key in keys {
            // Extract group name from key
            // Format: tenant:{tenant_id}:group:{group_name}
            if let Some(group_name) = key.strip_prefix(&prefix) {
                // Skip member keys (they have :member: in them)
                if !group_name.contains(":member:") {
                    groups.push(group_name.to_string());
                }
            }
        }

        Ok(groups)
    }

    /// Publish message to all group members
    ///
    /// ## Arguments
    /// - `group_name`: Group to publish to
    /// - `tenant_id`: Tenant for isolation
    /// - `message`: Message bytes to broadcast
    ///
    /// ## Returns
    /// List of actor IDs that received the message
    ///
    /// ## Semantics
    /// - Message delivered to all current members
    /// - Returns member list for caller verification
    /// - Message delivery is best-effort (actor may be offline)
    ///
    /// ## Errors
    /// - `GroupNotFound`: Group doesn't exist
    ///
    /// ## Performance
    /// O(n) where n = total members across cluster
    pub async fn publish_to_group(
        &self,
        group_name: &str,
        tenant_id: &str,
        topic: Option<&str>,
        message: Vec<u8>,
    ) -> Result<Vec<ActorId>, ProcessGroupError> {
        let start = std::time::Instant::now();
        trace!("Publishing message to process group");

        // Verify group exists
        let group_key = Self::group_key(tenant_id, group_name);
        if self.storage.get(&group_key).await?.is_none() {
            metrics::counter!("plexspaces_process_groups_publish_errors_total", "error" => "group_not_found").increment(1);
            warn!("Group not found: {}", group_name);
            return Err(ProcessGroupError::GroupNotFound(group_name.to_string()));
        }

        // Get all group memberships (for topic filtering)
        let membership_prefix = Self::membership_prefix(tenant_id, group_name);
        let keys = self.storage.list(&membership_prefix).await?;

        let mut recipients = Vec::new();
        for key in keys {
            if let Some(bytes) = self.storage.get(&key).await? {
                if let Ok(membership) = GroupMembership::decode(&bytes[..]) {
                    // Topic filtering: if topic specified, only include members subscribed to it
                    // If member has no topics (empty list), they receive all messages
                    let should_receive = if let Some(topic_filter) = topic {
                        // Topic specified: member must be subscribed to this topic OR have no topics (receive all)
                        membership.topics.is_empty() || membership.topics.contains(&topic_filter.to_string())
                    } else {
                        // No topic filter: all members receive
                        true
                    };

                    if should_receive {
                        recipients.push(membership.actor_id);
                    }
                }
            }
        }

        // Store message for each member to retrieve
        // Format: tenant:{tenant_id}:group:{group_name}:message:{message_id}
        let message_id = ulid::Ulid::new().to_string();
        let message_key = format!(
            "tenant:{}:group:{}:message:{}",
            tenant_id, group_name, message_id
        );

        // Store message metadata (recipients, payload, topic)
        #[derive(Debug)]
        struct BroadcastMessage {
            message_id: String,
            group_name: String,
            tenant_id: String,
            topic: Option<String>,
            recipients: Vec<String>,
            payload: Vec<u8>,
            published_at: i64,
        }

        let broadcast_msg = BroadcastMessage {
            message_id: message_id.clone(),
            group_name: group_name.to_string(),
            tenant_id: tenant_id.to_string(),
            topic: topic.map(|t| t.to_string()),
            recipients: recipients.clone(),
            payload: message,
            published_at: chrono::Utc::now().timestamp(),
        };

        // Serialize to bytes (simplified - in real implementation would use proto)
        let msg_bytes = format!(
            "{}|{}|{}|{:?}|{:?}|{:?}|{}",
            broadcast_msg.message_id,
            broadcast_msg.group_name,
            broadcast_msg.tenant_id,
            broadcast_msg.topic,
            broadcast_msg.recipients,
            broadcast_msg.payload,
            broadcast_msg.published_at
        )
        .into_bytes();

        // Store in KeyValueStore
        self.storage.put(&message_key, msg_bytes).await?;

        let duration = start.elapsed();
        let fanout_size = recipients.len();
        metrics::histogram!("plexspaces_process_groups_publish_duration_seconds").record(duration.as_secs_f64());
        metrics::histogram!("plexspaces_process_groups_fanout_size").record(fanout_size as f64);
        metrics::counter!("plexspaces_process_groups_publish_total", "group" => group_name.to_string(), "tenant" => tenant_id.to_string()).increment(1);
        debug!(
            duration_ms = duration.as_millis(),
            fanout_size = fanout_size,
            topic = ?topic,
            "Message published to process group"
        );

        Ok(recipients)
    }

    /// Update member count in group metadata
    async fn update_member_count(
        &self,
        tenant_id: &str,
        group_name: &str,
    ) -> Result<(), ProcessGroupError> {
        let group_key = Self::group_key(tenant_id, group_name);

        // Get group metadata
        let bytes = self
            .storage
            .get(&group_key)
            .await?
            .ok_or_else(|| ProcessGroupError::GroupNotFound(group_name.to_string()))?;

        let mut group = ProcessGroup::decode(&bytes[..])
            .map_err(|e| ProcessGroupError::SerializationError(e.to_string()))?;

        // Count members
        let membership_prefix = Self::membership_prefix(tenant_id, group_name);
        let member_keys = self.storage.list(&membership_prefix).await?;
        group.member_count = member_keys.len() as u32;

        // Update member_ids list
        group.member_ids.clear();
        for key in member_keys {
            if let Some(bytes) = self.storage.get(&key).await? {
                if let Ok(membership) = GroupMembership::decode(&bytes[..]) {
                    group.member_ids.push(membership.actor_id);
                }
            }
        }

        // Store updated group
        let value = group.encode_to_vec();
        self.storage.put(&group_key, value).await?;

        Ok(())
    }
}

// =============================================================================
// TESTS - Following TDD
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_keyvalue::InMemoryKVStore;

    // Test constants
    const TEST_TENANT: &str = "test-tenant";
    const TEST_NAMESPACE: &str = "test";
    const TEST_NODE_ID: &str = "test-node-1";

    /// Helper function to create test registry
    fn create_test_registry() -> ProcessGroupRegistry {
        let kv_store = Arc::new(InMemoryKVStore::new());
        ProcessGroupRegistry::new(TEST_NODE_ID, kv_store)
    }

    /// TEST 1: Can create empty group
    #[tokio::test]
    async fn test_create_empty_group() {
        let registry = create_test_registry();

        let group = registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        assert_eq!(group.group_name, "test-group");
        assert_eq!(group.tenant_id, TEST_TENANT);
        assert_eq!(group.namespace, TEST_NAMESPACE);
        assert_eq!(group.member_count, 0);
        assert!(group.member_ids.is_empty());
        assert!(group.created_at.is_some());
    }

    /// TEST 2: Cannot create duplicate group
    #[tokio::test]
    async fn test_cannot_create_duplicate_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let result = registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await;
        assert!(matches!(
            result,
            Err(ProcessGroupError::GroupAlreadyExists(_))
        ));
    }

    /// TEST 3: Can delete group
    #[tokio::test]
    async fn test_delete_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .delete_group("test-group", TEST_TENANT)
            .await
            .unwrap();

        // Verify group no longer exists
        let groups = registry.list_groups(TEST_TENANT).await.unwrap();
        assert!(!groups.contains(&"test-group".to_string()));
    }

    /// TEST 4: Delete is idempotent
    #[tokio::test]
    async fn test_delete_group_idempotent() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .delete_group("test-group", TEST_TENANT)
            .await
            .unwrap();

        // Delete again - should succeed (idempotent)
        let result = registry.delete_group("test-group", TEST_TENANT).await;
        assert!(result.is_ok());
    }

    /// TEST 5: Can join group
    #[tokio::test]
    async fn test_join_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0], "actor-1");
    }

    /// TEST 6: Cannot join non-existent group
    #[tokio::test]
    async fn test_cannot_join_nonexistent_group() {
        let registry = create_test_registry();

        let result = registry
            .join_group("nonexistent", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await;
        assert!(matches!(result, Err(ProcessGroupError::GroupNotFound(_))));
    }

    /// TEST 7: Can join same group multiple times (Erlang pg2 semantics)
    #[tokio::test]
    async fn test_multiple_joins() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Join 3 times
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();

        // Should still appear once in members list
        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0], "actor-1");
    }

    /// TEST 8: Can leave group
    #[tokio::test]
    async fn test_leave_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 0);
    }

    /// TEST 9: Leave decrements join_count (must leave equal times to joined)
    #[tokio::test]
    async fn test_leave_decrements_join_count() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Join 3 times
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();

        // Leave twice - should still be member
        registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();
        registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);

        // Leave third time - now fully removed
        registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 0);
    }

    /// TEST 10: Cannot leave group not joined
    #[tokio::test]
    async fn test_cannot_leave_unjoined_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let result = registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await;
        assert!(matches!(
            result,
            Err(ProcessGroupError::ActorNotInGroup { .. })
        ));
    }

    /// TEST 11: Can get all members
    #[tokio::test]
    async fn test_get_all_members() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-2".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-3".to_string(), vec![])
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 3);
        assert!(members.contains(&"actor-1".to_string()));
        assert!(members.contains(&"actor-2".to_string()));
        assert!(members.contains(&"actor-3".to_string()));
    }

    /// TEST 12: Can get local members only
    #[tokio::test]
    async fn test_get_local_members() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-2".to_string(), vec![])
            .await
            .unwrap();

        // All actors are local (same node_id)
        let local_members = registry
            .get_local_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(local_members.len(), 2);
    }

    /// TEST 13: Can list all groups
    #[tokio::test]
    async fn test_list_groups() {
        let registry = create_test_registry();

        registry
            .create_group("group-1", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .create_group("group-2", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .create_group("group-3", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let groups = registry.list_groups(TEST_TENANT).await.unwrap();
        assert_eq!(groups.len(), 3);
        assert!(groups.contains(&"group-1".to_string()));
        assert!(groups.contains(&"group-2".to_string()));
        assert!(groups.contains(&"group-3".to_string()));
    }

    /// TEST 14: Delete group removes all members
    #[tokio::test]
    async fn test_delete_group_removes_members() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-2".to_string(), vec![])
            .await
            .unwrap();

        registry
            .delete_group("test-group", TEST_TENANT)
            .await
            .unwrap();

        // Re-create group - should be empty
        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 0);
    }

    /// TEST 15: Member count updated on join/leave
    #[tokio::test]
    async fn test_member_count_tracking() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Join actors
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-2".to_string(), vec![])
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 2);

        // Leave one actor
        registry
            .leave_group("test-group", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();

        let members = registry
            .get_members("test-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 1);
    }

    /// TEST 16: Can publish message to group
    #[tokio::test]
    async fn test_publish_to_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-2".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("test-group", TEST_TENANT, &"actor-3".to_string(), vec![])
            .await
            .unwrap();

        // Publish message to group
        let message = b"Hello, group members!".to_vec();
        let recipients =         registry
            .publish_to_group("test-group", TEST_TENANT, None, message)
            .await
            .unwrap();

        // Should have published to all 3 members
        assert_eq!(recipients.len(), 3);
        assert!(recipients.contains(&"actor-1".to_string()));
        assert!(recipients.contains(&"actor-2".to_string()));
        assert!(recipients.contains(&"actor-3".to_string()));
    }

    /// TEST 17: Cannot publish to non-existent group
    #[tokio::test]
    async fn test_cannot_publish_to_nonexistent_group() {
        let registry = create_test_registry();

        let message = b"Test message".to_vec();
        let result = registry
            .publish_to_group("nonexistent", TEST_TENANT, None, message)
            .await;
        assert!(matches!(result, Err(ProcessGroupError::GroupNotFound(_))));
    }

    /// TEST 18: Publish to empty group returns empty list
    #[tokio::test]
    async fn test_publish_to_empty_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let message = b"Test message".to_vec();
        let recipients =         registry
            .publish_to_group("test-group", TEST_TENANT, None, message)
            .await
            .unwrap();

        // Empty group returns empty recipient list
        assert_eq!(recipients.len(), 0);
    }

    /// TEST 19: Topic-based pub/sub - actors subscribe to specific topics
    #[tokio::test]
    async fn test_topic_based_pubsub() {
        let registry = create_test_registry();

        registry
            .create_group("events", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Actor 1 subscribes to "user.login" topic
        registry
            .join_group("events", TEST_TENANT, &"actor-1".to_string(), vec!["user.login".to_string()])
            .await
            .unwrap();

        // Actor 2 subscribes to "user.logout" topic
        registry
            .join_group("events", TEST_TENANT, &"actor-2".to_string(), vec!["user.logout".to_string()])
            .await
            .unwrap();

        // Actor 3 subscribes to both topics
        registry
            .join_group("events", TEST_TENANT, &"actor-3".to_string(), vec!["user.login".to_string(), "user.logout".to_string()])
            .await
            .unwrap();

        // Actor 4 subscribes to all topics (empty list)
        registry
            .join_group("events", TEST_TENANT, &"actor-4".to_string(), vec![])
            .await
            .unwrap();

        // Publish to "user.login" topic - should reach actor-1, actor-3, and actor-4
        let message = b"User logged in".to_vec();
        let recipients = registry
            .publish_to_group("events", TEST_TENANT, Some("user.login"), message)
            .await
            .unwrap();

        assert_eq!(recipients.len(), 3);
        assert!(recipients.contains(&"actor-1".to_string()));
        assert!(recipients.contains(&"actor-3".to_string()));
        assert!(recipients.contains(&"actor-4".to_string()));
        assert!(!recipients.contains(&"actor-2".to_string()));

        // Publish to "user.logout" topic - should reach actor-2, actor-3, and actor-4
        let message = b"User logged out".to_vec();
        let recipients = registry
            .publish_to_group("events", TEST_TENANT, Some("user.logout"), message)
            .await
            .unwrap();

        assert_eq!(recipients.len(), 3);
        assert!(recipients.contains(&"actor-2".to_string()));
        assert!(recipients.contains(&"actor-3".to_string()));
        assert!(recipients.contains(&"actor-4".to_string()));
        assert!(!recipients.contains(&"actor-1".to_string()));

        // Publish without topic (broadcast to all) - should reach all actors
        let message = b"System message".to_vec();
        let recipients = registry
            .publish_to_group("events", TEST_TENANT, None, message)
            .await
            .unwrap();

        assert_eq!(recipients.len(), 4);
    }

    /// TEST 20: Topic merging on multiple joins
    #[tokio::test]
    async fn test_topic_merging_on_multiple_joins() {
        let registry = create_test_registry();

        registry
            .create_group("events", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Join with topic "user.login"
        registry
            .join_group("events", TEST_TENANT, &"actor-1".to_string(), vec!["user.login".to_string()])
            .await
            .unwrap();

        // Join again with topic "user.logout" - should merge topics
        registry
            .join_group("events", TEST_TENANT, &"actor-1".to_string(), vec!["user.logout".to_string()])
            .await
            .unwrap();

        // Actor should now receive both topics
        let message1 = b"Login event".to_vec();
        let recipients1 = registry
            .publish_to_group("events", TEST_TENANT, Some("user.login"), message1)
            .await
            .unwrap();
        assert!(recipients1.contains(&"actor-1".to_string()));

        let message2 = b"Logout event".to_vec();
        let recipients2 = registry
            .publish_to_group("events", TEST_TENANT, Some("user.logout"), message2)
            .await
            .unwrap();
        assert!(recipients2.contains(&"actor-1".to_string()));
    }

    /// TEST 21: Large cluster performance - many members
    #[tokio::test]
    async fn test_large_cluster_performance() {
        let registry = create_test_registry();

        registry
            .create_group("large-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Add 100 actors
        for i in 0..100 {
            registry
                .join_group("large-group", TEST_TENANT, &format!("actor-{}", i), vec![])
                .await
                .unwrap();
        }

        // Verify all members
        let members = registry
            .get_members("large-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 100);

        // Publish to all - should reach all 100
        let message = b"Broadcast message".to_vec();
        let recipients = registry
            .publish_to_group("large-group", TEST_TENANT, None, message)
            .await
            .unwrap();
        assert_eq!(recipients.len(), 100);
    }

    /// TEST 22: Sequential operations (concurrent testing simplified)
    #[tokio::test]
    async fn test_sequential_operations() {
        let registry = create_test_registry();

        registry
            .create_group("sequential-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Sequential joins (simulating concurrent behavior)
        for i in 0..10 {
            registry
                .join_group("sequential-group", TEST_TENANT, &format!("actor-{}", i), vec![])
                .await
                .unwrap();
        }

        // Verify all members
        let members = registry
            .get_members("sequential-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 10);

        // Sequential leaves
        for i in 0..10 {
            registry
                .leave_group("sequential-group", TEST_TENANT, &format!("actor-{}", i))
                .await
                .unwrap();
        }

        // Verify all removed
        let members = registry
            .get_members("sequential-group", TEST_TENANT)
            .await
            .unwrap();
        assert_eq!(members.len(), 0);
    }

    /// TEST 23: Multi-tenant isolation
    #[tokio::test]
    async fn test_multi_tenant_isolation() {
        let registry = create_test_registry();

        // Create same group name in different tenants
        registry
            .create_group("shared-group", "tenant-1", TEST_NAMESPACE)
            .await
            .unwrap();
        registry
            .create_group("shared-group", "tenant-2", TEST_NAMESPACE)
            .await
            .unwrap();

        // Join different tenants
        registry
            .join_group("shared-group", "tenant-1", &"actor-1".to_string(), vec![])
            .await
            .unwrap();
        registry
            .join_group("shared-group", "tenant-2", &"actor-2".to_string(), vec![])
            .await
            .unwrap();

        // Verify isolation
        let members_1 = registry
            .get_members("shared-group", "tenant-1")
            .await
            .unwrap();
        assert_eq!(members_1.len(), 1);
        assert_eq!(members_1[0], "actor-1");

        let members_2 = registry
            .get_members("shared-group", "tenant-2")
            .await
            .unwrap();
        assert_eq!(members_2.len(), 1);
        assert_eq!(members_2[0], "actor-2");
    }

    /// TEST 24: Topic filtering edge cases
    #[tokio::test]
    async fn test_topic_filtering_edge_cases() {
        let registry = create_test_registry();

        registry
            .create_group("topics", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Actor with empty topics (receives all)
        registry
            .join_group("topics", TEST_TENANT, &"actor-all".to_string(), vec![])
            .await
            .unwrap();

        // Actor with specific topic
        registry
            .join_group("topics", TEST_TENANT, &"actor-specific".to_string(), vec!["topic1".to_string()])
            .await
            .unwrap();

        // Publish to non-existent topic - only actor-all receives
        let message = b"Message".to_vec();
        let recipients = registry
            .publish_to_group("topics", TEST_TENANT, Some("nonexistent"), message)
            .await
            .unwrap();
        assert_eq!(recipients.len(), 1);
        assert_eq!(recipients[0], "actor-all");

        // Publish to topic1 - both receive
        let message = b"Message".to_vec();
        let recipients = registry
            .publish_to_group("topics", TEST_TENANT, Some("topic1"), message)
            .await
            .unwrap();
        assert_eq!(recipients.len(), 2);
        assert!(recipients.contains(&"actor-all".to_string()));
        assert!(recipients.contains(&"actor-specific".to_string()));
    }

    /// TEST 25: Error paths - get_members on non-existent group
    #[tokio::test]
    async fn test_get_members_nonexistent_group() {
        let registry = create_test_registry();

        let result = registry.get_members("nonexistent", TEST_TENANT).await;
        assert!(matches!(result, Err(ProcessGroupError::GroupNotFound(_))));
    }

    /// TEST 26: Error paths - get_local_members on non-existent group
    #[tokio::test]
    async fn test_get_local_members_nonexistent_group() {
        let registry = create_test_registry();

        let result = registry.get_local_members("nonexistent", TEST_TENANT).await;
        assert!(matches!(result, Err(ProcessGroupError::GroupNotFound(_))));
    }

    /// TEST 27: Error paths - leave non-existent group
    #[tokio::test]
    async fn test_leave_nonexistent_group() {
        let registry = create_test_registry();

        registry
            .create_group("test-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let result = registry
            .leave_group("test-group", TEST_TENANT, &"nonexistent-actor".to_string())
            .await;
        assert!(matches!(result, Err(ProcessGroupError::ActorNotInGroup { .. })));
    }

    /// TEST 28: Member count accuracy after multiple operations
    #[tokio::test]
    async fn test_member_count_accuracy() {
        let registry = create_test_registry();

        registry
            .create_group("count-test", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        // Join 3 actors
        for i in 1..=3 {
            registry
                .join_group("count-test", TEST_TENANT, &format!("actor-{}", i), vec![])
                .await
                .unwrap();
        }

        // Verify count
        let members = registry.get_members("count-test", TEST_TENANT).await.unwrap();
        assert_eq!(members.len(), 3);

        // Leave one
        registry
            .leave_group("count-test", TEST_TENANT, &"actor-1".to_string())
            .await
            .unwrap();

        // Verify count updated
        let members = registry.get_members("count-test", TEST_TENANT).await.unwrap();
        assert_eq!(members.len(), 2);
    }

    /// TEST 29: List groups with multiple groups
    #[tokio::test]
    async fn test_list_groups_multiple() {
        let registry = create_test_registry();

        // Create multiple groups
        for i in 1..=5 {
            registry
                .create_group(&format!("group-{}", i), TEST_TENANT, TEST_NAMESPACE)
                .await
                .unwrap();
        }

        let groups = registry.list_groups(TEST_TENANT).await.unwrap();
        assert_eq!(groups.len(), 5);
        for i in 1..=5 {
            assert!(groups.contains(&format!("group-{}", i)));
        }
    }

    /// TEST 30: Publish with empty group (no members)
    #[tokio::test]
    async fn test_publish_empty_group() {
        let registry = create_test_registry();

        registry
            .create_group("empty-group", TEST_TENANT, TEST_NAMESPACE)
            .await
            .unwrap();

        let message = b"Message".to_vec();
        let recipients = registry
            .publish_to_group("empty-group", TEST_TENANT, None, message)
            .await
            .unwrap();

        assert_eq!(recipients.len(), 0);
    }
}
