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

//! Byzantine Generals Consensus with LOCAL TupleSpace Barriers
//!
//! ## Purpose
//! Demonstrates using **LOCAL TupleSpace primitives** for distributed consensus coordination.
//! This is the canonical example showing how barriers enable multi-round distributed algorithms.
//!
//! ## Architecture Change (2025-01-16)
//! **BEFORE**: Used gRPC Barrier RPC (Actor → gRPC Service → TupleSpace)
//! **AFTER**: Uses LOCAL TupleSpace primitives (Actor → TupleSpace directly)
//!
//! **Rationale**: Industry standard (ZooKeeper, etcd, Redis) uses primitives, not high-level RPCs.
//! See docs/BARRIER_ARCHITECTURE_SUMMARY.md for details.
//!
//! ## Why This Test Matters
//! - **Real Distributed Use Case**: Byzantine Generals requires multiple voting rounds with synchronization
//! - **Barrier Between Rounds**: All generals must finish voting before moving to next round
//! - **LOCAL TupleSpace Primitives**: Uses write() + watch() + count() pattern
//! - **Validates Walking Skeleton**: Tests Phase 3 (TupleSpace Distributed Coordination) completeness
//!
//! ## Test Scenario
//! ```text
//! Round 1: Initial Vote
//! ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐
//! │General 0│  │General 1│  │General 2│  │General 3│
//! │(Loyal)  │  │(Traitor)│  │(Loyal)  │  │(Loyal)  │
//! └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘
//!      │            │            │            │
//!      ├─Vote:ATK───┤─Vote:RTR───┤─Vote:ATK───┤─Vote:ATK──┐
//!      │            │            │            │           │
//!      └────────────┴────────────┴────────────┴───────────┘
//!                            │
//!                   ┌────────▼────────┐
//!                   │ BARRIER ("r1")  │  ← All 4 generals wait (LOCAL)
//!                   │ expected: 4     │     watch() + count()
//!                   └────────┬────────┘
//!                            │ (all arrived)
//!                            ▼
//! Round 2: Exchange Votes
//! ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐
//! │General 0│  │General 1│  │General 2│  │General 3│
//! └────┬────┘  └────┬────┘  └────┬────┘  └────┬────┘
//!      │            │            │            │
//!      ├──Exchange votes with all other generals───┤
//!      │            │            │            │
//!      └────────────┴────────────┴────────────┴───────────┘
//!                            │
//!                   ┌────────▼────────┐
//!                   │ BARRIER ("r2")  │  ← All 4 generals wait (LOCAL)
//!                   │ expected: 4     │     watch() + count()
//!                   └────────┬────────┘
//!                            │ (all arrived)
//!                            ▼
//! Final Decision
//! ┌─────────┐  ┌─────────┐  ┌─────────┐  ┌─────────┐
//! │ATTACK   │  │RETREAT  │  │ATTACK   │  │ATTACK   │
//! │(Loyal)  │  │(Traitor)│  │(Loyal)  │  │(Loyal)  │
//! └─────────┘  └─────────┘  └─────────┘  └─────────┘
//!
//! Result: 3/4 agree on ATTACK → Consensus reached despite traitor
//! ```
//!
//! ## What This Tests
//! - ✅ LOCAL TupleSpace barrier pattern (write + watch + count)
//! - ✅ Multi-round synchronization (2 rounds with 2 barriers)
//! - ✅ Byzantine fault tolerance (1 traitor out of 4 generals)
//! - ✅ Distributed consensus despite malicious actor
//! - ✅ Production-ready barrier pattern

use std::sync::Arc;
use std::time::{Duration as StdDuration, Instant};
use plexspaces_node::{Node, NodeId, NodeConfig};
use plexspaces_tuplespace::{Tuple, TupleField, Pattern, PatternField};
use tokio::task::JoinSet;

/// Byzantine General representation
#[derive(Clone)]
struct General {
    id: usize,
    is_traitor: bool,
    initial_vote: Vote,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Vote {
    Attack,
    Retreat,
}

impl General {
    fn new(id: usize, is_traitor: bool, initial_vote: Vote) -> Self {
        Self { id, is_traitor, initial_vote }
    }

    fn actor_id(&self) -> String {
        format!("general-{}", self.id)
    }

    /// Get vote (traitor may lie)
    fn get_vote(&self, asking_general: usize) -> Vote {
        if self.is_traitor {
            // Traitor sends different votes to different generals to cause confusion
            if asking_general % 2 == 0 {
                Vote::Attack
            } else {
                Vote::Retreat
            }
        } else {
            self.initial_vote
        }
    }
}

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
    space.write(arrival_tuple).await
        .map_err(|e| format!("Failed to write barrier tuple: {}", e))?;

