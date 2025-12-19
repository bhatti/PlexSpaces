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

//! Integration tests for multi-node TupleSpace coordination with shared Redis backend
//!
//! ## Purpose
//! These tests validate that TupleSpace operations work correctly across multiple
//! "virtual nodes" (separate TupleSpace instances) connected to the same Redis backend.
//!
//! ## Test Scenarios
//! 1. Write on node1, read on node2 (shared database visibility)
//! 2. Atomic take() across nodes (no duplicate takes)
//! 3. Consistent count() results across nodes
//! 4. Distributed barrier synchronization pattern
//!
//! ## Requirements
//! - Redis running on localhost:6379
//! - Run with: `cargo test --features redis-backend -- redis_multinode --test-threads=1`
//!
//! ## Architecture Validated
//! ```
//! Node 1 (TupleSpace) ──┐
//!                        ├──→ Shared Redis @ localhost:6379
//! Node 2 (TupleSpace) ──┘
//! ```

#[cfg(feature = "redis-backend")]
mod redis_multinode_tests {
    use plexspaces_tuplespace::*;
    use plexspaces_proto::tuplespace::v1::{
        StorageProvider, TupleSpaceStorageConfig,
        tuple_space_storage_config, RedisStorageConfig,
    };
    use std::sync::Arc;
    use std::time::Duration;

    /// Create a TupleSpace instance connected to shared Redis
    async fn create_redis_tuplespace(node_id: &str) -> Arc<TupleSpace> {
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderRedis as i32,
            config: Some(tuple_space_storage_config::Config::Redis(
                RedisStorageConfig {
                    connection_string: "redis://localhost:6379".to_string(),
                    pool_size: 10,
                    key_prefix: format!("test-multinode-{}", node_id),
                    enable_pubsub: false,
                }
            )),
            enable_metrics: false,
            cleanup_interval: None,
        };

        let storage = storage::create_storage(config).await
            .expect("Failed to create Redis storage");

