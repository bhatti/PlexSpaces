// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Processor actor: Runs LLM inference (GPU-intensive).

use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
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
impl ActorBehavior for ProcessorBehavior {
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
                
                // Send reply via context
                if let Ok(reply_data) = serde_json::to_vec(&response) {
                    let reply = Message::new(reply_data);
                    let _ = ctx.reply(reply).await; // Ignore reply errors for now
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

