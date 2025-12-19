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

//! Byzantine Generals Problem - Simple implementation based on Erlang version
//!
//! This is a simplified implementation that follows the Erlang algorithm closely:
//! - Processes communicate via messages
//! - Source process sends initial messages in round 0
//! - Other processes relay messages in subsequent rounds
//! - Processes vote and make decisions
//! - Some processes can be faulty (traitors)

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use async_trait::async_trait;
use serde::{Serialize, Deserialize};

use plexspaces_core::{Actor, ActorContext, BehaviorError, BehaviorType};
use plexspaces_behavior::GenServer;
use plexspaces_mailbox::Message;

// ============================================================================
// Message Types
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GeneralMessage {
    /// Initialize with neighbor information
    Init {
        general_ids: Vec<usize>,
        source_id: usize,
        num_rounds: usize,
    },
    /// Receive a message with path and value
    ReceiveMessage {
        path: String,
        value: Value,
    },
    /// Send messages for a round
    SendMessages {
        round: usize,
    },
    /// Get result
    GetResult,
    /// Stop the general
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Value {
    Zero,
    One,
    Retreat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralResult {
    pub id: usize,
    pub is_source: bool,
    pub is_faulty: bool,
    pub decision: Value,
    pub message_count: usize,
}

// ============================================================================
// General Actor (GenServer)
// ============================================================================

pub struct General {
    id: usize,
    source_id: usize,
    num_rounds: usize,
    general_ids: Vec<usize>,
    values: HashMap<String, Value>,
    is_faulty: bool,
    message_count: usize,
}

impl General {
    pub fn new(id: usize, source_id: usize, num_rounds: usize) -> Self {
        // Process is faulty if it's the source or id == 2 (matching Erlang logic)
        let is_faulty = id == source_id || id == 2;
        
        Self {
            id,
            source_id,
            num_rounds,
            general_ids: Vec::new(),
            values: HashMap::new(),
            is_faulty,
            message_count: 0,
        }
    }

    fn handle_init(&mut self, general_ids: Vec<usize>) {
        self.general_ids = general_ids;
    }

    fn handle_receive_message(&mut self, path: String, value: Value) {
        self.values.insert(path, value);
        self.message_count += 1;
    }

    async fn handle_send_messages(
        &mut self,
        round: usize,
        ctx: &ActorContext,
    ) -> Result<(), BehaviorError> {
        if round == 0 && self.id == self.source_id {
            self.send_initial_messages(ctx).await?;
        } else if round > 0 {
            self.relay_messages(round, ctx).await?;
        }
        Ok(())
    }

    async fn send_initial_messages(&mut self, ctx: &ActorContext) -> Result<(), BehaviorError> {
        let base_value = Value::Zero;
        let general_ids = self.general_ids.clone();
        
        for &other_id in &general_ids {
            if other_id != self.id {
                let value = if self.is_faulty {
                    // Faulty commander sends different values
                    if other_id % 2 == 0 {
                        Value::Zero
                    } else {
                        Value::One
                    }
                } else {
                    base_value
                };
                
                let path = self.id.to_string();
                self.send_message_to_general(ctx, other_id, path, value).await?;
            }
        }
        
        Ok(())
    }

    async fn relay_messages(
        &mut self,
        round: usize,
        ctx: &ActorContext,
    ) -> Result<(), BehaviorError> {
        let paths = self.get_paths_for_round(round - 1);
        
        for path in paths {
            if let Some(&value) = self.values.get(&path) {
                let new_value = if self.is_faulty && self.id == 2 {
                    // Faulty process transforms value
                    Value::One
                } else {
                    value
                };
                
                let new_path = format!("{}{}", path, self.id);
                let general_ids = self.general_ids.clone();
                
                for &other_id in &general_ids {
                    if other_id != self.id {
                        // Don't send to processes already in path
                        let other_id_str = other_id.to_string();
                        if !new_path.contains(&other_id_str) {
                            self.send_message_to_general(ctx, other_id, new_path.clone(), new_value).await?;
                        }
                    }
                }
            }
        }
        
        Ok(())
    }

    fn get_paths_for_round(&self, round: usize) -> Vec<String> {
        if round == 0 {
            vec![self.source_id.to_string()]
        } else {
            self.values
                .keys()
                .filter(|path| path.len() == round + 1)
                .cloned()
                .collect()
        }
    }

    async fn send_message_to_general(
        &mut self,
        ctx: &ActorContext,
        target_id: usize,
        path: String,
        value: Value,
    ) -> Result<(), BehaviorError> {
        let actor_id = format!("general{}@{}", target_id, &ctx.node_id);
        let msg_bytes = bincode::serialize(&GeneralMessage::ReceiveMessage { path, value })
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))?;
        
        let message = Message::new(msg_bytes);
        let actor_service = ctx.get_actor_service().await
            .ok_or_else(|| BehaviorError::ProcessingError("ActorService not available".to_string()))?;
        
        actor_service.send(&actor_id, message).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send message: {}", e)))?;
        
        Ok(())
    }

    fn create_result(&self) -> GeneralResult {
        let decision = if self.id == self.source_id {
            // Source process uses its own value
            if self.is_faulty {
                Value::One
            } else {
                Value::Zero
            }
        } else {
            self.majority_vote()
        };
        
        GeneralResult {
            id: self.id,
            is_source: self.id == self.source_id,
            is_faulty: self.is_faulty,
            decision,
            message_count: self.message_count,
        }
    }

    fn majority_vote(&self) -> Value {
        let mut counts: HashMap<Value, usize> = HashMap::new();
        
        for value in self.values.values() {
            *counts.entry(*value).or_insert_with(|| 0) += 1;
        }
        
        let total_votes: usize = counts.values().sum();
        
        if total_votes == 0 {
            return Value::Retreat;
        }
        
        let majority_threshold = total_votes / 2;
        
        for (value, count) in counts {
            if count > majority_threshold {
                return value;
            }
        }
        
        Value::Retreat
    }
}

