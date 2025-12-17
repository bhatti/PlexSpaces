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

//! Distributed Barrier Integration Test - Multi-Round Coordination
//!
//! ## Purpose
//! Demonstrates LOCAL TupleSpace barrier pattern using primitives (write + count + watch).
//! Simulates a simplified consensus protocol with multiple synchronization points.
//!
//! ## Architecture Change (2025-01-16)
//! **BEFORE**: Used gRPC Barrier RPC (Actor → gRPC Service → TupleSpace)
//! **AFTER**: Uses LOCAL TupleSpace primitives (Actor → TupleSpace directly)
//!
//! **Rationale**: Industry standard (ZooKeeper, etcd, Redis) uses primitives, not high-level RPCs.
//! See docs/BARRIER_ARCHITECTURE_SUMMARY.md for details.
//!
//! ## Scenario
//! 4 actors coordinate through 3 rounds:
//! - Round 1: Initialization (write initial state)
//! - BARRIER 1: All actors wait (using TupleSpace.watch() + count())
//! - Round 2: Exchange data
//! - BARRIER 2: All actors wait (using TupleSpace.watch() + count())
//! - Round 3: Finalization
//! - BARRIER 3: All actors wait (using TupleSpace.watch() + count())
//!
//! This pattern is common in:
//! - Byzantine Generals (multi-round voting)
//! - Jacobi iteration (grid cell updates)
//! - MapReduce (shuffle barriers)
//! - Distributed training (gradient synchronization)

use plexspaces_node::{Node, NodeId, default_node_config};
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};

/// Barrier synchronization using LOCAL TupleSpace primitives
///
/// ## Algorithm
/// 1. Write arrival tuple: ("barrier", barrier_name, actor_id)
/// 2. Watch for pattern changes: watch(("barrier", barrier_name, _))
/// 3. Poll count until expected_count reached or timeout
///
/// ## Why LOCAL?
/// - No gRPC overhead (user insight: "As match should be local, we shouldn't need grpc")
/// - Uses Tokio channels for notifications (already implemented in TupleSpace)
/// - Aligns with industry practices (ZooKeeper watch(), etcd WATCH, Redis SUBSCRIBE)
async fn barrier_sync(
    space: &Arc<plexspaces_tuplespace::TupleSpace>,
    barrier_name: &str,
    actor_id: &str,
    expected_count: usize,
    timeout: StdDuration,
) -> Result<(), String> {
    // 1. Write arrival tuple
    let arrival_tuple = Tuple::new(vec![
        TupleField::String("barrier".to_string()),
        TupleField::String(barrier_name.to_string()),
        TupleField::String(actor_id.to_string()),
    ]);
    space
        .write(arrival_tuple)
        .await
        .map_err(|e| format!("Failed to write barrier tuple: {}", e))?;

    // 2. Create pattern to watch: ("barrier", barrier_name, _)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_name.to_string())),
        PatternField::Wildcard, // Any actor_id
    ]);

    // 3. Watch for changes
    let mut watcher = space.watch(pattern.clone()).await;

    // 4. Poll until count >= expected or timeout
    let deadline = Instant::now() + timeout;
    loop {
        let count = space
            .count(pattern.clone())
            .await
            .map_err(|e| format!("Failed to count barrier tuples: {}", e))?;

        if count >= expected_count {
            return Ok(()); // Success! All actors arrived
        }

        // Wait for next event or timeout
        tokio::select! {
            Some(_event) = watcher.recv() => {
                // New tuple added, check count again
                continue;
            }
            _ = tokio::time::sleep_until(deadline.into()) => {
                return Err(format!(
                    "Barrier '{}' timeout: {}/{} actors arrived",
                    barrier_name, count, expected_count
                ));
            }
        }
    }
}

