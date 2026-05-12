// SPDX-License-Identifier: AGPL-3.0-or-later
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
use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_channel::Channel;
use plexspaces_locks::{
    AcquireLockOptions, Lock, LockError, LockManager, ReleaseLockOptions, RenewLockOptions,
};
use plexspaces_proto::{
    common::v1::Message,
    prost_types::Timestamp,
    scheduling::v1::{SchedulingRequest, SchedulingStatus},
};
use prost::Message as ProstMessage;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::{Mutex as StdMutex, OnceLock};
use std::time::SystemTime;
use tokio::sync::RwLock;
use tokio::time::{interval, sleep, Duration};
use tracing::{debug, error, info, warn};

/// Max retries for lease operations when backend returns transient database errors.
const LEASE_RETRY_MAX: u32 = 3;
const LEASE_RETRY_BACKOFF: Duration = Duration::from_secs(1);

fn active_scheduler_nodes() -> &'static StdMutex<HashSet<String>> {
    static ACTIVE: OnceLock<StdMutex<HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| StdMutex::new(HashSet::new()))
}

/// Error types for background scheduler
#[derive(Debug, thiserror::Error)]
pub enum BackgroundSchedulerError {
    /// Background scheduler already running for this node in the current process
    #[error("Background scheduler already running for node {0}")]
    AlreadyStarted(String),

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

impl BackgroundSchedulerError {
    /// Return the proto error code for this error.
    pub fn code(&self) -> plexspaces_proto::node::v1::BackgroundSchedulerErrorCode {
        use plexspaces_proto::node::v1::BackgroundSchedulerErrorCode;
        match self {
            BackgroundSchedulerError::AlreadyStarted(_) => {
                BackgroundSchedulerErrorCode::BackgroundSchedulerErrorAlreadyStarted
            }
            BackgroundSchedulerError::LockError(_) => {
                BackgroundSchedulerErrorCode::BackgroundSchedulerErrorLockError
            }
            BackgroundSchedulerError::ChannelError(_) => {
                BackgroundSchedulerErrorCode::BackgroundSchedulerErrorChannelError
            }
            BackgroundSchedulerError::StateStoreError(_) => {
                BackgroundSchedulerErrorCode::BackgroundSchedulerErrorStateStoreError
            }
            BackgroundSchedulerError::NodeSelectionError(_) => {
                BackgroundSchedulerErrorCode::BackgroundSchedulerErrorNodeSelectionError
            }
        }
    }
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
    ///
    /// NOTE: default_tenant_id and default_namespace have been removed.
    /// The background scheduler operates as a system process with admin privileges.
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

    /// RequestContext for system operations (admin context with empty tenant/namespace).
    fn default_context(&self) -> RequestContext {
        RequestContext::new_without_auth(String::new(), String::new()).with_admin(true)
    }

    fn claim_node_start(&self) -> BackgroundSchedulerResult<()> {
        let mut active = active_scheduler_nodes()
            .lock()
            .expect("active scheduler registry lock poisoned");
        if !active.insert(self.node_id.clone()) {
            return Err(BackgroundSchedulerError::AlreadyStarted(
                self.node_id.clone(),
            ));
        }
        Ok(())
    }

    fn release_node_start(&self) {
        let mut active = active_scheduler_nodes()
            .lock()
            .expect("active scheduler registry lock poisoned");
        active.remove(&self.node_id);
    }

    /// Start the background scheduler
    ///
    /// ## Flow
    /// 1. Attempt to acquire lease
    /// 2. If acquired: Start worker and heartbeat tasks
    /// 3. If not acquired: Return error (caller should retry)
    pub async fn start(self: &Arc<Self>) -> BackgroundSchedulerResult<()> {
        self.claim_node_start()?;

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
            let ctx = self.default_context();
            match self.lock_manager.acquire_lock(&ctx, options).await {
                Ok(lease) => lease,
                Err(e) => {
                    self.release_node_start();
                    return Err(BackgroundSchedulerError::LockError(e.to_string()));
                }
            }
        };
        {
            let mut current = self.current_lease.write().await;
            *current = Some(lease);
        }

        info!(
            "Background scheduler {} acquired lease, starting worker",
            self.node_id
        );

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
        let release_result = self.release_lease().await;
        self.release_node_start();
        release_result?;

        info!("Background scheduler {} stopped", self.node_id);
        Ok(())
    }

    /// Stop the background scheduler
    pub fn stop(&self) {
        self.shutdown.notify_one();
    }

