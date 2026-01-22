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
use plexspaces_core::{ActorContext, BehaviorError, Actor, BehaviorType, Message};
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
        let request: TestMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        let reply_msg = match request {
            TestMessage::Request(data) => {
                self.state = data.clone();
                info!("✅ GenServer: Received request: {}, state now: {}", data, self.state);
                Message {
                    id: ulid::Ulid::new().to_string(),
                    payload: serde_json::to_vec(&TestMessage::Reply(format!("Echo: {}", data))).unwrap(),
                    ..Default::default()
                }
            }
            _ => return Err(BehaviorError::ProcessingError("Invalid message".to_string())),
        };
        
        // Send reply using ctx.send_reply() (matches working tests)
        if !msg.sender_id.is_empty() {
            let correlation = if msg.correlation_id.is_empty() { None } else { Some(msg.correlation_id.as_str()) };
            ctx.send_reply(
                correlation,
                &msg.sender_id,
                msg.receiver_id.clone(),
                reply_msg,
            ).await
                .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }
        Ok(())
    }
}
