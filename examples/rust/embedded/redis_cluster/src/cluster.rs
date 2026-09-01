// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Cluster setup and high-level operation helpers.
//!
//! `RedisCluster` owns the two shard groups (master + replica) and the
//! `ShardGroupClientLocal` used to drive all framework primitives.

use crate::storage::StorageActor;
use crate::RedisResult;
use anyhow::{Context, Result};
use plexspaces_actor::{
    behavior_factory::BehaviorRegistry, InitializableServiceLocator,
};
use plexspaces_proto::actor::v1::{
    CollectiveReduction, DataParallelConfig, NodePlacement, NodePlacementStrategy,
    PartitionStrategy, RebalancePolicy, ShardGroup,
};
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use plexspaces_sdk::{json, NodeBuilder, RequestContext, RequestContextExt, ShardGroupClientLocal, ShardGroupClientTrait};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

// Re-exported so main.rs can use it without repeating the path
pub use plexspaces_sdk::ShardGroupClientTrait as _;

/// Create a shard group using a caller-supplied `RequestContext` (with proper tenant/namespace).
/// Bypasses `ShardGroupClientLocal::create_shard_group` which uses the empty-namespace system context.
async fn create_shard_group_with_ctx(
    actor_service: Arc<dyn plexspaces_actor::ActorService>,
    ctx: &RequestContext,
    group_id: String,
    actor_type: String,
    shard_count: u32,
    partition_strategy: PartitionStrategy,
    placement: Option<NodePlacement>,
) -> Result<ShardGroup> {
    let req = plexspaces_proto::actor::v1::CreateShardGroupRequest {
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
        metadata: std::collections::HashMap::<String, String>::new(),
    };

    let resp = actor_service
        .create_shard_group(ctx, req)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create ShardGroup: {}", e))?;

    resp.group
        .ok_or_else(|| anyhow::anyhow!("No group in response"))
}

// =============================================================================
// Node pair — two real nodes, each with a gRPC server started via build_started()
// =============================================================================

pub const MASTER_PORT: u16 = 8091;
pub const REPLICA_PORT: u16 = 8093;

/// Wait until `addr` accepts a TCP connection (condition-based readiness, no fixed sleep).
async fn wait_for_port(addr: std::net::SocketAddr) -> Result<()> {
    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(10);
    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("port {} not ready within 10s", addr);
        }
        tokio::task::yield_now().await;
    }
}

