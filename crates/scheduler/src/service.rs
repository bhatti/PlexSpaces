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

//! gRPC service implementation for SchedulingService.

use crate::capacity_tracker::CapacityTracker;
use crate::state_store::SchedulingStateStore;
use plexspaces_channel::Channel;
use plexspaces_core::RequestContext;
use plexspaces_proto::{
    channel::v1::ChannelMessage,
    prost_types,
    scheduling::v1::{
        scheduling_service_server::SchedulingService, GetNodeCapacityRequest,
        GetNodeCapacityResponse, GetSchedulingStatusRequest, GetSchedulingStatusResponse,
        ListNodeCapacitiesRequest, ListNodeCapacitiesResponse, NodeCapacityEntry,
        ScheduleActorRequest, ScheduleActorResponse, SchedulingRequest, SchedulingStatus,
    },
};
use prost::Message;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use tonic::{Request, Response, Status};

/// SchedulingService gRPC implementation
pub struct SchedulingServiceImpl {
    /// State store for scheduling requests
    state_store: Arc<dyn SchedulingStateStore>,
    /// Channel for publishing scheduling requests
    request_channel: Arc<dyn Channel>,
    /// Capacity tracker for node capacity queries
    capacity_tracker: Arc<CapacityTracker>,
}

impl SchedulingServiceImpl {
    /// Create a new scheduling service implementation
    pub fn new(
        state_store: Arc<dyn SchedulingStateStore>,
        request_channel: Arc<dyn Channel>,
        capacity_tracker: Arc<CapacityTracker>,
    ) -> Self {
        Self {
            state_store,
            request_channel,
            capacity_tracker,
        }
    }
    
}

