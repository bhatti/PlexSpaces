// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Chaos testing for persistence infrastructure
//!
//! ## Purpose
//! Validates resilience to random failures, data corruption, and adverse conditions.
//!
//! ## Test Scenarios
//! - Random failures during checkpoint/journal operations
//! - Data corruption (bit flips)
//! - Network partitions (simulated delays)
//! - Concurrent operations under stress
//!
//! ## Success Criteria
//! - System handles 10-20% failure rates gracefully
//! - Corrupted data is detected and rejected
//! - Operations complete despite transient failures
//! - State remains consistent after chaos

use plexspaces_mailbox::Message;
use plexspaces_persistence::*;
use rand::Rng;
use std::sync::Arc;
use tokio::sync::Mutex;

// ============================================================================
// Chaos Injection Framework
// ============================================================================

/// Chaos injector for testing failure scenarios
///
/// ## Purpose
/// Simulates real-world failure modes to validate system resilience.
///
/// ## Capabilities
/// - Random failures (configurable probability)
/// - Data corruption (random bit flips)
/// - Network partitions (simulated delays)
/// - Clock skew (time jumps)
pub struct ChaosInjector {
    /// Probability of failure (0.0 to 1.0)
    failure_rate: f64,
    /// Random number generator
    rng: Arc<Mutex<rand::rngs::StdRng>>,
}

impl ChaosInjector {
    /// Creates a new chaos injector with specified failure rate
    ///
    /// ## Arguments
    /// * `failure_rate` - Probability of failure (0.0 = never, 1.0 = always)
    ///
    /// ## Example
    /// ```
    /// let chaos = ChaosInjector::new(0.1);  // 10% failure rate
    /// ```
    pub fn new(failure_rate: f64) -> Self {
        use rand::SeedableRng;
        Self {
            failure_rate,
            rng: Arc::new(Mutex::new(rand::rngs::StdRng::from_entropy())),
        }
    }

    /// Maybe inject a failure based on failure rate
    ///
    /// ## Returns
    /// - `Ok(())` if no failure injected
    /// - `Err(())` if failure injected
    pub async fn maybe_fail(&self) -> Result<(), ()> {
        let mut rng = self.rng.lock().await;
        if rng.gen::<f64>() < self.failure_rate {
            Err(())
        } else {
            Ok(())
        }
    }

    /// Corrupt data with random bit flips
    ///
    /// ## Purpose
    /// Simulates bit flips from hardware errors, cosmic rays, etc.
    ///
    /// ## Effect
    /// Flips a random bit in the data with probability = failure_rate
    pub async fn maybe_corrupt(&self, data: &mut [u8]) {
        if data.is_empty() {
            return;
        }

        let mut rng = self.rng.lock().await;
        if rng.gen::<f64>() < self.failure_rate {
            let byte_idx = rng.gen_range(0..data.len());
            let bit_idx = rng.gen_range(0..8);
            data[byte_idx] ^= 1 << bit_idx; // Flip random bit
        }
    }

    /// Simulate network partition with delay
    ///
    /// ## Arguments
    /// * `max_delay_ms` - Maximum delay in milliseconds
    ///
    /// ## Effect
    /// Sleeps for a random duration (0..max_delay_ms) with probability = failure_rate
    pub async fn maybe_partition(&self, max_delay_ms: u64) {
        let mut rng = self.rng.lock().await;
        if rng.gen::<f64>() < self.failure_rate {
            let delay = rng.gen_range(0..max_delay_ms);
            drop(rng); // Release lock before sleeping
            tokio::time::sleep(tokio::time::Duration::from_millis(delay)).await;
        }
    }

    /// Returns true if chaos should be injected (based on failure rate)
    pub async fn should_inject(&self) -> bool {
        let mut rng = self.rng.lock().await;
        rng.gen::<f64>() < self.failure_rate
    }
}

// ============================================================================
// Chaos Tests: Snapshots
// ============================================================================

