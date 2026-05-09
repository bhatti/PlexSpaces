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

//! `ChannelService` trait — moved here from plexspaces-actor to break the
//! actor → journaling → mailbox → channel → actor cycle.
//!
//! `channel` depends only on `plexspaces-service-traits` for this trait,
//! and `plexspaces-actor` re-exports it from here.

use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_common::RequestContext;
use plexspaces_proto::common::v1::Message;

/// Trait for channel operations (queue and topic patterns)
///
/// ## Purpose
/// Provides unified interface for channel operations (queue and topic patterns).
/// This is a Rust trait (not in proto), following proto-first principle.
///
/// ## Design
/// Channels provide two main patterns:
/// - **Queue**: Load-balanced to one consumer (work distribution)
/// - **Topic**: All subscribers receive (pub/sub)
#[async_trait]
pub trait ChannelService: Send + Sync {
    /// Send message to queue (load-balanced to one consumer)
    async fn send_to_queue(
        &self,
        queue_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    /// Publish message to topic (all subscribers receive)
    async fn publish_to_topic(
        &self,
        topic_name: &str,
        message: Message,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>>;

    /// Subscribe to topic (returns stream of messages)
    async fn subscribe_to_topic(
        &self,
        topic_name: &str,
    ) -> Result<BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>>;

    /// Receive from queue (blocking until message available)
    async fn receive_from_queue(
        &self,
        queue_name: &str,
        timeout: Option<std::time::Duration>,
    ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>>;
}

/// Trait for process group operations (Erlang pg/pg2-style pub/sub)
///
/// ## Purpose
/// Moved here from plexspaces-actor to break the
/// actor → journaling → mailbox → channel → actor cycle.
/// `channel` depends on `plexspaces-service-traits` for this trait,
/// and `plexspaces-actor` re-exports it from here.
#[async_trait]
pub trait ProcessGroupService: Send + Sync {
    /// Create a new process group
    async fn create_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Delete a process group
    async fn delete_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Join a process group
    async fn join_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
        topics: Vec<String>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Leave a process group
    async fn leave_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        actor_id: &str,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Get all members of a group (cluster-wide)
    async fn get_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;

    /// Get local members of a group (this node only)
    async fn get_local_members(
        &self,
        ctx: &RequestContext,
        group_name: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;

    /// List all groups for tenant
    async fn list_groups(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>>;

    /// Publish message to group members
    async fn publish_to_group(
        &self,
        ctx: &RequestContext,
        group_name: &str,
        topic: Option<&str>,
        message: Message,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>>;
}
