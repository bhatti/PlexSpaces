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

//! Byzantine Generals - Phase 2: Distributed Consensus Tests
//!
//! ## Purpose
//! Test Byzantine consensus using Redis-backed TupleSpace for distributed coordination.
//!
//! ## Phase 2 Goals
//! - Replace in-memory LatticeTupleSpace with Redis-backed RedisTupleSpace
//! - Enable multi-process consensus (generals on different nodes)
//! - Verify votes persist across process boundaries
//! - Test distributed pattern matching
//!
//! ## Test Strategy
//! 1. Start with single-process tests using RedisTupleSpace
//! 2. Verify vote persistence in Redis
//! 3. Test multiple generals sharing same Redis instance
//! 4. Future: Multi-process integration tests

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use plexspaces::journal::MemoryJournal;

#[cfg(feature = "redis-backend")]
use plexspaces::tuplespace::TupleSpace;

#[cfg(feature = "redis-backend")]
use plexspaces_proto::v1::tuplespace::{TupleSpaceConfig, RedisBackend};

/// Helper to create Redis-backed TupleSpace for tests
#[cfg(feature = "redis-backend")]
async fn create_redis_tuplespace(namespace: &str) -> Result<TupleSpace, Box<dyn std::error::Error>> {
    let config = TupleSpaceConfig {
        backend: Some(plexspaces_proto::v1::tuplespace::tuple_space_config::Backend::Redis(
            RedisBackend {
                url: "redis://127.0.0.1:6379".to_string(),
                namespace: namespace.to_string(),
            }
        )),
        pool_size: 5,
        default_ttl_seconds: 0,
        enable_indexing: false,
    };
    TupleSpace::from_config(config).await.map_err(|e| e.into())
}

// ============================================================================
// PHASE 2: DISTRIBUTED CONSENSUS WITH REDIS
// ============================================================================

/// TEST 1: Single process, Redis TupleSpace - 3 generals agree on Attack
///
/// ## Scenario
/// - Use RedisTupleSpace instead of LatticeTupleSpace
/// - 3 honest generals coordinate via Redis
/// - All should agree on Attack
///
/// ## What This Tests
/// - RedisTupleSpace write() operation
/// - RedisTupleSpace read() with pattern matching
/// - Vote persistence in Redis
/// - Consensus with distributed storage
#[tokio::test]
#[cfg(feature = "redis-backend")]
#[ignore] // Requires Redis server running on localhost:6379
async fn test_redis_3_generals_agree_on_attack() {
    use byzantine_generals::{General, Decision};

    // Connect to Redis (requires Redis server running)
    let namespace = format!("byzantine-test-3-generals-{}", ulid::Ulid::new());

    let tuplespace = Arc::new(
        create_redis_tuplespace(&namespace)
            .await
            .expect("Failed to connect to Redis - ensure Redis is running on localhost:6379")
    );
    let journal = Arc::new(MemoryJournal::new());

    // Spawn 3 generals
    let commander = General::new(
        "commander".to_string(),
        true,
        false,
        journal.clone(),
        tuplespace.clone(),
    );

    let lieutenant1 = General::new(
        "lieutenant_1".to_string(),
        false,
        false,
        journal.clone(),
        tuplespace.clone(),
    );

    let lieutenant2 = General::new(
        "lieutenant_2".to_string(),
        false,
        false,
        journal.clone(),
        tuplespace.clone(),
    );

    // Commander proposes Attack
    commander.propose(true).await.expect("Commander proposal failed");
    sleep(Duration::from_millis(100)).await;

    // All cast votes
    commander.cast_vote(0, Decision::Attack, vec!["commander".to_string()])
        .await.expect("Vote failed");
    lieutenant1.cast_vote(0, Decision::Attack, vec!["lieutenant_1".to_string()])
        .await.expect("Vote failed");
    lieutenant2.cast_vote(0, Decision::Attack, vec!["lieutenant_2".to_string()])
        .await.expect("Vote failed");

    sleep(Duration::from_millis(200)).await;

    // Verify votes are in Redis
    let votes = commander.read_votes(0).await.expect("Read votes failed");
    assert!(votes.len() >= 3, "Should have at least 3 votes in Redis, got {}", votes.len());

    // All decide
    let commander_decision = commander.decide().await.expect("Decision failed");
    let lt1_decision = lieutenant1.decide().await.expect("Decision failed");
    let lt2_decision = lieutenant2.decide().await.expect("Decision failed");

    assert_eq!(commander_decision, Decision::Attack);
    assert_eq!(lt1_decision, Decision::Attack);
    assert_eq!(lt2_decision, Decision::Attack);
}