        Arc::new(TupleSpace::with_storage_and_tenant(storage, "test-tenant", "test-namespace"))
    }

    /// Helper: Check if Redis is available
    async fn is_redis_available() -> bool {
        let config = TupleSpaceStorageConfig {
            provider: StorageProvider::StorageProviderRedis as i32,
            config: Some(tuple_space_storage_config::Config::Redis(
                RedisStorageConfig {
                    connection_string: "redis://localhost:6379".to_string(),
                    pool_size: 1,
                    key_prefix: "test-health".to_string(),
                    enable_pubsub: false,
                }
            )),
            enable_metrics: false,
            cleanup_interval: None,
        };

        storage::create_storage(config).await.is_ok()
    }

    #[tokio::test]
    async fn test_redis_multinode_write_read() {
        // Skip if Redis not available
        if !is_redis_available().await {
            eprintln!("⚠️  Skipping test: Redis not available on localhost:6379");
            eprintln!("   Start Redis: docker run -p 6379:6379 redis:7");
            return;
        }

        println!("🧪 Test: Write on node1, read on node2");

        // Create 2 virtual nodes connected to same Redis
        let node1 = create_redis_tuplespace("node1").await;
        let node2 = create_redis_tuplespace("node2").await;

        // Node 1: Write tuple
        let tuple = Tuple::new(vec![
            TupleField::String("sensor".to_string()),
            TupleField::String("temp".to_string()),
            TupleField::Float(OrderedFloat::new(72.5)),
        ]);

        node1.write(tuple.clone()).await
            .expect("Node 1 write failed");

        println!("  ✅ Node 1 wrote tuple");

        // Small delay to ensure Redis propagation
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Node 2: Read tuple (should see it via shared Redis!)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("sensor".to_string())),
            PatternField::Exact(TupleField::String("temp".to_string())),
            PatternField::Wildcard,
        ]);

        let result = node2.read(pattern).await
            .expect("Node 2 read failed");

        assert!(result.is_some(), "Node 2 should see tuple from Node 1");
        let retrieved = result.unwrap();
        assert_eq!(retrieved.fields()[0], TupleField::String("sensor".to_string()));
        assert_eq!(retrieved.fields()[1], TupleField::String("temp".to_string()));

        println!("  ✅ Node 2 read tuple from Node 1 (shared Redis)");
        println!("✅ Test passed: Multi-node write/read works!\n");
    }

    #[tokio::test]
    async fn test_redis_multinode_atomic_take() {
        if !is_redis_available().await {
            eprintln!("⚠️  Skipping test: Redis not available");
            return;
        }

        println!("🧪 Test: Atomic take() across nodes");

        let node1 = create_redis_tuplespace("atomic-node1").await;
        let node2 = create_redis_tuplespace("atomic-node2").await;

        // Node 1: Write job tuple
        let job_tuple = Tuple::new(vec![
            TupleField::String("job".to_string()),
            TupleField::String("data".to_string()),
            TupleField::String("pending".to_string()),
        ]);

        node1.write(job_tuple).await.expect("Write failed");
        println!("  ✅ Node 1 wrote job tuple");

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Node 1: Take job (atomic remove)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("job".to_string())),
            PatternField::Wildcard,
            PatternField::Exact(TupleField::String("pending".to_string())),
        ]);

        let taken = node1.take(pattern.clone()).await
            .expect("Take failed");

        assert!(taken.is_some(), "Node 1 should take the job");
        println!("  ✅ Node 1 took job tuple (atomic remove)");

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Node 2: Try to read (should NOT find it - it was removed!)
        let result = node2.read(pattern).await
            .expect("Read failed");

        assert!(result.is_none(), "Node 2 should NOT see tuple after Node 1 took it");
        println!("  ✅ Node 2 confirms tuple removed (atomic operation worked)");
        println!("✅ Test passed: Atomic take() works across nodes!\n");
    }

    #[tokio::test]
    async fn test_redis_multinode_consistent_count() {
        if !is_redis_available().await {
            eprintln!("⚠️  Skipping test: Redis not available");
            return;
        }

        println!("🧪 Test: Consistent count() across nodes");

        let node1 = create_redis_tuplespace("count-node1").await;
        let node2 = create_redis_tuplespace("count-node2").await;
        let node3 = create_redis_tuplespace("count-node3").await;

        // Nodes write multiple tuples
        for i in 1..=5 {
            let tuple = Tuple::new(vec![
                TupleField::String("metric".to_string()),
                TupleField::String(format!("node{}", i)),
                TupleField::Integer(i),
            ]);

            match i {
                1 | 2 => node1.write(tuple).await.expect("Write failed"),
                3 | 4 => node2.write(tuple).await.expect("Write failed"),
                _ => node3.write(tuple).await.expect("Write failed"),
            }
        }

        println!("  ✅ 3 nodes wrote 5 tuples total");

        tokio::time::sleep(Duration::from_millis(200)).await;

        // All nodes count - should see same result!
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("metric".to_string())),
            PatternField::Wildcard,
            PatternField::Wildcard,
        ]);

        let count1 = node1.count(pattern.clone()).await
            .expect("Count failed on node1");
        let count2 = node2.count(pattern.clone()).await
            .expect("Count failed on node2");
        let count3 = node3.count(pattern).await
            .expect("Count failed on node3");

        assert_eq!(count1, 5, "Node 1 should count 5 tuples");
        assert_eq!(count2, 5, "Node 2 should count 5 tuples");
        assert_eq!(count3, 5, "Node 3 should count 5 tuples");

        println!("  ✅ All 3 nodes see consistent count: {}", count1);
        println!("✅ Test passed: Consistent count() across nodes!\n");
    }

    #[tokio::test]
    async fn test_redis_multinode_distributed_barrier() {
        if !is_redis_available().await {
            eprintln!("⚠️  Skipping test: Redis not available");
            return;
        }

        println!("🧪 Test: Distributed barrier pattern");

        let node1 = create_redis_tuplespace("barrier-node1").await;
        let node2 = create_redis_tuplespace("barrier-node2").await;
        let node3 = create_redis_tuplespace("barrier-node3").await;

        let barrier_name = "round-1";
        let expected_count = 3;

        // Simulate 3 actors arriving at barrier (each on different "node")
        let arrival1 = Tuple::new(vec![
            TupleField::String("barrier".to_string()),
            TupleField::String(barrier_name.to_string()),
            TupleField::String("actor-1".to_string()),
        ]);

        let arrival2 = Tuple::new(vec![
            TupleField::String("barrier".to_string()),
            TupleField::String(barrier_name.to_string()),
            TupleField::String("actor-2".to_string()),
        ]);

        let arrival3 = Tuple::new(vec![
            TupleField::String("barrier".to_string()),
            TupleField::String(barrier_name.to_string()),
            TupleField::String("actor-3".to_string()),
        ]);

        node1.write(arrival1).await.expect("Write failed");
        println!("  ✅ Node 1: actor-1 arrived at barrier");

        node2.write(arrival2).await.expect("Write failed");
        println!("  ✅ Node 2: actor-2 arrived at barrier");

        node3.write(arrival3).await.expect("Write failed");
        println!("  ✅ Node 3: actor-3 arrived at barrier");

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Any node can check barrier count (coordinator pattern)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("barrier".to_string())),
            PatternField::Exact(TupleField::String(barrier_name.to_string())),
            PatternField::Wildcard,
        ]);

        let count = node1.count(pattern).await
            .expect("Count failed");

        assert_eq!(count, expected_count, "All 3 actors should be at barrier");
        println!("  ✅ Barrier complete: {} arrivals counted", count);
        println!("✅ Test passed: Distributed barrier synchronization works!\n");
    }

    #[tokio::test]
    async fn test_redis_multinode_work_queue_pattern() {
        if !is_redis_available().await {
            eprintln!("⚠️  Skipping test: Redis not available");
            return;
        }

        println!("🧪 Test: Work queue pattern (producer-consumer)");

        let producer = create_redis_tuplespace("producer").await;
        let worker1 = create_redis_tuplespace("worker1").await;
        let worker2 = create_redis_tuplespace("worker2").await;

        // Producer: Write 5 job tuples
        for i in 1..=5 {
            let job = Tuple::new(vec![
                TupleField::String("job".to_string()),
                TupleField::Integer(i),
                TupleField::String("pending".to_string()),
            ]);

            producer.write(job).await.expect("Write failed");
        }

        println!("  ✅ Producer wrote 5 jobs");

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Workers compete to take jobs (atomic - no duplicates!)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("job".to_string())),
            PatternField::Wildcard,
            PatternField::Exact(TupleField::String("pending".to_string())),
        ]);

        let mut worker1_jobs = 0;
        let mut worker2_jobs = 0;

        // Each worker tries to take 3 jobs
        for _ in 0..3 {
            if let Ok(Some(_)) = worker1.take(pattern.clone()).await {
                worker1_jobs += 1;
            }
            if let Ok(Some(_)) = worker2.take(pattern.clone()).await {
                worker2_jobs += 1;
            }
        }

        println!("  ✅ Worker 1 took {} jobs", worker1_jobs);
        println!("  ✅ Worker 2 took {} jobs", worker2_jobs);

        let total_taken = worker1_jobs + worker2_jobs;
        assert_eq!(total_taken, 5, "All 5 jobs should be taken exactly once");

        // Verify no jobs left
        let remaining = producer.count(pattern).await.expect("Count failed");
        assert_eq!(remaining, 0, "No jobs should remain");

        println!("  ✅ All jobs processed, none duplicated");
        println!("✅ Test passed: Work queue pattern works!\n");
    }
}
