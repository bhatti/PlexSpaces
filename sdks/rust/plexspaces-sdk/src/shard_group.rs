// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// SDK Helpers for ShardGroup (Data-Parallel Actors)
// Supports both WASM (ServiceLocator) and gRPC (optional feature)

use anyhow::{Context, Result};
use plexspaces_actor::RequestContextExt as _;
use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, AllReduceShardGroupResponse, BarrierShardGroupRequest,
    BarrierShardGroupResponse, BroadcastShardGroupRequest, BroadcastShardGroupResponse,
    BulkUpdateShardGroupRequest, CollectiveReduction, CollectiveTargetField, ConsistencyLevel,
    CreateShardGroupRequest, DataParallelConfig, MapShardGroupRequest, NodePlacement,
    PartitionStrategy, RebalancePolicy, ReduceShardGroupRequest, ReduceShardGroupResponse,
    ScatterGatherRequest, ShardGroupAggregationStrategy, SpawnActorRequest, SpawnActorsRequest,
    SpawnActorsResponse,
};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use plexspaces_proto::prost_types::{Duration, Timestamp};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

#[cfg(feature = "grpc")]
use tonic::Request;

fn create_shard_group_request(
    group_id: String,
    actor_type: String,
    shard_count: u32,
    partition_strategy: PartitionStrategy,
    placement: Option<NodePlacement>,
) -> CreateShardGroupRequest {
    CreateShardGroupRequest {
        request_id: ulid::Ulid::new().to_string(),
        config: Some(DataParallelConfig {
            group_id,
            shard_count,
            partition_strategy: partition_strategy as i32,
            rebalance_policy: RebalancePolicy::RebalancePolicyNone as i32,
            placement,
        }),
        actor_type,
        shard_config: None,
        initial_state: Vec::new(),
        metadata: HashMap::new(),
    }
}

fn make_call_message(payload: Vec<u8>) -> ProtoMessage {
    ProtoMessage {
        id: ulid::Ulid::new().to_string(),
        sender_id: "client".to_string(),
        receiver_id: String::new(),
        channel: String::new(),
        message_type: "call".to_string(),
        payload,
        timestamp: Some(Timestamp::from(SystemTime::now())),
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
    }
}

fn default_timeout() -> Option<Duration> {
    Some(Duration {
        seconds: plexspaces_actor::parallel::DEFAULT_SHARD_TIMEOUT_SECS as i64,
        nanos: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::create_shard_group_request;
    use plexspaces_proto::actor::v1::{NodePlacement, NodePlacementStrategy, PartitionStrategy};
    use std::collections::HashMap;

    #[test]
    fn create_shard_group_request_preserves_node_id_placement() {
        let placement = NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyNodeIds as i32,
            cluster: String::new(),
            node_ids: vec!["node-a".to_string(), "node-b".to_string()],
            required_labels: HashMap::new(),
            avoid_node_ids: vec![],
            resource_requirements: None,
            affinity_labels: HashMap::new(),
        };
        let request = create_shard_group_request(
            "group".to_string(),
            "worker".to_string(),
            2,
            PartitionStrategy::PartitionStrategyHash,
            Some(placement),
        );
        let placement = request
            .config
            .expect("config")
            .placement
            .expect("placement");

        assert_eq!(
            placement.strategy,
            NodePlacementStrategy::NodePlacementStrategyNodeIds as i32
        );
        assert_eq!(
            placement.node_ids,
            vec!["node-a".to_string(), "node-b".to_string()]
        );
        assert!(placement.required_labels.is_empty());
    }

    #[test]
    fn create_shard_group_request_preserves_registry_placement() {
        let mut labels = HashMap::new();
        labels.insert("role".to_string(), "worker".to_string());
        let placement = NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32,
            cluster: "heat-cluster".to_string(),
            node_ids: vec![],
            required_labels: labels.clone(),
            avoid_node_ids: vec![],
            resource_requirements: None,
            affinity_labels: HashMap::new(),
        };
        let request = create_shard_group_request(
            "group".to_string(),
            "worker".to_string(),
            4,
            PartitionStrategy::PartitionStrategyHash,
            Some(placement),
        );
        let placement = request
            .config
            .expect("config")
            .placement
            .expect("placement");

        assert_eq!(
            placement.strategy,
            NodePlacementStrategy::NodePlacementStrategyFromRegistry as i32
        );
        assert_eq!(placement.required_labels, labels);
        assert_eq!(placement.cluster, "heat-cluster");
    }

    #[test]
    fn create_shard_group_request_allows_no_placement() {
        let request = create_shard_group_request(
            "group".to_string(),
            "worker".to_string(),
            1,
            PartitionStrategy::PartitionStrategyHash,
            None,
        );

        assert!(request.config.expect("config").placement.is_none());
    }
}

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
        placement: Option<NodePlacement>,
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

    /// Broadcast a message to all shards in a group.
    async fn broadcast(
        &mut self,
        group_id: String,
        message: serde_json::Value,
        min_acks: u32,
    ) -> Result<BroadcastShardGroupResponse>;

    /// Reduce shard responses using a built-in collective reduction.
    async fn reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<ReduceShardGroupResponse>;

    /// All-reduce: reduce shard responses and fan the result back to all shards.
    async fn all_reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<AllReduceShardGroupResponse>;

    /// Synchronize a shard group at a framework barrier round.
    async fn barrier(
        &mut self,
        group_id: String,
        barrier_id: String,
        round: u64,
        min_acks: u32,
    ) -> Result<BarrierShardGroupResponse>;

    /// Spawn multiple actors using the canonical framework spawn contract.
    async fn spawn_actors(
        &mut self,
        requests: Vec<SpawnActorRequest>,
    ) -> Result<SpawnActorsResponse>;
}

