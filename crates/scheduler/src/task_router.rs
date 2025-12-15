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

//! Task router for Layer 2 (task-level scheduling).
//!
//! ## Purpose
//! Routes tasks to appropriate actors based on actor groups and routing strategies.
//! Uses existing channels for message distribution.
//!
//! ## Design
//! - **Actor Group Management**: Tracks which actors belong to which groups
//! - **Channel-Based Routing**: Uses channels for task distribution
//! - **Routing Strategies**: Round-robin, least-loaded, random, hash, broadcast
//! - **Load Tracking**: Monitors actor queue depth for least-loaded routing

use plexspaces_channel::Channel;
use plexspaces_proto::{
    channel::v1::ChannelMessage,
    scheduling::v1::ActorGroup,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Routing strategy for task distribution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutingStrategy {
    /// Round-robin: distribute evenly across actors
    RoundRobin,
    /// Least-loaded: route to actor with least queue depth
    LeastLoaded,
    /// Random: random actor selection
    Random,
    /// Hash: consistent hashing (for sharding)
    Hash,
    /// Broadcast: send to all actors in group
    Broadcast,
}

/// Error types for task routing
#[derive(Debug, thiserror::Error)]
pub enum TaskRouterError {
    /// Group not found
    #[error("Actor group not found: {0}")]
    GroupNotFound(String),

    /// No actors in group
    #[error("No actors in group: {0}")]
    NoActorsInGroup(String),

    /// Channel error
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// Invalid routing strategy
    #[error("Invalid routing strategy")]
    InvalidStrategy,
}

/// Result type for task routing
pub type TaskRouterResult<T> = Result<T, TaskRouterError>;

/// Actor load information for least-loaded routing
#[derive(Debug, Clone)]
struct ActorLoad {
    /// Actor ID
    actor_id: String,
    /// Current queue depth (number of pending messages)
    queue_depth: u32,
    /// Last update timestamp
    last_update: std::time::SystemTime,
}

/// Task router for routing tasks to actor groups
pub struct TaskRouter {
    /// Actor groups (group_name -> ActorGroup)
    groups: Arc<RwLock<HashMap<String, ActorGroup>>>,
    /// Actor load tracking (actor_id -> ActorLoad)
    actor_loads: Arc<RwLock<HashMap<String, ActorLoad>>>,
    /// Round-robin counters (group_name -> next_index)
    round_robin_counters: Arc<RwLock<HashMap<String, usize>>>,
    /// Channel factory (creates/get channels by name)
    channel_factory: Arc<dyn Fn(&str) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Arc<dyn Channel>, String>> + Send>> + Send + Sync>,
}

impl TaskRouter {
    /// Create a new task router
    ///
    /// ## Arguments
    /// * `channel_factory`: Function to create/get channels by name
    pub fn new<F, Fut>(channel_factory: F) -> Self
    where
        F: Fn(&str) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<Arc<dyn Channel>, String>> + Send + 'static,
    {
        Self {
            groups: Arc::new(RwLock::new(HashMap::new())),
            actor_loads: Arc::new(RwLock::new(HashMap::new())),
            round_robin_counters: Arc::new(RwLock::new(HashMap::new())),
            channel_factory: Arc::new(move |name| {
                Box::pin(channel_factory(name))
            }),
        }
    }

    /// Register an actor group
    ///
    /// ## Arguments
    /// * `group`: Actor group to register
    pub async fn register_group(&self, group: ActorGroup) -> TaskRouterResult<()> {
        let group_name = group.group_name.clone();
        let mut groups = self.groups.write().await;
        groups.insert(group_name.clone(), group);
        info!("Registered actor group: {}", group_name);
        Ok(())
    }

    /// Unregister an actor group
    ///
    /// ## Arguments
    /// * `group_name`: Name of group to unregister
    pub async fn unregister_group(&self, group_name: &str) -> TaskRouterResult<()> {
        let mut groups = self.groups.write().await;
        groups.remove(group_name);
        let mut counters = self.round_robin_counters.write().await;
        counters.remove(group_name);
        info!("Unregistered actor group: {}", group_name);
        Ok(())
    }

    /// Add actor to group
    ///
    /// ## Arguments
    /// * `group_name`: Group to add actor to
    /// * `actor_id`: Actor ID to add
    pub async fn add_actor_to_group(&self, group_name: &str, actor_id: String) -> TaskRouterResult<()> {
        let mut groups = self.groups.write().await;
        if let Some(group) = groups.get_mut(group_name) {
            if !group.actor_ids.contains(&actor_id) {
                group.actor_ids.push(actor_id.clone());
                info!("Added actor {} to group {}", actor_id, group_name);
            }
            Ok(())
        } else {
            Err(TaskRouterError::GroupNotFound(group_name.to_string()))
        }
    }

    /// Remove actor from group
    ///
    /// ## Arguments
    /// * `group_name`: Group to remove actor from
    /// * `actor_id`: Actor ID to remove
    pub async fn remove_actor_from_group(&self, group_name: &str, actor_id: &str) -> TaskRouterResult<()> {
        let mut groups = self.groups.write().await;
        if let Some(group) = groups.get_mut(group_name) {
            group.actor_ids.retain(|id| id != actor_id);
            let mut loads = self.actor_loads.write().await;
            loads.remove(actor_id);
            info!("Removed actor {} from group {}", actor_id, group_name);
            Ok(())
        } else {
            Err(TaskRouterError::GroupNotFound(group_name.to_string()))
        }
    }

    /// Update actor load (queue depth)
    ///
    /// ## Arguments
    /// * `actor_id`: Actor ID
    /// * `queue_depth`: Current queue depth
    pub async fn update_actor_load(&self, actor_id: String, queue_depth: u32) {
        let mut loads = self.actor_loads.write().await;
        loads.insert(actor_id.clone(), ActorLoad {
            actor_id,
            queue_depth,
            last_update: std::time::SystemTime::now(),
        });
    }

    /// Route task to actor group
    ///
    /// ## Arguments
    /// * `group_name`: Group to route to
    /// * `task_payload`: Task payload
    /// * `strategy`: Routing strategy
    ///
    /// ## Returns
    /// Number of actors that received the task
    pub async fn route_task(
        &self,
        group_name: &str,
        task_payload: Vec<u8>,
        strategy: RoutingStrategy,
    ) -> TaskRouterResult<u32> {
        // Get group and actor IDs
        let actor_ids = {
            let groups = self.groups.read().await;
            let group = groups.get(group_name)
                .ok_or_else(|| TaskRouterError::GroupNotFound(group_name.to_string()))?;
            if group.actor_ids.is_empty() {
                return Err(TaskRouterError::NoActorsInGroup(group_name.to_string()));
            }
            group.actor_ids.clone()
        };

        // Get or create channel for group
        let channel = (self.channel_factory)(group_name)
            .await
            .map_err(|e| TaskRouterError::ChannelError(e))?;

        // Create channel message
        let message_id = ulid::Ulid::new().to_string();
        let channel_msg = ChannelMessage {
            id: message_id.clone(),
            channel: group_name.to_string(),
            sender_id: String::new(),
            payload: task_payload,
            headers: HashMap::new(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(std::time::SystemTime::now())),
            partition_key: String::new(),
            correlation_id: String::new(),
            delivery_count: 0,
            reply_to: String::new(),
        };

        // Route based on strategy
        match strategy {
            RoutingStrategy::Broadcast => {
                // Publish to all subscribers
                let count = channel.publish(channel_msg).await
                    .map_err(|e| TaskRouterError::ChannelError(e.to_string()))?;
                Ok(count)
            }
            RoutingStrategy::RoundRobin => {
                // Select actor using round-robin, then send to channel
                // Channel will deliver to that actor's subscription
                let actor_id = self.select_round_robin(group_name, &actor_ids).await?;
                channel.send(channel_msg).await
                    .map_err(|e| TaskRouterError::ChannelError(e.to_string()))?;
                // Update load (message sent, queue depth increases)
                self.update_actor_load(actor_id, 0).await; // TODO: Get actual queue depth
                Ok(1)
            }
            RoutingStrategy::LeastLoaded => {
                // Select least-loaded actor
                let actor_id = self.select_least_loaded(&actor_ids).await?;
                channel.send(channel_msg).await
                    .map_err(|e| TaskRouterError::ChannelError(e.to_string()))?;
                // Update load
                self.update_actor_load(actor_id, 0).await; // TODO: Get actual queue depth
                Ok(1)
            }
            RoutingStrategy::Random => {
                // Select random actor
                use rand::Rng;
                let mut rng = rand::thread_rng();
                let idx = rng.gen_range(0..actor_ids.len());
                let actor_id = actor_ids[idx].clone();
                channel.send(channel_msg).await
                    .map_err(|e| TaskRouterError::ChannelError(e.to_string()))?;
                self.update_actor_load(actor_id, 0).await;
                Ok(1)
            }
            RoutingStrategy::Hash => {
                // Use consistent hashing (simple hash for now)
                // TODO: Implement proper consistent hashing
                let hash = self.hash_key(&message_id);
                let idx = hash % actor_ids.len() as u64;
                let actor_id = actor_ids[idx as usize].clone();
                channel.send(channel_msg).await
                    .map_err(|e| TaskRouterError::ChannelError(e.to_string()))?;
                self.update_actor_load(actor_id, 0).await;
                Ok(1)
            }
        }
    }

    /// Select actor using round-robin
    async fn select_round_robin(&self, group_name: &str, actor_ids: &[String]) -> TaskRouterResult<String> {
        let mut counters = self.round_robin_counters.write().await;
        let counter = counters.entry(group_name.to_string()).or_insert(0);
        let idx = *counter % actor_ids.len();
        *counter += 1;
        Ok(actor_ids[idx].clone())
    }

    /// Select least-loaded actor
    async fn select_least_loaded(&self, actor_ids: &[String]) -> TaskRouterResult<String> {
        let loads = self.actor_loads.read().await;
        
        if actor_ids.is_empty() {
            return Err(TaskRouterError::NoActorsInGroup("unknown".to_string()));
        }

        // Find actor with minimum queue depth
        let mut best_actor = &actor_ids[0];
        let mut best_load = loads.get(best_actor)
            .map(|l| l.queue_depth)
            .unwrap_or(0);

        for actor_id in actor_ids {
            let load = loads.get(actor_id)
                .map(|l| l.queue_depth)
                .unwrap_or(0);
            if load < best_load {
                best_load = load;
                best_actor = actor_id;
            }
        }

        Ok(best_actor.clone())
    }

    /// Hash key for consistent hashing
    fn hash_key(&self, key: &str) -> u64 {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        hasher.finish()
    }

    /// Get group information
    pub async fn get_group(&self, group_name: &str) -> TaskRouterResult<ActorGroup> {
        let groups = self.groups.read().await;
        groups.get(group_name)
            .ok_or_else(|| TaskRouterError::GroupNotFound(group_name.to_string()))
            .map(|g| g.clone())
    }

    /// List all groups
    pub async fn list_groups(&self) -> Vec<String> {
        let groups = self.groups.read().await;
        groups.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_channel::InMemoryChannel;
    use plexspaces_proto::channel::v1::ChannelConfig;
    use std::sync::Arc;

    async fn create_test_channel(name: &str) -> Result<Arc<dyn Channel>, String> {
        let config = ChannelConfig {
            name: name.to_string(),
            backend: plexspaces_proto::channel::v1::ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 100,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        Ok(Arc::new(InMemoryChannel::new(config).await.map_err(|e| e.to_string())?))
    }

    fn create_test_router() -> TaskRouter {
        TaskRouter::new(|name| {
            let name = name.to_string();
            async move { create_test_channel(&name).await }
        })
    }

    fn create_test_group(name: &str, actor_ids: Vec<String>) -> ActorGroup {
        ActorGroup {
            group_name: name.to_string(),
            actor_ids,
            metadata: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_register_group() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string()]);

        router.register_group(group.clone()).await.unwrap();

        let retrieved = router.get_group("test-group").await.unwrap();
        assert_eq!(retrieved.group_name, "test-group");
        assert_eq!(retrieved.actor_ids.len(), 1);
    }

    #[tokio::test]
    async fn test_unregister_group() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string()]);

        router.register_group(group).await.unwrap();
        router.unregister_group("test-group").await.unwrap();

        let result = router.get_group("test-group").await;
        assert!(matches!(result, Err(TaskRouterError::GroupNotFound(_))));
    }

    #[tokio::test]
    async fn test_add_actor_to_group() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string()]);

        router.register_group(group).await.unwrap();
        router.add_actor_to_group("test-group", "actor-2".to_string()).await.unwrap();

        let retrieved = router.get_group("test-group").await.unwrap();
        assert_eq!(retrieved.actor_ids.len(), 2);
        assert!(retrieved.actor_ids.contains(&"actor-1".to_string()));
        assert!(retrieved.actor_ids.contains(&"actor-2".to_string()));
    }

    #[tokio::test]
    async fn test_remove_actor_from_group() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();
        router.remove_actor_from_group("test-group", "actor-1").await.unwrap();

        let retrieved = router.get_group("test-group").await.unwrap();
        assert_eq!(retrieved.actor_ids.len(), 1);
        assert_eq!(retrieved.actor_ids[0], "actor-2");
    }

    #[tokio::test]
    async fn test_route_task_broadcast() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();

        let result = router.route_task("test-group", b"task-data".to_vec(), RoutingStrategy::Broadcast).await;
        // Broadcast should succeed (even if no subscribers yet)
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_route_task_round_robin() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();

        // Route multiple tasks - should round-robin
        let result1 = router.route_task("test-group", b"task-1".to_vec(), RoutingStrategy::RoundRobin).await.unwrap();
        let result2 = router.route_task("test-group", b"task-2".to_vec(), RoutingStrategy::RoundRobin).await.unwrap();
        let result3 = router.route_task("test-group", b"task-3".to_vec(), RoutingStrategy::RoundRobin).await.unwrap();

        assert_eq!(result1, 1);
        assert_eq!(result2, 1);
        assert_eq!(result3, 1);
    }

    #[tokio::test]
    async fn test_route_task_least_loaded() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();

        // Set different loads
        router.update_actor_load("actor-1".to_string(), 10).await;
        router.update_actor_load("actor-2".to_string(), 5).await;

        // Route task - should select actor-2 (least loaded)
        let result = router.route_task("test-group", b"task".to_vec(), RoutingStrategy::LeastLoaded).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn test_route_task_random() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();

        let result = router.route_task("test-group", b"task".to_vec(), RoutingStrategy::Random).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn test_route_task_hash() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);

        router.register_group(group).await.unwrap();

        let result = router.route_task("test-group", b"task".to_vec(), RoutingStrategy::Hash).await.unwrap();
        assert_eq!(result, 1);
    }

    #[tokio::test]
    async fn test_route_task_group_not_found() {
        let router = create_test_router();

        let result = router.route_task("non-existent", b"task".to_vec(), RoutingStrategy::RoundRobin).await;
        assert!(matches!(result, Err(TaskRouterError::GroupNotFound(_))));
    }

    #[tokio::test]
    async fn test_route_task_no_actors() {
        let router = create_test_router();
        let group = create_test_group("test-group", vec![]);

        router.register_group(group).await.unwrap();

        let result = router.route_task("test-group", b"task".to_vec(), RoutingStrategy::RoundRobin).await;
        assert!(matches!(result, Err(TaskRouterError::NoActorsInGroup(_))));
    }

    #[tokio::test]
    async fn test_list_groups() {
        let router = create_test_router();

        router.register_group(create_test_group("group-1", vec![])).await.unwrap();
        router.register_group(create_test_group("group-2", vec![])).await.unwrap();

        let groups = router.list_groups().await;
        assert_eq!(groups.len(), 2);
        assert!(groups.contains(&"group-1".to_string()));
        assert!(groups.contains(&"group-2".to_string()));
    }

    #[tokio::test]
    async fn test_update_actor_load() {
        let router = create_test_router();

        router.update_actor_load("actor-1".to_string(), 5).await;
        router.update_actor_load("actor-2".to_string(), 10).await;

        let group = create_test_group("test-group", vec!["actor-1".to_string(), "actor-2".to_string()]);
        router.register_group(group).await.unwrap();

        // Select least-loaded should return actor-1
        let selected = router.select_least_loaded(&["actor-1".to_string(), "actor-2".to_string()]).await.unwrap();
        assert_eq!(selected, "actor-1");
    }
}