/// TEST 2: Redis persistence - votes survive across general restarts
///
/// ## Scenario
/// - General 1 votes, then "crashes" (dropped)
/// - General 2 created with same ID, reads votes from Redis
/// - Should see General 1's vote still in Redis
///
/// ## What This Tests
/// - Vote persistence in Redis
/// - Simulated crash recovery
/// - Redis as durable storage
#[tokio::test]
#[cfg(feature = "redis-backend")]
#[ignore] // Requires Redis server
async fn test_redis_vote_persistence_across_restart() {
    use byzantine_generals::{General, Decision};

    // Use unique namespace with timestamp to avoid collision with previous runs
    let namespace = format!("byzantine-test-persistence-{}", ulid::Ulid::new());

    // Clear any previous test data
    let redis_url = "redis://127.0.0.1:6379";
    let client = redis::Client::open(redis_url).expect("Redis client failed");
    let mut conn = client.get_multiplexed_async_connection().await.expect("Redis connection failed");
    let _: () = redis::cmd("DEL")
        .arg(format!("{}:*", namespace))
        .query_async(&mut conn)
        .await
        .unwrap_or(());

    let tuplespace = Arc::new(
        create_redis_tuplespace(&namespace)
            .await
            .expect("Failed to connect to Redis")
    );
    let journal = Arc::new(MemoryJournal::new());

    // General 1 votes
    {
        let general1 = General::new(
            "general".to_string(),
            false,
            false,
            journal.clone(),
            tuplespace.clone(),
        );

        general1.cast_vote(0, Decision::Attack, vec!["general".to_string()])
            .await.expect("Vote failed");

        sleep(Duration::from_millis(100)).await;

        // Verify vote is in Redis
        let votes = general1.read_votes(0).await.expect("Read failed");
        assert_eq!(votes.len(), 1, "Should have 1 vote");
    } // general1 dropped (simulates crash)

    sleep(Duration::from_millis(50)).await;

    // General 2 (restarted) reads votes - should see general1's vote
    {
        let general2 = General::new(
            "general_restarted".to_string(),
            false,
            false,
            journal.clone(),
            tuplespace.clone(),
        );

        let votes = general2.read_votes(0).await.expect("Read failed");
        assert!(votes.len() >= 1, "Should see persisted vote after restart");
        assert_eq!(votes[0].value, Decision::Attack, "Vote value should persist");
    }
}

/// TEST 3: Redis TupleSpace - 7 generals with mixed votes
///
/// ## Scenario
/// - 7 generals using Redis TupleSpace
/// - 5 vote Attack, 2 vote Retreat
/// - All should decide Attack (majority)
///
/// ## What This Tests
/// - Redis TupleSpace scales to multiple generals
/// - Pattern matching works correctly in Redis
/// - Majority voting with distributed storage
#[tokio::test]
#[cfg(feature = "redis-backend")]
#[ignore] // Requires Redis server
async fn test_redis_7_generals_mixed_votes() {
    use byzantine_generals::{General, Decision};

    let namespace = format!("byzantine-test-7-generals-{}", ulid::Ulid::new());

    let tuplespace = Arc::new(
        create_redis_tuplespace(&namespace)
            .await
            .expect("Failed to connect to Redis")
    );
    let journal = Arc::new(MemoryJournal::new());

    // Spawn 7 generals
    let generals: Vec<General> = (0..7)
        .map(|i| {
            General::new(
                format!("general_{}", i),
                i == 0,
                false,
                journal.clone(),
                tuplespace.clone(),
            )
        })
        .collect();

    // Commander proposes
    generals[0].propose(true).await.expect("Proposal failed");
    sleep(Duration::from_millis(100)).await;

    // 5 vote Attack, 2 vote Retreat
    for i in 0..5 {
        generals[i]
            .cast_vote(0, Decision::Attack, vec![format!("general_{}", i)])
            .await
            .expect("Vote failed");
    }
    for i in 5..7 {
        generals[i]
            .cast_vote(0, Decision::Retreat, vec![format!("general_{}", i)])
            .await
            .expect("Vote failed");
    }

    sleep(Duration::from_millis(300)).await;

    // Verify all votes are in Redis
    let votes = generals[0].read_votes(0).await.expect("Read failed");
    assert!(votes.len() >= 7, "Should have all 7 votes in Redis, got {}", votes.len());

    // All decide - majority should win
    for (i, general) in generals.iter().enumerate() {
        let decision = general.decide().await.expect("Decision failed");
        assert_eq!(
            decision,
            Decision::Attack,
            "General {} should decide Attack (majority)", i
        );
    }
}

