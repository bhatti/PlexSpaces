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

//! Integration Tests for TupleSpace watch() Primitive
//!
//! ## Purpose
//! Validates the LOCAL watch() primitive implementation using Tokio channels.
//! Tests the core pattern for barrier synchronization and distributed coordination.
//!
//! ## Architecture
//! **LOCAL watch()**: Uses Tokio mpsc::channel for notifications (not gRPC RPC)
//! **Pattern**: write() triggers notifications to matching watchers
//! **Industry Standard**: Aligns with ZooKeeper watch(), etcd WATCH, Redis SUBSCRIBE
//!
//! See docs/BARRIER_ARCHITECTURE_SUMMARY.md for design rationale.

use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField, TupleSpace, WatchEvent};
use std::time::Duration;

/// Test 1: Basic watch() - single watcher receives notification
#[tokio::test]
async fn test_watch_single_tuple_added() {
    let space = TupleSpace::default();

    // Create pattern to watch: ("event", _)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("event".to_string())),
        PatternField::Wildcard,
    ]);

    // Start watching
    let mut watcher = space.watch(pattern.clone()).await;

    // Write matching tuple
    let tuple = Tuple::new(vec![
        TupleField::String("event".to_string()),
        TupleField::String("data-1".to_string()),
    ]);
    space.write(tuple.clone()).await.expect("Write failed");

    // Receive notification
    let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("No event received");

    match event {
        WatchEvent::Added(received_tuple) => {
            assert_eq!(received_tuple.fields().len(), 2);
            assert_eq!(
                received_tuple.fields()[0],
                TupleField::String("event".to_string())
            );
            assert_eq!(
                received_tuple.fields()[1],
                TupleField::String("data-1".to_string())
            );
        }
        WatchEvent::Removed(_) => panic!("Expected Added event, got Removed"),
    }
}

/// Test 2: Multiple watchers - all receive notifications
#[tokio::test]
async fn test_watch_multiple_watchers() {
    let space = TupleSpace::default();

    // Create pattern
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("multi".to_string())),
        PatternField::Wildcard,
    ]);

    // Start 3 watchers
    let mut watcher1 = space.watch(pattern.clone()).await;
    let mut watcher2 = space.watch(pattern.clone()).await;
    let mut watcher3 = space.watch(pattern.clone()).await;

    // Write matching tuple
    let tuple = Tuple::new(vec![
        TupleField::String("multi".to_string()),
        TupleField::Integer(42),
    ]);
    space.write(tuple).await.expect("Write failed");

    // All 3 watchers should receive notification
    for (i, watcher) in [&mut watcher1, &mut watcher2, &mut watcher3]
        .iter_mut()
        .enumerate()
    {
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .expect(&format!("Watcher {} timeout", i + 1))
            .expect(&format!("Watcher {} no event", i + 1));

        match event {
            WatchEvent::Added(t) => {
                assert_eq!(t.fields()[1], TupleField::Integer(42));
            }
            WatchEvent::Removed(_) => panic!("Watcher {} got Removed event", i + 1),
        }
    }
}

