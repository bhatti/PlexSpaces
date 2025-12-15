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

//! Regular Actor Wrapper - implements Actor trait for regular (non-virtual) actors
//!
//! ## Purpose
//! Wraps regular actors to implement the Actor trait. Uses ActorRef's tell() method
//! which already handles local/remote routing correctly.

use async_trait::async_trait;
use std::sync::Arc;
use plexspaces_core::{MessageSender, ActorId, ServiceLocator};
use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{Mailbox, Message};

/// Regular Actor Wrapper - implements Actor trait for regular actors
///
/// ## Purpose
/// Wraps a regular actor to implement the Actor trait. Uses ActorRef::tell() which
/// handles mailbox insertion (mailbox is private to ActorRef).
pub struct RegularActorWrapper {
    /// Actor ID
    actor_id: ActorId,
    /// Actor's mailbox (for creating ActorRef)
    mailbox: Arc<Mailbox>,
    /// ServiceLocator for creating ActorRef
    service_locator: Arc<ServiceLocator>,
}

impl RegularActorWrapper {
    /// Create a new RegularActorWrapper
    pub fn new(actor_id: ActorId, mailbox: Arc<Mailbox>, service_locator: Arc<ServiceLocator>) -> Self {
        Self {
            actor_id,
            mailbox,
            service_locator,
        }
    }
}

#[async_trait]
impl MessageSender for RegularActorWrapper {
    async fn tell(&self, message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Regular actor - use ActorRef::tell() which handles mailbox insertion
        let actor_ref = ActorRef::local(
            self.actor_id.clone(),
            self.mailbox.clone(),
            self.service_locator.clone(),
        );
        actor_ref.tell(message).await
            .map_err(|e| format!("ActorRef::tell() failed: {}", e).into())
    }
}
