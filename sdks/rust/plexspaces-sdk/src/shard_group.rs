// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// SDK Helpers for ShardGroup (Data-Parallel Actors)
// Supports both WASM (ServiceLocator) and gRPC (optional feature)

use anyhow::{Context, Result};
use plexspaces_proto::actor::v1::{
    BulkUpdateShardGroupRequest, ConsistencyLevel, CreateShardGroupRequest,
    MapShardGroupRequest, PartitionStrategy, ScatterGatherRequest,
    ShardGroupAggregationStrategy,
};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use prost_types::Duration;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(feature = "grpc")]
use tonic::Request;

/// ShardGroup client trait - unified interface for WASM and gRPC
#[async_trait::async_trait]
pub trait ShardGroupClientTrait: Send + Sync {
    /// Create a ShardGroup with specified configuration
    async fn create_shard_group(
        &mut self,
        group_id: String,
        actor_type: String,
        shard_count: u32,
        partition_strategy: PartitionStrategy,
        labels: HashMap<String, String>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup>;

    /// Bulk update shard group (DPA UpdateFunction)
    async fn bulk_update(
        &mut self,
        group_id: String,
        updates: HashMap<String, serde_json::Value>,
        consistency_level: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse>;

    /// Map over all shards in parallel (DPA Map operator)
    async fn map(
        &mut self,
        group_id: String,
        query: serde_json::Value,
    ) -> Result<plexspaces_proto::actor::v1::MapShardGroupResponse>;

    /// Scatter-gather query (DPA Scatter-Gather)
    async fn scatter_gather(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<plexspaces_proto::actor::v1::ScatterGatherResponse>;
}

/// ShardGroup client for WASM/internal apps (uses ServiceLocator directly)
pub struct ShardGroupClientLocal {
    actor_service: Arc<dyn plexspaces_core::ActorService>,
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
}

impl ShardGroupClientLocal {
    /// Create a new ShardGroupClientLocal from ServiceLocator
    pub async fn new(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Result<Self> {
        let actor_service = service_locator
            .get_actor_service()
            .await
            .context("ActorService not available in ServiceLocator")?;
        Ok(Self {
            actor_service,
            service_locator,
        })
    }
}

#[async_trait::async_trait]
impl ShardGroupClientTrait for ShardGroupClientLocal {
    async fn create_shard_group(
        &mut self,
        group_id: String,
        actor_type: String,
        shard_count: u32,
        partition_strategy: PartitionStrategy,
        labels: HashMap<String, String>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        
        // Create request context (use system context for internal operations)
        // Clone service_locator to avoid Send bound issues
        let service_locator = self.service_locator.clone();
        let ctx = service_locator
            .request_context_for_system_operations()
            .await;

        let req = CreateShardGroupRequest {
            group_id,
            actor_type,
            shard_count,
            partition_strategy: partition_strategy as i32,
            shard_config: None,
            initial_state: Vec::new(),
            metadata: HashMap::new(),
            labels,
        };

        // Call ActorService directly (no gRPC)
        // Clone actor_service to avoid Send bound issues
        let actor_service = self.actor_service.clone();
        let resp = actor_service
            .create_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create ShardGroup: {}", e))?;

        resp.group
            .ok_or_else(|| anyhow::anyhow!("No group in response"))
    }

    async fn bulk_update(
        &mut self,
        group_id: String,
        updates: HashMap<String, serde_json::Value>,
        consistency_level: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse> {
        
        let service_locator = self.service_locator.clone();
        let ctx = service_locator
            .request_context_for_system_operations()
            .await;

        // Convert JSON values to Messages
        let mut proto_updates = HashMap::new();
        for (key, value) in updates {
            let message = ProtoMessage {
                id: ulid::Ulid::new().to_string(),
                sender_id: "client".to_string(),
                receiver_id: String::new(),
                channel: String::new(),
                message_type: "call".to_string(),
                payload: serde_json::to_vec(&value)?,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                headers: HashMap::new(),
                priority: 0,
                ttl: None,
                delivery_count: 0,
                idempotency_key: String::new(),
                correlation_id: String::new(),
                reply_to: String::new(),
                partition_key: key.clone(),
                uri_method: String::new(),
                uri_path: String::new(),
            };
            proto_updates.insert(key, message);
        }

        let req = BulkUpdateShardGroupRequest {
            group_id,
            updates: proto_updates,
            consistency_level: consistency_level as i32,
            timeout: Some(Duration {
                seconds: 30,
                nanos: 0,
            }),
            wait_for_responses,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .bulk_update_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bulk update ShardGroup: {}", e))
    }

    async fn map(
        &mut self,
        group_id: String,
        query: serde_json::Value,
    ) -> Result<plexspaces_proto::actor::v1::MapShardGroupResponse> {
        let group_id_for_logging = group_id.clone();
        
        let service_locator = self.service_locator.clone();
        let ctx = service_locator
            .request_context_for_system_operations()
            .await;

        let message = ProtoMessage {
            id: ulid::Ulid::new().to_string(),
            sender_id: "client".to_string(),
            receiver_id: String::new(),
            channel: String::new(),
            message_type: "call".to_string(),
            payload: serde_json::to_vec(&query)?,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            headers: HashMap::new(),
            priority: 0,
            ttl: None,
            delivery_count: 0,
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            uri_method: String::new(),
            uri_path: String::new(),
        };

        let req = MapShardGroupRequest {
            group_id: group_id.clone(),
            map_function: Some(message),
            timeout: Some(Duration {
                seconds: 30, // Increased timeout for multi-node scenarios
                nanos: 0,
            }),
            min_responses: 0,
        };

        let actor_service = self.actor_service.clone();
        let result = actor_service
            .map_shard_group(&ctx, req)
            .await;
        
        match result {
            Ok(resp) => {
                // Check if we got any successful responses
                if resp.shard_results.is_empty() {
                    tracing::error!(group_id = %group_id_for_logging, "Map ShardGroup returned no results - all shards may have failed");
                    return Err(anyhow::anyhow!("Map ShardGroup returned no results - all shards may have failed"));
                }
                let successful = resp.shard_results.iter().filter(|r| r.success).count();
                if successful == 0 {
                    let errors: Vec<String> = resp.shard_results.iter()
                        .filter_map(|r| if !r.success { Some(r.error.clone()) } else { None })
                        .collect();
                    tracing::error!(
                        group_id = %group_id_for_logging,
                        shard_count = resp.shard_results.len(),
                        errors = ?errors,
                        "Map ShardGroup failed: all {} shards failed",
                        resp.shard_results.len()
                    );
                    return Err(anyhow::anyhow!("Map ShardGroup failed: all {} shards failed. Errors: {:?}", resp.shard_results.len(), errors));
                }
                tracing::info!(
                    group_id = %group_id_for_logging,
                    successful,
                    total = resp.shard_results.len(),
                    "Map ShardGroup completed: {}/{} shards succeeded",
                    successful,
                    resp.shard_results.len()
                );
                Ok(resp)
            }
            Err(e) => {
                tracing::error!(
                    group_id = %group_id_for_logging,
                    error = %e,
                    "Failed to map ShardGroup: {}",
                    e
                );
                Err(anyhow::anyhow!("Failed to map ShardGroup {}: {}", group_id_for_logging, e))
            }
        }
    }

    async fn scatter_gather(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<plexspaces_proto::actor::v1::ScatterGatherResponse> {
        
        let service_locator = self.service_locator.clone();
        let ctx = service_locator
            .request_context_for_system_operations()
            .await;

        let message = ProtoMessage {
            id: ulid::Ulid::new().to_string(),
            sender_id: "client".to_string(),
            receiver_id: String::new(),
            channel: String::new(),
            message_type: "call".to_string(),
            payload: serde_json::to_vec(&query)?,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            headers: HashMap::new(),
            priority: 0,
            ttl: None,
            delivery_count: 0,
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            uri_method: String::new(),
            uri_path: String::new(),
        };

        let req = ScatterGatherRequest {
            group_id,
            query: Some(message),
            aggregation: aggregation as i32,
            timeout: Some(Duration {
                seconds: 30, // Increased timeout for multi-node scenarios
                nanos: 0,
            }),
            min_responses,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .scatter_gather(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to scatter-gather ShardGroup: {}", e))
    }
}

/// ShardGroup client for gRPC (remote nodes)
#[cfg(feature = "grpc")]
pub struct ShardGroupClientGrpc {
    client: plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient<tonic::transport::Channel>,
}

#[cfg(feature = "grpc")]
impl ShardGroupClientGrpc {
    /// Create a new ShardGroupClientGrpc connected to the specified node
    pub async fn connect(node_addr: impl Into<String>) -> Result<Self> {
        let addr = node_addr.into();
        let client = plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient::connect(addr.clone())
            .await
            .with_context(|| format!("Failed to connect to node: {}", addr))?;
        Ok(Self { client })
    }
}

#[cfg(feature = "grpc")]
#[async_trait::async_trait]
impl ShardGroupClientTrait for ShardGroupClientGrpc {
    async fn create_shard_group(
        &mut self,
        group_id: String,
        actor_type: String,
        shard_count: u32,
        partition_strategy: PartitionStrategy,
        labels: HashMap<String, String>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        let req = CreateShardGroupRequest {
            group_id,
            actor_type,
            shard_count,
            partition_strategy: partition_strategy as i32,
            shard_config: None,
            initial_state: Vec::new(),
            metadata: HashMap::new(),
            labels,
        };

        let resp = self
            .client
            .create_shard_group(Request::new(req))
            .await
            .context("Failed to create ShardGroup")?
            .into_inner();

        resp.group
            .context("No group in response")
    }

    async fn bulk_update(
        &mut self,
        group_id: String,
        updates: HashMap<String, serde_json::Value>,
        consistency_level: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse> {
        // Convert JSON values to Messages
        let mut proto_updates = HashMap::new();
        for (key, value) in updates {
            let message = ProtoMessage {
                id: ulid::Ulid::new().to_string(),
                sender_id: "client".to_string(),
                receiver_id: String::new(),
                channel: String::new(),
                message_type: "call".to_string(),
                payload: serde_json::to_vec(&value)?,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                headers: HashMap::new(),
                priority: 0,
                ttl: None,
                delivery_count: 0,
                idempotency_key: String::new(),
                correlation_id: String::new(),
                reply_to: String::new(),
                partition_key: key.clone(),
                uri_method: String::new(),
                uri_path: String::new(),
            };
            proto_updates.insert(key, message);
        }

        let req = BulkUpdateShardGroupRequest {
            group_id,
            updates: proto_updates,
            consistency_level: consistency_level as i32,
            timeout: Some(Duration {
                seconds: 30,
                nanos: 0,
            }),
            wait_for_responses,
        };

        let resp = self
            .client
            .bulk_update_shard_group(Request::new(req))
            .await
            .context("Failed to bulk update ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn map(
        &mut self,
        group_id: String,
        query: serde_json::Value,
    ) -> Result<plexspaces_proto::actor::v1::MapShardGroupResponse> {
        let message = ProtoMessage {
            id: ulid::Ulid::new().to_string(),
            sender_id: "client".to_string(),
            receiver_id: String::new(),
            channel: String::new(),
            message_type: "call".to_string(),
            payload: serde_json::to_vec(&query)?,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            headers: HashMap::new(),
            priority: 0,
            ttl: None,
            delivery_count: 0,
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            uri_method: String::new(),
            uri_path: String::new(),
        };

        let req = MapShardGroupRequest {
            group_id,
            map_function: Some(message),
            timeout: Some(Duration {
                seconds: 10,
                nanos: 0,
            }),
            min_responses: 0,
        };

        let resp = self
            .client
            .map_shard_group(Request::new(req))
            .await
            .context("Failed to map ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn scatter_gather(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<plexspaces_proto::actor::v1::ScatterGatherResponse> {
        let message = ProtoMessage {
            id: ulid::Ulid::new().to_string(),
            sender_id: "client".to_string(),
            receiver_id: String::new(),
            channel: String::new(),
            message_type: "call".to_string(),
            payload: serde_json::to_vec(&query)?,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            headers: HashMap::new(),
            priority: 0,
            ttl: None,
            delivery_count: 0,
            idempotency_key: String::new(),
            correlation_id: String::new(),
            reply_to: String::new(),
            partition_key: String::new(),
            uri_method: String::new(),
            uri_path: String::new(),
        };

        let req = ScatterGatherRequest {
            group_id,
            query: Some(message),
            aggregation: aggregation as i32,
            timeout: Some(Duration {
                seconds: 30, // Increased timeout for multi-node scenarios
                nanos: 0,
            }),
            min_responses,
        };

        let resp = self
            .client
            .scatter_gather(Request::new(req))
            .await
            .context("Failed to scatter-gather ShardGroup")?
            .into_inner();

        Ok(resp)
    }
}

// Type alias for convenience (defaults to gRPC if feature enabled, otherwise Local)
#[cfg(feature = "grpc")]
pub type ShardGroupClient = ShardGroupClientGrpc;

#[cfg(not(feature = "grpc"))]
pub type ShardGroupClient = ShardGroupClientLocal;

// Convenience constructors
impl ShardGroupClientLocal {
    /// Connect using ServiceLocator (for WASM/internal apps)
    pub async fn connect(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Result<Self> {
        Self::new(service_locator).await
    }
}

#[cfg(feature = "grpc")]
impl ShardGroupClientGrpc {
    /// Connect to remote node via gRPC
    pub async fn connect_grpc(node_addr: impl Into<String>) -> Result<Self> {
        Self::connect(node_addr).await
    }
}

// Unified client that works for both WASM and gRPC
pub enum UnifiedShardGroupClient {
    Local(ShardGroupClientLocal),
    #[cfg(feature = "grpc")]
    Grpc(ShardGroupClientGrpc),
}

impl UnifiedShardGroupClient {
    /// Create client from ServiceLocator (WASM/internal)
    pub async fn from_service_locator(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Result<Self> {
        Ok(Self::Local(ShardGroupClientLocal::new(service_locator).await?))
    }

    /// Create client from node address (gRPC)
    #[cfg(feature = "grpc")]
    pub async fn from_node_addr(node_addr: impl Into<String>) -> Result<Self> {
        Ok(Self::Grpc(ShardGroupClientGrpc::connect(node_addr).await?))
    }
}

#[async_trait::async_trait]
impl ShardGroupClientTrait for UnifiedShardGroupClient {
    async fn create_shard_group(
        &mut self,
        group_id: String,
        actor_type: String,
        shard_count: u32,
        partition_strategy: PartitionStrategy,
        labels: HashMap<String, String>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        match self {
            Self::Local(client) => client.create_shard_group(group_id, actor_type, shard_count, partition_strategy, labels).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.create_shard_group(group_id, actor_type, shard_count, partition_strategy, labels).await,
        }
    }

    async fn bulk_update(
        &mut self,
        group_id: String,
        updates: HashMap<String, serde_json::Value>,
        consistency_level: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse> {
        match self {
            Self::Local(client) => client.bulk_update(group_id, updates, consistency_level, wait_for_responses).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.bulk_update(group_id, updates, consistency_level, wait_for_responses).await,
        }
    }

    async fn map(
        &mut self,
        group_id: String,
        query: serde_json::Value,
    ) -> Result<plexspaces_proto::actor::v1::MapShardGroupResponse> {
        match self {
            Self::Local(client) => client.map(group_id, query).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.map(group_id, query).await,
        }
    }

    async fn scatter_gather(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<plexspaces_proto::actor::v1::ScatterGatherResponse> {
        match self {
            Self::Local(client) => client.scatter_gather(group_id, query, aggregation, min_responses).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.scatter_gather(group_id, query, aggregation, min_responses).await,
        }
    }
}