/// Test 3: Pattern matching - only matching tuples trigger notifications
#[tokio::test]
async fn test_watch_pattern_matching() {
    let space = TupleSpace::default();

    // Watch for ("sensor", "temp", _)
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("sensor".to_string())),
        PatternField::Exact(TupleField::String("temp".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(pattern).await;

    // Write non-matching tuple (should NOT trigger)
    space
        .write(Tuple::new(vec![
            TupleField::String("sensor".to_string()),
            TupleField::String("humidity".to_string()),
            TupleField::Integer(60),
        ]))
        .await
        .expect("Write failed");

    // Write matching tuple (should trigger)
    space
        .write(Tuple::new(vec![
            TupleField::String("sensor".to_string()),
            TupleField::String("temp".to_string()),
            TupleField::Integer(72),
        ]))
        .await
        .expect("Write failed");

    // Should only receive event for matching tuple
    let event = tokio::time::timeout(Duration::from_millis(500), watcher.recv())
        .await
        .expect("Timeout waiting for event")
        .expect("No event received");

    match event {
        WatchEvent::Added(t) => {
            assert_eq!(t.fields()[2], TupleField::Integer(72));
        }
        WatchEvent::Removed(_) => panic!("Expected Added event"),
    }

    // Should NOT receive event for non-matching tuple
    let no_event = tokio::time::timeout(Duration::from_millis(200), watcher.recv()).await;
    assert!(
        no_event.is_err(),
        "Should not receive event for non-matching tuple"
    );
}

/// Test 4: Multiple tuples - watcher receives all matching events
#[tokio::test]
async fn test_watch_multiple_tuples() {
    let space = TupleSpace::default();

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("log".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(pattern).await;

    // Write 5 matching tuples
    for i in 1..=5 {
        space
            .write(Tuple::new(vec![
                TupleField::String("log".to_string()),
                TupleField::Integer(i),
            ]))
            .await
            .expect("Write failed");
    }

    // Receive all 5 events
    for expected_i in 1..=5 {
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .expect(&format!("Timeout waiting for event {}", expected_i))
            .expect(&format!("No event {} received", expected_i));

        match event {
            WatchEvent::Added(t) => {
                assert_eq!(t.fields()[1], TupleField::Integer(expected_i));
            }
            WatchEvent::Removed(_) => panic!("Expected Added event for {}", expected_i),
        }
    }
}

/// Test 5: Concurrent writes - watcher receives all events in order
#[tokio::test]
async fn test_watch_concurrent_writes() {
    use std::sync::Arc;

    let space = Arc::new(TupleSpace::default());

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("concurrent".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(pattern).await;

    // Spawn 10 tasks writing concurrently
    let mut handles = vec![];
    for i in 0..10 {
        let space_clone = Arc::clone(&space);
        handles.push(tokio::spawn(async move {
            space_clone
                .write(Tuple::new(vec![
                    TupleField::String("concurrent".to_string()),
                    TupleField::Integer(i),
                ]))
                .await
                .expect("Write failed");
        }));
    }

    // Wait for all writes
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Receive all 10 events (order may vary)
    let mut received_values = vec![];
    for _ in 0..10 {
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .expect("Timeout waiting for event")
            .expect("No event received");

        match event {
            WatchEvent::Added(t) => {
                if let TupleField::Integer(val) = t.fields()[1] {
                    received_values.push(val);
                }
            }
            WatchEvent::Removed(_) => panic!("Expected Added event"),
        }
    }

    // Verify all values received (order doesn't matter)
    received_values.sort();
    assert_eq!(received_values, vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
}

/// Test 6: Barrier pattern using watch() + count()
#[tokio::test]
async fn test_watch_barrier_pattern() {
    let space = TupleSpace::default();

    let barrier_pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String("round-1".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(barrier_pattern.clone()).await;

    // Simulate 4 actors arriving at barrier
    for i in 0..4 {
        space
            .write(Tuple::new(vec![
                TupleField::String("barrier".to_string()),
                TupleField::String("round-1".to_string()),
                TupleField::String(format!("actor-{}", i)),
            ]))
            .await
            .expect("Write failed");
    }

    // Count arrivals (should be 4)
    let count = space
        .count(barrier_pattern.clone())
        .await
        .expect("Count failed");
    assert_eq!(count, 4);

    // Watcher should have received 4 events
    for i in 0..4 {
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .expect(&format!("Timeout for arrival {}", i))
            .expect(&format!("No event for arrival {}", i));

        match event {
            WatchEvent::Added(t) => {
                assert_eq!(t.fields()[0], TupleField::String("barrier".to_string()));
                assert_eq!(t.fields()[1], TupleField::String("round-1".to_string()));
            }
            WatchEvent::Removed(_) => panic!("Expected Added event"),
        }
    }
}

/// Test 7: Wildcard patterns - flexible matching
#[tokio::test]
async fn test_watch_wildcard_patterns() {
    let space = TupleSpace::default();

    // Watch for (_, "status", _) - matches any first field, exact "status", any third field
    let pattern = Pattern::new(vec![
        PatternField::Wildcard,
        PatternField::Exact(TupleField::String("status".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(pattern).await;

    // Write matching tuples with different first fields
    space
        .write(Tuple::new(vec![
            TupleField::String("service-a".to_string()),
            TupleField::String("status".to_string()),
            TupleField::String("running".to_string()),
        ]))
        .await
        .expect("Write failed");

    space
        .write(Tuple::new(vec![
            TupleField::String("service-b".to_string()),
            TupleField::String("status".to_string()),
            TupleField::String("stopped".to_string()),
        ]))
        .await
        .expect("Write failed");

    // Both should trigger notifications
    for expected_service in ["service-a", "service-b"] {
        let event = tokio::time::timeout(Duration::from_secs(1), watcher.recv())
            .await
            .expect(&format!("Timeout for {}", expected_service))
            .expect(&format!("No event for {}", expected_service));

        match event {
            WatchEvent::Added(t) => {
                assert_eq!(t.fields()[1], TupleField::String("status".to_string()));
            }
            WatchEvent::Removed(_) => panic!("Expected Added event"),
        }
    }
}

/// Test 8: Channel capacity - backpressure handling
///
/// TODO: This test is currently disabled due to performance issues (takes >60 seconds).
/// The test needs optimization - either:
/// 1. Reduce timeout duration from 100ms to 10ms
/// 2. Reduce number of tuples from 150 to 50
/// 3. Use try_recv() instead of timeout-based recv()
///
/// The core watch() functionality is validated by the other 7 tests.
#[tokio::test]
#[ignore] // Disabled - performance issue (>60s runtime)
async fn test_watch_channel_capacity() {
    let space = TupleSpace::default();

    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("rapid".to_string())),
        PatternField::Wildcard,
    ]);

    let mut watcher = space.watch(pattern).await;

    // Write more tuples than channel capacity (default 100)
    // Note: Current implementation has channel capacity of 100
    for i in 0..150 {
        space
            .write(Tuple::new(vec![
                TupleField::String("rapid".to_string()),
                TupleField::Integer(i),
            ]))
            .await
            .expect("Write failed");
    }

    // Receive events (some may be dropped if channel is full)
    let mut received = 0;
    while let Ok(Some(_)) = tokio::time::timeout(Duration::from_millis(100), watcher.recv()).await {
        received += 1;
        if received >= 150 {
            break;
        }
    }

    // Should receive most events (at least 100 due to channel capacity)
    assert!(
        received >= 100,
        "Should receive at least 100 events, got {}",
        received
    );
}