#[async_trait]
impl Actor for General {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let general_msg: GeneralMessage = bincode::deserialize(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Deserialization error: {}", e)))?;
        
        match general_msg {
            GeneralMessage::Init { general_ids, .. } => {
                self.handle_init(general_ids);
            }
            GeneralMessage::ReceiveMessage { path, value } => {
                self.handle_receive_message(path, value);
            }
            GeneralMessage::SendMessages { round } => {
                self.handle_send_messages(round, ctx).await?;
            }
            GeneralMessage::GetResult => {
                // Result is returned via reply using ctx.send_reply()
                let result = self.create_result();
                let result_bytes = bincode::serialize(&result)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))?;
                
                if let Some(sender_id) = &msg.sender {
                    let reply = Message::new(result_bytes);
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            GeneralMessage::Stop => {
                // Stop handling - actor will be terminated
            }
        }
        
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for General {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // GenServer handles requests - delegate to handle_message
        self.handle_message(ctx, msg).await
    }
}

// ============================================================================
// Algorithm Runner
// ============================================================================

pub struct ByzantineAlgorithm {
    num_processes: usize,
    source_id: usize,
    num_rounds: usize,
    general_ids: Vec<usize>,
}

impl ByzantineAlgorithm {
    pub fn new(num_processes: usize, source_id: usize, num_rounds: usize) -> Result<Self, String> {
        if num_processes < 4 {
            return Err("Need at least 4 processes for Byzantine Generals Problem".to_string());
        }
        if source_id >= num_processes {
            return Err("Source must be less than number of processes".to_string());
        }
        if num_rounds < 1 {
            return Err("Need at least 1 round".to_string());
        }
        
        let general_ids: Vec<usize> = (0..num_processes).collect();
        
        Ok(Self {
            num_processes,
            source_id,
            num_rounds,
            general_ids,
        })
    }

    pub async fn run(
        &self,
        ctx: &ActorContext,
    ) -> Result<(u64, usize, Vec<GeneralResult>), String> {
        use std::time::Instant;
        let start_time = Instant::now();
        
        // Initialize all generals
        for &general_id in &self.general_ids {
            let init_msg = GeneralMessage::Init {
                general_ids: self.general_ids.clone(),
                source_id: self.source_id,
                num_rounds: self.num_rounds,
            };
            let msg_bytes = bincode::serialize(&init_msg)
                .map_err(|e| format!("Serialization error: {}", e))?;
            
            let message = Message::new(msg_bytes);
            let actor_id = format!("general{}@{}", general_id, &ctx.node_id);
            let actor_service = ctx.get_actor_service().await
                .ok_or_else(|| "ActorService not available".to_string())?;
            
            actor_service.send(&actor_id, message).await
                .map_err(|e| format!("Failed to initialize general {}: {}", general_id, e))?;
        }
        
        // Run rounds
        for round in 0..self.num_rounds {
            for &general_id in &self.general_ids {
                let send_msg = GeneralMessage::SendMessages { round };
                let msg_bytes = bincode::serialize(&send_msg)
                    .map_err(|e| format!("Serialization error: {}", e))?;
                
                let message = Message::new(msg_bytes);
                let actor_id = format!("general{}@{}", general_id, &ctx.node_id);
                let actor_service = ctx.get_actor_service().await
                    .ok_or_else(|| "ActorService not available".to_string())?;
                
                actor_service.send(&actor_id, message).await
                    .map_err(|e| format!("Failed to send round {} message to general {}: {}", round, general_id, e))?;
            }
            
            // Wait a bit for messages to propagate
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
        
        // Collect results using ActorRef::ask
        let mut results = Vec::new();
        let actor_registry = ctx.service_locator.get_service::<plexspaces_core::ActorRegistry>().await
            .ok_or_else(|| "ActorRegistry not available".to_string())?;
        
        for &general_id in &self.general_ids {
            let get_result_msg = GeneralMessage::GetResult;
            let msg_bytes = bincode::serialize(&get_result_msg)
                .map_err(|e| format!("Serialization error: {}", e))?;
            
            let mut message = Message::new(msg_bytes);
            let actor_id = format!("general{}@{}", general_id, &ctx.node_id);
            
            // Get ActorRef and use ask pattern
            if actor_registry.lookup_actor(&actor_id).await.is_some() {
                // Create ActorRef using remote() - works for both local and remote actors
                use plexspaces_actor::ActorRef;
                let actor_ref = ActorRef::remote(
                    actor_id.clone(),
                    ctx.node_id.clone(),
                    ctx.service_locator.clone(),
                );
                
                // Set receiver in message
                message.receiver = actor_id.clone();
                
                // Use ActorRef::ask for request-reply
                let reply = actor_ref.ask(message, std::time::Duration::from_secs(5)).await
                    .map_err(|e| format!("Failed to get result from general {}: {}", general_id, e))?;
                
                let result: GeneralResult = bincode::deserialize(reply.payload())
                    .map_err(|e| format!("Deserialization error: {}", e))?;
                
                results.push(result);
            } else {
                return Err(format!("General {} not found in registry", general_id));
            }
        }
        
        let duration = start_time.elapsed().as_millis() as u64;
        let total_messages: usize = results.iter().map(|r| r.message_count).sum();
        
        Ok((duration, total_messages, results))
    }
}
