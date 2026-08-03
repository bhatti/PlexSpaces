// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Partition strategies for ShardGroup routing
//!
//! ## Purpose
//! Implements partitioning strategies for routing messages to shards in a ShardGroup.
//! Inspired by NSDI'22 Data-Parallel Actors paper.
//!
//! ## Strategies
//! - **Hash**: Simple hash-based partitioning (uniform distribution)
//! - **ConsistentHash**: Consistent hashing with virtual nodes (minimal rebalancing)
//! - **Range**: Range-based partitioning (ordered keys)

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

use futures::future::join_all;
use tonic::Status;
use ulid::Ulid;

use plexspaces_actor::parallel::{
    build_collective_message, reduce_values, resolve_timeout, scatter_stats_from_results,
    select_collective_value, shard_group_config, shard_query_responses_from_results,
};
use plexspaces_actor::{
    monitoring::{
        record_node_shard_groups_created, record_node_shard_messages_received,
        record_node_shard_operation, record_node_shard_operation_failed,
    },
    RequestContext, RequestContextExt, ServiceLocator as ServiceLocatorTrait,
};
use plexspaces_proto::actor::v1::{
    AllReduceShardGroupRequest, AllReduceShardGroupResponse, BarrierShardGroupRequest,
    BarrierShardGroupResponse, BroadcastShardGroupRequest, BroadcastShardGroupResponse,
    BulkUpdateShardGroupRequest, BulkUpdateShardGroupResponse, CreateShardGroupRequest,
    CreateShardGroupResponse, MapShardGroupRequest, MapShardGroupResponse, PartitionStrategy,
    ReduceShardGroupRequest, ReduceShardGroupResponse, ScatterGatherRequest, ScatterGatherResponse,
    ScatterGatherStats, ShardGroup, ShardGroupAggregationStrategy, ShardGroupState,
    ShardQueryResponse, ShardUpdateStats, SpawnActorRequest,
};
use plexspaces_proto::common::v1::Message;

use super::ActorServiceImpl;

/// Calculate shard ID from partition key using specified strategy
///
/// ## Arguments
/// * `partition_key` - Key to partition on (bytes)
/// * `strategy` - Partition strategy enum value
/// * `shard_count` - Total number of shards
/// * `range_ranges` - Optional range boundaries for Range partitioning (sorted ascending)
///
/// ## Returns
/// Shard ID (0 to shard_count-1)
pub fn calculate_shard_id(
    partition_key: &[u8],
    strategy: i32,
    shard_count: u32,
    range_ranges: Option<&[Vec<u8>]>, // Range boundaries for Range partitioning
) -> Result<u32, String> {
    if shard_count == 0 {
        return Err("shard_count must be > 0".to_string());
    }

    match strategy {
        x if x == PartitionStrategy::PartitionStrategyHash as i32 => {
            hash_partition(partition_key, shard_count)
        }
        x if x == PartitionStrategy::PartitionStrategyConsistentHash as i32 => {
            consistent_hash_partition(partition_key, shard_count)
        }
        x if x == PartitionStrategy::PartitionStrategyRange as i32 => {
            range_partition(partition_key, shard_count, range_ranges)
        }
        _ => Err(format!("Unsupported partition strategy: {}", strategy)),
    }
}

/// Hash-based partitioning (uniform distribution)
///
/// ## Algorithm
/// ```
/// hash(key) % shard_count
/// ```
///
/// ## Pros
/// - Uniform distribution
/// - Simple implementation
///
/// ## Cons
/// - Full reshuffle on scale (4→8 shards = ~50% keys move)
///
/// ## Use Cases
/// - Uniform access patterns
/// - Infrequent scaling
fn hash_partition(partition_key: &[u8], shard_count: u32) -> Result<u32, String> {
    let mut hasher = DefaultHasher::new();
    partition_key.hash(&mut hasher);
    let hash = hasher.finish();
    Ok((hash % shard_count as u64) as u32)
}

/// Consistent hashing with virtual nodes (minimal rebalancing)
///
/// ## Algorithm
/// ```
/// 1. Create virtual nodes: Each shard has V virtual nodes (default V=100)
/// 2. Hash each virtual node: hash("shard-{i}-vn-{j}") → position on ring
/// 3. Hash partition key: hash(key) → position on ring
/// 4. Find closest virtual node clockwise → shard
/// ```
///
/// ## Pros
/// - Minimal key movement on scale (1/N keys move)
/// - Better load distribution with virtual nodes
///
/// ## Cons
/// - More complex than hash
/// - Requires virtual node management
///
/// ## Use Cases
/// - Frequent scaling
/// - Large datasets
/// - Need minimal rebalancing
fn consistent_hash_partition(partition_key: &[u8], shard_count: u32) -> Result<u32, String> {
    const VIRTUAL_NODES_PER_SHARD: u32 = 100; // Default virtual nodes per shard

    // Hash the partition key to get position on ring
    let mut hasher = DefaultHasher::new();
    partition_key.hash(&mut hasher);
    let key_hash = hasher.finish();

    // Find closest virtual node clockwise
    let mut best_shard = 0u32;
    let mut best_distance = u64::MAX;

    // Check all virtual nodes for all shards
    for shard_id in 0..shard_count {
        for vn in 0..VIRTUAL_NODES_PER_SHARD {
            // Hash virtual node identifier: "shard-{shard_id}-vn-{vn}"
            let vn_key = format!("shard-{}-vn-{}", shard_id, vn);
            let mut vn_hasher = DefaultHasher::new();
            vn_key.hash(&mut vn_hasher);
            let vn_hash = vn_hasher.finish();

            // Calculate clockwise distance on ring
            let distance = if vn_hash >= key_hash {
                vn_hash - key_hash
            } else {
                // Wrap around: distance = (max - key_hash) + vn_hash
                (u64::MAX - key_hash) + vn_hash + 1
            };

            if distance < best_distance {
                best_distance = distance;
                best_shard = shard_id;
            }
        }
    }

    Ok(best_shard)
}

