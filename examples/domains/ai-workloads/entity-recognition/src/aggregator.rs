// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Aggregator actor: Aggregates results (CPU-intensive).

use crate::processor;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
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
                
                // Send reply via context
                if let Ok(reply_data) = serde_json::to_vec(&response) {
                    let reply = Message::new(reply_data);
                    let _ = if let Some(sender_id) = &msg.sender { ctx.send_reply(msg.correlation_id.as_deref(), sender_id, msg.receiver.clone(), reply).await } else { Ok(()) }; // Ignore reply errors for now
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

