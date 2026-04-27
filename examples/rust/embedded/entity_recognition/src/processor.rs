// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Processor actor: Runs LLM inference (GPU-intensive).

use plexspaces_core::{Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType};
use plexspaces_sdk::{new_message, Message};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Processor actor behavior
pub struct ProcessorBehavior;

impl ProcessorBehavior {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait::async_trait]
impl ActorTrait for ProcessorBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse request
        let request: ProcessorRequest = match serde_json::from_slice(&msg.payload) {
            Ok(req) => req,
            Err(_) => {
                return Err(BehaviorError::UnsupportedMessage);
            }
        };

        match request {
            ProcessorRequest::ProcessDocument { document_id, content: _content } => {
                info!("Processor: Processing document {} (GPU-intensive)", document_id);
                
                // Simulate GPU-intensive LLM inference
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                
                // Extract entities (mock implementation)
                let entities = vec![
                    Entity { entity_type: "PERSON".to_string(), value: "John Doe".to_string() },
                    Entity { entity_type: "ORG".to_string(), value: "Acme Corp".to_string() },
                    Entity { entity_type: "LOCATION".to_string(), value: "New York".to_string() },
                ];
                
                let response = ProcessorResponse::Processed {
                    document_id,
                    entities,
                };
                
                if !msg.sender_id.is_empty() {
                    let reply_value = serde_json::to_value(&response)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize reply: {}", e)))?;
                    let reply = new_message("reply", reply_value);
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    ctx.send_reply(correlation_id, &msg.sender_id, msg.receiver_id.clone().into(), reply)
                        .await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                
                Ok(())
            }
        }
    }
}

/// Processor request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessorRequest {
    ProcessDocument { document_id: String, content: String },
}

/// Processor response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProcessorResponse {
    Processed { document_id: String, entities: Vec<Entity> },
}

/// Extracted entity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_type: String,
    pub value: String,
}