/// Range-based partitioning (ordered keys)
///
/// ## Algorithm
/// ```
/// 1. Define ranges: [range_0, range_1, ..., range_N-1]
/// 2. Compare key with ranges (binary search)
/// 3. Return shard for matching range
/// ```
///
/// ## Pros
/// - Efficient range queries (query single shard)
/// - Preserves key ordering
///
/// ## Cons
/// - Requires range boundaries
/// - Potential hotspots if ranges uneven
///
/// ## Use Cases
/// - Time-series data (timestamp ranges)
/// - Ordered keys (e.g., user IDs in ranges)
/// - Range queries common
fn range_partition(
    partition_key: &[u8],
    shard_count: u32,
    range_ranges: Option<&[Vec<u8>]>,
) -> Result<u32, String> {
    // If no ranges provided, use simple byte comparison
    // Shard i handles keys where: key >= range[i] && key < range[i+1]
    if let Some(ranges) = range_ranges {
        if ranges.len() != shard_count as usize {
            return Err(format!(
                "Range boundaries count ({}) must match shard_count ({})",
                ranges.len(),
                shard_count
            ));
        }

        // Binary search for matching range
        for (i, range_boundary) in ranges.iter().enumerate() {
            if partition_key < range_boundary.as_slice() {
                return Ok(i as u32);
            }
        }

        // Key >= last boundary, assign to last shard
        Ok(shard_count - 1)
    } else {
        // No ranges provided: use simple byte-based range partitioning
        // Divide key space evenly: shard = (first_byte * shard_count) / 256
        if partition_key.is_empty() {
            return Err("Partition key cannot be empty for range partitioning".to_string());
        }

        let first_byte = partition_key[0] as u32;
        let shard_id = (first_byte * shard_count) / 256;
        Ok(shard_id.min(shard_count - 1))
    }
}

// ============================================================================
// ShardGroup implementation — shard placement, parallel operations, and
// all *_internal helpers called by the gRPC trait impl in mod.rs.
// ============================================================================

impl ActorServiceImpl {
    /// Resolve the target node IDs for shard placement.
    ///
    /// Strategy controls resolution semantics:
    /// - `FROM_REGISTRY` ignores `node_ids` and lists currently known nodes.
    /// - `NODE_IDS` uses only the explicit `node_ids`.
    /// - `SAME_NODE`, `UNSPECIFIED`, or no placement target the local node.
    pub(super) async fn resolve_shard_group_target_nodes(
        &self,
        ctx: &RequestContext,
        placement: Option<&plexspaces_proto::actor::v1::NodePlacement>,
        local_node_id: &str,
    ) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::actor::v1::NodePlacementStrategy;

        let strategy = placement
            .and_then(|p| NodePlacementStrategy::try_from(p.strategy).ok())
            .unwrap_or(NodePlacementStrategy::NodePlacementStrategyUnspecified);

        let target_nodes = match placement {
            Some(placement)
                if strategy == NodePlacementStrategy::NodePlacementStrategyFromRegistry =>
            {
                let node_registry =
                    self.service_locator
                        .get_node_registry()
                        .await
                        .ok_or_else(|| {
                            "NodeRegistry not available for from_registry placement".to_string()
                        })?;
                let local_cluster = self
                    .service_locator
                    .get_node_config()
                    .await
                    .map(|config| config.cluster_name)
                    .unwrap_or_default();
                let cluster = if placement.cluster.is_empty() {
                    if local_cluster.is_empty() {
                        None
                    } else {
                        Some(local_cluster.as_str())
                    }
                } else {
                    Some(placement.cluster.as_str())
                };
                let (registrations, _) = node_registry
                    .list_nodes(ctx, cluster, 1000, "")
                    .await
                    .map_err(|e| format!("list_nodes failed: {}", e))?;
                if registrations.is_empty() {
                    tracing::warn!(
                        local_node_id = %local_node_id,
                        list_cluster_filter = ?cluster,
                        node_config_cluster_name = %local_cluster,
                        placement_cluster_field = %placement.cluster,
                        "from_registry placement: list_nodes returned zero members (SWIM/cache empty or cluster label filter excluded all nodes)"
                    );
                }
                registrations
                    .into_iter()
                    .map(|registration| registration.node_id)
                    .collect()
            }
            Some(placement) if strategy == NodePlacementStrategy::NodePlacementStrategyNodeIds => {
                placement.node_ids.clone()
            }
            _ => vec![local_node_id.to_string()],
        };

        if target_nodes.is_empty() {
            return Err("Placement produced no target nodes for shard group creation".into());
        }