/// Test multi-round distributed algorithm with 3 barriers using LOCAL TupleSpace
#[tokio::test]
async fn test_multi_round_barrier_coordination() {
    // Setup - Create node with LOCAL TupleSpace
    let node = Arc::new(Node::new(
        NodeId::new("test-node-multi-round"),
        default_node_config(),
    ));

    let tuplespace = node.tuplespace();
    let num_actors = 4;
    let timeout = StdDuration::from_secs(30);

    let mut handles = vec![];

    // Spawn 4 actors executing a 3-round protocol
    for i in 0..num_actors {
        let tuplespace = tuplespace.clone();
        let actor_id = format!("actor-{}", i);

        let handle = tokio::spawn(async move {
            // Round 1: Initialization
            let init_tuple = Tuple::new(vec![
                TupleField::String("state".to_string()),
                TupleField::String("round-1".to_string()),
                TupleField::String(actor_id.clone()),
                TupleField::Integer(i as i64 * 10),
            ]);
            tuplespace
                .write(init_tuple)
                .await
                .expect(&format!("[{}] Round 1 write failed", actor_id));

            // BARRIER 1: Wait for all actors to complete Round 1 (LOCAL)
            barrier_sync(
                &tuplespace,
                "round-1-complete",
                &actor_id,
                num_actors,
                timeout,
            )
            .await
            .expect(&format!("[{}] Barrier 1 failed", actor_id));

            // Round 2: Exchange data (read from TupleSpace)
            for j in 0..num_actors {
                if j != i {
                    let read_pattern = Pattern::new(vec![
                        PatternField::Exact(TupleField::String("state".to_string())),
                        PatternField::Exact(TupleField::String("round-1".to_string())),
                        PatternField::Exact(TupleField::String(format!("actor-{}", j))),
                        PatternField::Wildcard,
                    ]);
                    tuplespace.read(read_pattern).await.expect(&format!(
                        "[{}] Round 2 read from actor-{} failed",
                        actor_id, j
                    ));
                }
            }

            // Write Round 2 result
            let round2_tuple = Tuple::new(vec![
                TupleField::String("result".to_string()),
                TupleField::String("round-2".to_string()),
                TupleField::String(actor_id.clone()),
                TupleField::Integer((i as i64 * 10) + 5),
            ]);
            tuplespace
                .write(round2_tuple)
                .await
                .expect(&format!("[{}] Round 2 write failed", actor_id));

            // BARRIER 2: Wait for all actors to complete Round 2 (LOCAL)
            barrier_sync(
                &tuplespace,
                "round-2-complete",
                &actor_id,
                num_actors,
                timeout,
            )
            .await
            .expect(&format!("[{}] Barrier 2 failed", actor_id));

            // Round 3: Finalization (aggregate results)
            let mut sum = 0i64;
            for j in 0..num_actors {
                let read_pattern = Pattern::new(vec![
                    PatternField::Exact(TupleField::String("result".to_string())),
                    PatternField::Exact(TupleField::String("round-2".to_string())),
                    PatternField::Exact(TupleField::String(format!("actor-{}", j))),
                    PatternField::Wildcard,
                ]);
                let tuple = tuplespace
                    .read(read_pattern)
                    .await
                    .expect(&format!(
                        "[{}] Round 3 read from actor-{} failed",
                        actor_id, j
                    ))
                    .expect(&format!("[{}] No tuple found for actor-{}", actor_id, j));

                // Extract value from fourth field
                if let Some(TupleField::Integer(val)) = tuple.fields().get(3) {
                    sum += val;
                }
            }

            // Write final result
            let final_tuple = Tuple::new(vec![
                TupleField::String("final".to_string()),
                TupleField::String(actor_id.clone()),
                TupleField::Integer(sum),
            ]);
            tuplespace
                .write(final_tuple)
                .await
                .expect(&format!("[{}] Final write failed", actor_id));

            // BARRIER 3: Wait for all actors to complete Round 3 (LOCAL)
            barrier_sync(
                &tuplespace,
                "round-3-complete",
                &actor_id,
                num_actors,
                timeout,
            )
            .await
            .expect(&format!("[{}] Barrier 3 failed", actor_id));

            (i, sum)
        });

        handles.push(handle);
    }

    // Wait for all actors
    let results = futures::future::join_all(handles).await;

    // Verify all actors completed successfully
    for (i, join_result) in results.iter().enumerate() {
        let (actor_id, sum) = join_result.as_ref().unwrap();
        assert_eq!(*actor_id, i, "Actor ID mismatch");

        // Expected sum: (0*10+5) + (1*10+5) + (2*10+5) + (3*10+5) = 5 + 15 + 25 + 35 = 80
        assert_eq!(*sum, 80, "Actor {} computed wrong sum", i);
    }

    println!("✅ Multi-round barrier coordination successful!");
    println!("✅ All 4 actors completed 3 rounds with 3 barriers");
    println!("✅ Final aggregated sum: 80 (verified across all actors)");
    println!("✅ Using LOCAL TupleSpace primitives (watch + count), not gRPC RPC");
}