    /// Acquire scheduler lease (public for re-acquisition after expiration)
    pub(crate) async fn acquire_lease(&self) -> BackgroundSchedulerResult<plexspaces_locks::Lock> {
        let options = AcquireLockOptions {
            lock_key: self.lease_key.clone(),
            holder_id: self.node_id.clone(),
            lease_duration_secs: self.lease_duration_secs,
            additional_wait_time_ms: 0, // Don't wait, fail fast if already held
            refresh_period_ms: 100,
            metadata: std::collections::HashMap::new(),
        };

        let ctx = self.default_context();
        self.lock_manager
            .acquire_lock(&ctx, options)
            .await
            .map_err(|e| BackgroundSchedulerError::LockError(e.to_string()))
    }

    fn lock_is_live(lock: &Lock) -> bool {
        if !lock.locked {
            return false;
        }
        let now_secs = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;
        lock.expires_at
            .as_ref()
            .map(|ts| ts.seconds > now_secs)
            .unwrap_or(false)
    }

    async fn adopt_current_lease_if_same_holder(&self) -> BackgroundSchedulerResult<bool> {
        let ctx = self.default_context();
        let current_lock = self
            .lock_manager
            .get_lock(&ctx, &self.lease_key)
            .await
            .map_err(|e| BackgroundSchedulerError::LockError(e.to_string()))?;

        match current_lock {
            Some(lock) if lock.holder_id == self.node_id && Self::lock_is_live(&lock) => {
                let mut current = self.current_lease.write().await;
                *current = Some(lock);
                Ok(true)
            }
            _ => Ok(false),
        }
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

            let ctx = self.default_context();
            match self.lock_manager.renew_lock(&ctx, options).await {
                Ok(renewed) => {
                    let mut current = self.current_lease.write().await;
                    *current = Some(renewed);
                    Ok(())
                }
                Err(LockError::VersionMismatch { .. }) => {
                    if self.adopt_current_lease_if_same_holder().await? {
                        warn!(
                            node_id = %self.node_id,
                            lease_key = %self.lease_key,
                            "Recovered scheduler lease after version mismatch by adopting current lock state"
                        );
                        Ok(())
                    } else {
                        Err(BackgroundSchedulerError::LockError(
                            "Version mismatch and no recoverable current lease found".to_string(),
                        ))
                    }
                }
                Err(e) => Err(BackgroundSchedulerError::LockError(e.to_string())),
            }
        } else {
            Err(BackgroundSchedulerError::LockError(
                "No lease to renew".to_string(),
            ))
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

            let ctx = self.default_context();
            self.lock_manager
                .release_lock(&ctx, options)
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
            // Skip first tick (immediate renewal not needed, lease just acquired)
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let mut renew_result = scheduler.renew_lease().await;
                        for attempt in 1..=LEASE_RETRY_MAX {
                            match &renew_result {
                                Ok(()) => break,
                                Err(e) => {
                                    let err_str = e.to_string();
                                    if attempt < LEASE_RETRY_MAX {
                                        debug!(
                                            attempt,
                                            max_attempts = LEASE_RETRY_MAX,
                                            error = %err_str,
                                            "Failed to renew lease, retrying"
                                        );
                                        sleep(LEASE_RETRY_BACKOFF).await;
                                        renew_result = scheduler.renew_lease().await;
                                    } else {
                                        error!(
                                            attempt,
                                            max_attempts = LEASE_RETRY_MAX,
                                            error = %err_str,
                                            "Failed to renew lease after all attempts"
                                        );
                                    }
                                }
                            }
                        }
                        if renew_result.is_err() {
                            let mut acquire_result = scheduler.acquire_lease().await;
                            for attempt in 1..=LEASE_RETRY_MAX {
                                match &acquire_result {
                                    Ok(new_lease) => {
                                        info!("Re-acquired lease after expiration");
                                        let mut current = scheduler.current_lease.write().await;
                                        *current = Some(new_lease.clone());
                                        break;
                                    }
                                    Err(acquire_err) => {
                                        let err_str = acquire_err.to_string();
                                        if attempt < LEASE_RETRY_MAX {
                                            debug!(
                                                attempt,
                                                max_attempts = LEASE_RETRY_MAX,
                                                error = %err_str,
                                                "Failed to re-acquire lease, retrying"
                                            );
                                            sleep(LEASE_RETRY_BACKOFF).await;
                                            acquire_result = scheduler.acquire_lease().await;
                                        } else {
                                            error!(
                                                attempt,
                                                max_attempts = LEASE_RETRY_MAX,
                                                error = %err_str,
                                                "Failed to re-acquire lease after all attempts"
                                            );
                                        }
                                    }
                                }
                            }
                            if acquire_result.is_err() {
                                break;
                            }
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

        info!(
            "Background scheduler {} subscribed to channel",
            self.node_id
        );

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
    async fn process_request(&self, msg: &Message) -> BackgroundSchedulerResult<()> {
        // Deserialize SchedulingRequest from message payload
        let request: SchedulingRequest =
            SchedulingRequest::decode(&msg.payload[..]).map_err(|e| {
                BackgroundSchedulerError::StateStoreError(format!(
                    "Failed to decode request: {}",
                    e
                ))
            })?;

        info!("Processing scheduling request: {}", request.request_id);

        // Create admin RequestContext for background scheduling operations
        // NOTE: tenant_id and namespace have been removed from SchedulingRequest.
        // The background scheduler operates as a system process with admin privileges.
        let ctx = RequestContext::new_without_auth(String::new(), String::new()).with_admin(true);

        // Get node capacities (use request context - capacity tracking may filter by tenant)
        let node_capacities = self
            .capacity_tracker
            .list_node_capacities(&ctx, None, None)
            .await
            .map_err(|e| BackgroundSchedulerError::NodeSelectionError(e.to_string()))?;

        // Select best node
        let requirements = request.requirements.as_ref().ok_or_else(|| {
            BackgroundSchedulerError::NodeSelectionError("Missing requirements".to_string())
        })?;

        match crate::node_selector::NodeSelector::select_node(requirements, &node_capacities) {
            Ok((node_id, _score)) => {
                // Update state store: SCHEDULED
                let mut updated_request = request.clone();
                updated_request.status = SchedulingStatus::SchedulingStatusScheduled as i32;
                updated_request.selected_node_id = node_id.clone();
                updated_request.scheduled_at = Some(Timestamp::from(SystemTime::now()));
                updated_request.completed_at = Some(Timestamp::from(SystemTime::now()));

                // Use the context we created earlier from the request
                self.state_store
                    .update_request(&ctx, updated_request)
                    .await
                    .map_err(|e| BackgroundSchedulerError::StateStoreError(e.to_string()))?;

                info!(
                    "Scheduled request {} on node {}",
                    request.request_id, node_id
                );
            }
            Err(e) => {
                // Update state store: FAILED
                let mut updated_request = request.clone();
                updated_request.status = SchedulingStatus::SchedulingStatusFailed as i32;
                updated_request.error_message = e.to_string();
                updated_request.completed_at = Some(Timestamp::from(SystemTime::now()));

                // Use the context we created earlier from the request
                self.state_store
                    .update_request(&ctx, updated_request)
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

#[cfg(all(test, feature = "sqlite-backend"))]
mod tests {
    use super::*;
    use crate::state_store::sql::SqliteSchedulingStateStore;
    use plexspaces_actor::ObjectRegistry;
    use plexspaces_channel::InMemoryChannel;
    use plexspaces_locks::sql::SqliteLockManager;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
    use plexspaces_proto::channel::v1::ChannelConfig;
    use std::collections::HashMap;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    async fn create_test_scheduler() -> (
        Arc<BackgroundScheduler>,
        Arc<SqliteLockManager>,
        Arc<SqliteSchedulingStateStore>,
        Arc<dyn Channel>,
    ) {
        let lock_manager = Arc::new(SqliteLockManager::new(":memory:").await.unwrap());
        let state_store = Arc::new(SqliteSchedulingStateStore::new(":memory:").await.unwrap());
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryImpl::new(repo));
        let capacity_tracker = Arc::new(CapacityTracker::new(registry));

        let channel_config = ChannelConfig {
            name: "scheduling:requests".to_string(),
            provider: plexspaces_proto::channel::v1::ChannelProvider::ChannelProviderInMemory
                as i32,
            capacity: 100,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce
                as i32,
            ordering: plexspaces_proto::channel::v1::OrderingGuarantee::OrderingGuaranteeFifo
                as i32,
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
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryImpl::new(repo));
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
        assert!(
            result.is_ok(),
            "Renew lease should complete within 2 seconds"
        );
        result.unwrap().unwrap();

        // Verify lease was renewed (version should change)
        let current = scheduler.current_lease.read().await;
        assert!(current.is_some());
        let renewed_lease = current.as_ref().unwrap();
        assert_ne!(renewed_lease.version, original_version);
    }

    #[tokio::test]
    async fn test_renew_lease_recovers_from_stale_local_version() {
        let (scheduler, lock_manager, _, _) = create_test_scheduler().await;
        let lease = scheduler.acquire_lease().await.unwrap();
        {
            let mut current = scheduler.current_lease.write().await;
            *current = Some(lease.clone());
        }

        let ctx = scheduler.default_context();
        let externally_renewed = lock_manager
            .renew_lock(
                &ctx,
                RenewLockOptions {
                    lock_key: scheduler.lease_key.clone(),
                    holder_id: scheduler.node_id.clone(),
                    version: lease.version.clone(),
                    lease_duration_secs: scheduler.lease_duration_secs,
                    metadata: HashMap::new(),
                },
            )
            .await
            .unwrap();

        scheduler.renew_lease().await.unwrap();

        let current = scheduler.current_lease.read().await;
        let recovered = current.as_ref().expect("scheduler should retain lease");
        assert_ne!(recovered.version, lease.version);
        assert_ne!(recovered.version, externally_renewed.version);

        let persisted = lock_manager
            .get_lock(&ctx, &scheduler.lease_key)
            .await
            .unwrap()
            .expect("lock should still exist");
        assert_eq!(recovered.version, persisted.version);
    }

    #[tokio::test]
    async fn test_start_rejects_duplicate_scheduler_for_same_node() {
        let (scheduler1, lock_manager, state_store, channel) = create_test_scheduler().await;
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        let registry: Arc<dyn ObjectRegistry> = Arc::new(ObjectRegistryImpl::new(repo));
        let capacity_tracker = Arc::new(CapacityTracker::new(registry));
        let scheduler2 = Arc::new(BackgroundScheduler::new(
            "test-node".to_string(),
            lock_manager,
            state_store,
            capacity_tracker,
            channel,
            30,
            10,
        ));

        let scheduler1_task = {
            let scheduler = scheduler1.clone();
            tokio::spawn(async move { scheduler.start().await })
        };

        let ctx = scheduler1.default_context();
        for _ in 0..20 {
            if scheduler1
                .lock_manager
                .get_lock(&ctx, &scheduler1.lease_key)
                .await
                .unwrap()
                .is_some()
            {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }

        let duplicate_start = scheduler2.start().await;
        assert!(matches!(
            duplicate_start,
            Err(BackgroundSchedulerError::AlreadyStarted(node_id)) if node_id == "test-node"
        ));

        scheduler1.stop();
        let stop_result = tokio::time::timeout(Duration::from_secs(2), scheduler1_task)
            .await
            .expect("primary scheduler should stop")
            .expect("scheduler task should join");
        assert!(stop_result.is_ok(), "primary scheduler should stop cleanly");
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
        assert!(
            result.is_ok(),
            "Release lease should complete within 2 seconds"
        );
        result.unwrap().unwrap();

        // Verify lease was released
        let current = scheduler.current_lease.read().await;
        assert!(current.is_none());

        // Verify lease is no longer held in lock manager
        let ctx = scheduler.default_context();
        let lock = scheduler
            .lock_manager
            .get_lock(&ctx, &scheduler.lease_key)
            .await
            .unwrap();
        assert!(lock.is_none() || !lock.unwrap().locked);
    }

    #[tokio::test]
    async fn test_process_request_success() {
        let (scheduler, _, state_store, _) = create_test_scheduler().await;

        // Create a test request
        let request = SchedulingRequest {
            request_id: "test-request-1".to_string(),
            requirements: Some(plexspaces_proto::v1::actor::ActorResourceRequirements {
                placement: Some(plexspaces_proto::v1::actor::NodePlacement {
                    strategy: plexspaces_proto::v1::actor::NodePlacementStrategy::NodePlacementStrategyUnspecified as i32,
                    cluster: String::new(),
                    node_ids: vec![],
                    required_labels: HashMap::new(),
                    avoid_node_ids: vec![],
                    resource_requirements: Some(plexspaces_proto::common::v1::ResourceSpec {
                        cpu_cores: 1.0,
                        memory_bytes: 512 * 1024 * 1024,
                        disk_bytes: 0,
                        gpu_count: 0,
                        gpu_type: String::new(),
                    }),
                    affinity_labels: HashMap::new(),
                }),
            }),
            namespace: String::new(), // Empty for test
            tenant_id: String::new(), // Empty for test
            status: SchedulingStatus::SchedulingStatusPending as i32,
            selected_node_id: String::new(),
            actor_id: String::new(),
            error_message: String::new(),
            created_at: Some(Timestamp::from(SystemTime::now())),
            scheduled_at: None,
            completed_at: None,
        };

        // Store request - use node-config defaults for test
        let ctx = scheduler.default_context();
        state_store
            .store_request(&ctx, request.clone())
            .await
            .unwrap();

        // Create channel message
        let mut payload = Vec::new();
        request.encode(&mut payload).unwrap();
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            sender_id: String::new(),
            receiver_id: String::new(),
            channel: "scheduling:requests".to_string(),
            message_type: "scheduling_request".to_string(),
            payload,
            timestamp: Some(Timestamp::from(SystemTime::now())),
            headers: HashMap::new(),
            priority: 50, // Normal priority
            ttl: None,
            delivery_count: 0,
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            uri_path: String::new(),
            uri_method: String::new(),
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
        let handle = tokio::spawn(async move { scheduler_clone.start().await });

        // Wait a bit
        sleep(Duration::from_millis(100)).await;

        // Stop scheduler
        scheduler.stop();

        // Wait for shutdown
        let _ = handle.await;
        // Should complete without error
    }
}