    // 2. Create pattern to watch: ("barrier", barrier_name, _)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_name.to_string())),
        PatternField::Wildcard,  // Any actor_id
    ]);

    // 3. Watch for changes
    let mut watcher = space.watch(pattern.clone()).await;

    // 4. Poll until count >= expected or timeout
    let deadline = Instant::now() + timeout;
    loop {
        let count = space.count(pattern.clone()).await
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

/// Test Byzantine Generals consensus with LOCAL TupleSpace barriers
#[tokio::test]
async fn test_byzantine_consensus_with_grpc_barriers() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("byzantine=debug")
        .try_init();

    // Setup: 4 generals, 1 traitor (general-1)
    let generals = vec![
        General::new(0, false, Vote::Attack),   // Loyal
        General::new(1, true, Vote::Retreat),   // TRAITOR
        General::new(2, false, Vote::Attack),   // Loyal
        General::new(3, false, Vote::Attack),   // Loyal
    ];

    // Create node with LOCAL TupleSpace
    let node = Arc::new(Node::new(
        NodeId::new("byzantine-test-node"),
        NodeConfig::default(),
    ));

    let tuplespace = node.tuplespace();
    let num_generals = generals.len();
    let timeout = StdDuration::from_secs(30);

    // Spawn general tasks
    let mut join_set = JoinSet::new();

    for general in generals.clone() {
        let tuplespace = tuplespace.clone();
        let generals = generals.clone();

        join_set.spawn(async move {
            let actor_id = general.actor_id();

            println!("[{}] Starting consensus (traitor: {})", actor_id, general.is_traitor);

            // Round 1: Broadcast initial vote via LOCAL TupleSpace
            println!("[{}] Round 1: Broadcasting vote {:?}", actor_id, general.initial_vote);

            for other_id in 0..generals.len() {
                if other_id != general.id {
                    let vote = general.get_vote(other_id);
                    let vote_str = format!("{:?}", vote);

                    // Write vote tuple: ("vote", "r1", from_id, to_id, vote)
                    let vote_tuple = Tuple::new(vec![
                        TupleField::String("vote".to_string()),
                        TupleField::String("r1".to_string()),
                        TupleField::Integer(general.id as i64),
                        TupleField::Integer(other_id as i64),
                        TupleField::String(vote_str),
                    ]);

                    tuplespace.write(vote_tuple).await.expect("Vote write failed");
                }
            }

            // BARRIER 1: Wait for all generals to finish Round 1 voting (LOCAL)
            println!("[{}] Waiting at barrier 'round-1'", actor_id);
            barrier_sync(&tuplespace, "round-1", &actor_id, num_generals, timeout).await
                .expect("Barrier 1 failed");
            println!("[{}] Passed barrier 'round-1' ({} generals arrived)", actor_id, num_generals);

            // Round 2: Collect votes from other generals
            println!("[{}] Round 2: Collecting votes from others", actor_id);
            let mut votes_received = Vec::new();

            for other_id in 0..generals.len() {
                if other_id != general.id {
                    // Read vote from other general using LOCAL TupleSpace
                    let read_pattern = Pattern::new(vec![
                        PatternField::Exact(TupleField::String("vote".to_string())),
                        PatternField::Exact(TupleField::String("r1".to_string())),
                        PatternField::Exact(TupleField::Integer(other_id as i64)),
                        PatternField::Exact(TupleField::Integer(general.id as i64)),
                        PatternField::Wildcard,
                    ]);

                    let tuple = tuplespace.read(read_pattern).await
                        .expect("Read vote failed")
                        .expect(&format!("No vote found from general-{}", other_id));

                    // Extract vote from fifth field
                    if let Some(TupleField::String(vote_str)) = tuple.fields().get(4) {
                        let vote = match vote_str.as_str() {
                            "Attack" => Vote::Attack,
                            "Retreat" => Vote::Retreat,
                            _ => panic!("Invalid vote"),
                        };
                        votes_received.push(vote);
                        println!("[{}] Received vote from general-{}: {:?}",
                            actor_id, other_id, vote);
                    }
                }
            }

            // BARRIER 2: Wait for all generals to finish Round 2 (vote collection) (LOCAL)
            println!("[{}] Waiting at barrier 'round-2'", actor_id);
            barrier_sync(&tuplespace, "round-2", &actor_id, num_generals, timeout).await
                .expect("Barrier 2 failed");
            println!("[{}] Passed barrier 'round-2' ({} generals arrived)", actor_id, num_generals);

            // Final decision: Majority vote (including own vote)
            votes_received.push(general.initial_vote);
            let attack_count = votes_received.iter().filter(|&&v| v == Vote::Attack).count();
            let retreat_count = votes_received.iter().filter(|&&v| v == Vote::Retreat).count();

            let final_decision = if attack_count > retreat_count {
                Vote::Attack
            } else {
                Vote::Retreat
            };

            println!("[{}] Final decision: {:?} (Attack: {}, Retreat: {})",
                actor_id, final_decision, attack_count, retreat_count);

            (general.id, general.is_traitor, final_decision)
        });
    }

    // Wait for all generals to complete
    let mut results = Vec::new();
    while let Some(result) = join_set.join_next().await {
        results.push(result.unwrap());
    }

    // Verify consensus
    results.sort_by_key(|(id, _, _)| *id);

    println!("\n=== Final Results ===");
    let mut loyal_attack = 0;
    let mut loyal_retreat = 0;

    for (id, is_traitor, decision) in &results {
        let status = if *is_traitor { "TRAITOR" } else { "Loyal" };
        println!("General {}: {} → {:?}", id, status, decision);

        if !is_traitor {
            match decision {
                Vote::Attack => loyal_attack += 1,
                Vote::Retreat => loyal_retreat += 1,
            }
        }
    }

    // Assert: Loyal generals reach consensus (all should decide ATTACK)
    assert_eq!(loyal_attack, 3, "All 3 loyal generals should decide ATTACK");
    assert_eq!(loyal_retreat, 0, "No loyal generals should decide RETREAT");

    // Note: Traitor's decision doesn't matter - loyal generals still reach consensus
    println!("\n✅ Consensus reached: 3/3 loyal generals decided ATTACK despite 1 traitor");
    println!("✅ LOCAL TupleSpace barriers (watch + count) successfully coordinated 2 rounds");
}
