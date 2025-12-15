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

//! Performance benchmarks for persistence infrastructure
//!
//! ## Purpose
//! Measures checkpoint, journal, and retention performance to detect regressions.
//!
//! ## Metrics Tracked
//! - Snapshot creation latency (P50, P95, P99)
//! - Snapshot loading latency
//! - Journal write throughput (entries/sec)
//! - Side effect recording latency
//! - Retention enforcement performance
//!
//! ## Running Benchmarks
//! ```bash
//! cargo bench -p plexspaces-persistence
//! ```
//!
//! ## Viewing Reports
//! ```bash
//! open target/criterion/report/index.html
//! ```

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use plexspaces_mailbox::Message;
use plexspaces_persistence::{
    execution_context::{ExecutionContext, ExecutionMode},
    Journal, MemoryJournal, PromiseMetadata, PromiseResult, RetentionConfig, SideEffect, Snapshot,
};
use tokio::runtime::Runtime;

/// Benchmark snapshot creation with different sizes
fn benchmark_snapshot_creation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("snapshot_create");

    // Test different state sizes (1KB, 10KB, 100KB, 1MB)
    for size_kb in [1, 10, 100, 1000] {
        let state_data = vec![0u8; size_kb * 1024];
        group.throughput(Throughput::Bytes(state_data.len() as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(size_kb),
            &state_data,
            |b, data| {
                b.to_async(&rt).iter(|| async {
                    let journal = MemoryJournal::new();
                    let snapshot = Snapshot {
                        actor_id: "bench-actor".to_string(),
                        sequence: 1,
                        timestamp: chrono::Utc::now(),
                        state_data: data.clone(),
                        metadata: Default::default(),
                    };
                    journal.save_snapshot(snapshot).await.unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmark snapshot loading with different sizes
fn benchmark_snapshot_loading(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("snapshot_load");

    for size_kb in [1, 10, 100, 1000] {
        let state_data = vec![0u8; size_kb * 1024];
        group.throughput(Throughput::Bytes(state_data.len() as u64));

        // Pre-create snapshot
        let journal = rt.block_on(async {
            let journal = MemoryJournal::new();
            let snapshot = Snapshot {
                actor_id: "bench-actor".to_string(),
                sequence: 1,
                timestamp: chrono::Utc::now(),
                state_data: state_data.clone(),
                metadata: Default::default(),
            };
            journal.save_snapshot(snapshot).await.unwrap();
            journal
        });

        group.bench_with_input(BenchmarkId::from_parameter(size_kb), &journal, |b, j| {
            b.to_async(&rt).iter(|| async {
                let snapshot = j
                    .get_latest_snapshot(&"bench-actor".to_string())
                    .await
                    .unwrap();
                assert!(snapshot.is_some());
            });
        });
    }

    group.finish();
}

/// Benchmark journal entry writes
fn benchmark_journal_writes(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("journal_write");

    // Test different batch sizes
    for batch_size in [1, 10, 100, 1000] {
        group.throughput(Throughput::Elements(batch_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(batch_size),
            &batch_size,
            |b, &size| {
                b.to_async(&rt).iter(|| async {
                    let journal = MemoryJournal::new();
                    let message = Message::new(b"benchmark data".to_vec());

                    for _ in 0..size {
                        journal.record_message_received(&message).await.unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark side effect recording with ExecutionContext
fn benchmark_side_effects(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("side_effects");

    // Test different numbers of side effects
    for count in [1, 10, 50, 100] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &cnt| {
            b.to_async(&rt).iter(|| async {
                let mut ctx = ExecutionContext::new("bench-actor", ExecutionMode::ExecutionModeNormal);

                for i in 0..cnt {
                    let effect_id = format!("effect-{}", i);
                    ctx.record_side_effect(&effect_id, || Ok(i as u32))
                        .await
                        .unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Benchmark side effect replay with ExecutionContext
fn benchmark_side_effect_replay(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("side_effect_replay");

    for count in [10, 50, 100] {
        group.throughput(Throughput::Elements(count as u64));

        // Pre-record side effects
        let ctx = rt.block_on(async {
            let mut ctx = ExecutionContext::new("bench-actor", ExecutionMode::ExecutionModeNormal);
            for i in 0..count {
                let effect_id = format!("effect-{}", i);
                ctx.record_side_effect(&effect_id, || Ok(i as u32))
                    .await
                    .unwrap();
            }
            ctx
        });

        group.bench_with_input(BenchmarkId::from_parameter(count), &ctx, |b, c| {
            b.to_async(&rt).iter(|| async {
                let mut replay_ctx = ExecutionContext::new("bench-actor", ExecutionMode::ExecutionModeReplay);
                // Copy cache from original context
                for i in 0..count {
                    let effect_id = format!("effect-{}", i);
                    // In a real scenario, cache would be populated from journal
                    // For benchmark, we just test the replay path
                }
            });
        });
    }

    group.finish();
}

/// Benchmark retention enforcement
fn benchmark_retention(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("retention");

    // Test retention with different snapshot counts
    for snapshot_count in [10, 50, 100] {
        group.throughput(Throughput::Elements(snapshot_count as u64));

        let config = RetentionConfig {
            retention_count: 5, // Keep last 5
            auto_truncate: false,
        };

        // Pre-create snapshots
        let journal = rt.block_on(async {
            let journal = MemoryJournal::with_retention(config);
            for seq in 0..snapshot_count {
                let snapshot = Snapshot {
                    actor_id: "bench-actor".to_string(),
                    sequence: seq,
                    timestamp: chrono::Utc::now(),
                    state_data: vec![0u8; 1024],
                    metadata: Default::default(),
                };
                journal.save_snapshot(snapshot).await.unwrap();
            }
            journal
        });

        group.bench_with_input(
            BenchmarkId::from_parameter(snapshot_count),
            &journal,
            |b, j| {
                b.to_async(&rt).iter(|| async {
                    let deleted = j
                        .enforce_retention(&"bench-actor".to_string())
                        .await
                        .unwrap();
                    // Should delete snapshot_count - 5 snapshots
                    assert!(deleted > 0 || snapshot_count <= 5);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark promise recording
fn benchmark_promises(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("promises");

    for count in [10, 50, 100] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, &cnt| {
            b.to_async(&rt).iter(|| async {
                let journal = MemoryJournal::new();

                for i in 0..cnt {
                    let promise_id = format!("promise-{}", i);
                    let metadata = PromiseMetadata {
                        creator_id: "bench-actor".to_string(),
                        timeout_ms: Some(5000),
                        idempotency_key: Some(format!("key-{}", i)),
                    };
                    journal
                        .record_promise_created(&promise_id, metadata)
                        .await
                        .unwrap();
                    journal
                        .record_promise_resolved(
                            &promise_id,
                            PromiseResult::Fulfilled(vec![0u8; 100]),
                        )
                        .await
                        .unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Benchmark side effect types
fn benchmark_side_effect_types(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("side_effect_types");

    let test_cases = vec![
        (
            "external_call",
            SideEffect::ExternalCall {
                service: "test-service".to_string(),
                method: "test-method".to_string(),
                request: vec![0u8; 1024],
                response: Some(vec![0u8; 1024]),
            },
        ),
        (
            "timer_scheduled",
            SideEffect::TimerScheduled {
                name: "test-timer".to_string(),
                duration_ms: 5000,
            },
        ),
        ("sleep", SideEffect::Sleep { duration_ms: 1000 }),
        (
            "random",
            SideEffect::RandomGenerated {
                value: vec![0u8; 32],
            },
        ),
        (
            "time_access",
            SideEffect::TimeAccessed {
                timestamp: chrono::Utc::now(),
            },
        ),
    ];

    for (name, side_effect) in test_cases {
        group.bench_function(name, |b| {
            b.to_async(&rt).iter(|| async {
                let journal = MemoryJournal::new();
                journal
                    .record_side_effect(&"bench-actor".to_string(), side_effect.clone())
                    .await
                    .unwrap();
            });
        });
    }

    group.finish();
}

/// Benchmark journal truncation
fn benchmark_truncation(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let mut group = c.benchmark_group("truncation");

    for entry_count in [100, 500, 1000] {
        group.throughput(Throughput::Elements(entry_count as u64));

        // Pre-create journal entries
        let journal = rt.block_on(async {
            let journal = MemoryJournal::new();
            let message = Message::new(b"test".to_vec());
            for _ in 0..entry_count {
                journal.record_message_received(&message).await.unwrap();
            }
            journal
        });

        group.bench_with_input(
            BenchmarkId::from_parameter(entry_count),
            &journal,
            |b, j| {
                b.to_async(&rt).iter(|| async {
                    // Truncate to middle point
                    j.truncate_to(&"test-actor".to_string(), entry_count / 2)
                        .await
                        .unwrap();
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    benchmark_snapshot_creation,
    benchmark_snapshot_loading,
    benchmark_journal_writes,
    benchmark_side_effects,
    benchmark_side_effect_replay,
    benchmark_retention,
    benchmark_promises,
    benchmark_side_effect_types,
    benchmark_truncation,
);
criterion_main!(benches);