        Ok(target_nodes)
    }

    /// Internal implementation of create_shard_group (used by both gRPC and trait)
    pub(super) async fn create_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: CreateShardGroupRequest,
    ) -> Result<CreateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let config = req.config.as_ref().ok_or("config is required")?;
        let group_id = config.group_id.as_str();
        let shard_count = config.shard_count;

        if shard_count == 0 {
            return Err("config.shard_count must be >= 1".into());
        }
        if shard_count > 1_000_000_000 {
            return Err("config.shard_count must be <= 1000000000".into());
        }
        if config.group_id.is_empty() {
            return Err("config.group_id is required".into());
        }
        if req.actor_type.is_empty() {
            return Err("actor_type is required".into());
        }

        // Check if group already exists
        {
            let groups = self.shard_groups.read().await;
            if groups.contains_key(group_id) {
                return Err(format!("ShardGroup {} already exists", group_id).into());
            }
        }

        let registry = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not available".to_string())?;
        let local_node_id = registry.local_node_id();

        let target_nodes = self
            .resolve_shard_group_target_nodes(ctx, config.placement.as_ref(), local_node_id)
            .await?;

        let actor_factory = self
            .service_locator
            .get_actor_factory()
            .await
            .ok_or_else(|| "Actor factory not available".to_string())?;

        let role = req.actor_type.clone();
        let resolved_actor_type =
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                manager
                    .resolve_actor_type_for_name(ctx.namespace(), &req.actor_type)
                    .await
            } else {
                req.actor_type.clone()
            };
        let definition_spec =
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                manager
                    .get_virtual_actor_definition(ctx.namespace(), &role)
                    .await
                    .map(|metadata| metadata.spec)
            } else {
                None
            };

        let mut shard_actor_ids = Vec::with_capacity(shard_count as usize);

        for shard_id in 0..shard_count {
            let actor_id_base = format!("{}-{}", group_id, ulid::Ulid::new());

            let mut shard_config = req.shard_config.clone().unwrap_or_default();
            shard_config.actor_groups.push(config.group_id.clone());
            if config.placement.is_some() {
                use plexspaces_proto::v1::actor::ActorResourceRequirements;
                shard_config.resource_requirements = Some(ActorResourceRequirements {
                    placement: config.placement.clone(),
                });
            }

            let target_node = &target_nodes[shard_id as usize % target_nodes.len()];

            if target_node == local_node_id {
                let full_id = self
                    .build_canonical_actor_id(
                        &actor_id_base,
                        &resolved_actor_type,
                        ctx.namespace(),
                        target_node,
                    )
                    .map_err(|e| e.to_string())?;

                let shard_spawn_spec = {
                    use plexspaces_actor::ActorSpawnSpec;
                    use plexspaces_proto::common::v1::ActorIdentity;
                    ActorSpawnSpec {
                        identity: Some(ActorIdentity {
                            name: full_id.name().to_string(),
                            actor_type: resolved_actor_type.clone(),
                        }),
                        role: role.clone(),
                        namespace: ctx.namespace().to_string(),
                        tenant_id: ctx.tenant_id().to_string(),
                        visibility: definition_spec
                            .as_ref()
                            .map(|spec| spec.visibility)
                            .unwrap_or_default(),
                        behavior_kind: definition_spec
                            .as_ref()
                            .map(|spec| spec.behavior_kind.clone())
                            .unwrap_or_default(),
                        args: definition_spec
                            .as_ref()
                            .map(|spec| spec.args.clone())
                            .unwrap_or_default(),
                        facets: vec![],
                        config: Some(shard_config),
                        labels: std::collections::HashMap::new(),
                        ..Default::default()
                    }
                };
                match actor_factory
                    .spawn_actor(ctx, &shard_spawn_spec, vec![])
                    .await
                {
                    Ok(_sender) => {
                        shard_actor_ids.push(full_id.to_string());
                    }
                    Err(e) => {
                        for spawned_id in &shard_actor_ids {
                            if let Ok(spawned_id) = self.parse_canonical_actor_id(spawned_id) {
                                let _ = actor_factory.stop_actor(ctx, &spawned_id).await;
                            }
                        }
                        return Err(
                            format!("Failed to spawn shard {} (local): {}", shard_id, e).into()
                        );
                    }
                }
            } else {
                let remote_actor_id = self
                    .build_canonical_actor_id(
                        &actor_id_base,
                        &resolved_actor_type,
                        ctx.namespace(),
                        target_node,
                    )
                    .map_err(|e| e.to_string())?;
                let remote_spawn_spec = {
                    use plexspaces_actor::ActorSpawnSpec;
                    use plexspaces_proto::common::v1::ActorIdentity;
                    ActorSpawnSpec {
                        identity: Some(ActorIdentity {
                            name: remote_actor_id.name().to_string(),
                            actor_type: resolved_actor_type.clone(),
                        }),
                        role: role.clone(),
                        namespace: ctx.namespace().to_string(),
                        tenant_id: ctx.tenant_id().to_string(),
                        visibility: definition_spec
                            .as_ref()
                            .map(|spec| spec.visibility)
                            .unwrap_or_default(),
                        behavior_kind: definition_spec
                            .as_ref()
                            .map(|spec| spec.behavior_kind.clone())
                            .unwrap_or_default(),
                        args: definition_spec
                            .as_ref()
                            .map(|spec| spec.args.clone())
                            .unwrap_or_default(),
                        facets: vec![],
                        config: Some(shard_config.clone()),
                        labels: std::collections::HashMap::new(),
                        ..Default::default()
                    }
                };
                let channel = self
                    .service_locator
                    .get_actor_service_client(target_node)
                    .await
                    .map_err(|e| format!("Failed to get client for node {}: {}", target_node, e))?;
                let mut client = plexspaces_proto::ActorServiceClient::new(channel);
                let spawn_req = SpawnActorRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    spec: Some(remote_spawn_spec),
                    namespace: ctx.namespace().to_string(),
                    instances_count: 1,
                };
                let mut remote_req = tonic::Request::new(spawn_req);
                plexspaces_actor::apply_request_context_to_grpc_metadata(
                    ctx,
                    remote_req.metadata_mut(),
                );
                let spawn_response = client
                    .spawn_actor(remote_req)
                    .await
                    .map_err(|e| format!("Remote spawn to {} failed: {}", target_node, e))?;
                let actor_ref = spawn_response.into_inner().actor_ref;
                if actor_ref.is_empty() {
                    for spawned_id in &shard_actor_ids {
                        if let Ok(spawned_id) = self.parse_canonical_actor_id(spawned_id) {
                            let _ = actor_factory.stop_actor(ctx, &spawned_id).await;
                        }
                    }
                    return Err(format!(
                        "Remote spawn to {} returned empty actor_ref",
                        target_node
                    )
                    .into());
                }
                shard_actor_ids.push(actor_ref);
            }
        }

        let group = ShardGroup {
            config: Some(config.clone()),
            actor_type: req.actor_type.clone(),
            shard_actor_ids: shard_actor_ids.clone(),
            state: ShardGroupState::ShardGroupStateActive as i32,
            created_at: Some(prost_types::Timestamp {
                seconds: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            metadata: req.metadata.clone(),
            rebalance_status: None,
        };

        {
            let mut groups = self.shard_groups.write().await;
            groups.insert(config.group_id.clone(), group.clone());
        }

        if let Some(task_router) = self.service_locator.get_task_router().await {
            if let Err(e) = task_router.register_group(group.clone()).await {
                tracing::warn!(
                    group_id = %config.group_id,
                    error = %e,
                    "Failed to register ShardGroup in TaskRouter (non-fatal)"
                );
            } else if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    group_id = %config.group_id,
                    shard_count = shard_actor_ids.len(),
                    "Registered ShardGroup in TaskRouter"
                );
            }
        }

        record_node_shard_groups_created(self.local_node_id.as_str());

        metrics::counter!("plexspaces_shard_group_created_total",
            "group_id" => config.group_id.clone(),
            "actor_type" => req.actor_type.clone(),
            "shard_count" => shard_count.to_string())
        .increment(1);

        tracing::info!(
            group_id = %config.group_id,
            shard_count = shard_count,
            "Created ShardGroup"
        );

        Ok(CreateShardGroupResponse {
            request_id: req.request_id.clone(),
            group: Some(group),
        })
    }

    /// Internal implementation of bulk_update_shard_group
    pub(super) async fn bulk_update_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BulkUpdateShardGroupRequest,
    ) -> Result<BulkUpdateShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };

        let request_id = req.request_id.clone();
        let timeout = resolve_timeout(req.timeout.as_ref());

        let _total_updates = req.updates.len();
        let mut updates_by_shard: std::collections::HashMap<u32, Vec<(String, Message)>> =
            std::collections::HashMap::new();
        for (partition_key_str, mut message) in req.updates {
            let partition_key = partition_key_str.as_bytes();
            let shard_id = calculate_shard_id(
                partition_key,
                shard_group_config(&group).partition_strategy,
                shard_group_config(&group).shard_count,
                None,
            )
            .map_err(|e| format!("Partition calculation failed: {}", e))?;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .ok_or_else(|| format!("Invalid shard_id {}", shard_id))?
                .clone();

            if message.id.is_empty() {
                message.id = format!("req-{}", ulid::Ulid::new().to_string());
            } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                message.id = format!("req-{}", message.id);
            }

            message.receiver_id = shard_actor_id.clone();
            updates_by_shard
                .entry(shard_id)
                .or_default()
                .push((partition_key_str, message));
        }

        let mut handles = Vec::new();
        let _shard_stats_map: std::collections::HashMap<u32, ShardUpdateStats> =
            std::collections::HashMap::new();

        for (shard_id, updates) in updates_by_shard {
            let _shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .unwrap()
                .clone();
            let ctx = ctx.clone();
            let wait_for_responses = req.wait_for_responses;

            let registry_for_shard = self
                .service_locator
                .actor_registry()
                .await
                .ok_or_else(|| Status::internal("ActorRegistry not available"))?;
            let handle = tokio::spawn(async move {
                let mut succeeded = 0u32;
                let mut failed = 0u32;

                for (_key, mut message) in updates.clone() {
                    if message.id.is_empty() {
                        message.id = format!("req-{}", ulid::Ulid::new().to_string());
                    } else if !message.id.starts_with("req-") && !message.id.starts_with("res-") {
                        message.id = format!("req-{}", message.id);
                    }
                    let receiver_id = message.receiver_id.clone();
                    let actor_id = match plexspaces_actor::ActorId::from_canonical(&receiver_id) {
                        Ok(id) => id,
                        Err(_) => {
                            failed += 1;
                            continue;
                        }
                    };
                    let route_result = if wait_for_responses {
                        registry_for_shard
                            .ask(&ctx, &actor_id, message, timeout)
                            .await
                            .map(|_| ())
                    } else {
                        registry_for_shard.tell(&ctx, &actor_id, message).await
                    };
                    if route_result.is_ok() {
                        succeeded += 1;
                    } else {
                        failed += 1;
                    }
                }

                (shard_id, succeeded, failed, updates.len() as u32)
            });
            handles.push(handle);
        }

        let results = tokio::time::timeout(timeout, join_all(handles))
            .await
            .map_err(|_| "Bulk update timeout")?;

        let mut total_sent = 0u32;
        let mut total_succeeded = 0u32;
        let mut total_failed = 0u32;
        let mut shard_stats = Vec::new();

        for result in results {
            let (shard_id, succeeded, failed, sent) = result.unwrap_or((0, 0, 0, 0));
            total_sent += sent;
            total_succeeded += succeeded;
            total_failed += failed;

            let shard_actor_id = group
                .shard_actor_ids
                .get(shard_id as usize)
                .cloned()
                .unwrap_or_default();

            shard_stats.push(ShardUpdateStats {
                shard_id,
                shard_actor_id,
                updates_sent: sent,
                updates_succeeded: succeeded,
                updates_failed: failed,
            });
        }

        Ok(BulkUpdateShardGroupResponse {
            request_id,
            updates_sent: total_sent,
            updates_succeeded: total_succeeded,
            updates_failed: total_failed,
            shard_stats,
            errors: Vec::new(),
        })
    }

    /// Unified parallel operation helper (Erlang pmap pattern)
    pub(super) async fn parallel_operation_unified(
        &self,
        ctx: RequestContext,
        group_id: String,
        shard_actor_ids: Vec<String>,
        query_message: Message,
        timeout: Duration,
        operation_name: &str,
    ) -> Result<
        Vec<(u32, String, Duration, bool, String, Option<Message>)>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        let start_time = Instant::now();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                group_id = %group_id,
                shard_count = shard_actor_ids.len(),
                timeout_secs = timeout.as_secs(),
                tenant_id = %ctx.tenant_id(),
                "[{}] Starting parallel operation (ask_with_sender)",
                operation_name
            );
        }

        let registry = self
            .service_locator
            .actor_registry()
            .await
            .ok_or_else(|| "ActorRegistry not found in ServiceLocator".to_string())?;
        let mut handles = Vec::with_capacity(shard_actor_ids.len());
        for (shard_id, shard_actor_id) in shard_actor_ids.iter().enumerate() {
            let correlation_id = format!("req-shard-{}-{}", shard_id, Ulid::new().to_string());
            let request_start = Instant::now();
            let mut msg = query_message.clone();
            let message_id = if msg.id.is_empty() {
                format!("req-{}", Ulid::new().to_string())
            } else if !msg.id.starts_with("req-") && !msg.id.starts_with("res-") {
                format!("req-{}", msg.id)
            } else {
                msg.id.clone()
            };
            msg.id = message_id.clone();
            msg.receiver_id = shard_actor_id.clone();
            msg.message_type = "call".to_string();

            let registry_clone = registry.clone();
            let cid = correlation_id.clone();
            let sid = shard_actor_id.clone();
            let _mid = message_id.clone();
            let t = timeout;
            let ctx_task = ctx.clone();

            let shard_actor_id_parsed = plexspaces_actor::ActorId::from_canonical(&sid)
                .map_err(|e| format!("Invalid shard actor ID '{}': {}", sid, e))?;

            let handle = tokio::spawn(async move {
                let result = registry_clone
                    .ask_with_sender(&ctx_task, &shard_actor_id_parsed, msg, t, cid)
                    .await
                    .map_err(|e| match e {
                        plexspaces_actor::ActorRegistryError::ActorNotFound(id) => {
                            plexspaces_actor::ActorRefError::ActorNotFound(id.into())
                        }
                        plexspaces_actor::ActorRegistryError::Timeout => {
                            plexspaces_actor::ActorRefError::Timeout
                        }
                        plexspaces_actor::ActorRegistryError::VisibilityDenied(m) => {
                            plexspaces_actor::ActorRefError::VisibilityDenied(m)
                        }
                        other => plexspaces_actor::ActorRefError::SendFailed(other.to_string()),
                    });
                (shard_id as u32, sid, request_start, result)
            });
            handles.push(handle);
        }

        let join_results = join_all(handles).await;

        let mut results = Vec::with_capacity(join_results.len());
        for join_result in join_results {
            let (shard_id, shard_actor_id, request_start, result) =
                join_result.map_err(|e| format!("Task join error: {}", e))?;
            let latency = request_start.elapsed();
            match result {
                Ok(reply) => {
                    if reply.message_type == "error_reply" {
                        let error_msg = String::from_utf8(reply.payload.clone())
                            .ok()
                            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
                            .and_then(|v| v.get("error")?.as_str().map(String::from))
                            .unwrap_or_else(|| "Actor handler failed".to_string());
                        tracing::warn!(
                            group_id = %group_id,
                            shard_id = shard_id,
                            actor_id = %shard_actor_id,
                            latency_ms = latency.as_millis(),
                            error = %error_msg,
                            "❌ [{}] Shard returned error reply",
                            operation_name
                        );
                        results.push((shard_id, shard_actor_id, latency, false, error_msg, None));
                    } else {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!(
                                group_id = %group_id,
                                shard_id = shard_id,
                                actor_id = %shard_actor_id,
                                latency_ms = latency.as_millis(),
                                "[{}] Received reply",
                                operation_name
                            );
                        }
                        results.push((
                            shard_id,
                            shard_actor_id,
                            latency,
                            true,
                            String::new(),
                            Some(reply),
                        ));
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    tracing::warn!(
                        group_id = %group_id,
                        shard_id = shard_id,
                        actor_id = %shard_actor_id,
                        latency_ms = latency.as_millis(),
                        error = %error_msg,
                        "❌ [{}] Shard failed",
                        operation_name
                    );
                    results.push((shard_id, shard_actor_id, latency, false, error_msg, None));
                }
            }
        }

        results.sort_by_key(|r| r.0);

        let received_count = results.iter().filter(|r| r.3).count();
        let failed_count = results.len() - received_count;

        for result in &results {
            if result.3 {
                record_node_shard_messages_received(self.local_node_id.as_str());
            }
        }

        if failed_count > 0 {
            let errors: Vec<String> = results
                .iter()
                .filter_map(|r| {
                    if !r.3 {
                        Some(format!("Shard {} ({}): {}", r.0, r.1, r.4))
                    } else {
                        None
                    }
                })
                .collect();
            tracing::warn!(
                group_id = %group_id,
                total_duration_ms = start_time.elapsed().as_millis(),
                received = received_count,
                failed = failed_count,
                total = results.len(),
                errors = ?errors,
                "⚠️  [{}] Collected replies: {}/{} succeeded, {} failed",
                operation_name,
                received_count,
                results.len(),
                failed_count
            );
        } else if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                group_id = %group_id,
                total_duration_ms = start_time.elapsed().as_millis(),
                received = received_count,
                total = results.len(),
                "[{}] Collected replies: {}/{} succeeded",
                operation_name,
                received_count,
                results.len()
            );
        }

        Ok(results)
    }

    /// Internal implementation of map_shard_group
    pub(super) async fn map_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: MapShardGroupRequest,
    ) -> Result<MapShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let start_time = Instant::now();
        let group_id = req.group_id.clone();
        let request_id = req.request_id.clone();

        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(10));

        let query_proto = req
            .map_function
            .ok_or_else(|| "map_function is required".to_string())?;

        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                group_id.clone(),
                group.shard_actor_ids.clone(),
                query_proto,
                timeout,
                "MAP_SHARD_GROUP",
            )
            .await?;

        let mut shard_responses = Vec::new();
        let mut shards_responded = 0;
        let mut shards_failed = 0;
        let mut max_latency = Duration::ZERO;
        let mut min_latency = Duration::MAX;

        for (shard_id, shard_actor_id, latency, success, error, proto_response) in results {
            if success {
                shards_responded += 1;
                if latency < min_latency {
                    min_latency = latency;
                }
            } else {
                shards_failed += 1;
            }
            if latency > max_latency {
                max_latency = latency;
            }

            shard_responses.push(ShardQueryResponse {
                request_id: ulid::Ulid::new().to_string(),
                shard_id,
                shard_actor_id,
                response: proto_response,
                latency: Some(prost_types::Duration {
                    seconds: latency.as_secs() as i64,
                    nanos: latency.subsec_nanos() as i32,
                }),
                success,
                error,
            });
        }

        let total_duration = start_time.elapsed();

        record_node_shard_operation(self.local_node_id.as_str());
        if shards_failed > 0 {
            record_node_shard_operation_failed(self.local_node_id.as_str());
        }

        if shards_failed > 0 {
            let failed_shards: Vec<String> = shard_responses
                .iter()
                .filter_map(|r| {
                    if !r.success {
                        Some(format!(
                            "Shard {} ({}): {}",
                            r.shard_id, r.shard_actor_id, r.error
                        ))
                    } else {
                        None
                    }
                })
                .collect();
            tracing::warn!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = shard_group_config(&group).shard_count,
                shards_responded,
                shards_failed,
                failed_shards = ?failed_shards,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "⚠️  [MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded, {} failed",
                shards_responded,
                shard_group_config(&group).shard_count,
                shards_failed
            );
        } else if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = shard_group_config(&group).shard_count,
                shards_responded,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                "[MAP_SHARD_GROUP] Parallel map operation completed: {}/{} shards responded successfully",
                shards_responded,
                shard_group_config(&group).shard_count
            );
        }

        Ok(MapShardGroupResponse {
            request_id,
            shard_results: shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: shard_group_config(&group).shard_count,
                shards_responded,
                shards_failed,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        })
    }

    /// Internal implementation of scatter_gather
    pub(super) async fn scatter_gather_internal(
        &self,
        ctx: &RequestContext,
        req: ScatterGatherRequest,
    ) -> Result<ScatterGatherResponse, Box<dyn std::error::Error + Send + Sync>> {
        use std::time::SystemTime;
        let start_time = Instant::now();
        let group_id = req.group_id.clone();
        let request_id = req.request_id.clone();

        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", group_id))?
                .clone()
        };

        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));

        let query = req.query.ok_or_else(|| "query is required".to_string())?;

        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                group_id.clone(),
                group.shard_actor_ids.clone(),
                query,
                timeout,
                "SCATTER_GATHER",
            )
            .await?;

        let mut shard_responses = Vec::new();
        let mut shards_responded = 0;
        let mut shards_failed = 0;
        let mut max_latency = Duration::ZERO;
        let mut min_latency = Duration::MAX;
        let mut successful_responses = Vec::new();

        for (shard_id, shard_actor_id, latency, success, error, proto_response) in results {
            if success {
                shards_responded += 1;
                if latency < min_latency {
                    min_latency = latency;
                }
                if let Some(ref resp) = proto_response {
                    successful_responses.push((shard_id, resp.clone()));
                }
            } else {
                shards_failed += 1;
            }
            if latency > max_latency {
                max_latency = latency;
            }

            shard_responses.push(ShardQueryResponse {
                request_id: ulid::Ulid::new().to_string(),
                shard_id,
                shard_actor_id,
                response: proto_response,
                latency: Some(prost_types::Duration {
                    seconds: latency.as_secs() as i64,
                    nanos: latency.subsec_nanos() as i32,
                }),
                success,
                error,
            });
        }

        if shards_responded < req.min_responses as usize {
            let error_msg = format!(
                "Scatter-gather failed: only {} shards responded, minimum required: {}",
                shards_responded, req.min_responses
            );
            tracing::error!(
                group_id = %group_id,
                shards_responded,
                min_required = req.min_responses,
                "❌ [SCATTER_GATHER] {}",
                error_msg
            );
            return Err(error_msg.into());
        }

        let result = match req.aggregation {
            x if x == ShardGroupAggregationStrategy::ShardGroupAggregationConcat as i32 => {
                let mut aggregated_payloads = Vec::new();
                for (_shard_id, resp) in successful_responses {
                    aggregated_payloads.push(resp.payload);
                }
                Some(Message {
                    id: format!("scatter-gather-{}", Ulid::new()),
                    sender_id: "scatter-gather".to_string(),
                    receiver_id: String::new(),
                    channel: String::new(),
                    message_type: "aggregated".to_string(),
                    payload: aggregated_payloads.concat(),
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    headers: std::collections::HashMap::new(),
                    priority: 0,
                    ttl: None,
                    delivery_count: 0,
                    idempotency_key: String::new(),
                    correlation_id: String::new(),
                    reply_to: String::new(),
                    partition_key: String::new(),
                    uri_path: String::new(),
                    uri_method: String::new(),
                })
            }
            x if x == ShardGroupAggregationStrategy::ShardGroupAggregationMerge as i32 => {
                let mut sum: i64 = 0;
                for (_shard_id, resp) in successful_responses {
                    if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&resp.payload) {
                        if let Some(num) = value.as_i64() {
                            sum += num;
                        } else if let Some(num) = value.as_f64() {
                            sum += num as i64;
                        }
                    }
                }
                Some(Message {
                    id: format!("scatter-gather-{}", Ulid::new()),
                    sender_id: "scatter-gather".to_string(),
                    receiver_id: String::new(),
                    channel: String::new(),
                    message_type: "aggregated".to_string(),
                    payload: serde_json::json!({ "sum": sum }).to_string().into_bytes(),
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    headers: std::collections::HashMap::new(),
                    priority: 0,
                    ttl: None,
                    delivery_count: 0,
                    idempotency_key: String::new(),
                    correlation_id: String::new(),
                    reply_to: String::new(),
                    partition_key: String::new(),
                    uri_path: String::new(),
                    uri_method: String::new(),
                })
            }
            _ => successful_responses
                .first()
                .map(|(_shard_id, resp)| resp.clone()),
        };

        let total_duration = start_time.elapsed();

        record_node_shard_operation(self.local_node_id.as_str());
        if shards_failed > 0 {
            record_node_shard_operation_failed(self.local_node_id.as_str());
        }

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                group_id = %group_id,
                total_duration_ms = total_duration.as_millis(),
                shards_queried = shard_group_config(&group).shard_count,
                shards_responded,
                shards_failed,
                min_latency_ms = if min_latency == Duration::MAX { 0 } else { min_latency.as_millis() },
                max_latency_ms = max_latency.as_millis(),
                has_aggregated_result = result.is_some(),
                "[SCATTER_GATHER] Scatter-gather operation completed: {}/{} shards responded successfully",
                shards_responded,
                shard_group_config(&group).shard_count
            );
        }

        Ok(ScatterGatherResponse {
            request_id,
            result,
            shard_responses,
            stats: Some(ScatterGatherStats {
                shards_queried: shard_group_config(&group).shard_count,
                shards_responded: shards_responded as u32,
                shards_failed: shards_failed as u32,
                max_latency: Some(prost_types::Duration {
                    seconds: max_latency.as_secs() as i64,
                    nanos: max_latency.subsec_nanos() as i32,
                }),
            }),
        })
    }

    pub(super) async fn broadcast_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BroadcastShardGroupRequest,
    ) -> Result<BroadcastShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let request_id = req.request_id.clone();
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };
        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));
        let message = req.message.ok_or("message is required")?;
        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                req.group_id.clone(),
                group.shard_actor_ids.clone(),
                message,
                timeout,
                "BROADCAST_SHARD_GROUP",
            )
            .await?;
        let stats = scatter_stats_from_results(shard_group_config(&group).shard_count, &results);
        if stats.shards_responded < req.min_acks {
            return Err(format!(
                "Broadcast failed: only {} shards acknowledged, minimum required: {}",
                stats.shards_responded, req.min_acks
            )
            .into());
        }
        Ok(BroadcastShardGroupResponse {
            request_id,
            shard_responses: shard_query_responses_from_results(results),
            stats: Some(stats),
        })
    }

    pub(super) async fn reduce_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: ReduceShardGroupRequest,
    ) -> Result<ReduceShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let request_id = req.request_id.clone();
        let group = {
            let groups = self.shard_groups.read().await;
            groups
                .get(&req.group_id)
                .ok_or_else(|| format!("ShardGroup {} not found", req.group_id))?
                .clone()
        };
        let timeout = req
            .timeout
            .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
            .unwrap_or(Duration::from_secs(5));
        let map_function = req.map_function.ok_or("map_function is required")?;
        let results = self
            .parallel_operation_unified(
                ctx.clone(),
                req.group_id.clone(),
                group.shard_actor_ids.clone(),
                map_function,
                timeout,
                "REDUCE_SHARD_GROUP",
            )
            .await?;
        let stats = scatter_stats_from_results(shard_group_config(&group).shard_count, &results);
        if stats.shards_responded < req.min_responses {
            return Err(format!(
                "Reduce failed: only {} shards responded, minimum required: {}",
                stats.shards_responded, req.min_responses
            )
            .into());
        }
        let mut values = Vec::new();
        for (_shard_id, _actor_id, _latency, success, _error, response) in &results {
            if *success {
                let response = response
                    .as_ref()
                    .ok_or("Missing shard response for successful reduction")?;
                values.push(select_collective_value(response, req.target.as_ref())?);
            }
        }
        let reduced_value = reduce_values(values, req.reduction)?;
        let result = build_collective_message(
            "collective",
            serde_json::to_vec(&reduced_value)?,
            std::collections::HashMap::from([
                ("plexspaces-collective-op".to_string(), "reduce".to_string()),
                ("plexspaces-group-id".to_string(), req.group_id.clone()),
            ]),
        );
        Ok(ReduceShardGroupResponse {
            request_id,
            result: Some(result),
            shard_responses: shard_query_responses_from_results(results),
            stats: Some(stats),
        })
    }

    pub(super) async fn all_reduce_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: AllReduceShardGroupRequest,
    ) -> Result<AllReduceShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let request_id = req.request_id.clone();
        let reduce_req = ReduceShardGroupRequest {
            request_id: ulid::Ulid::new().to_string(),
            group_id: req.group_id.clone(),
            map_function: req.map_function.clone(),
            timeout: req.timeout,
            min_responses: req.min_responses,
            reduction: req.reduction,
            target: req.target.clone(),
        };
        let reduce_resp = self.reduce_shard_group_internal(ctx, reduce_req).await?;
        let reduced_message = reduce_resp
            .result
            .clone()
            .ok_or("AllReduce failed to produce reduced result")?;
        let mut broadcast_message = reduced_message.clone();
        broadcast_message.message_type = "event".to_string();
        broadcast_message.headers.insert(
            "plexspaces-collective-op".to_string(),
            "all-reduce-result".to_string(),
        );
        let broadcast_resp = self
            .broadcast_shard_group_internal(
                ctx,
                BroadcastShardGroupRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    group_id: req.group_id,
                    message: Some(broadcast_message),
                    timeout: req.timeout,
                    min_acks: req.min_responses,
                },
            )
            .await?;
        Ok(AllReduceShardGroupResponse {
            request_id,
            result: Some(reduced_message),
            shard_responses: broadcast_resp.shard_responses,
            stats: broadcast_resp.stats,
        })
    }

    pub(super) async fn barrier_shard_group_internal(
        &self,
        ctx: &RequestContext,
        req: BarrierShardGroupRequest,
    ) -> Result<BarrierShardGroupResponse, Box<dyn std::error::Error + Send + Sync>> {
        let request_id = req.request_id.clone();
        let payload = serde_json::json!({
            "barrier_id": req.barrier_id,
            "round": req.round,
        });
        let message = build_collective_message(
            "info",
            serde_json::to_vec(&payload)?,
            std::collections::HashMap::from([(
                "plexspaces-collective-op".to_string(),
                "barrier".to_string(),
            )]),
        );
        let response = self
            .broadcast_shard_group_internal(
                ctx,
                BroadcastShardGroupRequest {
                    request_id: ulid::Ulid::new().to_string(),
                    group_id: req.group_id,
                    message: Some(message),
                    timeout: req.timeout,
                    min_acks: req.min_acks,
                },
            )
            .await?;
        Ok(BarrierShardGroupResponse {
            request_id,
            shard_responses: response.shard_responses,
            stats: response.stats,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_partition() {
        let key1 = b"user-001";
        let key2 = b"user-002";
        let shard_count = 4;

        let shard1 = hash_partition(key1, shard_count).unwrap();
        let shard2 = hash_partition(key2, shard_count).unwrap();

        assert!(shard1 < shard_count);
        assert!(shard2 < shard_count);

        // Same key should map to same shard
        let shard1_again = hash_partition(key1, shard_count).unwrap();
        assert_eq!(shard1, shard1_again);
    }

    #[test]
    fn test_consistent_hash_partition() {
        let key1 = b"user-001";
        let key2 = b"user-002";
        let shard_count = 4;

        let shard1 = consistent_hash_partition(key1, shard_count).unwrap();
        let shard2 = consistent_hash_partition(key2, shard_count).unwrap();

        assert!(shard1 < shard_count);
        assert!(shard2 < shard_count);

        // Same key should map to same shard
        let shard1_again = consistent_hash_partition(key1, shard_count).unwrap();
        assert_eq!(shard1, shard1_again);
    }

    #[test]
    fn test_range_partition() {
        let shard_count = 4;

        // Test with explicit ranges
        let ranges = vec![
            b"a".to_vec(), // Shard 0: < "a"
            b"m".to_vec(), // Shard 1: "a" <= key < "m"
            b"t".to_vec(), // Shard 2: "m" <= key < "t"
            b"z".to_vec(), // Shard 3: "t" <= key < "z"
        ];

        assert_eq!(
            range_partition(b"apple", shard_count, Some(&ranges)).unwrap(),
            1
        );
        assert_eq!(
            range_partition(b"zebra", shard_count, Some(&ranges)).unwrap(),
            3
        );
        assert_eq!(
            range_partition(b"0", shard_count, Some(&ranges)).unwrap(),
            0
        );

        // Test without ranges (byte-based)
        let shard = range_partition(b"\x40", shard_count, None).unwrap();
        assert!(shard < shard_count);
    }
}