#[tonic::async_trait]
impl SchedulingService for SchedulingServiceImpl {
    async fn schedule_actor(
        &self,
        request: Request<ScheduleActorRequest>,
    ) -> Result<Response<ScheduleActorResponse>, Status> {
        let req = request.into_inner();

        // Validate request
        if req.requirements.is_none() {
            return Err(Status::invalid_argument("requirements is required"));
        }

        // Generate request ID if not provided
        let request_id = if req.request_id.is_empty() {
            ulid::Ulid::new().to_string()
        } else {
            req.request_id
        };

        // Create scheduling request
        let scheduling_request = SchedulingRequest {
            request_id: request_id.clone(),
            requirements: req.requirements,
            namespace: req.namespace,
            tenant_id: req.tenant_id,
            status: SchedulingStatus::SchedulingStatusPending as i32,
            selected_node_id: String::new(),
            actor_id: String::new(),
            error_message: String::new(),
            created_at: Some(prost_types::Timestamp::from(SystemTime::now())),
            scheduled_at: None,
            completed_at: None,
        };

        // Store request in state store (PENDING)
        self.state_store
            .store_request(scheduling_request.clone())
            .await
            .map_err(|e| Status::internal(format!("Failed to store request: {}", e)))?;

        // Publish to scheduling:requests channel
        let mut payload = Vec::new();
        scheduling_request
            .encode(&mut payload)
            .map_err(|e| Status::internal(format!("Failed to encode request: {}", e)))?;

        let channel_msg = ChannelMessage {
            id: ulid::Ulid::new().to_string(),
            channel: "scheduling:requests".to_string(),
            sender_id: String::new(),
            payload,
            headers: HashMap::new(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            partition_key: String::new(),
            correlation_id: String::new(),
            delivery_count: 0,
            reply_to: String::new(),
        };

        self.request_channel
            .publish(channel_msg)
            .await
            .map_err(|e| Status::internal(format!("Failed to publish request: {}", e)))?;

        // Return immediately with request_id and PENDING status
        Ok(Response::new(ScheduleActorResponse {
            request_id,
            status: SchedulingStatus::SchedulingStatusPending as i32,
            node_id: String::new(),
            node_address: String::new(),
            error_message: String::new(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
        }))
    }

    async fn get_scheduling_status(
        &self,
        request: Request<GetSchedulingStatusRequest>,
    ) -> Result<Response<GetSchedulingStatusResponse>, Status> {
        let req = request.into_inner();

        // Query state store for request
        let scheduling_request = self
            .state_store
            .get_request(&req.request_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get request: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Scheduling request {} not found", req.request_id)))?;

        Ok(Response::new(GetSchedulingStatusResponse {
            request: Some(scheduling_request),
        }))
    }

    async fn get_node_capacity(
        &self,
        request: Request<GetNodeCapacityRequest>,
    ) -> Result<Response<GetNodeCapacityResponse>, Status> {
        // Create RequestContext from request metadata (before consuming request)
        // Scheduler uses internal context for system-level operations
        // Scheduler operations always use internal context
        let ctx = RequestContext::internal();
        
        // Now consume request
        let req = request.into_inner();

        // Query capacity tracker for node capacity
        let capacity = self
            .capacity_tracker
            .get_node_capacity(&ctx, &req.node_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to get node capacity: {}", e)))?
            .ok_or_else(|| Status::not_found(format!("Node {} not found", req.node_id)))?;

        Ok(Response::new(GetNodeCapacityResponse {
            capacity: Some(capacity),
        }))
    }

    async fn list_node_capacities(
        &self,
        request: Request<ListNodeCapacitiesRequest>,
    ) -> Result<Response<ListNodeCapacitiesResponse>, Status> {
        // Create RequestContext from request metadata (before consuming request)
        // Scheduler uses internal context for system-level operations
        // Scheduler operations always use internal context
        let ctx = RequestContext::internal();
        
        // Now consume request
        let req = request.into_inner();

        // Extract filters
        let label_filter = if req.label_filters.is_empty() {
            None
        } else {
            Some(req.label_filters)
        };

        // Query capacity tracker for node capacities (no min_resources filter for now)
        let capacities = self
            .capacity_tracker
            .list_node_capacities(&ctx, label_filter.as_ref(), None)
            .await
            .map_err(|e| Status::internal(format!("Failed to list node capacities: {}", e)))?;

        // Convert to NodeCapacityEntry format
        let entries: Vec<NodeCapacityEntry> = capacities
            .into_iter()
            .map(|(node_id, capacity)| NodeCapacityEntry {
                node_id,
                capacity: Some(capacity),
            })
            .collect();

        // TODO: Apply pagination (for now, return all results)
        // In the future, we should paginate based on req.page

        let total_count = entries.len();
        Ok(Response::new(ListNodeCapacitiesResponse {
            capacities: entries,
            page: req.page.map(|_pr| {
                // Create a simple page response (no next page for now)
                plexspaces_proto::common::v1::PageResponse {
                    total_size: total_count as i32,
                    offset: 0,
                    limit: total_count as i32,
                    has_next: false,
                }
            }),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capacity_tracker::CapacityTracker;
    use crate::state_store::memory::MemorySchedulingStateStore;
    use plexspaces_channel::InMemoryChannel;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_proto::{
        actor::v1::ActorResourceRequirements,
        channel::v1::ChannelConfig,
        common::v1::ResourceSpec,
    };
    use std::sync::Arc;

    fn create_test_service() -> (
        SchedulingServiceImpl,
        Arc<MemorySchedulingStateStore>,
        Arc<InMemoryChannel>,
    ) {
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
        let channel = Arc::new(
            futures::executor::block_on(InMemoryChannel::new(channel_config)).unwrap(),
        );

        let service = SchedulingServiceImpl::new(
            state_store.clone(),
            channel.clone(),
            capacity_tracker,
        );

        (service, state_store, channel)
    }

    #[tokio::test]
    async fn test_schedule_actor() {
        let (service, state_store, _) = create_test_service();

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

        let response = service
            .schedule_actor(Request::new(req))
            .await
            .unwrap()
            .into_inner();

        assert_eq!(response.status, SchedulingStatus::SchedulingStatusPending as i32);
        assert!(!response.request_id.is_empty());

        // Verify request was stored
        let stored = state_store.get_request(&response.request_id).await.unwrap();
        assert!(stored.is_some());
        assert_eq!(
            stored.unwrap().status,
            SchedulingStatus::SchedulingStatusPending as i32
        );
    }

    #[tokio::test]
    async fn test_schedule_actor_missing_requirements() {
        let (service, _, _) = create_test_service();

        let req = ScheduleActorRequest {
            requirements: None,
            namespace: "default".to_string(),
            tenant_id: "default".to_string(),
            request_id: String::new(),
        };

        let result = service.schedule_actor(Request::new(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::InvalidArgument);
    }

    #[tokio::test]
    async fn test_get_scheduling_status() {
        let (service, state_store, _) = create_test_service();

        // Create and store a request
        let request_id = "test-request-1".to_string();
        let scheduling_request = SchedulingRequest {
            request_id: request_id.clone(),
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
            status: SchedulingStatus::SchedulingStatusScheduled as i32,
            selected_node_id: "node-1".to_string(),
            actor_id: "actor-1".to_string(),
            error_message: String::new(),
            created_at: Some(prost_types::Timestamp::from(SystemTime::now())),
            scheduled_at: Some(prost_types::Timestamp::from(SystemTime::now())),
            completed_at: Some(prost_types::Timestamp::from(SystemTime::now())),
        };

        state_store.store_request(scheduling_request.clone()).await.unwrap();

        // Get status
        let req = GetSchedulingStatusRequest {
            request_id: request_id.clone(),
        };
        let response = service
            .get_scheduling_status(Request::new(req))
            .await
            .unwrap()
            .into_inner();

        assert!(response.request.is_some());
        let returned_request = response.request.unwrap();
        assert_eq!(returned_request.request_id, request_id);
        assert_eq!(
            returned_request.status,
            SchedulingStatus::SchedulingStatusScheduled as i32
        );
        assert_eq!(returned_request.selected_node_id, "node-1");
    }

    #[tokio::test]
    async fn test_get_scheduling_status_not_found() {
        let (service, _, _) = create_test_service();

        let req = GetSchedulingStatusRequest {
            request_id: "non-existent".to_string(),
        };

        let result = service.get_scheduling_status(Request::new(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_get_node_capacity_not_found() {
        let (service, _, _) = create_test_service();

        let req = GetNodeCapacityRequest {
            node_id: "non-existent".to_string(),
        };

        let result = service.get_node_capacity(Request::new(req)).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code(), tonic::Code::NotFound);
    }

    #[tokio::test]
    async fn test_list_node_capacities() {
        let (service, _, _) = create_test_service();

        let req = ListNodeCapacitiesRequest {
            label_filters: HashMap::new(),
            page: None,
        };

        let response = service
            .list_node_capacities(Request::new(req))
            .await
            .unwrap()
            .into_inner();

        // Should return empty list (no nodes registered)
        assert_eq!(response.capacities.len(), 0);
    }
}
