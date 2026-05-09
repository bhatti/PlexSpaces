// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Aggregator actor: Aggregates results (CPU-intensive).

use crate::processor;
use plexspaces_actor::{Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType};
use plexspaces_sdk::{new_message, Message};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

/// Aggregator actor behavior
pub struct AggregatorBehavior {
    /// Collected entities
    entities: HashMap<String, Vec<processor::Entity>>,
    /// Expected document count
    expected_count: u32,
}

impl AggregatorBehavior {
    pub fn new(expected_count: u32) -> Self {
        Self {
            entities: HashMap::new(),
            expected_count,
        }
    }
}

#[async_trait::async_trait]
impl ActorTrait for AggregatorBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse request
        let request: AggregatorRequest = match serde_json::from_slice(&msg.payload) {
            Ok(req) => req,
            Err(_) => {
                return Err(BehaviorError::UnsupportedMessage);
            }
        };

        match request {
            AggregatorRequest::AddEntities { document_id, entities } => {
                info!("Aggregator: Adding entities from document {}", document_id);
                
                self.entities.insert(document_id, entities);
                
                // Check if all documents processed
                let response = if self.entities.len() as u32 >= self.expected_count {
                    // Aggregate all entities
                    let mut all_entities = Vec::new();
                    for entities in self.entities.values() {
                        all_entities.extend(entities.clone());
                    }
                    
                    AggregatorResponse::Complete {
                        total_entities: all_entities.len(),
                        entities: all_entities,
                    }
                } else {
                    AggregatorResponse::Partial {
                        processed: self.entities.len() as u32,
                        expected: self.expected_count,
                    }
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

/// Aggregator request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregatorRequest {
    AddEntities { document_id: String, entities: Vec<processor::Entity> },
}

/// Aggregator response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AggregatorResponse {
    Partial { processed: u32, expected: u32 },
    Complete { total_entities: usize, entities: Vec<processor::Entity> },
}
