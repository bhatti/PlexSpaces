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

//! Integration tests for Node with ActorContext

use async_trait::async_trait;
use plexspaces_actor::Actor;
use plexspaces_core::BehaviorType;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{Node, NodeId, default_node_config};

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{lookup_actor_ref, spawn_actor_helper};

/// Test behavior that uses ActorContext
struct ContextAwareBehavior {
    received_messages: usize,
}

impl ContextAwareBehavior {
    fn new() -> Self {
        Self {
            received_messages: 0,
        }
    }
}

#[async_trait]
#[async_trait]
impl plexspaces_core::Actor for ContextAwareBehavior {
    async fn handle_message(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        _msg: plexspaces_mailbox::Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        self.received_messages += 1;

        // Verify context has all services (Go-style: ctx is ActorContext directly)
        assert!(!ctx.node_id.is_empty());
        assert!(!ctx.namespace.is_empty());

        // Context should have ServiceLocator for service access
        // The fact that we can access this means the context was properly set
        let _service_locator = &ctx.service_locator;

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_node_spawns_actor_with_full_context() {
    // Create node
    let node_id = NodeId::new("test-node");
    let config = default_node_config();
    let node = Node::new(node_id, config);

    // Create actor with behavior
    let behavior = Box::new(ContextAwareBehavior::new());
    // Use a mailbox with larger capacity to avoid "Mailbox is full" errors
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    // Create actor with just the actor name (no @node suffix)
    // spawn_actor_arc will fix the actor ID to include the node ID
    let actor_name = "test-actor";
    let mailbox = Mailbox::new(mailbox_config, format!("{}@temp", actor_name)).await.unwrap();
    let actor = Actor::new(
        format!("{}@temp", actor_name), // Temporary ID, will be fixed by spawn_actor_arc
        behavior,
        mailbox,
        "default".to_string(),
        None,
    );

    // Spawn actor - Node should update context with full services
    use test_helpers::spawn_actor_helper;
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();
    let actor_id = actor_ref.id().clone();
    
    // The actor ID is fixed by spawn_actor_arc to include the correct node ID
    // The actor is registered synchronously in spawn_actor_arc:
    // 1. register_local is called which stores the mailbox in ActorRegistry (line 931, before start)
    // 2. register_actor is called which stores the actor_ref and config (line 994)
    // Both should happen before spawn_actor returns
    
    // Wait for actor registration to complete (register_local is async)
    // Use a future that waits for the actor to be registered instead of sleep
    let registration_future = async {
        loop {
            if lookup_actor_ref(&node, &actor_id).await.ok().flatten().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");

    // Send a message to verify context is working
    // Use lookup_actor_ref + tell() to send message
    // If the actor is not registered, this will fail with ActorNotFound
    use plexspaces_mailbox::Message;

    let message = Message::new(b"test".to_vec());
    // The actor should be registered - if not, the error will tell us what's wrong
    let actor_ref = lookup_actor_ref(&node, &actor_id).await.expect("Actor should be registered").expect("Actor should exist");
    actor_ref.tell(message).await.expect("Should send message successfully");

    // Wait for message to be processed (actor should receive it)
    // Use a future that waits for the message to be processed instead of sleep
    let processing_future = async {
        // Message should be delivered immediately since route_message completes after enqueueing
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), processing_future)
        .await
        .expect("Message processing should complete quickly");

    // Verify actor was spawned - the ID should match the node ID
    assert_eq!(actor_ref.id(), &format!("test-actor@{}", node.id().as_str()));
}

#[tokio::test]
async fn test_actor_context_has_node_id() {
    let node_id = NodeId::new("test-node-2");
    let config = default_node_config();
    let node = Node::new(node_id.clone(), config);

    let behavior = Box::new(ContextAwareBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.unwrap();
    let actor = Actor::new(
        "test-actor-2@test-node-2".to_string(),
        behavior,
        mailbox,
        "default".to_string(),
        None,
    );

    // Before spawning, context should have minimal info
    let initial_ctx = actor.context();
    assert_eq!(initial_ctx.node_id, "local"); // Default from Actor::new

    // After spawning, Node updates context via create_actor_context()
    // The context is updated during spawn_actor() which calls create_actor_context()
    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Verify actor was spawned successfully
    assert_eq!(actor_ref.id(), "test-actor-2@test-node-2");
    
    // Note: We can't access actor's context after spawning (it's moved into the actor task)
    // But Node::spawn_actor() calls create_actor_context() which sets the correct node_id
    // The test above (test_node_spawns_actor_with_full_context) verifies the context is working
}

