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

//! MessageSender trait for sending messages to actors.
//!
//! # Purpose
//! Provides a simple trait-based interface for sending messages to actors, inspired by Orleans grains.

use crate::ActorStateHandle;
use async_trait::async_trait;
use plexspaces_common::RequestContext;
use plexspaces_proto::common::v1::Message;
use std::any::Any;
use std::sync::Arc;

/// MessageSender trait — interface for sending messages to actors.
///
/// # Purpose
/// Trait for sending messages to actors.
/// This is the "sender" side — use this to send messages to actors.
#[async_trait]
pub trait MessageSender: Send + Sync + Any {
    /// Send a message to the actor (fire-and-forget).
    async fn tell(
        &self,
        ctx: &RequestContext,
        message: Message,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;

    /// Send a message and wait for a reply (request-reply pattern).
    async fn ask(
        &self,
        _ctx: &RequestContext,
        _message: Message,
        _timeout: std::time::Duration,
    ) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        Err("ask() not supported by this MessageSender implementation".into())
    }

    /// Returns the actor ID when this sender is an actor reference (e.g. from spawn).
    fn actor_id(&self) -> Option<String> {
        None
    }

    /// Returns the tenant_id carried by this sender when available.
    fn tenant_id(&self) -> Option<&str> {
        None
    }

    /// Returns the namespace carried by this sender when available.
    fn namespace(&self) -> Option<&str> {
        None
    }

    /// Returns the actor type carried by this sender when available.
    fn actor_type(&self) -> Option<String> {
        None
    }

    /// Updates the actor type carried by this sender when supported.
    async fn set_actor_type(&self, _actor_type: Option<String>) {}

    /// Returns the runtime behavior kind carried by this sender when available.
    fn behavior_kind(&self) -> Option<String> {
        None
    }

    /// Updates the runtime behavior kind carried by this sender when supported.
    async fn set_behavior_kind(&self, _behavior_kind: Option<String>) {}

    /// Returns local lifecycle/state access when this sender points to a local actor runtime.
    fn local_state_handle(&self) -> Option<Arc<dyn ActorStateHandle>> {
        None
    }

    /// Updates the local lifecycle/state handle carried by this sender when supported.
    async fn set_local_state_handle(&self, _handle: Option<Arc<dyn ActorStateHandle>>) {}

    /// Returns the creation timestamp recorded when this actor was spawned.
    fn created_at(&self) -> Option<prost_types::Timestamp> {
        None
    }

    /// Returns a reference to `self` as `dyn Any`.
    fn as_any(&self) -> &dyn Any;
}
