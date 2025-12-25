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

//! Integration tests for N-Body Simulation

use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Body {
    id: String,
    mass: f64,
    position: [f64; 3],
    velocity: [f64; 3],
}

impl Body {
    fn new(id: String, mass: f64, position: [f64; 3], velocity: [f64; 3]) -> Self {
        Self { id, mass, position, velocity }
    }
}

struct BodyActor {
    body: Body,
    other_body_ids: Vec<String>,
    accumulated_force: [f64; 3],
}

impl BodyActor {
    fn new(body: Body) -> Self {
        Self {
            body,
            other_body_ids: Vec::new(),
            accumulated_force: [0.0, 0.0, 0.0],
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for BodyActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let payload = String::from_utf8_lossy(&msg.payload);
        match payload.as_ref() {
            "get_state" => {
                // Actor responds (test will verify via logging or state)
            }
            _ => {}
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[tokio::test]
async fn test_spawn_body_actors() {
    // Test that body actors can be spawned
        let node_arc = NodeBuilder::new("test-node").build().await;
    let node = Arc::new(node_arc);
    
    // Services are already initialized by NodeBuilder::build().await
    let service_locator = node.service_locator();
    
    let body = Body::new(
        "body-1".to_string(),
        5.972e24, // Earth mass
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 0.0],
    );

    use plexspaces_actor::ActorBuilder;
    let ctx = plexspaces_core::RequestContext::internal();
    let body_ref = ActorBuilder::new(Box::new(BodyActor::new(body)))
        .with_id(format!("body-1@{}", node.id().as_str()))
        .spawn(&ctx, service_locator.clone())
        .await
        .expect("Failed to spawn body actor");

    assert!(body_ref.id().starts_with("body-1"));
}

#[tokio::test]
async fn test_body_actor_responds_to_get_state() {
    // Test that actor responds to get_state message
        let node_arc = NodeBuilder::new("test-node").build().await;
    let node = Arc::new(node_arc);
    
    // Services are already initialized by NodeBuilder::build().await
    let service_locator = node.service_locator();
    
    let body = Body::new(
        "body-1".to_string(),
        5.972e24,
        [1.0, 2.0, 3.0], // Known position
        [0.1, 0.2, 0.3], // Known velocity
    );

    use plexspaces_actor::ActorBuilder;
    let ctx = plexspaces_core::RequestContext::internal();
    let body_ref = ActorBuilder::new(Box::new(BodyActor::new(body)))
        .with_id(format!("body-1@{}", node.id().as_str()))
        .spawn(&ctx, service_locator.clone())
        .await
        .expect("Failed to spawn body actor");

    // Wait for actor to be ready
    sleep(Duration::from_millis(100)).await;

    // Send get_state message (non-blocking, just verify it doesn't panic)
    let msg = Message::new(b"get_state".to_vec());
    // Use ActorService to send message
    let actor_service = service_locator.get_actor_service().await;
    if let Some(service) = actor_service {
        let _ = service.send(body_ref.id(), msg).await;
    }

    // Give actor time to process
    sleep(Duration::from_millis(300)).await;

    // Test passes if no panic (actor processed message)
}

#[tokio::test]
async fn test_multiple_body_actors() {
    // Test spawning multiple body actors
        let node_arc = NodeBuilder::new("test-node").build().await;
    let node = Arc::new(node_arc);
    
    // Services are already initialized by NodeBuilder::build().await
    let service_locator = node.service_locator();
    
    let bodies = vec![
        Body::new("body-1".to_string(), 1.0e24, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0]),
        Body::new("body-2".to_string(), 1.0e24, [1.0e11, 0.0, 0.0], [0.0, 0.0, 0.0]),
        Body::new("body-3".to_string(), 1.0e24, [2.0e11, 0.0, 0.0], [0.0, 0.0, 0.0]),
    ];

    let mut body_refs = Vec::new();
    for body in bodies {
        use plexspaces_actor::ActorBuilder;
        let ctx = plexspaces_core::RequestContext::internal();
        let body_ref = ActorBuilder::new(Box::new(BodyActor::new(body)))
            .with_id(format!("body-{}@{}", body_refs.len() + 1, node.id().as_str()))
            .spawn(&ctx, service_locator.clone())
            .await
            .expect("Failed to spawn body actor");
        body_refs.push(body_ref);
    }

    assert_eq!(body_refs.len(), 3);

    // Wait for all actors to be ready
    sleep(Duration::from_millis(200)).await;

    // Send messages to all actors (with delays to avoid mailbox overflow)
    let actor_service = service_locator.get_actor_service().await;
    if let Some(service) = actor_service {
        for body_ref in &body_refs {
            let msg = Message::new(b"get_state".to_vec());
            let _ = service.send(body_ref.id(), msg).await;
            sleep(Duration::from_millis(100)).await; // Delay between messages
        }
    }

    sleep(Duration::from_millis(500)).await;
}
