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
pub enum GeneralMessage {
    /// Initialize with peer information
    Init { peer_ids: Vec<usize> },
    /// Receive a vote from another general
    Vote { from: usize, path: String, value: Value },
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
}

// =============================================================================
// Actor Implementation
// =============================================================================

#[async_trait]
impl Actor for General {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Deserialize message
        let general_msg: GeneralMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid message: {}", e)))?;
        
        match general_msg {
            GeneralMessage::Init { peer_ids } => {
                self.peer_ids = peer_ids;
            }
            GeneralMessage::Vote { path, value, .. } => {
                self.record_vote(path, value);
            }
            GeneralMessage::GetDecision => {
                // In a real implementation, we'd send reply via ctx.send_reply()
                let decision = self.decision();
                tracing::info!(
                    general_id = self.id,
                    decision = ?decision.value,
                    is_faulty = decision.is_faulty,
                    "General decision"
                );
            }
        }
        
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("ByzantineGeneral".to_string())
    }
}
