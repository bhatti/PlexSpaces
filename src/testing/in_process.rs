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

//! In-process test environment - Erlang-like lightweight processes
//!
//! This is the simplest and fastest environment for testing.
//! Actors run as async tasks in the same process.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::{mpsc, RwLock};
use tokio::task::JoinHandle;

use crate::actor::ActorState;
use crate::journal::MemoryJournal;
use crate::mailbox::{mailbox_config_default, Mailbox, Message};
use crate::tuplespace::lattice_space::LatticeTupleSpace;
use crate::ActorId; // Re-exported from core crate
use crate::ConsistencyLevel;

use super::{
    ActorConfig, ActorHandle, EnvironmentMetrics, EnvironmentType, ResourceProfile,
    TestEnvironment, TestError,
};

/// Local actor instance running in-process
pub struct LocalActor {
    id: ActorId,
    mailbox: Arc<Mailbox>,
    state: Arc<RwLock<ActorState>>,
    _journal: Arc<MemoryJournal>,
    _tuplespace: Arc<LatticeTupleSpace>,
    message_rx: mpsc::Receiver<Message>,
    message_count: Arc<RwLock<u64>>,
}

impl LocalActor {
    pub async fn new(
        config: ActorConfig,
        tuplespace: Arc<LatticeTupleSpace>,
    ) -> Result<(Self, mpsc::Sender<Message>), TestError> {
        let (tx, rx) = mpsc::channel(100);

        let mailbox_config = mailbox_config_default();
        // Mailbox::new is async, await it directly
        let mailbox = Mailbox::new(mailbox_config, config.id.clone())
            .await
            .map_err(|e| TestError::EnvironmentError(format!("Failed to create mailbox: {}", e)))?;
        
        let actor = LocalActor {
            id: config.id.clone(),
            mailbox: Arc::new(mailbox),
            state: Arc::new(RwLock::new(ActorState::Creating)),
            _journal: Arc::new(MemoryJournal::new()),
            _tuplespace: tuplespace,
            message_rx: rx,
            message_count: Arc::new(RwLock::new(0)),
        };

        Ok((actor, tx))
    }

    pub async fn run(mut self) {
        // Set state to active
        *self.state.write().await = ActorState::Active;

        // Process messages
        while let Some(msg) = self.message_rx.recv().await {
            // Increment message count
            *self.message_count.write().await += 1;

            // Process message based on type
            match msg.message_type.as_str() {
                "STOP" => {
                    *self.state.write().await = ActorState::Terminated;
                    break;
                }
                "CRASH" => {
                    *self.state.write().await = ActorState::Failed("Simulated crash".to_string());
                    panic!("Actor {} crashed (simulated)", self.id);
                }
                _ => {
                    // Process normally
                    self.mailbox.send(msg).await.ok();
                }
            }
        }
    }

    pub fn id(&self) -> &ActorId {
        &self.id
    }

    pub async fn get_state(&self) -> ActorState {
        self.state.read().await.clone()
    }

    pub async fn get_message_count(&self) -> u64 {
        *self.message_count.read().await
    }
}

/// In-process test environment
pub struct InProcessEnvironment {
    actors: Arc<RwLock<HashMap<String, LocalActorInfo>>>,
    tuplespace: Arc<LatticeTupleSpace>,
    metrics: Arc<RwLock<InProcessMetrics>>,
    partitions: Arc<RwLock<NetworkPartitions>>,
}

struct LocalActorInfo {
    handle: Arc<JoinHandle<()>>,
    sender: mpsc::Sender<Message>,
    _id: ActorId,
    _started_at: Instant,
    _resource_profile: ResourceProfile,
}

#[derive(Default)]
struct InProcessMetrics {
    messages_sent: u64,
    messages_received: u64,
    total_latency_ns: u128,
    message_count: u64,
    memory_usage_bytes: usize,
}

#[derive(Default)]
struct NetworkPartitions {
    partitioned: bool,
    group1: Vec<String>,
    group2: Vec<String>,
}

impl Default for InProcessEnvironment {
    fn default() -> Self {
        Self::new()
    }
}

impl InProcessEnvironment {
    pub fn new() -> Self {
        InProcessEnvironment {
            actors: Arc::new(RwLock::new(HashMap::new())),
            tuplespace: Arc::new(LatticeTupleSpace::new(
                "test-node".to_string(),
                ConsistencyLevel::Eventual,
            )),
            metrics: Arc::new(RwLock::new(InProcessMetrics::default())),
            partitions: Arc::new(RwLock::new(NetworkPartitions::default())),
        }
    }