/// ShardGroup client for WASM/internal apps (uses ServiceLocator directly)
pub struct ShardGroupClientLocal {
    actor_service: Arc<dyn plexspaces_actor::ActorService>,
    service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    operation_ctx: Option<plexspaces_actor::RequestContext>,
}

impl ShardGroupClientLocal {
    /// Create a new ShardGroupClientLocal from ServiceLocator
    pub async fn new(service_locator: Arc<dyn plexspaces_actor::ServiceLocator>) -> Result<Self> {
        let actor_service = service_locator
            .get_actor_service()
            .await
            .context("ActorService not available in ServiceLocator")?;
        Ok(Self {
            actor_service,
            service_locator,
            operation_ctx: None,
        })
    }

    /// Override the request context used for all shard operations (broadcast, scatter_gather, etc.).
    /// Required when actors were created with a non-empty tenant/namespace.
    pub fn with_namespace(mut self, tenant: impl Into<String>, namespace: impl Into<String>) -> Self {
        self.operation_ctx = Some(
            plexspaces_actor::RequestContext::new_without_auth(tenant.into(), namespace.into())
                .with_admin(true),
        );
        self
    }

    fn get_operation_ctx(&self) -> plexspaces_actor::RequestContext {
        self.operation_ctx
            .clone()
            .unwrap_or_else(|| plexspaces_actor::RequestContext::new_without_auth(String::new(), String::new()).with_admin(true))
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
        placement: Option<NodePlacement>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        let ctx = self.get_operation_ctx();

        let req = create_shard_group_request(
            group_id,
            actor_type,
            shard_count,
            partition_strategy,
            placement,
        );

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
        let ctx = self.get_operation_ctx();

        let mut proto_updates = HashMap::new();
        for (key, value) in updates {
            let mut message = make_call_message(serde_json::to_vec(&value)?);
            message.partition_key = key.clone();
            proto_updates.insert(key, message);
        }

        let req = BulkUpdateShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            updates: proto_updates,
            consistency_level: consistency_level as i32,
            timeout: default_timeout(),
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

        let ctx = self.get_operation_ctx();

        let message = make_call_message(serde_json::to_vec(&query)?);

        let req = MapShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id: group_id.clone(),
            map_function: Some(message),
            timeout: default_timeout(),
            min_responses: 0,
        };

        let actor_service = self.actor_service.clone();
        let result = actor_service.map_shard_group(&ctx, req).await;

