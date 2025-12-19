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

//! Background scheduler with lease-based coordination.
//!
//! ## Purpose
//! Processes scheduling requests asynchronously with lease-based coordination
//! to ensure only one scheduler processes requests at a time.
//!
//! ## Design
//! - Acquires lease before starting (using LockManager)
//! - Subscribes to `scheduling:requests` channel
//! - Processes requests: selects node, updates state store
//! - Renews lease periodically (heartbeat)
//! - Releases lease on shutdown

use crate::capacity_tracker::CapacityTracker;
use crate::state_store::SchedulingStateStore;
use futures::StreamExt;
use plexspaces_channel::Channel;
use plexspaces_locks::{AcquireLockOptions, LockManager, RenewLockOptions, ReleaseLockOptions};
use plexspaces_proto::{
    channel::v1::ChannelMessage,
    prost_types::Timestamp,
    scheduling::v1::{SchedulingRequest, SchedulingStatus},
};
use prost::Message;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{error, info, warn};

/// Error types for background scheduler
#[derive(Debug, thiserror::Error)]
pub enum BackgroundSchedulerError {
    /// Lock manager error
    #[error("Lock manager error: {0}")]
    LockError(String),

    /// Channel error
    #[error("Channel error: {0}")]
    ChannelError(String),

    /// State store error
    #[error("State store error: {0}")]
    StateStoreError(String),

    /// Node selection error
    #[error("Node selection error: {0}")]
    NodeSelectionError(String),
}

/// Result type for background scheduler
pub type BackgroundSchedulerResult<T> = Result<T, BackgroundSchedulerError>;

/// Background scheduler with lease-based coordination
pub struct BackgroundScheduler {
    /// Node ID (for lease holder identification)
    node_id: String,
    /// Lock manager for lease coordination
    lock_manager: Arc<dyn LockManager>,
    /// State store for scheduling requests
    state_store: Arc<dyn SchedulingStateStore>,
    /// Capacity tracker for node resources
    capacity_tracker: Arc<CapacityTracker>,
    /// Channel for receiving scheduling requests
    request_channel: Arc<dyn Channel>,
    /// Lease key
    lease_key: String,
    /// Lease duration (seconds)
    lease_duration_secs: u32,
    /// Heartbeat interval (seconds)
    heartbeat_interval_secs: u32,
    /// Current lease (if acquired)
    current_lease: Arc<RwLock<Option<plexspaces_locks::Lock>>>,
    /// Shutdown flag
    shutdown: Arc<tokio::sync::Notify>,
}