    async fn is_partitioned(&self, from: &str, to: &str) -> bool {
        let partitions = self.partitions.read().await;
        if !partitions.partitioned {
            return false;
        }

        // Check if actors are in different partition groups
        let from_in_g1 = partitions.group1.contains(&from.to_string());
        let from_in_g2 = partitions.group2.contains(&from.to_string());
        let to_in_g1 = partitions.group1.contains(&to.to_string());
        let to_in_g2 = partitions.group2.contains(&to.to_string());

        // They're partitioned if they're in different groups
        (from_in_g1 && to_in_g2) || (from_in_g2 && to_in_g1)
    }
}

#[async_trait]
impl TestEnvironment for InProcessEnvironment {
    fn environment_type(&self) -> EnvironmentType {
        EnvironmentType::InProcess
    }

    async fn deploy_actor(&self, config: ActorConfig) -> Result<ActorHandle, TestError> {
        // Create local actor
        let (actor, sender) = LocalActor::new(config.clone(), self.tuplespace.clone()).await?;
        let actor_id = actor.id.clone();

        // Spawn as async task
        let handle = Arc::new(tokio::spawn(actor.run()));

        // Store actor info
        let info = LocalActorInfo {
            handle: handle.clone(),
            sender,
            _id: actor_id,
            _started_at: Instant::now(),
            _resource_profile: config.resources,
        };

        self.actors.write().await.insert(config.id.clone(), info);

        Ok(ActorHandle::Local(config.id, handle))
    }

    async fn send_message(&self, actor: &ActorHandle, msg: Message) -> Result<(), TestError> {
        let start = Instant::now();

        // Extract actor ID from handle
        let actor_id = match actor {
            ActorHandle::Local(id, _) => id.clone(),
            _ => return Err(TestError::EnvironmentError("Wrong handle type".to_string())),
        };

        // Check for network partition
        if let Some(sender_id) = msg.sender.as_ref() {
            if self.is_partitioned(sender_id, &actor_id).await {
                return Err(TestError::MessageFailed("Network partition".to_string()));
            }
        }

        // Send message
        let actors = self.actors.read().await;
        let info = actors
            .get(&actor_id)
            .ok_or_else(|| TestError::ActorNotFound(actor_id.clone()))?;

        info.sender
            .send(msg)
            .await
            .map_err(|e| TestError::MessageFailed(e.to_string()))?;

        // Update metrics
        let latency = start.elapsed();
        let mut metrics = self.metrics.write().await;
        metrics.messages_sent += 1;
        metrics.messages_received += 1;
        metrics.total_latency_ns += latency.as_nanos();
        metrics.message_count += 1;

        Ok(())
    }

    async fn get_state(&self, actor: &ActorHandle) -> Result<ActorState, TestError> {
        match actor {
            ActorHandle::Local(_, _handle) => {
                // For now, return Active state
                // In real implementation, we'd query the actual actor
                Ok(ActorState::Active)
            }
            _ => Err(TestError::EnvironmentError("Wrong handle type".to_string())),
        }
    }

    async fn kill_actor(&self, actor: &ActorHandle) -> Result<(), TestError> {
        match actor {
            ActorHandle::Local(_, handle) => {
                handle.abort();
                Ok(())
            }
            _ => Err(TestError::EnvironmentError("Wrong handle type".to_string())),
        }
    }

    async fn create_partition(
        &self,
        group1: Vec<ActorHandle>,
        group2: Vec<ActorHandle>,
    ) -> Result<(), TestError> {
        let mut partitions = self.partitions.write().await;
        partitions.partitioned = true;

        // Convert handles to IDs
        // For simplicity, we'll use indices
        partitions.group1 = group1
            .iter()
            .enumerate()
            .map(|(i, _)| format!("general_{}", i))
            .collect();

        partitions.group2 = group2
            .iter()
            .enumerate()
            .map(|(i, _)| format!("general_{}", i + group1.len()))
            .collect();

        Ok(())
    }

    async fn heal_partition(&self) -> Result<(), TestError> {
        let mut partitions = self.partitions.write().await;
        partitions.partitioned = false;
        partitions.group1.clear();
        partitions.group2.clear();
        Ok(())
    }

    async fn collect_metrics(&self) -> Result<EnvironmentMetrics, TestError> {
        let metrics = self.metrics.read().await;

        let avg_latency_ms = if metrics.message_count > 0 {
            (metrics.total_latency_ns as f64 / metrics.message_count as f64) / 1_000_000.0
        } else {
            0.0
        };

        Ok(EnvironmentMetrics {
            messages_sent: metrics.messages_sent,
            messages_received: metrics.messages_received,
            avg_latency_ms,
            cpu_usage_percent: 0.0, // Not measured in-process
            memory_usage_mb: (metrics.memory_usage_bytes / 1_024_1_024) as u64,
            network_bytes_sent: 0,
            network_bytes_received: 0,
        })
    }