        match result {
            Ok(resp) => {
                if resp.shard_results.is_empty() {
                    tracing::error!(group_id = %group_id_for_logging, "Map ShardGroup returned no results - all shards may have failed");
                    return Err(anyhow::anyhow!(
                        "Map ShardGroup returned no results - all shards may have failed"
                    ));
                }
                let successful = resp.shard_results.iter().filter(|r| r.success).count();
                if successful == 0 {
                    let errors: Vec<String> = resp
                        .shard_results
                        .iter()
                        .filter_map(|r| {
                            if !r.success {
                                Some(r.error.clone())
                            } else {
                                None
                            }
                        })
                        .collect();
                    tracing::error!(
                        group_id = %group_id_for_logging,
                        shard_count = resp.shard_results.len(),
                        errors = ?errors,
                        "Map ShardGroup failed: all {} shards failed",
                        resp.shard_results.len()
                    );
                    return Err(anyhow::anyhow!(
                        "Map ShardGroup failed: all {} shards failed. Errors: {:?}",
                        resp.shard_results.len(),
                        errors
                    ));
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
                Err(anyhow::anyhow!(
                    "Failed to map ShardGroup {}: {}",
                    group_id_for_logging,
                    e
                ))
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
        let ctx = self.get_operation_ctx();

        let message = make_call_message(serde_json::to_vec(&query)?);

        let req = ScatterGatherRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            query: Some(message),
            aggregation: aggregation as i32,
            timeout: default_timeout(),
            min_responses,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .scatter_gather(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to scatter-gather ShardGroup: {}", e))
    }

    async fn broadcast(
        &mut self,
        group_id: String,
        message: serde_json::Value,
        min_acks: u32,
    ) -> Result<BroadcastShardGroupResponse> {
        let ctx = self.get_operation_ctx();

        let msg = make_call_message(serde_json::to_vec(&message)?);

        let req = BroadcastShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            message: Some(msg),
            timeout: default_timeout(),
            min_acks,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .broadcast_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to broadcast ShardGroup: {}", e))
    }

    async fn reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<ReduceShardGroupResponse> {
        let ctx = self.get_operation_ctx();

        let msg = make_call_message(serde_json::to_vec(&query)?);

        let req = ReduceShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            map_function: Some(msg),
            timeout: default_timeout(),
            min_responses,
            reduction: reduction as i32,
            target: target.map(|p| CollectiveTargetField { value_path: p }),
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .reduce_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to reduce ShardGroup: {}", e))
    }

    async fn all_reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<AllReduceShardGroupResponse> {
        let ctx = self.get_operation_ctx();

        let msg = make_call_message(serde_json::to_vec(&query)?);

        let req = AllReduceShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            map_function: Some(msg),
            timeout: default_timeout(),
            min_responses,
            reduction: reduction as i32,
            target: target.map(|p| CollectiveTargetField { value_path: p }),
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .all_reduce_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to all-reduce ShardGroup: {}", e))
    }

    async fn barrier(
        &mut self,
        group_id: String,
        barrier_id: String,
        round: u64,
        min_acks: u32,
    ) -> Result<BarrierShardGroupResponse> {
        let ctx = self.get_operation_ctx();

        let req = BarrierShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            barrier_id,
            round,
            timeout: default_timeout(),
            min_acks,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .barrier_shard_group(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to barrier ShardGroup: {}", e))
    }

    async fn spawn_actors(
        &mut self,
        requests: Vec<SpawnActorRequest>,
    ) -> Result<SpawnActorsResponse> {
        let ctx = self.get_operation_ctx();

        let req = SpawnActorsRequest {
            request_id: ulid::Ulid::new().to_string(),
            requests,
        };

        let actor_service = self.actor_service.clone();
        actor_service
            .spawn_actors(&ctx, req)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to spawn actors: {}", e))
    }
}

/// ShardGroup client for gRPC (remote nodes)
#[cfg(feature = "grpc")]
pub struct ShardGroupClientGrpc {
    client: plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient<
        tonic::transport::Channel,
    >,
}

#[cfg(feature = "grpc")]
impl ShardGroupClientGrpc {
    /// Create a new ShardGroupClientGrpc connected to the specified node
    pub async fn connect(node_addr: impl Into<String>) -> Result<Self> {
        let addr = node_addr.into();
        let client =
            plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient::connect(
                addr.clone(),
            )
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
        placement: Option<NodePlacement>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        let req = create_shard_group_request(
            group_id,
            actor_type,
            shard_count,
            partition_strategy,
            placement,
        );

        let resp = self
            .client
            .create_shard_group(Request::new(req))
            .await
            .context("Failed to create ShardGroup")?
            .into_inner();

        resp.group.context("No group in response")
    }

    async fn bulk_update(
        &mut self,
        group_id: String,
        updates: HashMap<String, serde_json::Value>,
        consistency_level: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<plexspaces_proto::actor::v1::BulkUpdateShardGroupResponse> {
        let mut proto_updates = HashMap::new();
        for (key, value) in updates {
            let mut message = make_call_message(serde_json::to_vec(&value)?);
            message.partition_key = key.clone();
            proto_updates.insert(key, message);
        }

        let req = BulkUpdateShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            updates: proto_updates,
            consistency_level: consistency_level as i32,
            timeout: default_timeout(),
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
        let message = make_call_message(serde_json::to_vec(&query)?);

        let req = MapShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
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
        let message = make_call_message(serde_json::to_vec(&query)?);

        let req = ScatterGatherRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            query: Some(message),
            aggregation: aggregation as i32,
            timeout: default_timeout(),
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

    async fn broadcast(
        &mut self,
        group_id: String,
        message: serde_json::Value,
        min_acks: u32,
    ) -> Result<BroadcastShardGroupResponse> {
        let msg = make_call_message(serde_json::to_vec(&message)?);

        let req = BroadcastShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            message: Some(msg),
            timeout: default_timeout(),
            min_acks,
        };

        let resp = self
            .client
            .broadcast_shard_group(Request::new(req))
            .await
            .context("Failed to broadcast ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<ReduceShardGroupResponse> {
        let msg = make_call_message(serde_json::to_vec(&query)?);

        let req = ReduceShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            map_function: Some(msg),
            timeout: default_timeout(),
            min_responses,
            reduction: reduction as i32,
            target: target.map(|p| CollectiveTargetField { value_path: p }),
        };

        let resp = self
            .client
            .reduce_shard_group(Request::new(req))
            .await
            .context("Failed to reduce ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn all_reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<AllReduceShardGroupResponse> {
        let msg = make_call_message(serde_json::to_vec(&query)?);

        let req = AllReduceShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            map_function: Some(msg),
            timeout: default_timeout(),
            min_responses,
            reduction: reduction as i32,
            target: target.map(|p| CollectiveTargetField { value_path: p }),
        };

        let resp = self
            .client
            .all_reduce_shard_group(Request::new(req))
            .await
            .context("Failed to all-reduce ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn barrier(
        &mut self,
        group_id: String,
        barrier_id: String,
        round: u64,
        min_acks: u32,
    ) -> Result<BarrierShardGroupResponse> {
        let req = BarrierShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id,
            barrier_id,
            round,
            timeout: default_timeout(),
            min_acks,
        };

        let resp = self
            .client
            .barrier_shard_group(Request::new(req))
            .await
            .context("Failed to barrier ShardGroup")?
            .into_inner();

        Ok(resp)
    }

    async fn spawn_actors(
        &mut self,
        requests: Vec<SpawnActorRequest>,
    ) -> Result<SpawnActorsResponse> {
        let req = SpawnActorsRequest {
            request_id: ulid::Ulid::new().to_string(),
            requests,
        };

        let resp = self
            .client
            .spawn_actors(Request::new(req))
            .await
            .context("Failed to spawn actors")?
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
    pub async fn connect(
        service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    ) -> Result<Self> {
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
    pub async fn from_service_locator(
        service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    ) -> Result<Self> {
        Ok(Self::Local(
            ShardGroupClientLocal::new(service_locator).await?,
        ))
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
        placement: Option<NodePlacement>,
    ) -> Result<plexspaces_proto::actor::v1::ShardGroup> {
        match self {
            Self::Local(client) => {
                client
                    .create_shard_group(
                        group_id,
                        actor_type,
                        shard_count,
                        partition_strategy,
                        placement,
                    )
                    .await
            }
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => {
                client
                    .create_shard_group(
                        group_id,
                        actor_type,
                        shard_count,
                        partition_strategy,
                        placement,
                    )
                    .await
            }
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
            Self::Local(client) => {
                client
                    .bulk_update(group_id, updates, consistency_level, wait_for_responses)
                    .await
            }
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => {
                client
                    .bulk_update(group_id, updates, consistency_level, wait_for_responses)
                    .await
            }
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
            Self::Local(client) => {
                client
                    .scatter_gather(group_id, query, aggregation, min_responses)
                    .await
            }
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => {
                client
                    .scatter_gather(group_id, query, aggregation, min_responses)
                    .await
            }
        }
    }

    async fn broadcast(
        &mut self,
        group_id: String,
        message: serde_json::Value,
        min_acks: u32,
    ) -> Result<BroadcastShardGroupResponse> {
        match self {
            Self::Local(client) => client.broadcast(group_id, message, min_acks).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.broadcast(group_id, message, min_acks).await,
        }
    }

    async fn reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<ReduceShardGroupResponse> {
        match self {
            Self::Local(client) => {
                client
                    .reduce(group_id, query, reduction, target, min_responses)
                    .await
            }
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => {
                client
                    .reduce(group_id, query, reduction, target, min_responses)
                    .await
            }
        }
    }

    async fn all_reduce(
        &mut self,
        group_id: String,
        query: serde_json::Value,
        reduction: CollectiveReduction,
        target: Option<String>,
        min_responses: u32,
    ) -> Result<AllReduceShardGroupResponse> {
        match self {
            Self::Local(client) => {
                client
                    .all_reduce(group_id, query, reduction, target, min_responses)
                    .await
            }
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => {
                client
                    .all_reduce(group_id, query, reduction, target, min_responses)
                    .await
            }
        }
    }

    async fn barrier(
        &mut self,
        group_id: String,
        barrier_id: String,
        round: u64,
        min_acks: u32,
    ) -> Result<BarrierShardGroupResponse> {
        match self {
            Self::Local(client) => client.barrier(group_id, barrier_id, round, min_acks).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.barrier(group_id, barrier_id, round, min_acks).await,
        }
    }

    async fn spawn_actors(
        &mut self,
        requests: Vec<SpawnActorRequest>,
    ) -> Result<SpawnActorsResponse> {
        match self {
            Self::Local(client) => client.spawn_actors(requests).await,
            #[cfg(feature = "grpc")]
            Self::Grpc(client) => client.spawn_actors(requests).await,
        }
    }
}
