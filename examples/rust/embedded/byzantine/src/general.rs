// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// General Actor for Byzantine Consensus
//
// Implements a general in the Byzantine Generals Problem.
// Each general votes and relays messages to reach consensus.

use async_trait::async_trait;
use plexspaces_core::{Actor, ActorContext, BehaviorError, BehaviorType, Message};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// =============================================================================
// Message Types
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action")]
pub enum GeneralMessage {
    /// Initialize with peer information
    Init { peer_ids: Vec<usize> },
    /// Receive a vote from another general
    Vote { from: usize, path: String, value: String },
    /// Request current decision
    GetDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Value {
    Zero,
    One,
    Retreat, // Default when no majority
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Decision {
    pub general_id: usize,
    pub value: Value,
    pub is_faulty: bool,
}

// =============================================================================
// General Actor
// =============================================================================

/// A general in the Byzantine Generals Problem
pub struct General {
    pub id: usize,
    pub source_id: usize,
    pub num_rounds: usize,
    pub peer_ids: Vec<usize>,
    pub votes: HashMap<String, Value>,
    pub is_faulty: bool,
}

impl General {
    /// Create a new general
    ///
    /// Generals with id == source_id or id == 2 are marked as faulty
    /// (matching the classic Erlang implementation)
    pub fn new(id: usize, source_id: usize, num_rounds: usize) -> Self {
        let is_faulty = id == source_id || id == 2;
        Self {
            id,
            source_id,
            num_rounds,
            peer_ids: Vec::new(),
            votes: HashMap::new(),
            is_faulty,
        }
    }

    /// Record a vote
    pub fn record_vote(&mut self, path: String, value: Value) {
        self.votes.insert(path, value);
    }

    /// Compute majority vote
    pub fn majority_vote(&self) -> Value {
        let mut counts: HashMap<Value, usize> = HashMap::new();
        
        for value in self.votes.values() {
            *counts.entry(*value).or_insert(0) += 1;
        }
        
        let total: usize = counts.values().sum();
        if total == 0 {
            return Value::Retreat;
        }
        
        let threshold = total / 2;
        for (value, count) in counts {
            if count > threshold {
                return value;
            }
        }
        
        Value::Retreat
    }

    /// Get current decision
    pub fn decision(&self) -> Decision {
        let value = if self.id == self.source_id {
            if self.is_faulty { Value::One } else { Value::Zero }
        } else {
            self.majority_vote()
        };
        
        Decision {
            general_id: self.id,
            value,
            is_faulty: self.is_faulty,
        }
    }
    
    /// Parse value string to Value enum
    fn parse_value(s: &str) -> Value {
        match s {
            "Zero" => Value::Zero,
            "One" => Value::One,
            _ => Value::Retreat,
        }
    }
}

// =============================================================================
// Actor Implementation
// =============================================================================

#[async_trait]
impl Actor for General {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Deserialize message
        let general_msg: GeneralMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid message: {}", e)))?;
        
        match general_msg {
            GeneralMessage::Init { peer_ids } => {
                self.peer_ids = peer_ids;
            }
            GeneralMessage::Vote { from, path, value } => {
                let vote_value = Self::parse_value(&value);
                self.record_vote(path, vote_value);
            }
            GeneralMessage::GetDecision => {
                let decision = self.decision();
                
                // Send reply for call messages
                if msg.message_type == "call" && !msg.sender_id.is_empty() {
                    let reply_payload = serde_json::to_vec(&decision)
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to serialize decision: {}", e)))?;
                    
                    let reply_msg = Message {
                        id: ulid::Ulid::new().to_string(),
                        message_type: "reply".to_string(),
                        payload: reply_payload,
                        ..Default::default()
                    };
                    
                    // Get current actor ID from self_ref or use receiver_id from message
                    let current_actor_id = ctx.self_ref()
                        .map(|r| r.id().clone())
                        .unwrap_or_else(|| msg.receiver_id.clone());
                    
                    // Extract correlation_id - Message has String, not Option<String>
                    let correlation_id_opt = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    
                    ctx.send_reply(
                        correlation_id_opt,
                        &msg.sender_id,
                        current_actor_id.clone(),
                        reply_msg,
                    ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
        }
        
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("ByzantineGeneral".to_string())
    }
}