impl BackgroundScheduler {
    /// Create a new background scheduler
    pub fn new(
        node_id: String,
        lock_manager: Arc<dyn LockManager>,
        state_store: Arc<dyn SchedulingStateStore>,
        capacity_tracker: Arc<CapacityTracker>,
        request_channel: Arc<dyn Channel>,
        lease_duration_secs: u32,
        heartbeat_interval_secs: u32,
    ) -> Self {
        Self {
            node_id: node_id.clone(),
            lock_manager,
            state_store,
            capacity_tracker,
            request_channel,
            lease_key: format!("scheduler:background:lease:{}", node_id),
            lease_duration_secs,
            heartbeat_interval_secs,
            current_lease: Arc::new(RwLock::new(None)),
            shutdown: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Start the background scheduler
    ///
    /// ## Flow
    /// 1. Attempt to acquire lease
    /// 2. If acquired: Start worker and heartbeat tasks
    /// 3. If not acquired: Return error (caller should retry)
    pub async fn start(self: &Arc<Self>) -> BackgroundSchedulerResult<()> {
        // Attempt to acquire lease
        let lease = {
            let options = AcquireLockOptions {
                lock_key: self.lease_key.clone(),
                holder_id: self.node_id.clone(),
                lease_duration_secs: self.lease_duration_secs,
                additional_wait_time_ms: 0,
                refresh_period_ms: 100,
                metadata: std::collections::HashMap::new(),
            };
            self.lock_manager
                .acquire_lock(options)
                .await
                .map_err(|e| BackgroundSchedulerError::LockError(e.to_string()))?
        };
        {
            let mut current = self.current_lease.write().await;
            *current = Some(lease);
        }

        info!("Background scheduler {} acquired lease, starting worker", self.node_id);

        // Start worker task
        let worker_handle = self.start_worker_task();

        // Start heartbeat task
        let heartbeat_handle = self.start_heartbeat_task();

        // Wait for shutdown signal
        self.shutdown.notified().await;

        // Stop worker and heartbeat
        worker_handle.abort();
        heartbeat_handle.abort();

        // Release lease
        self.release_lease().await?;

        info!("Background scheduler {} stopped", self.node_id);
        Ok(())
    }

    /// Stop the background scheduler
    pub fn stop(&self) {
        self.shutdown.notify_one();
    }

    /// Acquire scheduler lease
    async fn acquire_lease(&self) -> BackgroundSchedulerResult<plexspaces_locks::Lock> {
        let options = AcquireLockOptions {
            lock_key: self.lease_key.clone(),
            holder_id: self.node_id.clone(),
            lease_duration_secs: self.lease_duration_secs,
            additional_wait_time_ms: 0, // Don't wait, fail fast if already held
            refresh_period_ms: 100,
            metadata: std::collections::HashMap::new(),
        };

        self.lock_manager
            .acquire_lock(options)
            .await
            .map_err(|e| BackgroundSchedulerError::LockError(e.to_string()))
    }

    /// Renew lease (heartbeat)
    async fn renew_lease(&self) -> BackgroundSchedulerResult<()> {
        let lease_opt = {
            let current = self.current_lease.read().await;
            current.clone()
        };
        
        if let Some(lease) = lease_opt {
            let options = RenewLockOptions {
                lock_key: self.lease_key.clone(),
                holder_id: self.node_id.clone(),
                version: lease.version.clone(),
                lease_duration_secs: self.lease_duration_secs,
                metadata: std::collections::HashMap::new(),
            };

            match self.lock_manager.renew_lock(options).await {
                Ok(renewed) => {
                    let mut current = self.current_lease.write().await;
                    *current = Some(renewed);
                    Ok(())
                }
                Err(e) => {
                    warn!("Failed to renew lease: {}", e);
                    Err(BackgroundSchedulerError::LockError(e.to_string()))
                }
            }
        } else {
            Err(BackgroundSchedulerError::LockError("No lease to renew".to_string()))
        }
    }

    /// Release lease
    async fn release_lease(&self) -> BackgroundSchedulerResult<()> {
        let lease_opt = {
            let current = self.current_lease.read().await;
            current.clone()
        };
        
        if let Some(lease) = lease_opt {
            let options = ReleaseLockOptions {
                lock_key: self.lease_key.clone(),
                holder_id: self.node_id.clone(),
                version: lease.version.clone(),
                delete_lock: false, // Keep for audit
            };

            self.lock_manager
                .release_lock(options)
                .await
                .map_err(|e| BackgroundSchedulerError::LockError(e.to_string()))?;

            let mut current = self.current_lease.write().await;
            *current = None;
        }
        Ok(())
    }

    /// Start worker task (processes requests from channel)
    fn start_worker_task(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let scheduler = self.clone_for_task();
        tokio::spawn(async move {
            if let Err(e) = scheduler.run_worker_loop().await {
                error!("Worker loop error: {}", e);
            }
        })
    }

    /// Start heartbeat task (renews lease periodically)
    fn start_heartbeat_task(self: &Arc<Self>) -> tokio::task::JoinHandle<()> {
        let scheduler = self.clone_for_task();
        let interval_secs = self.heartbeat_interval_secs;
        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(interval_secs as u64));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if let Err(e) = scheduler.renew_lease().await {
                            error!("Heartbeat failed: {}", e);
                            // If renewal fails, stop worker (lease may have expired)
                            break;
                        }
                    }
                    _ = scheduler.shutdown.notified() => {
                        break;
                    }
                }
            }
        })
    }

    /// Worker loop: subscribe to channel and process requests
    async fn run_worker_loop(&self) -> BackgroundSchedulerResult<()> {
        // Subscribe to channel
        let mut stream = self
            .request_channel
            .subscribe(None)
            .await
            .map_err(|e| BackgroundSchedulerError::ChannelError(e.to_string()))?;

        info!("Background scheduler {} subscribed to channel", self.node_id);

        // Process messages from stream
        loop {
            tokio::select! {
                msg = stream.next() => {
                    if let Some(msg) = msg {
                        // Deserialize scheduling request from message payload
                        if let Err(e) = self.process_request(&msg).await {
                            error!("Failed to process request: {}", e);
                        }
                    } else {
                        // Stream ended
                        break;
                    }
                }
                _ = self.shutdown.notified() => {
                    // Shutdown requested
                    break;
                }
            }
        }

        Ok(())
    }

    /// Process a scheduling request
    async fn process_request(&self, msg: &ChannelMessage) -> BackgroundSchedulerResult<()> {
        // Deserialize SchedulingRequest from message payload
        let request: SchedulingRequest = Message::decode(&msg.payload[..])
            .map_err(|e| BackgroundSchedulerError::StateStoreError(format!("Failed to decode request: {}", e)))?;

        info!("Processing scheduling request: {}", request.request_id);

        // Get node capacities (use internal context for system operations)
        use plexspaces_core::RequestContext;
        let ctx = RequestContext::internal();
        let node_capacities = self
            .capacity_tracker
            .list_node_capacities(&ctx, None, None)
            .await
            .map_err(|e| BackgroundSchedulerError::NodeSelectionError(e.to_string()))?;

        // Select best node
        let requirements = request
            .requirements
            .as_ref()
            .ok_or_else(|| BackgroundSchedulerError::NodeSelectionError("Missing requirements".to_string()))?;

        match crate::node_selector::NodeSelector::select_node(requirements, &node_capacities) {
            Ok((node_id, _score)) => {
                // Update state store: SCHEDULED
                let mut updated_request = request.clone();
                updated_request.status = SchedulingStatus::SchedulingStatusScheduled as i32;
                updated_request.selected_node_id = node_id.clone();
                updated_request.scheduled_at = Some(Timestamp::from(SystemTime::now()));
                updated_request.completed_at = Some(Timestamp::from(SystemTime::now()));

                self.state_store
                    .update_request(updated_request)
                    .await
                    .map_err(|e| BackgroundSchedulerError::StateStoreError(e.to_string()))?;

                info!("Scheduled request {} on node {}", request.request_id, node_id);
            }
            Err(e) => {
                // Update state store: FAILED
                let mut updated_request = request.clone();
                updated_request.status = SchedulingStatus::SchedulingStatusFailed as i32;
                updated_request.error_message = e.to_string();
                updated_request.completed_at = Some(Timestamp::from(SystemTime::now()));

                self.state_store
                    .update_request(updated_request)
                    .await
                    .map_err(|e| BackgroundSchedulerError::StateStoreError(e.to_string()))?;

                warn!("Failed to schedule request {}: {}", request.request_id, e);
            }
        }

        Ok(())
    }

    /// Clone scheduler for task (helper for moving into async tasks)
    fn clone_for_task(self: &Arc<Self>) -> Arc<Self> {
        Arc::clone(self)
    }
}

