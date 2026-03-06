// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unified Parallel Processing API
// Aligns ShardGroup, ElasticPool, and resource-based routing for cohesive data-parallel operations

use crate::{ShardGroupClientTrait, UnifiedShardGroupClient};
use anyhow::Result;
use plexspaces_proto::actor::v1::{
    ConsistencyLevel, PartitionStrategy, ShardGroupAggregationStrategy,
};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Parallel processing client over ShardGroup (map/reduce, scatter-gather).
/// Uses ServiceLocator for local access; gRPC for remote can be added separately.
pub struct ParallelClient {
    shard_client: UnifiedShardGroupClient,
}

impl ParallelClient {
    /// Connect to a node via gRPC (requires `grpc` feature).
    #[cfg(feature = "grpc")]
    pub async fn connect(node_addr: &str) -> Result<Self> {
        let shard_client = UnifiedShardGroupClient::from_node_addr(node_addr).await?;
        Ok(Self { shard_client })
    }

    /// Create parallel client from ServiceLocator (in-process / embedded).
    pub async fn from_service_locator(service_locator: Arc<dyn plexspaces_core::ServiceLocator>) -> Result<Self> {
        let shard_client = UnifiedShardGroupClient::from_service_locator(service_locator).await?;
        Ok(Self { shard_client })
    }

    /// Create a worker pool (ShardGroup) with resource-based placement
    ///
    /// ## Unified API Design
    /// - Uses ShardGroup for data-parallel sharding
    /// - Labels flow to ActorResourceRequirements for node placement
    /// - Aligns with ElasticPool patterns (worker pool abstraction)
    /// - Removes boilerplate: auto-creates RequestContext, handles errors
    pub async fn create_worker_pool(
        &mut self,
        pool_id: &str,
        actor_type: &str,
        worker_count: u32,
        partition_strategy: PartitionStrategy,
        labels: HashMap<String, String>,
    ) -> Result<String> {
        // Create ShardGroup (acts as worker pool)
        // SDK removes boilerplate: no need to manually create RequestContext or convert types
        let group = self
            .shard_client
            .create_shard_group(
                pool_id.to_string(),
                actor_type.to_string(),
                worker_count,
                partition_strategy,
                labels,
            )
            .await?;
        Ok(group.group_id)
    }

    /// Parallel Map: Apply function to all workers in parallel
    ///
    /// ## Unified API Design
    /// - Uses ShardGroup.map() for parallel execution
    /// - Aligns with ElasticPool checkout pattern (workers process tasks)
    /// - Returns individual worker results (like ElasticPool checkout/checkin)
    pub async fn parallel_map(
        &mut self,
        pool_id: &str,
        task: Value,
    ) -> Result<Vec<Value>> {
        let map_resp = self.shard_client.map(pool_id.to_string(), task).await?;
        
        let mut results = Vec::new();
        let mut failed_count = 0;
        let mut error_details = Vec::new();
        
        let total_shards = map_resp.shard_results.len();
        for shard_resp in map_resp.shard_results {
            if shard_resp.success {
                if let Some(response) = shard_resp.response {
                    match serde_json::from_slice::<Value>(&response.payload) {
                        Ok(payload) => results.push(payload),
                        Err(e) => {
                            failed_count += 1;
                            error_details.push(format!("Shard {}: JSON parse error: {}", shard_resp.shard_id, e));
                        }
                    }
                } else {
                    failed_count += 1;
                    error_details.push(format!("Shard {}: No response payload", shard_resp.shard_id));
                }
            } else {
                failed_count += 1;
                error_details.push(format!("Shard {} ({}): {}", shard_resp.shard_id, shard_resp.shard_actor_id, shard_resp.error));
            }
        }
        
        // Log detailed error information if any failures occurred
        if failed_count > 0 {
            tracing::warn!(
                pool_id = %pool_id,
                successful = results.len(),
                failed = failed_count,
                total = total_shards,
                errors = ?error_details,
                "Parallel map completed with {} failures out of {} shards",
                failed_count,
                total_shards
            );
        }
        
        // Log stats if available
        if let Some(stats) = map_resp.stats {
            tracing::info!(
                pool_id = %pool_id,
                shards_queried = stats.shards_queried,
                shards_responded = stats.shards_responded,
                shards_failed = stats.shards_failed,
                max_latency_ms = stats.max_latency.as_ref().map(|d| d.seconds * 1000 + d.nanos as i64 / 1_000_000).unwrap_or(0),
                "Parallel map stats: {}/{} shards responded",
                stats.shards_responded,
                stats.shards_queried
            );
        }
        
        Ok(results)
    }