/// TEST 4: Redis TTL - votes with lease expire automatically
///
/// ## Scenario
/// - Cast vote with short TTL (200ms)
/// - Verify vote exists immediately
/// - Wait for TTL expiry
/// - Verify vote is gone
///
/// ## What This Tests
/// - Redis TTL integration
/// - Lease expiry via Redis EXPIRE
/// - Automatic cleanup
#[tokio::test]
#[cfg(feature = "redis-backend")]
#[ignore] // Requires Redis server
async fn test_redis_vote_ttl_expiry() {
    use byzantine_generals::{General, Decision};
    use plexspaces::tuplespace::{Tuple, TupleField, Lease};
    use chrono::Duration as ChronoDuration;

    let namespace = format!("byzantine-test-ttl-{}", ulid::Ulid::new());

    let tuplespace = Arc::new(
        create_redis_tuplespace(&namespace)
            .await
            .expect("Failed to connect to Redis")
    );

    // Write a vote tuple with 2 second lease (long enough to verify, short enough to test expiry)
    let lease = Lease::new(ChronoDuration::seconds(2));
    let tuple = Tuple::new(vec![
        TupleField::String("vote".to_string()),
        TupleField::String("test_general".to_string()),
        TupleField::Integer(0),
        TupleField::String("Attack".to_string()),
        TupleField::String("test_general".to_string()),
    ]).with_lease(lease);

    tuplespace.write(tuple.clone()).await.expect("Write failed");

    // Immediately verify vote exists
    sleep(Duration::from_millis(100)).await;
    let pattern = plexspaces::tuplespace::Pattern::new(vec![
        plexspaces::tuplespace::PatternField::Exact(TupleField::String("vote".to_string())),
        plexspaces::tuplespace::PatternField::Wildcard,
        plexspaces::tuplespace::PatternField::Exact(TupleField::Integer(0)),
        plexspaces::tuplespace::PatternField::Wildcard,
        plexspaces::tuplespace::PatternField::Wildcard,
    ]);

    let votes = tuplespace.read_all(pattern.clone()).await.expect("Read failed");
    assert_eq!(votes.len(), 1, "Vote should exist immediately");

    // Wait for TTL expiry (2 seconds + 500ms buffer)
    sleep(Duration::from_millis(2500)).await;

    // Vote should be expired and removed by Redis
    let votes_after = tuplespace.read_all(pattern).await.expect("Read failed");
    assert_eq!(votes_after.len(), 0, "Vote should be expired and removed by Redis TTL");
}

/// TEST 5: Redis concurrent access - multiple generals write simultaneously
///
/// ## Scenario
/// - 10 generals all vote at the same time
/// - All votes should be stored correctly
/// - No race conditions or lost votes
///
/// ## What This Tests
/// - Redis handles concurrent writes
/// - No vote conflicts or overwrites
/// - Atomicity of Redis operations
#[tokio::test]
#[cfg(feature = "redis-backend")]
#[ignore] // Requires Redis server
async fn test_redis_concurrent_voting() {
    use byzantine_generals::{General, Decision};

    // Use unique namespace to avoid collision with previous runs
    let namespace = format!("byzantine-test-concurrent-{}", ulid::Ulid::new());

    // Clear any previous test data
    let redis_url = "redis://127.0.0.1:6379";
    let client = redis::Client::open(redis_url).expect("Redis client failed");
    let mut conn = client.get_multiplexed_async_connection().await.expect("Redis connection failed");
    let _: () = redis::cmd("DEL")
        .arg(format!("{}:*", namespace))
        .query_async(&mut conn)
        .await
        .unwrap_or(());

    let tuplespace = Arc::new(
        create_redis_tuplespace(&namespace)
            .await
            .expect("Failed to connect to Redis")
    );
    let journal = Arc::new(MemoryJournal::new());

    // Spawn 10 generals
    let generals: Vec<General> = (0..10)
        .map(|i| {
            General::new(
                format!("general_{}", i),
                false,
                false,
                journal.clone(),
                tuplespace.clone(),
            )
        })
        .collect();

    // All vote concurrently
    let mut tasks = vec![];
    for i in 0..10 {
        let g = generals[i].clone();
        let task = tokio::spawn(async move {
            g.cast_vote(0, Decision::Attack, vec![format!("general_{}", i)])
                .await
                .expect("Vote failed");
        });
        tasks.push(task);
    }

    // Wait for all concurrent votes
    for task in tasks {
        task.await.expect("Task failed");
    }

    sleep(Duration::from_millis(200)).await;

    // Verify all 10 votes are in Redis
    let votes = generals[0].read_votes(0).await.expect("Read failed");
    assert_eq!(votes.len(), 10, "All 10 concurrent votes should be stored");
}

// ============================================================================
// HELPER FUNCTIONS
// ============================================================================

/// Helper to check if Redis is available
#[cfg(feature = "redis-backend")]
async fn redis_available() -> bool {
    use redis::Client;

    match Client::open("redis://127.0.0.1:6379") {
        Ok(client) => {
            match client.get_connection() {
                Ok(_) => true,
                Err(_) => false,
            }
        }
        Err(_) => false,
    }
}