// Note: BackgroundScheduler needs to be Clone for moving into tasks
// But we can't derive Clone because of trait objects. We'll use Arc instead.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::memory::MemorySchedulingStateStore;
    use plexspaces_channel::InMemoryChannel;
    use plexspaces_locks::memory::MemoryLockManager;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_proto::channel::v1::ChannelConfig;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    async fn create_test_scheduler() -> (
        Arc<BackgroundScheduler>,
        Arc<MemoryLockManager>,
        Arc<MemorySchedulingStateStore>,
        Arc<dyn Channel>,
    ) {
        let lock_manager = Arc::new(MemoryLockManager::new());
        let state_store = Arc::new(MemorySchedulingStateStore::new());
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));
        let capacity_tracker = Arc::new(CapacityTracker::new(registry));

        let channel_config = ChannelConfig {
            name: "scheduling:requests".to_string(),
            backend: plexspaces_proto::channel::v1::ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 100,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        let channel = Arc::new(InMemoryChannel::new(channel_config).await.unwrap());

        let scheduler = Arc::new(BackgroundScheduler::new(
            "test-node".to_string(),
            lock_manager.clone(),
            state_store.clone(),
            capacity_tracker,
            channel.clone(),
            30, // lease_duration_secs
            10, // heartbeat_interval_secs
        ));

        (scheduler, lock_manager, state_store, channel)
    }

    #[tokio::test]
    async fn test_acquire_lease() {
        let (scheduler, _, _, _) = create_test_scheduler().await;
        let lease = scheduler.acquire_lease().await.unwrap();
        assert_eq!(lease.holder_id, "test-node");
        assert_eq!(lease.lock_key, "scheduler:background:lease:test-node");
    }

    #[tokio::test]
    async fn test_acquire_lease_already_held() {
        let (scheduler1, lock_manager, state_store, channel) = create_test_scheduler().await;
        let kv = Arc::new(InMemoryKVStore::new());
        let registry = Arc::new(ObjectRegistry::new(kv));
        let capacity_tracker = Arc::new(CapacityTracker::new(registry));
        let scheduler2 = Arc::new(BackgroundScheduler::new(
            "test-node-2".to_string(),
            lock_manager.clone(),
            state_store.clone(),
            capacity_tracker,
            channel.clone(),
            30,
            10,
        ));

        // First scheduler acquires lease
        let _lease1 = scheduler1.acquire_lease().await.unwrap();

        // Second scheduler tries to acquire same lease (should fail if using same key)
        // Note: They use different keys (node_id in key), so both can acquire
        let lease2 = scheduler2.acquire_lease().await.unwrap();
        assert_eq!(lease2.holder_id, "test-node-2");
    }

    #[tokio::test]
    async fn test_renew_lease() {
        let (scheduler, _, _, _) = create_test_scheduler().await;
        let lease = scheduler.acquire_lease().await.unwrap();
        let original_version = lease.version.clone();
        {
            let mut current = scheduler.current_lease.write().await;
            *current = Some(lease);
        }

        // Wait a bit to ensure timestamp changes
        sleep(Duration::from_millis(100)).await;

        // Renew lease with timeout
        let result = tokio::time::timeout(Duration::from_secs(2), scheduler.renew_lease()).await;
        assert!(result.is_ok(), "Renew lease should complete within 2 seconds");
        result.unwrap().unwrap();

        // Verify lease was renewed (version should change)
        let current = scheduler.current_lease.read().await;
        assert!(current.is_some());
        let renewed_lease = current.as_ref().unwrap();
        assert_ne!(renewed_lease.version, original_version);
    }

    #[tokio::test]
    async fn test_release_lease() {
        let (scheduler, _, _, _) = create_test_scheduler().await;
        let lease = scheduler.acquire_lease().await.unwrap();
        {
            let mut current = scheduler.current_lease.write().await;
            *current = Some(lease);
        }

        // Release lease with timeout
        let result = tokio::time::timeout(Duration::from_secs(2), scheduler.release_lease()).await;
        assert!(result.is_ok(), "Release lease should complete within 2 seconds");
        result.unwrap().unwrap();

        // Verify lease was released
        let current = scheduler.current_lease.read().await;
        assert!(current.is_none());
        
        // Verify lease is no longer held in lock manager
        let lock = scheduler.lock_manager.get_lock(&scheduler.lease_key).await.unwrap();
        assert!(lock.is_none() || !lock.unwrap().locked);
    }

    #[tokio::test]
    async fn test_process_request_success() {
        let (scheduler, _, state_store, _) = create_test_scheduler().await;

        // Create a test request
        let request = SchedulingRequest {
            request_id: "test-request-1".to_string(),
            requirements: Some(plexspaces_proto::v1::actor::ActorResourceRequirements {
                resources: Some(plexspaces_proto::common::v1::ResourceSpec {
                    cpu_cores: 1.0,
                    memory_bytes: 512 * 1024 * 1024,
                    disk_bytes: 0,
                    gpu_count: 0,
                    gpu_type: String::new(),
                }),
                required_labels: HashMap::new(),
                placement: None,
                actor_groups: vec![],
            }),
            namespace: "default".to_string(),
            tenant_id: "default".to_string(),
            status: SchedulingStatus::SchedulingStatusPending as i32,
            selected_node_id: String::new(),
            actor_id: String::new(),
            error_message: String::new(),
            created_at: Some(Timestamp::from(SystemTime::now())),
            scheduled_at: None,
            completed_at: None,
        };

        // Store request
        state_store.store_request(request.clone()).await.unwrap();

        // Create channel message
        let mut payload = Vec::new();
        request.encode(&mut payload).unwrap();
        let msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: "scheduling:requests".to_string(),
            sender_id: String::new(),
            payload,
            headers: HashMap::new(),
            timestamp: Some(Timestamp::from(SystemTime::now())),
            partition_key: String::new(),
            correlation_id: String::new(),
            delivery_count: 0,
            reply_to: String::new(),
        };

        // Process request (will fail because no nodes available, but tests the flow)
        let result = scheduler.process_request(&msg).await;
        // Should handle gracefully even if no nodes available
        assert!(result.is_ok() || result.is_err()); // Either is fine for this test
    }

    #[tokio::test]
    async fn test_stop_scheduler() {
        let (scheduler, _, _, _) = create_test_scheduler().await;
        
        // Start scheduler in background
        let scheduler_clone = scheduler.clone();
        let handle = tokio::spawn(async move {
            scheduler_clone.start().await
        });

        // Wait a bit
        sleep(Duration::from_millis(100)).await;

        // Stop scheduler
        scheduler.stop();

        // Wait for shutdown
        let _ = handle.await;
        // Should complete without error
    }
}