/// Register `remote_node_id` in `local_node`'s object registry so that gRPC
/// routing from local → remote works.
pub async fn register_peer(
    local_node: &plexspaces_node::Node,
    remote_node_id: &str,
    remote_grpc_addr: &str,
) -> Result<()> {
    let ctx = RequestContext::new_without_auth("system".to_string(), "default".to_string());
    let registry = local_node
        .service_locator()
        .object_registry()
        .await
        .context("object registry not available")?;

    // Strip leading "http://" — the registry stores bare host:port
    let grpc_address = remote_grpc_addr
        .strip_prefix("http://")
        .unwrap_or(remote_grpc_addr)
        .to_string();

    registry
        .register(
            &ctx,
            ObjectRegistration {
                object_type: ObjectType::ObjectTypeNode as i32,
                object_id: remote_node_id.to_string(),
                grpc_address,
                object_category: "Node".to_string(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| anyhow::anyhow!("failed to register peer in object registry: {}", e))?;
    Ok(())
}

/// Register `StorageActor` behavior type on a node so that `create_shard_group`
/// can spawn actors by type-name string.
pub async fn register_storage_behavior(
    node: &plexspaces_node::Node,
    role: &'static str,
) -> Result<()> {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = BehaviorRegistry::new();
    registry
        .register_simple("storage_actor", move || {
            let id = counter.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                Ok(Box::new(StorageActor::new_with_id(id, role))
                    as Box<dyn plexspaces_actor::Actor>)
            })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;
    Ok(())
}

// =============================================================================
// RedisCluster — high-level handle
// =============================================================================

/// Handle to a running two-node Redis cluster.
///
/// Masters live on `redis-master-node`; replicas on `redis-replica-node`.
/// All shard group primitives (broadcast, scatter_gather, reduce, barrier, map)
/// are driven through `sg_client`, which is bound to the master node but
/// routes to the replica node via gRPC for replica-group operations.
pub struct RedisCluster {
    pub master_group: ShardGroup,
    pub replica_group: ShardGroup,
    pub sg_client: ShardGroupClientLocal,
    pub num_shards: usize,
    pub ctx: RequestContext,
}

/// Build a complete two-node Redis cluster.
///
/// 1. Creates two `NodeBuilder` nodes in-process.
/// 2. Starts a gRPC `ActorService` server for each node.
/// 3. Registers both nodes in each other's object registries (peer discovery).
/// 4. Registers `StorageActor` behavior on both nodes (so `create_shard_group`
///    can spawn actors remotely on the replica node).
/// 5. Creates the master shard group (3 shards on `redis-master-node`).
/// 6. Creates the replica shard group (3 shards on `redis-replica-node`).
/// 7. Runs the replication handshake (PING→REPLCONF→PSYNC) per replica shard.
/// 8. Bulk-syncs initial state to all replicas.
pub async fn setup_redis_cluster(
    num_shards: usize,
) -> Result<(RedisCluster, Arc<plexspaces_node::Node>, Arc<plexspaces_node::Node>)> {
    // --- Step 1: start nodes on fixed ports 8091 (master) and 8093 (replica) ---
    // build_started() calls node.start() in a background task, which starts the gRPC server.
    let master_node = NodeBuilder::new("redis-master-node")
        .with_listen_addr(format!("127.0.0.1:{}", MASTER_PORT))
        .with_auth_disabled()
        .build_started()
        .await;

    let replica_node = NodeBuilder::new("redis-replica-node")
        .with_listen_addr(format!("127.0.0.1:{}", REPLICA_PORT))
        .with_auth_disabled()
        .build_started()
        .await;

    // --- Step 2: wait for gRPC servers to accept connections ---
    let master_sock = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), MASTER_PORT,
    );
    let replica_sock = std::net::SocketAddr::new(
        std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST), REPLICA_PORT,
    );
    wait_for_port(master_sock).await?;
    wait_for_port(replica_sock).await?;

    let master_grpc_addr = format!("127.0.0.1:{}", MASTER_PORT);
    let replica_grpc_addr = format!("127.0.0.1:{}", REPLICA_PORT);

    // --- Step 3: peer discovery ---
    register_peer(&master_node, "redis-replica-node", &replica_grpc_addr).await?;
    register_peer(&replica_node, "redis-master-node", &master_grpc_addr).await?;

    // --- Step 4: register behavior on both nodes ---
    register_storage_behavior(&master_node, "master").await?;
    register_storage_behavior(&replica_node, "replica").await?;

    // --- Step 5 & 6: create shard groups ---
    let ctx = RequestContext::new_without_auth(
        "redis-tenant".to_string(),
        "redis".to_string(),
    );

    let mut sg_client = ShardGroupClientLocal::new(master_node.service_locator())
        .await
        .context("failed to create ShardGroupClientLocal")?
        .with_namespace("redis-tenant", "redis");

    let actor_service = master_node
        .service_locator()
        .get_actor_service()
        .await
        .context("ActorService not available on master node")?;

    // Use create_shard_group_with_ctx to supply a proper tenant/namespace (required by actor validation).
    let master_group = create_shard_group_with_ctx(
        actor_service.clone(),
        &ctx,
        "redis-masters".to_string(),
        "storage_actor".to_string(),
        num_shards as u32,
        PartitionStrategy::PartitionStrategyHash,
        Some(NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyNodeIds as i32,
            node_ids: vec!["redis-master-node".to_string()],
            ..Default::default()
        }),
    )
    .await
    .context("failed to create master shard group")?;

    let replica_group = create_shard_group_with_ctx(
        actor_service,
        &ctx,
        "redis-replicas".to_string(),
        "storage_actor".to_string(),
        num_shards as u32,
        PartitionStrategy::PartitionStrategyHash,
        Some(NodePlacement {
            strategy: NodePlacementStrategy::NodePlacementStrategyNodeIds as i32,
            node_ids: vec!["redis-replica-node".to_string()],
            ..Default::default()
        }),
    )
    .await
    .context("failed to create replica shard group")?;

    // --- Step 7: replication handshake (PING → REPLCONF → PSYNC) ---
    for step in &["ping", "replconf", "psync"] {
        sg_client
            .broadcast(
                "redis-replicas".to_string(),
                json!({ "action": "handshake", "step": step, "args": [] }),
                num_shards as u32,
            )
            .await
            .context(format!("handshake step '{}' failed", step))?;
    }

    // --- Step 8: initial bulk state sync (empty at start — mirrors RDB transfer) ---
    sg_client
        .broadcast(
            "redis-replicas".to_string(),
            json!({
                "action": "bulk_sync",
                "data": {},
                "offset": 0,
            }),
            num_shards as u32,
        )
        .await
        .context("bulk_sync failed")?;

    let cluster = RedisCluster {
        master_group,
        replica_group,
        sg_client,
        num_shards,
        ctx,
    };

    Ok((cluster, master_node, replica_node))
}