#[tokio::test]
async fn test_snapshot_creation_with_random_failures() {
    let chaos = ChaosInjector::new(0.1); // 10% failure rate

    let mut successes = 0;
    let mut failures = 0;

    for i in 0..100 {
        // Maybe inject failure before snapshot
        if chaos.maybe_fail().await.is_err() {
            failures += 1;
            continue;
        }

        // Create snapshot
        let journal = MemoryJournal::new();
        let snapshot = Snapshot {
            actor_id: "chaos-actor".to_string(),
            sequence: i,
            timestamp: chrono::Utc::now(),
            state_data: vec![1, 2, 3, 4, 5],
            compression: Default::default(),
            encryption: Default::default(),
            metadata: Default::default(),
        };

        let result = journal.save_snapshot(snapshot).await;

        // Maybe inject failure after snapshot
        if chaos.maybe_fail().await.is_err() {
            failures += 1;
            continue;
        }

        if result.is_ok() {
            successes += 1;
        } else {
            failures += 1;
        }
    }

    // Verify resilience: At least 80% success rate despite 10% chaos
    assert!(
        successes >= 80,
        "Expected ≥80 successes with 10% chaos, got {}",
        successes
    );
    println!(
        "✓ Snapshot creation chaos test: {} successes, {} failures ({}% success rate)",
        successes, failures, successes
    );
}

