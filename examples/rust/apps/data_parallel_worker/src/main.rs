// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Data Parallel Worker App
// 
// This app provides worker actors for data-parallel processing.
// It can be deployed as WASM to PlexSpaces nodes.
//
// When deployed, the worker actor behavior is registered on the node,
// allowing ShardGroups to spawn worker actors for parallel processing.

mod worker_actor;

use plexspaces_sdk::{ParallelClient, NodeBuilder};
use plexspaces_core::BehaviorRegistry;
use plexspaces_proto::actor::v1::{PartitionStrategy, ShardGroupAggregationStrategy, ConsistencyLevel};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, Level};
use tracing_subscriber;

use worker_actor::WorkerActor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("data_parallel_worker=info,plexspaces=warn")
        .init();

    info!("Data Parallel Worker App starting...");

    // Create a local node to register the worker behavior
    // Use a different port (9000) to avoid conflicts with test-local.sh nodes (8000, 8010, 8020)
    let node = NodeBuilder::new("worker-registry-node".to_string())
        .with_listen_addr("0.0.0.0:9000".to_string())
        .build()
        .await;

    // Register worker actor behavior
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry.register_simple("worker", || {
        Box::pin(async move {
            Ok(Box::new(WorkerActor::new("worker".to_string())) as Box<dyn plexspaces_core::Actor>)
        })
    }).await;
    
    // Register the registry with ServiceLocator
    let service_locator = node.service_locator();
    service_locator.register_behavior_registry(Arc::new(behavior_registry)).await;

    info!("✅ Worker actor behavior registered as 'worker'");

    // Start the node (required for ShardGroup operations)
    info!("Starting node...");
    let node_arc = Arc::new(node);
    let node_for_start = node_arc.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            eprintln!("Node failed to start: {}", e);
        }
    });

    // Wait for node to initialize
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    info!("✅ Node started successfully");

    // Create ParallelClient from service locator
    let mut client = ParallelClient::from_service_locator(service_locator.clone()).await?;
    
    // Benchmark configuration
    const WORKER_COUNT: u32 = 20;
    const TOTAL_MESSAGES: usize = 10000;
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Benchmark Configuration:");
    info!("  Workers per node: {}", WORKER_COUNT);
    info!("  Total messages: {}", TOTAL_MESSAGES);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Get initial metrics
    let metrics_accessor = service_locator.get_node_metrics_accessor().await;
    let initial_metrics = if let Some(ref accessor) = metrics_accessor {
        Some(accessor.get_metrics().await)
    } else {
        None
    };
    
    // Create a worker pool (ShardGroup) with 20 workers
    info!("Creating ShardGroup 'worker-pool-1' with {} workers...", WORKER_COUNT);
    let create_start = Instant::now();
    let pool_id = client.create_worker_pool(
        "worker-pool-1",
        "worker",
        WORKER_COUNT,
        PartitionStrategy::PartitionStrategyHash,
        HashMap::new(),
    ).await?;
    let create_duration = create_start.elapsed();
    info!("✅ Created ShardGroup: {} (took {:?})", pool_id, create_duration);

    // Benchmark: Bulk updates (10,000 messages)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Benchmark: Bulk Updates ({} messages)", TOTAL_MESSAGES);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let bulk_start = Instant::now();
    let mut updates = HashMap::new();
    for i in 0..TOTAL_MESSAGES {
        let key = format!("key-{:05}", i);
        updates.insert(key.clone(), json!({ "action": "set", "key": key, "value": i }));
    }
    
    let update_start = Instant::now();
    let update_stats = client.parallel_update(
        &pool_id,
        updates,
        ConsistencyLevel::ConsistencyLevelEventual,
        false,
    ).await?;
    let update_duration = update_start.elapsed();
    let bulk_duration = bulk_start.elapsed();
    
    info!("✅ Bulk update completed:");
    info!("  Total duration: {:?}", bulk_duration);
    info!("  Update duration: {:?}", update_duration);
    let updates_sent = *update_stats.get("updates_sent").unwrap_or(&0) as u64;
    let updates_succeeded = *update_stats.get("updates_succeeded").unwrap_or(&0) as u64;
    let updates_failed = *update_stats.get("updates_failed").unwrap_or(&0) as u64;
    info!("  Messages sent: {}", updates_sent);
    info!("  Messages succeeded: {}", updates_succeeded);
    info!("  Messages failed: {}", updates_failed);
    info!("  Throughput: {:.2} msg/s", TOTAL_MESSAGES as f64 / update_duration.as_secs_f64());
    info!("  Avg latency per message: {:.2} ms", update_duration.as_millis() as f64 / TOTAL_MESSAGES as f64);

    // Benchmark: Parallel map (query all workers)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Benchmark: Parallel Map (query all {} workers)", WORKER_COUNT);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let map_start = Instant::now();
    let map_task = json!({
        "action": "get_total_count"
    });
    
    let map_results = client.parallel_map(&pool_id, map_task.clone()).await?;
    let map_duration = map_start.elapsed();
    
    info!("✅ Parallel map completed:");
    info!("  Duration: {:?}", map_duration);
    info!("  Results: {} workers responded", map_results.len());
    info!("  Avg latency per worker: {:.2} ms", map_duration.as_millis() as f64 / WORKER_COUNT as f64);

    // Benchmark: Parallel reduce (aggregate stats)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Benchmark: Parallel Reduce (aggregate from {} workers)", WORKER_COUNT);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let reduce_start = Instant::now();
    let reduce_result = client.parallel_reduce(
        &pool_id,
        json!({ "action": "stats" }),
        ShardGroupAggregationStrategy::ShardGroupAggregationConcat,
        WORKER_COUNT,
    ).await?;
    let reduce_duration = reduce_start.elapsed();
    
    info!("✅ Parallel reduce completed:");
    info!("  Duration: {:?}", reduce_duration);
    info!("  Result: {:?}", reduce_result);

    // Get final metrics and compute coordination overhead
    if let Some(ref accessor) = metrics_accessor {
        let final_metrics = accessor.get_metrics().await;
        
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        info!("Node Metrics Summary:");
        info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        
        if let Some(ref initial) = initial_metrics {
            let messages_routed = final_metrics.messages_routed.saturating_sub(initial.messages_routed);
            let local_deliveries = final_metrics.local_deliveries.saturating_sub(initial.local_deliveries);
            let remote_deliveries = final_metrics.remote_deliveries.saturating_sub(initial.remote_deliveries);
            let failed_deliveries = final_metrics.failed_deliveries.saturating_sub(initial.failed_deliveries);
            
            info!("  Messages routed: {}", messages_routed);
            info!("  Local deliveries: {}", local_deliveries);
            info!("  Remote deliveries: {}", remote_deliveries);
            info!("  Failed deliveries: {}", failed_deliveries);
            info!("  Delivery success rate: {:.2}%", 
                if messages_routed > 0 {
                    ((local_deliveries + remote_deliveries) as f64 / messages_routed as f64) * 100.0
                } else {
                    0.0
                }
            );
        }
        
        info!("  Active actors: {}", final_metrics.active_actors);
        info!("  Shard operations: {}", final_metrics.shard_operations_total);
        info!("  Shard operations failed: {}", final_metrics.shard_operations_failed);
    }

    // E2E Summary
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("E2E Benchmark Summary:");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  Workers: {}", WORKER_COUNT);
    info!("  Total messages: {}", TOTAL_MESSAGES);
    info!("  Bulk update: {:?} ({:.2} msg/s)", update_duration, TOTAL_MESSAGES as f64 / update_duration.as_secs_f64());
    info!("  Parallel map: {:?} ({:.2} workers/s)", map_duration, WORKER_COUNT as f64 / map_duration.as_secs_f64());
    info!("  Parallel reduce: {:?}", reduce_duration);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    info!("✅ Benchmark completed successfully!");
    Ok(())
}