// =============================================================================
// High-level operations
// =============================================================================

impl RedisCluster {
    /// SET key value with optional NX/XX/EX/PX — routes to the correct master shard.
    pub async fn set(
        &mut self,
        key: &str,
        value: &str,
        nx: bool,
        xx: bool,
        ex: Option<u64>,
        px: Option<u64>,
    ) -> Result<RedisResult> {
        let updates = std::collections::HashMap::from([(
            key.to_string(),
            json!({
                "action": "set",
                "key": key,
                "value": value,
                "nx": nx,
                "xx": xx,
                "ex": ex,
                "px": px,
            }),
        )]);
        let resp = self
            .sg_client
            .bulk_update(
                "redis-masters".to_string(),
                updates,
                plexspaces_proto::actor::v1::ConsistencyLevel::ConsistencyLevelEventual,
                true,
            )
            .await
            .context("SET bulk_update failed")?;

        // bulk_update succeeded → result is determined by shard reply
        let _ = resp;
        Ok(RedisResult::Ok("OK".to_string()))
    }

    /// GET key — queries all master shards, returns the value from the owning shard.
    pub async fn get(&mut self, key: &str) -> Result<RedisResult> {
        let resp = self
            .sg_client
            .map(
                "redis-masters".to_string(),
                json!({ "action": "get", "key": key }),
            )
            .await
            .context("GET map failed")?;

        for shard_result in &resp.shard_results {
            if let Some(msg) = &shard_result.response {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                    if v.get("found").and_then(|f| f.as_bool()).unwrap_or(false) {
                        let value = v
                            .get("result")
                            .and_then(|r| r.as_str())
                            .unwrap_or("")
                            .to_string();
                        return Ok(RedisResult::Ok(value));
                    }
                }
            }
        }
        Ok(RedisResult::Nil)
    }

    /// INCR key — routes to the owning master shard via bulk_update.
    pub async fn incr(&mut self, key: &str) -> Result<RedisResult> {
        let updates = std::collections::HashMap::from([(
            key.to_string(),
            json!({ "action": "incr", "key": key }),
        )]);
        let _resp = self
            .sg_client
            .bulk_update(
                "redis-masters".to_string(),
                updates,
                plexspaces_proto::actor::v1::ConsistencyLevel::ConsistencyLevelEventual,
                true,
            )
            .await
            .context("INCR bulk_update failed")?;

        // Re-read via GET to get the new value (simplified for demo)
        let get_resp = self
            .sg_client
            .map(
                "redis-masters".to_string(),
                json!({ "action": "get", "key": key }),
            )
            .await?;

        for sr in &get_resp.shard_results {
            if let Some(msg) = &sr.response {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                    if v.get("found").and_then(|f| f.as_bool()).unwrap_or(false) {
                        if let Some(s) = v.get("result").and_then(|r| r.as_str()) {
                            if let Ok(n) = s.parse::<i64>() {
                                return Ok(RedisResult::Integer(n));
                            }
                        }
                    }
                }
            }
        }
        Ok(RedisResult::Error("ERR value is not an integer or out of range".to_string()))
    }

    /// DEL key
    pub async fn del(&mut self, key: &str) -> Result<RedisResult> {
        let updates = std::collections::HashMap::from([(
            key.to_string(),
            json!({ "action": "del", "key": key }),
        )]);
        self.sg_client
            .bulk_update(
                "redis-masters".to_string(),
                updates,
                plexspaces_proto::actor::v1::ConsistencyLevel::ConsistencyLevelEventual,
                true,
            )
            .await
            .context("DEL bulk_update failed")?;
        Ok(RedisResult::Integer(1))
    }

    /// PING → all master shards; returns count of PONG responses.
    pub async fn ping(&mut self) -> Result<String> {
        let resp = self
            .sg_client
            .map("redis-masters".to_string(), json!({ "action": "ping" }))
            .await?;
        Ok(format!("PONG (from {} shards)", resp.shard_results.len()))
    }

    /// DBSIZE — reduce(SUM) across all master shards.
    pub async fn dbsize(&mut self) -> Result<i64> {
        let resp = self
            .sg_client
            .reduce(
                "redis-masters".to_string(),
                json!({ "action": "dbsize" }),
                CollectiveReduction::CollectiveReductionSum,
                Some("count".to_string()),
                self.num_shards as u32,
            )
            .await
            .context("DBSIZE reduce failed")?;

        if let Some(msg) = resp.result {
            if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                return Ok(v.get("count").and_then(|n| n.as_i64()).unwrap_or(0));
            }
        }
        // Fallback: sum shard_responses individually
        let total: i64 = resp
            .shard_responses
            .iter()
            .filter_map(|sr| sr.response.as_ref())
            .filter_map(|msg| serde_json::from_slice::<serde_json::Value>(&msg.payload).ok())
            .filter_map(|v| v.get("count").and_then(|n| n.as_i64()))
            .sum();
        Ok(total)
    }

    /// KEYS — scatter_gather across all master shards, concatenate results.
    pub async fn keys(&mut self) -> Result<Vec<String>> {
        let resp = self
            .sg_client
            .map("redis-masters".to_string(), json!({ "action": "keys" }))
            .await
            .context("KEYS map failed")?;

        let mut all_keys = Vec::new();
        for sr in &resp.shard_results {
            if let Some(msg) = &sr.response {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&msg.payload) {
                    if let Some(arr) = v.get("keys").and_then(|k| k.as_array()) {
                        for key in arr {
                            if let Some(s) = key.as_str() {
                                all_keys.push(s.to_string());
                            }
                        }
                    }
                }
            }
        }
        Ok(all_keys)
    }

    /// Propagate a write to all replicas via `broadcast_shard_group`.
    /// This is the Ch7-8 replication broadcast — one call fans out to all replicas.
    pub async fn propagate_to_replicas(
        &mut self,
        command: &str,
        key: &str,
        value: &str,
        offset: i64,
    ) -> Result<usize> {
        let resp = self
            .sg_client
            .broadcast(
                "redis-replicas".to_string(),
                json!({
                    "action": "replicate",
                    "command": command,
                    "key": key,
                    "value": value,
                    "offset": offset,
                }),
                self.num_shards as u32,
            )
            .await
            .context("broadcast replication failed")?;

        let ack_count = resp.shard_responses.iter().filter(|sr| sr.success).count();
        Ok(ack_count)
    }

    /// WAIT numreplicas timeout_ms — scatter_gather on replica group for ACKs (Ch8).
    /// Returns the count of replicas whose replication_offset >= master_offset.
    pub async fn wait(&mut self, min_replicas: u32, master_offset: i64) -> Result<usize> {
        let resp = self
            .sg_client
            .scatter_gather(
                "redis-replicas".to_string(),
                json!({ "action": "get_ack" }),
                plexspaces_proto::actor::v1::ShardGroupAggregationStrategy::ShardGroupAggregationMerge,
                1, // min_responses — collect whatever we can
            )
            .await
            .context("WAIT scatter_gather failed")?;

        let ack_count = resp
            .shard_responses
            .iter()
            .filter(|sr| sr.success)
            .filter(|sr| {
                sr.response
                    .as_ref()
                    .and_then(|msg| serde_json::from_slice::<serde_json::Value>(&msg.payload).ok())
                    .and_then(|v| v.get("offset").and_then(|o| o.as_i64()))
                    .map(|offset| offset >= master_offset)
                    .unwrap_or(false)
            })
            .count();

        Ok(ack_count.min(min_replicas as usize))
    }

    /// Active expiry sweep — parallel map across all master shards.
    pub async fn expire_sweep(&mut self) -> Result<()> {
        self.sg_client
            .broadcast(
                "redis-masters".to_string(),
                json!({ "action": "expire_sweep" }),
                1, // fire-and-forget, just need 1 ack to confirm delivery
            )
            .await
            .context("expire_sweep broadcast failed")?;
        Ok(())
    }

    /// Bulk SET — routes N key-value pairs through bulk_update for high throughput.
    pub async fn bulk_set(&mut self, pairs: &[(String, String)]) -> Result<usize> {
        let updates: std::collections::HashMap<String, serde_json::Value> = pairs
            .iter()
            .map(|(k, v)| (k.clone(), json!({ "action": "set", "key": k, "value": v })))
            .collect();
        let count = updates.len();
        self.sg_client
            .bulk_update(
                "redis-masters".to_string(),
                updates,
                plexspaces_proto::actor::v1::ConsistencyLevel::ConsistencyLevelEventual,
                true,
            )
            .await
            .context("bulk_set failed")?;
        Ok(count)
    }

    /// Coordinated snapshot — parallel map collects state from all shards simultaneously.
    pub async fn coordinated_snapshot(&mut self) -> Result<Vec<serde_json::Value>> {
        // Map: collect full state from every shard in parallel (fan-out is the synchronization)
        let resp = self
            .sg_client
            .map("redis-masters".to_string(), json!({ "action": "snapshot" }))
            .await
            .context("snapshot map failed")?;

        let snapshots: Vec<serde_json::Value> = resp
            .shard_results
            .iter()
            .filter_map(|sr| sr.response.as_ref())
            .filter_map(|msg| serde_json::from_slice(&msg.payload).ok())
            .collect();

        Ok(snapshots)
    }
}