    async fn cleanup(&self) -> Result<(), TestError> {
        // Stop all actors
        let actors = self.actors.write().await;
        for (_, info) in actors.iter() {
            info.handle.abort();
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mailbox::MessagePriority;

    #[tokio::test]
    async fn test_in_process_deploy() {
        let env = InProcessEnvironment::new();

        let config = ActorConfig {
            id: "test-actor".to_string(),
            actor_type: "general".to_string(),
            resources: ResourceProfile::minimal(),
            metadata: HashMap::new(),
        };

        let handle = env.deploy_actor(config).await.unwrap();

        match handle {
            ActorHandle::Local(_, _) => (),
            _ => panic!("Expected Local handle"),
        }

        // Verify actor is in registry
        let actors = env.actors.read().await;
        assert!(actors.contains_key("test-actor"));
    }

    #[tokio::test]
    async fn test_in_process_messaging() {
        let env = InProcessEnvironment::new();

        // Deploy two actors
        let actor1 = env
            .deploy_actor(ActorConfig {
                id: "actor1".to_string(),
                actor_type: "general".to_string(),
                resources: ResourceProfile::minimal(),
                metadata: HashMap::new(),
            })
            .await
            .unwrap();

        let actor2 = env
            .deploy_actor(ActorConfig {
                id: "actor2".to_string(),
                actor_type: "general".to_string(),
                resources: ResourceProfile::minimal(),
                metadata: HashMap::new(),
            })
            .await
            .unwrap();

        // Send message from actor1 to actor2
        let mut msg = Message::new(b"Hello".to_vec());
        msg.sender = Some("actor1".to_string());
        msg.receiver = "actor2".to_string();
        msg.message_type = "TEST".to_string();
        msg.priority = crate::mailbox::MessagePriority::Normal;

        env.send_message(&actor2, msg).await.unwrap();

        // Verify metrics
        let metrics = env.collect_metrics().await.unwrap();
        assert_eq!(metrics.messages_sent, 1);
        assert_eq!(metrics.messages_received, 1);
    }

    #[tokio::test]
    async fn test_in_process_partition() {
        let env = InProcessEnvironment::new();

        // Deploy 4 actors
        let mut actors = Vec::new();
        for i in 0..4 {
            let actor = env
                .deploy_actor(ActorConfig {
                    id: format!("general_{}", i),
                    actor_type: "general".to_string(),
                    resources: ResourceProfile::minimal(),
                    metadata: HashMap::new(),
                })
                .await
                .unwrap();
            actors.push(actor);
        }

        // Create partition
        env.create_partition(actors[0..2].to_vec(), actors[2..4].to_vec())
            .await
            .unwrap();

        // Verify partition blocks messages
        let mut msg = Message::new(b"Hello".to_vec());
        msg.sender = Some("general_0".to_string());
        msg.receiver = "general_2".to_string();
        msg.message_type = "TEST".to_string();
        msg.priority = crate::mailbox::MessagePriority::Normal;

        let result = env.send_message(&actors[2], msg).await;
        assert!(result.is_err());

        // Heal partition
        env.heal_partition().await.unwrap();

        // Now message should succeed
        let mut msg2 = Message::new(b"Hello again".to_vec());
        msg2.sender = Some("general_0".to_string());
        msg2.receiver = "general_2".to_string();
        msg2.message_type = "TEST".to_string();
        msg2.priority = crate::mailbox::MessagePriority::Normal;

        env.send_message(&actors[2], msg2).await.unwrap();
    }

    #[tokio::test]
    async fn test_in_process_kill_actor() {
        let env = InProcessEnvironment::new();

        let handle = env
            .deploy_actor(ActorConfig {
                id: "mortal".to_string(),
                actor_type: "general".to_string(),
                resources: ResourceProfile::minimal(),
                metadata: HashMap::new(),
            })
            .await
            .unwrap();

        // Kill the actor
        env.kill_actor(&handle).await.unwrap();

        // Verify actor is terminated
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Sending message should fail
        let mut msg = Message::new(b"Hello".to_vec());
        msg.receiver = "mortal".to_string();
        msg.message_type = "TEST".to_string();
        msg.priority = crate::mailbox::MessagePriority::Normal;

        let result = env.send_message(&handle, msg).await;
        assert!(result.is_err());
    }
}