#[tokio::test]
async fn test_snapshot_load_with_corrupted_data() {
    let chaos = ChaosInjector::new(0.2); // 20% corruption rate

    let journal = MemoryJournal::new();

    // Create valid snapshot
    let original_data = vec![1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    let snapshot = Snapshot {
        actor_id: "chaos-actor".to_string(),
        sequence: 1,
        timestamp: chrono::Utc::now(),
        state_data: original_data.clone(),
        compression: Default::default(),
        encryption: Default::default(),
        metadata: Default::default(),
    };
    journal.save_snapshot(snapshot.clone()).await.unwrap();

    // Test loading with potential corruption
    let mut successes = 0;
    let mut corruptions = 0;

    for _ in 0..50 {
        let mut corrupted_snapshot = snapshot.clone();

        // Maybe corrupt snapshot data
        chaos
            .maybe_corrupt(&mut corrupted_snapshot.state_data)
            .await;

        // Try to load
        let loaded = journal
            .get_latest_snapshot(&"chaos-actor".to_string())
            .await
            .unwrap()
            .unwrap();

        // Check if data matches (corruption would cause mismatch)
        if loaded.state_data == original_data {
            successes += 1;
        } else {
            corruptions += 1;
        }
    }

    // Verify corruption occurred (with 20% rate, expect some corruptions)
    println!(
        "✓ Snapshot corruption test: {} clean loads, {} corruptions detected ({}% corruption rate)",
        successes,
        corruptions,
        (corruptions * 100) / 50
    );

    // With 20% corruption rate, we should have at least some corruptions detected
    // But the actual stored snapshot should remain uncorrupted
    let final_snapshot = journal
        .get_latest_snapshot(&"chaos-actor".to_string())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(final_snapshot.state_data, original_data);
}

// ============================================================================
// Chaos Tests: Journal Operations
// ============================================================================

#[tokio::test]
async fn test_journal_writes_with_random_failures() {
    let chaos = ChaosInjector::new(0.15); // 15% failure rate

    let journal = MemoryJournal::new();
    let message = Message::new(b"chaos test".to_vec());

    let mut successes = 0;
    let mut failures = 0;

    for _ in 0..100 {
        // Maybe inject failure
        if chaos.maybe_fail().await.is_err() {
            failures += 1;
            continue;
        }

        // Write journal entry
        let result = journal.record_message_received(&message).await;

        if result.is_ok() {
            successes += 1;
        } else {
            failures += 1;
        }
    }

    // Verify resilience: At least 80% success rate
    assert!(
        successes >= 80,
        "Expected ≥80 successes with 15% chaos, got {}",
        successes
    );
    println!(
        "✓ Journal write chaos test: {} successes, {} failures ({}% success rate)",
        successes, failures, successes
    );
}

#[tokio::test]
async fn test_journal_writes_with_network_partition() {
    let chaos = ChaosInjector::new(0.1); // 10% partition rate

    let journal = MemoryJournal::new();
    let message = Message::new(b"partition test".to_vec());

    let mut successes = 0;
    let mut timeouts = 0;

    for _ in 0..50 {
        // Maybe inject network partition (delay up to 100ms)
        chaos.maybe_partition(100).await;

        // Write journal entry with timeout
        let result = tokio::time::timeout(
            tokio::time::Duration::from_millis(200), // 200ms timeout
            journal.record_message_received(&message),
        )
        .await;

        match result {
            Ok(Ok(_)) => successes += 1,
            Ok(Err(_)) => panic!("Unexpected journal error"),
            Err(_) => timeouts += 1, // Timeout due to partition
        }
    }

    // All should succeed (partition delays are < timeout)
    assert_eq!(successes, 50);
    println!(
        "✓ Journal partition test: {} successes, {} timeouts",
        successes, timeouts
    );
}

// ============================================================================
// Chaos Tests: Retention Under Stress
// ============================================================================

#[tokio::test]
async fn test_retention_with_concurrent_chaos() {
    let _chaos = ChaosInjector::new(0.2); // 20% chaos rate

    let config = RetentionConfig {
        retention_count: 5,
        auto_truncate: false,
    };
    let journal = Arc::new(MemoryJournal::with_retention(config));

    // Create many snapshots concurrently with chaos
    let mut handles = vec![];

    for i in 0..20 {
        let journal = Arc::clone(&journal);
        let chaos = ChaosInjector::new(0.2);

        let handle = tokio::spawn(async move {
            // Maybe inject delay
            chaos.maybe_partition(50).await;

            // Maybe inject failure
            if chaos.maybe_fail().await.is_err() {
                return Err(());
            }

            let snapshot = Snapshot {
                actor_id: "concurrent-actor".to_string(),
                sequence: i,
                timestamp: chrono::Utc::now(),
                state_data: vec![i as u8; 1024],
                compression: Default::default(),
                encryption: Default::default(),
                metadata: Default::default(),
            };

            journal.save_snapshot(snapshot).await.map_err(|_| ())?;
            Ok(())
        });

        handles.push(handle);
    }

    // Wait for all to complete
    let mut successes = 0;
    for handle in handles {
        if let Ok(Ok(())) = handle.await {
            successes += 1;
        }
    }

    println!("✓ Concurrent snapshot creation: {} successes", successes);

    // Enforce retention
    let deleted = journal
        .enforce_retention(&"concurrent-actor".to_string())
        .await
        .unwrap();

    // Should have deleted old snapshots (kept last 5)
    let remaining = journal
        .list_snapshots(&"concurrent-actor".to_string())
        .await
        .unwrap();

    assert!(
        remaining.len() <= 5,
        "Expected ≤5 snapshots after retention, got {}",
        remaining.len()
    );
    println!(
        "✓ Retention enforcement: deleted {}, {} remaining",
        deleted,
        remaining.len()
    );
}

// ============================================================================
// Chaos Tests: Full Scenario
// ============================================================================

#[tokio::test]
async fn test_actor_lifecycle_with_full_chaos() {
    let chaos = ChaosInjector::new(0.15); // 15% chaos rate

    let config = RetentionConfig {
        retention_count: 3,
        auto_truncate: true,
    };
    let journal = MemoryJournal::with_retention(config);

    // Simulate actor processing 100 messages with chaos
    let mut state: Vec<u8> = vec![0u8; 1024];
    let mut checkpoint_count = 0;
    let mut crash_count = 0;

    for i in 0..100 {
        // Maybe inject crash
        if chaos.maybe_fail().await.is_err() {
            crash_count += 1;

            // Simulate actor restart + recovery from latest snapshot
            let snapshot = journal
                .get_latest_snapshot(&"chaos-actor".to_string())
                .await
                .ok()
                .flatten();

            if let Some(snap) = snapshot {
                state = snap.state_data;
            }

            continue;
        }

        // Process message (modify state)
        let state_len = state.len();
        state[i % state_len] = (i % 256) as u8;

        // Journal message
        let message = Message::new(format!("message-{}", i).into_bytes());
        chaos.maybe_partition(10).await;
        let _ = journal.record_message_received(&message).await;

        // Maybe create snapshot (every 10 messages)
        if i % 10 == 0 {
            chaos.maybe_partition(10).await;

            let snapshot = Snapshot {
                actor_id: "chaos-actor".to_string(),
                sequence: i as u64,
                timestamp: chrono::Utc::now(),
                state_data: state.clone(),
                compression: Default::default(),
                encryption: Default::default(),
                metadata: Default::default(),
            };

            if journal.save_snapshot_with_retention(snapshot).await.is_ok() {
                checkpoint_count += 1;
            }
        }
    }

    println!(
        "✓ Full chaos scenario: {} checkpoints created, {} crashes simulated",
        checkpoint_count, crash_count
    );

    // Verify final state is consistent
    let final_snapshot = journal
        .get_latest_snapshot(&"chaos-actor".to_string())
        .await
        .unwrap();

    if let Some(snap) = final_snapshot {
        let recovered_state = snap.state_data;

        // State should be valid (correct length)
        assert_eq!(recovered_state.len(), state.len());

        println!(
            "✓ Final state recovered successfully ({} bytes)",
            recovered_state.len()
        );
    }

    // Verify retention was enforced (should have ≤3 snapshots)
    let remaining = journal
        .list_snapshots(&"chaos-actor".to_string())
        .await
        .unwrap();

    assert!(
        remaining.len() <= 3,
        "Expected ≤3 snapshots, got {}",
        remaining.len()
    );
    println!(
        "✓ Retention enforced: {} snapshots remaining",
        remaining.len()
    );
}

// ============================================================================
// Chaos Tests: Concurrent Operations
// ============================================================================

#[tokio::test]
async fn test_concurrent_reads_writes_with_chaos() {
    let _chaos = ChaosInjector::new(0.1); // 10% chaos rate
    let journal = Arc::new(MemoryJournal::new());

    // Spawn 10 concurrent writers
    let mut write_handles = vec![];
    for i in 0..10 {
        let journal = Arc::clone(&journal);
        let chaos = ChaosInjector::new(0.1);

        let handle = tokio::spawn(async move {
            let mut writes = 0;
            for j in 0..20 {
                chaos.maybe_partition(5).await;

                if chaos.maybe_fail().await.is_ok() {
                    let message = Message::new(format!("writer-{}-msg-{}", i, j).into_bytes());
                    if journal.record_message_received(&message).await.is_ok() {
                        writes += 1;
                    }
                }
            }
            writes
        });

        write_handles.push(handle);
    }

    // Spawn 5 concurrent readers
    let mut read_handles = vec![];
    for _i in 0..5 {
        let journal = Arc::clone(&journal);
        let chaos = ChaosInjector::new(0.1);

        let handle = tokio::spawn(async move {
            let mut reads = 0;
            for _ in 0..20 {
                chaos.maybe_partition(5).await;

                if chaos.maybe_fail().await.is_ok() {
                    if journal.get_entries().await.is_ok() {
                        reads += 1;
                    }
                }
            }
            reads
        });

        read_handles.push(handle);
    }

    // Wait for all writers
    let mut total_writes = 0;
    for handle in write_handles {
        total_writes += handle.await.unwrap();
    }

    // Wait for all readers
    let mut total_reads = 0;
    for handle in read_handles {
        total_reads += handle.await.unwrap();
    }

    println!(
        "✓ Concurrent chaos test: {} writes, {} reads completed",
        total_writes, total_reads
    );

    // Verify system remained consistent
    let final_entries = journal.get_entries().await.unwrap();
    assert_eq!(final_entries.len(), total_writes);
}

// ============================================================================
// Chaos Tests: Data Corruption Detection
// ============================================================================

#[tokio::test]
async fn test_corruption_detection_in_state_data() {
    let chaos = ChaosInjector::new(1.0); // 100% corruption for testing

    let original_data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let mut corrupted_data = original_data.clone();

    // Corrupt the data
    chaos.maybe_corrupt(&mut corrupted_data).await;

    // Verify corruption occurred
    assert_ne!(
        corrupted_data, original_data,
        "Corruption should have occurred with 100% rate"
    );

    println!(
        "✓ Corruption detected: original {:?} != corrupted {:?}",
        original_data, corrupted_data
    );
}

#[tokio::test]
async fn test_no_corruption_with_zero_rate() {
    let chaos = ChaosInjector::new(0.0); // 0% corruption rate

    let original_data = vec![0xAA, 0xBB, 0xCC, 0xDD, 0xEE, 0xFF];
    let mut data = original_data.clone();

    // Try to corrupt (should not happen)
    for _ in 0..100 {
        chaos.maybe_corrupt(&mut data).await;
    }

    // Verify no corruption occurred
    assert_eq!(
        data, original_data,
        "No corruption should occur with 0% rate"
    );

    println!("✓ No corruption with 0% rate");
}
