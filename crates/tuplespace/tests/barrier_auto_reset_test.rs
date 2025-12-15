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

//! Barrier Auto-Reset Tests (TDD)
//!
//! ## Purpose
//! Test that barriers automatically reset after all participants arrive,
//! allowing for repeated synchronization (like MPI_Barrier).
//!
//! ## Design Decision (from user requirement)
//! User specified: "barrier should auto-reset for all"
//!
//! This means:
//! - After N participants arrive, barrier triggers
//! - Barrier immediately resets to 0 waiters
//! - Same barrier can be used again for next round
//!
//! ## Use Case
//! Iterative algorithms like Jacobi iteration, where actors need to
//! synchronize at the end of each iteration:
//! ```
//! for iteration in 0..max_iterations {
//!     // Do work
//!     compute_jacobi_update();
//!
//!     // Synchronize with all actors
//!     barrier.wait("jacobi-iteration", count).await;
//!
//!     // Next iteration starts with reset barrier
//! }
//! ```

use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField, TupleSpace};
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// TDD RED: Test that barrier auto-resets after all participants arrive
///
/// ## Test Flow
/// 1. Create barrier for 3 participants
/// 2. First round: 3 actors arrive, barrier triggers
/// 3. Second round: Same 3 actors arrive again, barrier should trigger again
/// 4. Verify both rounds complete successfully
#[tokio::test]
async fn test_barrier_auto_reset_single_space() {
    let space = Arc::new(TupleSpace::new());

    // Barrier configuration
    let barrier_name = "iteration-barrier".to_string();
    let participant_count = 3;

    // Pattern: ("barrier", "iteration-barrier", participant_id)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_name.clone())),
        PatternField::Wildcard,
    ]);

    // === ITERATION 1 ===
    println!("=== Iteration 1 ===");

    // Spawn 3 actors that wait at barrier
    let mut iteration1_handles = Vec::new();
    for i in 0..participant_count {
        let space_clone = space.clone();
        let barrier_name_clone = barrier_name.clone();
        let pattern_clone = pattern.clone();

        let handle = tokio::spawn(async move {
            println!("  Actor {} waiting at iteration 1 barrier", i);

            // Register with barrier
            let mut rx = space_clone
                .barrier(
                    barrier_name_clone.clone(),
                    pattern_clone.clone(),
                    participant_count,
                )
                .await;

            // Write arrival tuple
            space_clone
                .write(Tuple::new(vec![
                    TupleField::String("barrier".to_string()),
                    TupleField::String(barrier_name_clone),
                    TupleField::Integer(i as i64),
                ]))
                .await
                .expect("Failed to write arrival tuple");

            // Wait for barrier to trigger
            let result = timeout(Duration::from_secs(5), rx.recv()).await;
            assert!(
                result.is_ok(),
                "Actor {} timed out waiting for iteration 1 barrier",
                i
            );

            println!("  Actor {} passed iteration 1 barrier", i);
        });

        iteration1_handles.push(handle);
    }

    // Wait for all iteration 1 participants to complete
    for handle in iteration1_handles {
        handle.await.expect("Actor panicked in iteration 1");
    }

    println!("✓ Iteration 1 barrier completed\n");

    // Clear tuples for next iteration (simulate cleanup)
    space.clear().await;

    // === ITERATION 2 ===
    println!("=== Iteration 2 (Testing Auto-Reset) ===");

    // Spawn same 3 actors for iteration 2
    // If barrier auto-resets, this should work exactly like iteration 1
    let mut iteration2_handles = Vec::new();
    for i in 0..participant_count {
        let space_clone = space.clone();
        let barrier_name_clone = barrier_name.clone();
        let pattern_clone = pattern.clone();

        let handle = tokio::spawn(async move {
            println!("  Actor {} waiting at iteration 2 barrier", i);

            // Register with barrier (should work if barrier reset)
            let mut rx = space_clone
                .barrier(
                    barrier_name_clone.clone(),
                    pattern_clone.clone(),
                    participant_count,
                )
                .await;

            // Write arrival tuple
            space_clone
                .write(Tuple::new(vec![
                    TupleField::String("barrier".to_string()),
                    TupleField::String(barrier_name_clone),
                    TupleField::Integer(i as i64),
                ]))
                .await
                .expect("Failed to write arrival tuple");

            // Wait for barrier to trigger
            let result = timeout(Duration::from_secs(5), rx.recv()).await;
            assert!(
                result.is_ok(),
                "Actor {} timed out waiting for iteration 2 barrier",
                i
            );

            println!("  Actor {} passed iteration 2 barrier", i);
        });

        iteration2_handles.push(handle);
    }

    // Wait for all iteration 2 participants to complete
    for handle in iteration2_handles {
        handle.await.expect("Actor panicked in iteration 2");
    }

    println!("✓ Iteration 2 barrier completed");
    println!("✓ Barrier auto-reset successful!");
}