    /// Parallel Reduce: Aggregate results from all workers
    ///
    /// ## Unified API Design
    /// - Uses ShardGroup.scatter_gather() for aggregation
    /// - Aligns with ElasticPool pattern (collect results from all workers)
    /// - Supports multiple aggregation strategies (sum, concat, merge)
    pub async fn parallel_reduce(
        &mut self,
        pool_id: &str,
        query: Value,
        aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<Value> {
        let scatter_resp = self
            .shard_client
            .scatter_gather(pool_id.to_string(), query, aggregation, min_responses)
            .await?;
        
        if let Some(result) = scatter_resp.result {
            // Parse payload - handle concatenated JSON objects from Concat aggregation
            let payload_str = String::from_utf8_lossy(&result.payload);
            
            // Try parsing as single JSON value first
            match serde_json::from_str::<Value>(&payload_str) {
                Ok(payload) => Ok(payload),
                Err(e) => {
                    // If that fails, it might be concatenated JSON objects (from Concat aggregation)
                    // Use streaming deserializer to parse multiple JSON values
                    use serde_json::Deserializer;
                    let mut stream = Deserializer::from_str(&payload_str).into_iter::<Value>();
                    let mut results = Vec::new();
                    
                    while let Some(Ok(value)) = stream.next() {
                        results.push(value);
                    }
                    
                    if results.is_empty() {
                        // If streaming parser also failed, log error and return context
                        tracing::warn!(
                            pool_id = %pool_id,
                            error = %e,
                            payload_preview = %payload_str.chars().take(200).collect::<String>(),
                            "Failed to parse scatter_gather result as single JSON or concatenated JSON objects"
                        );
                        Err(anyhow::anyhow!("Failed to parse scatter_gather result: {}. Payload preview: {}", e, payload_str.chars().take(200).collect::<String>()))
                    } else if results.len() == 1 {
                        // Single result, return it directly
                        Ok(results.into_iter().next().unwrap())
                    } else {
                        // Multiple results, return as array
                        Ok(json!({ "results": results }))
                    }
                }
            }
        } else {
            Ok(json!({ "error": "No results from workers" }))
        }
    }

    /// Parallel Update: Bulk updates to workers (DPA UpdateFunction)
    ///
    /// ## Unified API Design
    /// - Uses ShardGroup.bulk_update() for parallel writes
    /// - Routes to workers based on partition key (like ElasticPool checkout by key)
    /// - Supports consistency levels (eventual vs strong)
    pub async fn parallel_update(
        &mut self,
        pool_id: &str,
        updates: HashMap<String, Value>,
        consistency: ConsistencyLevel,
        wait_for_responses: bool,
    ) -> Result<HashMap<String, u32>> {
        let bulk_resp = self
            .shard_client
            .bulk_update(pool_id.to_string(), updates, consistency, wait_for_responses)
            .await?;
        
        let mut stats = HashMap::new();
        stats.insert("updates_sent".to_string(), bulk_resp.updates_sent);
        stats.insert("updates_succeeded".to_string(), bulk_resp.updates_succeeded);
        stats.insert("updates_failed".to_string(), bulk_resp.updates_failed);
        
        Ok(stats)
    }

    /// Map-Reduce: High-level parallel map/reduce operation
    ///
    /// ## Unified API Design
    /// Combines parallel_map + parallel_reduce for complete map-reduce pattern:
    /// 1. Map: Apply function to all workers in parallel
    /// 2. Reduce: Aggregate results using specified strategy
    ///
    /// Aligns with:
    /// - ShardGroup (data-parallel sharding)
    /// - ElasticPool (worker pool abstraction)
    /// - Resource-based routing (labels → node placement)
    pub async fn map_reduce(
        &mut self,
        pool_id: &str,
        map_task: Value,
        reduce_aggregation: ShardGroupAggregationStrategy,
        min_responses: u32,
    ) -> Result<Value> {
        // Step 1: Map (parallel execution)
        let _map_results = self.parallel_map(pool_id, map_task.clone()).await?;
        
        // Step 2: Reduce (aggregate results)
        // For reduce, we can either:
        // a) Use scatter_gather directly (more efficient)
        // b) Aggregate map results client-side (more flexible)
        // Using scatter_gather for efficiency
        self.parallel_reduce(pool_id, map_task, reduce_aggregation, min_responses).await
    }
}
