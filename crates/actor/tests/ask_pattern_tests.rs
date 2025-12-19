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

//! Tests for ask pattern (request-reply) with GenServerBehavior

use plexspaces_actor::ActorBuilder;
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorError, Actor, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, debug, warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
enum TestMessage {
    Request(String),
    Reply(String),
}

struct TestGenServer {
    state: String,
}

impl TestGenServer {
    fn new() -> Self {
        Self {
            state: "initial".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for TestGenServer {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for TestGenServer {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: TestMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match request {
            TestMessage::Request(data) => {
                self.state = data.clone();
                info!("✅ GenServer: Received request: {}, state now: {}", data, self.state);
                let reply = TestMessage::Reply(format!("Echo: {}", data));
                let reply_msg = Message::new(serde_json::to_vec(&reply).unwrap());
                info!("✅ GenServer: Created reply message");
                
                // Send reply using ActorService
                if let Some(sender_id) = &msg.sender {
                    let actor_service = ctx.service_locator.get_actor_service().await
                        .ok_or_else(|| BehaviorError::ProcessingError("ActorService not available".to_string()))?;
                    
                    let mut reply_msg_final = reply_msg;
                    reply_msg_final.receiver = sender_id.clone();
                    reply_msg_final.sender = Some(msg.receiver.clone());
                    if let Some(corr_id) = &msg.correlation_id {
                        reply_msg_final.correlation_id = Some(corr_id.clone());
                    }
                    
                    actor_service.send(sender_id, reply_msg_final).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                Ok(())
            }
            _ => {
                warn!("❌ GenServer: Invalid message type");
                Err(BehaviorError::ProcessingError("Invalid message".to_string()))
            }
        }
    }
}

#[tokio::test]
async fn test_ask_pattern_basic() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // Create node
    let node = NodeBuilder::new("test-node-ask")
        .build();

    // Create and spawn actor
    let actor_id = "test-server@test-node-ask".to_string();
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(TestGenServer::new());
    let ctx = plexspaces_core::RequestContext::internal();
    let _core_ref = plexspaces_actor::ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .with_namespace("default".to_string())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .unwrap();
    
    // Get ActorRef using remote() pointing to local node
    let actor_ref = plexspaces_actor::ActorRef::remote(
        actor_id,
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    // Wait for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test ask pattern
    let request = TestMessage::Request("hello".to_string());
    let msg = Message::new(serde_json::to_vec(&request).unwrap())
        .with_message_type("call".to_string());
    
    debug!("Sending ask request...");
    let result = actor_ref
        .ask(msg, Duration::from_secs(5))
        .await;
    
    match result {
        Ok(reply) => {
            debug!("Received reply: {:?}", reply.payload());
            let reply_msg: TestMessage = serde_json::from_slice(reply.payload())
                .unwrap();
            match reply_msg {
                TestMessage::Reply(data) => {
                    assert_eq!(data, "Echo: hello");
                    debug!("✅ Ask pattern test passed!");
                }
                _ => panic!("Unexpected reply type"),
            }
        }
        Err(e) => {
            panic!("Ask pattern failed: {:?}", e);
        }
    }
}

#[tokio::test]
async fn test_ask_pattern_multiple_requests() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // Create node
    let node = NodeBuilder::new("test-node-ask-multi")
        .build();

    // Create and spawn actor
    let actor_id = "test-server-multi@test-node-ask-multi".to_string();
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(TestGenServer::new());
    let ctx = plexspaces_core::RequestContext::internal();
    let _core_ref = plexspaces_actor::ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .with_namespace("default".to_string())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .unwrap();
    
    // Get ActorRef using remote() pointing to local node
    let actor_ref = plexspaces_actor::ActorRef::remote(
        actor_id,
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    // Wait for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Send multiple requests
    for i in 0..5 {
        let request = TestMessage::Request(format!("request-{}", i));
        let msg = Message::new(serde_json::to_vec(&request).unwrap())
            .with_message_type("call".to_string());
        
        let result = actor_ref
            .ask(msg, Duration::from_secs(5))
            .await;
        
        match result {
            Ok(reply) => {
                let reply_msg: TestMessage = serde_json::from_slice(reply.payload())
                    .unwrap();
                match reply_msg {
                    TestMessage::Reply(data) => {
                        assert_eq!(data, format!("Echo: request-{}", i));
                    }
                    _ => panic!("Unexpected reply type"),
                }
            }
            Err(e) => {
                panic!("Ask pattern failed for request {}: {:?}", i, e);
            }
        }
    }
    
    debug!("✅ Multiple ask pattern test passed!");
}