/// TDD RED: Test barrier with different participant counts per iteration
///
/// ## Scenario
/// - Iteration 1: 3 participants
/// - Iteration 2: 5 participants (scaled up)
/// - Barrier should auto-reset and work with new count
#[tokio::test]
async fn test_barrier_auto_reset_with_different_counts() {
    let space = Arc::new(TupleSpace::new());
    let barrier_name = "dynamic-barrier".to_string();

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_name.clone())),
        PatternField::Wildcard,
    ]);

    // === Round 1: 3 participants ===
    println!("=== Round 1: 3 participants ===");
    let round1_count = 3;

    let mut round1_handles = Vec::new();
    for i in 0..round1_count {
        let space_clone = space.clone();
        let barrier_name_clone = barrier_name.clone();
        let pattern_clone = pattern.clone();

        let handle = tokio::spawn(async move {
            let mut rx = space_clone
                .barrier(
                    barrier_name_clone.clone(),
                    pattern_clone.clone(),
                    round1_count,
                )
                .await;

            space_clone
                .write(Tuple::new(vec![
                    TupleField::String("barrier".to_string()),
                    TupleField::String(barrier_name_clone),
                    TupleField::Integer(i as i64),
                ]))
                .await
                .unwrap();

            timeout(Duration::from_secs(3), rx.recv())
                .await
                .expect("Timeout in round 1");
        });

        round1_handles.push(handle);
    }

    for handle in round1_handles {
        handle.await.unwrap();
    }

    println!("✓ Round 1 completed");
    space.clear().await;

    // === Round 2: 5 participants (scaled up) ===
    println!("=== Round 2: 5 participants ===");
    let round2_count = 5;

    let mut round2_handles = Vec::new();
    for i in 0..round2_count {
        let space_clone = space.clone();
        let barrier_name_clone = barrier_name.clone();
        let pattern_clone = pattern.clone();

        let handle = tokio::spawn(async move {
            let mut rx = space_clone
                .barrier(
                    barrier_name_clone.clone(),
                    pattern_clone.clone(),
                    round2_count,
                )
                .await;

            space_clone
                .write(Tuple::new(vec![
                    TupleField::String("barrier".to_string()),
                    TupleField::String(barrier_name_clone),
                    TupleField::Integer(i as i64),
                ]))
                .await
                .unwrap();

            timeout(Duration::from_secs(3), rx.recv())
                .await
                .expect("Timeout in round 2");
        });

        round2_handles.push(handle);
    }

    for handle in round2_handles {
        handle.await.unwrap();
    }

    println!("✓ Round 2 completed");
    println!("✓ Dynamic participant count works!");
}

/// TDD RED: Test multiple iterations in loop (like Jacobi)
///
/// ## Scenario
/// Simulates iterative algorithm with 10 iterations,
/// each requiring barrier synchronization
#[tokio::test]
async fn test_barrier_multi_iteration_loop() {
    let space = Arc::new(TupleSpace::new());
    let barrier_name = "jacobi-iteration".to_string();
    let participant_count = 4;
    let iterations = 10;

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_name.clone())),
        PatternField::Wildcard,
    ]);

    println!(
        "=== Running {} iterations with {} participants ===",
        iterations, participant_count
    );

    for iteration in 0..iterations {
        println!("Iteration {}", iteration);

        let mut handles = Vec::new();
        for i in 0..participant_count {
            let space_clone = space.clone();
            let barrier_name_clone = barrier_name.clone();
            let pattern_clone = pattern.clone();

            let handle = tokio::spawn(async move {
                // Register at barrier
                let mut rx = space_clone
                    .barrier(
                        barrier_name_clone.clone(),
                        pattern_clone.clone(),
                        participant_count,
                    )
                    .await;

                // Write arrival
                space_clone
                    .write(Tuple::new(vec![
                        TupleField::String("barrier".to_string()),
                        TupleField::String(barrier_name_clone),
                        TupleField::Integer(i as i64),
                    ]))
                    .await
                    .unwrap();

                // Wait for all to arrive
                timeout(Duration::from_secs(3), rx.recv())
                    .await
                    .expect(&format!("Timeout at iteration {}", iteration));
            });

            handles.push(handle);
        }

        // All participants complete this iteration
        for handle in handles {
            handle.await.unwrap();
        }

        // Clear for next iteration
        space.clear().await;
    }

    println!("✓ All {} iterations completed successfully!", iterations);
}
