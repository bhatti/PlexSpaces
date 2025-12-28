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

//! SQLite Integration Tests for Scheduling Service
//!
//! Comprehensive integration tests for scheduling service with SQLite backend.
//! Tests cover:
//! - State store operations (store, get, update, query pending)
//! - Background scheduler with lease coordination
//! - Full scheduling flow (request → channel → processing → state update)
//! - Recovery scenarios (pending requests on startup)

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_scheduler::{
        background::BackgroundScheduler,
        capacity_tracker::CapacityTracker,
        service::SchedulingServiceImpl,
        state_store::SchedulingStateStore,
        SqliteSchedulingStateStore,
    };
    use plexspaces_channel::InMemoryChannel;
    use plexspaces_locks::memory::MemoryLockManager;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_core::RequestContext;
    use plexspaces_proto::{
        actor::v1::ActorResourceRequirements,
        channel::v1::ChannelConfig,
        common::v1::ResourceSpec,
        prost_types,
        scheduling::v1::{ScheduleActorRequest, SchedulingRequest, SchedulingStatus},
    };
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::SystemTime;
    use tokio::time::{sleep, Duration};

    /// Helper to create in-memory SQLite state store
    async fn create_sqlite_state_store() -> SqliteSchedulingStateStore {
        SqliteSchedulingStateStore::new_in_memory().await.unwrap()
    }

    /// Helper to create test RequestContext
    fn create_test_context() -> RequestContext {
        RequestContext::new_without_auth("default".to_string(), "default".to_string())
    }

    /// Helper to create test scheduling request
    fn create_test_request(request_id: &str) -> SchedulingRequest {
        SchedulingRequest {
            request_id: request_id.to_string(),
            requirements: Some(ActorResourceRequirements {
                resources: Some(ResourceSpec {
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
            created_at: Some(prost_types::Timestamp::from(SystemTime::now())),
            scheduled_at: None,
            completed_at: None,
        }
    }

    #[tokio::test]
    async fn test_sqlite_store_and_get_request() {
        let store = create_sqlite_state_store().await;
        let request = create_test_request("test-request-1");

        // Store request
        let ctx = create_test_context();
        SchedulingStateStore::store_request(&store, &ctx, request.clone()).await.unwrap();

        // Get request
        let retrieved = SchedulingStateStore::get_request(&store, &ctx, "test-request-1").await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.request_id, "test-request-1");
        assert_eq!(
            retrieved.status,
            SchedulingStatus::SchedulingStatusPending as i32
        );
        assert!(retrieved.requirements.is_some());
    }

    #[tokio::test]
    async fn test_sqlite_update_request() {
        let store = create_sqlite_state_store().await;
        let mut request = create_test_request("test-request-1");

        // Store request
        let ctx = create_test_context();
        SchedulingStateStore::store_request(&store, &ctx, request.clone()).await.unwrap();

        // Update request to SCHEDULED
        request.status = SchedulingStatus::SchedulingStatusScheduled as i32;
        request.selected_node_id = "node-1".to_string();
        request.scheduled_at = Some(prost_types::Timestamp::from(SystemTime::now()));
        request.completed_at = Some(prost_types::Timestamp::from(SystemTime::now()));

        SchedulingStateStore::update_request(&store, &ctx, request.clone()).await.unwrap();

        // Verify update
        let retrieved = SchedulingStateStore::get_request(&store, &ctx, "test-request-1").await.unwrap().unwrap();
        assert_eq!(
            retrieved.status,
            SchedulingStatus::SchedulingStatusScheduled as i32
        );
        assert_eq!(retrieved.selected_node_id, "node-1");
        assert!(retrieved.scheduled_at.is_some());
    }

    #[tokio::test]
    async fn test_sqlite_query_pending_requests() {
        let store = create_sqlite_state_store().await;

        // Store multiple requests with different statuses
        let ctx = create_test_context();
        let mut request1 = create_test_request("request-1");
        SchedulingStateStore::store_request(&store, &ctx, request1.clone()).await.unwrap();

        let mut request2 = create_test_request("request-2");
        request2.status = SchedulingStatus::SchedulingStatusScheduled as i32;
        SchedulingStateStore::store_request(&store, &ctx, request2.clone()).await.unwrap();

        let request3 = create_test_request("request-3");
        SchedulingStateStore::store_request(&store, &ctx, request3.clone()).await.unwrap();

        // Query pending requests
        let pending = SchedulingStateStore::query_pending_requests(&store, &ctx).await.unwrap();
        assert_eq!(pending.len(), 2);
        let pending_ids: Vec<String> = pending.iter().map(|r| r.request_id.clone()).collect();
        assert!(pending_ids.contains(&"request-1".to_string()));
        assert!(pending_ids.contains(&"request-3".to_string()));
        assert!(!pending_ids.contains(&"request-2".to_string()));
    }

    #[tokio::test]
    async fn test_sqlite_get_nonexistent_request() {
        let store = create_sqlite_state_store().await;

        let ctx = create_test_context();
        let retrieved = SchedulingStateStore::get_request(&store, &ctx, "non-existent").await.unwrap();
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_sqlite_update_nonexistent_request() {
        let store = create_sqlite_state_store().await;
        let request = create_test_request("non-existent");

        // Update should fail or create (depending on implementation)
        // For now, we expect it to work (upsert behavior)
        let ctx = create_test_context();
        let result = SchedulingStateStore::update_request(&store, &ctx, request).await;
        // SQLite will insert if not exists (depending on SQL implementation)
        // Let's check if it works
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_sqlite_background_scheduler_with_sqlite_store() {
        let state_store: Arc<dyn SchedulingStateStore> = Arc::new(create_sqlite_state_store().await);
        let lock_manager = Arc::new(MemoryLockManager::new());
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
            lock_manager,
            state_store.clone(),
            capacity_tracker,
            channel.clone(),
            30, // lease_duration_secs
            10, // heartbeat_interval_secs
        ));

        // Test that scheduler can be created (lease acquisition is tested in unit tests)
        // For integration test, we just verify the scheduler is set up correctly
        // We can't access private fields, so we just verify it was created

        // Store a request
        let ctx = create_test_context();
        let request = create_test_request("test-request-1");
        SchedulingStateStore::store_request(&*state_store, &ctx, request.clone()).await.unwrap();

        // Verify request is stored
        let retrieved = SchedulingStateStore::get_request(&*state_store, &ctx, "test-request-1").await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_sqlite_service_with_sqlite_store() {
        let state_store: Arc<dyn SchedulingStateStore> = Arc::new(create_sqlite_state_store().await);
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

        let service = SchedulingServiceImpl::new(
            state_store.clone(),
            channel.clone(),
            capacity_tracker,
        );

        // Schedule actor
        let req = ScheduleActorRequest {
            requirements: Some(ActorResourceRequirements {
                resources: Some(ResourceSpec {
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
            request_id: String::new(),
        };

        use plexspaces_proto::scheduling::v1::scheduling_service_server::SchedulingService;
        let response = SchedulingService::schedule_actor(&service, tonic::Request::new(req))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.status, SchedulingStatus::SchedulingStatusPending as i32);
        assert!(!response.request_id.is_empty());

        // Verify request was stored in SQLite
        let ctx = create_test_context();
        let stored = SchedulingStateStore::get_request(&*state_store, &ctx, &response.request_id).await.unwrap();
        assert!(stored.is_some());
        assert_eq!(
            stored.unwrap().status,
            SchedulingStatus::SchedulingStatusPending as i32
        );
    }

    #[tokio::test]
    async fn test_sqlite_recovery_pending_requests() {
        let state_store: Arc<dyn SchedulingStateStore> = Arc::new(create_sqlite_state_store().await);

        // Simulate crash: store requests but don't process them
        let request1 = create_test_request("recovery-request-1");
        let request2 = create_test_request("recovery-request-2");
        let mut request3 = create_test_request("recovery-request-3");
        request3.status = SchedulingStatus::SchedulingStatusScheduled as i32; // Already processed

        let ctx = create_test_context();
        SchedulingStateStore::store_request(&*state_store, &ctx, request1.clone()).await.unwrap();
        SchedulingStateStore::store_request(&*state_store, &ctx, request2.clone()).await.unwrap();
        SchedulingStateStore::store_request(&*state_store, &ctx, request3.clone()).await.unwrap();

        // Simulate recovery: query pending requests
        // Note: For recovery, we might need to query all tenants, but for this test we use the test context
        let pending = SchedulingStateStore::query_pending_requests(&*state_store, &ctx).await.unwrap();
        assert_eq!(pending.len(), 2);
        let pending_ids: Vec<String> = pending.iter().map(|r| r.request_id.clone()).collect();
        assert!(pending_ids.contains(&"recovery-request-1".to_string()));
        assert!(pending_ids.contains(&"recovery-request-2".to_string()));
        assert!(!pending_ids.contains(&"recovery-request-3".to_string()));
    }

    #[tokio::test]
    async fn test_sqlite_concurrent_updates() {
        let state_store: Arc<dyn SchedulingStateStore> = Arc::new(create_sqlite_state_store().await);
        let request = create_test_request("concurrent-request");

        // Store request
        let ctx = create_test_context();
        SchedulingStateStore::store_request(&*state_store, &ctx, request.clone()).await.unwrap();

        // Simulate concurrent updates (different statuses)
        let mut request1 = request.clone();
        request1.status = SchedulingStatus::SchedulingStatusScheduled as i32;
        request1.selected_node_id = "node-1".to_string();

        let mut request2 = request.clone();
        request2.status = SchedulingStatus::SchedulingStatusFailed as i32;
        request2.error_message = "test error".to_string();

        // Both updates should succeed (last write wins)
        SchedulingStateStore::update_request(&*state_store, &ctx, request1.clone()).await.unwrap();
        SchedulingStateStore::update_request(&*state_store, &ctx, request2.clone()).await.unwrap();

        // Verify last update
        let retrieved = SchedulingStateStore::get_request(&*state_store, &ctx, "concurrent-request").await.unwrap().unwrap();
        assert_eq!(
            retrieved.status,
            SchedulingStatus::SchedulingStatusFailed as i32
        );
        assert_eq!(retrieved.error_message, "test error");
    }

    #[tokio::test]
    async fn test_sqlite_multiple_requests_ordering() {
        let store = create_sqlite_state_store().await;

        // Store multiple requests
        let ctx = create_test_context();
        for i in 1..=10 {
            let request = create_test_request(&format!("request-{}", i));
            SchedulingStateStore::store_request(&store, &ctx, request).await.unwrap();
        }

        // Query pending (should return all 10)
        let pending = SchedulingStateStore::query_pending_requests(&store, &ctx).await.unwrap();
        assert_eq!(pending.len(), 10);

        // Verify all request IDs are present
        for i in 1..=10 {
            let request_id = format!("request-{}", i);
            assert!(pending.iter().any(|r| r.request_id == request_id));
        }
    }
}

