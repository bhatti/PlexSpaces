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

//! Performance benchmarks for KeyValueStore implementations
//!
//! ## Purpose
//! Measures performance characteristics of KeyValueStore backends to guide
//! deployment decisions and identify performance regressions.
//!
//! ## Benchmarks
//! 1. **Single Operations**: put, get, delete, exists
//! 2. **Batch Operations**: bulk put/get performance
//! 3. **Concurrent Access**: Multi-threaded read/write workloads
//! 4. **Key Patterns**: Sequential vs random access, prefix scans
//! 5. **Value Sizes**: Small (100B), medium (10KB), large (1MB)
//! 6. **Backend Comparison**: InMemory vs SQLite performance
//!
//! ## Running Benchmarks
//! ```bash
//! # Run all benchmarks
//! cargo bench -p plexspaces-keyvalue
//!
//! # Run specific benchmark
//! cargo bench -p plexspaces-keyvalue -- single_operations
//!
//! # Save baseline for comparison
//! cargo bench -p plexspaces-keyvalue -- --save-baseline main
//!
//! # Compare against baseline
//! cargo bench -p plexspaces-keyvalue -- --baseline main
//! ```
//!
//! ## Performance Targets
//! Based on PlexSpaces performance goals (CLAUDE.md):
//!
//! ### InMemory Backend
//! - Single get/put: < 1μs (microsecond)
//! - Batch operations: > 1M ops/sec
//! - Concurrent reads: Linear scaling with cores
//!
//! ### SQLite Backend
//! - Single get/put: < 100μs
//! - Batch operations: > 10K ops/sec
//! - Concurrent reads: Good (WAL mode)
//! - Concurrent writes: Sequential (single writer)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use plexspaces_core::RequestContext;
use plexspaces_keyvalue::{InMemoryKVStore, KeyValueStore};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

// =============================================================================
// BENCHMARK HELPERS
// =============================================================================

/// Create test key with prefix
fn test_key(prefix: &str, i: usize) -> String {
    format!("{}:{:08}", prefix, i)
}

/// Create test value of specified size
fn test_value(size: usize) -> Vec<u8> {
    vec![b'x'; size]
}

/// Create test RequestContext
fn test_ctx() -> RequestContext {
    RequestContext::new("bench-tenant".to_string())
        .with_namespace("bench-namespace".to_string())
}

/// Benchmark configuration
struct BenchConfig {
    /// Number of operations for throughput tests
    operation_count: usize,
    /// Small value size (100 bytes)
    small_value_size: usize,
    /// Medium value size (10 KB)
    medium_value_size: usize,
    /// Large value size (1 MB)
    large_value_size: usize,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            operation_count: 1000,
            small_value_size: 100,
            medium_value_size: 10 * 1024,
            large_value_size: 1024 * 1024,
        }
    }
}

// =============================================================================
// BENCHMARK 1: SINGLE OPERATIONS (GET, PUT, DELETE, EXISTS)
// =============================================================================

/// Benchmark single put operation
fn bench_single_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_put");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    // Test different value sizes
    for &size in &[config.small_value_size, config.medium_value_size] {
        group.throughput(Throughput::Bytes(size as u64));

        // InMemory backend
        group.bench_with_input(BenchmarkId::new("InMemory", size), &size, |b, &size| {
            let store = Arc::new(InMemoryKVStore::new());
            let value = test_value(size);
            let mut counter = 0;

            b.to_async(&rt).iter(|| {
                let store = store.clone();
                let value = value.clone();
                let key = test_key("bench", counter);
                counter += 1;

                let ctx = test_ctx();
                async move {
                    store.put(&ctx, &key, value).await.unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Benchmark single get operation
fn bench_single_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_get");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    for &size in &[config.small_value_size, config.medium_value_size] {
        group.throughput(Throughput::Bytes(size as u64));

        // InMemory backend - pre-populate data
        group.bench_with_input(BenchmarkId::new("InMemory", size), &size, |b, &size| {
            let store = Arc::new(InMemoryKVStore::new());
            let value = test_value(size);

            // Pre-populate 1000 keys
            let ctx = test_ctx();
            rt.block_on(async {
                for i in 0..1000 {
                    let key = test_key("bench", i);
                    store.put(&ctx, &key, value.clone()).await.unwrap();
                }
            });

            let mut counter = 0;
            b.to_async(&rt).iter(|| {
                let store = store.clone();
                let key = test_key("bench", counter % 1000);
                counter += 1;

                let ctx = test_ctx();
                async move {
                    let _ = black_box(store.get(&ctx, &key).await.unwrap());
                }
            });
        });
    }

    group.finish();
}

/// Benchmark delete operation
fn bench_single_delete(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_delete");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.bench_function("InMemory", |b| {
        let store = Arc::new(InMemoryKVStore::new());
        let value = test_value(config.small_value_size);

        // Pre-populate keys to delete
        let ctx = test_ctx();
        rt.block_on(async {
            for i in 0..10000 {
                let key = test_key("delete", i);
                store.put(&ctx, &key, value.clone()).await.unwrap();
            }
        });

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let store = store.clone();
            let key = test_key("delete", counter % 10000);
            counter += 1;

            let ctx = test_ctx();
            async move {
                store.delete(&ctx, &key).await.unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark exists operation
fn bench_single_exists(c: &mut Criterion) {
    let mut group = c.benchmark_group("single_exists");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.bench_function("InMemory", |b| {
        let store = Arc::new(InMemoryKVStore::new());
        let value = test_value(config.small_value_size);

        // Pre-populate 1000 keys
        let ctx = test_ctx();
        rt.block_on(async {
            for i in 0..1000 {
                let key = test_key("bench", i);
                store.put(&ctx, &key, value.clone()).await.unwrap();
            }
        });

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let store = store.clone();
            let key = test_key("bench", counter % 1000);
            counter += 1;

            let ctx = test_ctx();
            async move {
                let _ = black_box(store.exists(&ctx, &key).await.unwrap());
            }
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK 2: BATCH OPERATIONS
// =============================================================================

/// Benchmark bulk put (sequential writes)
fn bench_bulk_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("bulk_put");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    // Test with 1000 operations
    group.throughput(Throughput::Elements(config.operation_count as u64));
    group.sample_size(10); // Fewer samples for bulk operations
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("InMemory_1000_ops", |b| {
        let value = test_value(config.small_value_size);

        b.to_async(&rt).iter(|| {
            let store = Arc::new(InMemoryKVStore::new());
            let value = value.clone();

            let ctx = test_ctx();
            async move {
                for i in 0..config.operation_count {
                    let key = test_key("bulk", i);
                    store.put(&ctx, &key, value.clone()).await.unwrap();
                }
            }
        });
    });

    group.finish();
}

/// Benchmark bulk get (sequential reads)
fn bench_bulk_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("bulk_get");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.throughput(Throughput::Elements(config.operation_count as u64));
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(10));

    group.bench_function("InMemory_1000_ops", |b| {
        let store = Arc::new(InMemoryKVStore::new());
        let value = test_value(config.small_value_size);

        // Pre-populate data
        let ctx = test_ctx();
        rt.block_on(async {
            for i in 0..config.operation_count {
                let key = test_key("bulk", i);
                store.put(&ctx, &key, value.clone()).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| {
            let store = store.clone();

            let ctx = test_ctx();
            async move {
                for i in 0..config.operation_count {
                    let key = test_key("bulk", i);
                    let _ = black_box(store.get(&ctx, &key).await.unwrap());
                }
            }
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK 3: CONCURRENT ACCESS PATTERNS
// =============================================================================

/// Benchmark concurrent reads (multi-threaded)
fn bench_concurrent_reads(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_reads");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    // Test with different thread counts
    for &thread_count in &[1, 2, 4, 8] {
        group.throughput(Throughput::Elements(
            (config.operation_count * thread_count) as u64,
        ));

        group.bench_with_input(
            BenchmarkId::new("InMemory", thread_count),
            &thread_count,
            |b, &thread_count| {
                let store = Arc::new(InMemoryKVStore::new());
                let value = test_value(config.small_value_size);

                // Pre-populate data
                let ctx = test_ctx();
                rt.block_on(async {
                    for i in 0..config.operation_count {
                        let key = test_key("concurrent", i);
                        store.put(&ctx, &key, value.clone()).await.unwrap();
                    }
                });

                b.to_async(&rt).iter(|| {
                    let store = store.clone();

                    async move {
                        let mut handles = Vec::new();

                        for _ in 0..thread_count {
                            let store = store.clone();
                            let handle = tokio::spawn(async move {
                                for i in 0..config.operation_count {
                                    let key = test_key("concurrent", i);
                                    let ctx = test_ctx();
                                    let _ = black_box(store.get(&ctx, &key).await.unwrap());
                                }
                            });
                            handles.push(handle);
                        }

                        for handle in handles {
                            handle.await.unwrap();
                        }
                    }
                });
            },
        );
    }

    group.finish();
}

/// Benchmark mixed read/write workload
fn bench_mixed_workload(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_workload");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    // 80% reads, 20% writes (typical production ratio)
    group.throughput(Throughput::Elements(config.operation_count as u64));

    group.bench_function("InMemory_80r_20w", |b| {
        let store = Arc::new(InMemoryKVStore::new());
        let value = test_value(config.small_value_size);

        // Pre-populate some data
        let ctx = test_ctx();
        rt.block_on(async {
            for i in 0..config.operation_count / 2 {
                let key = test_key("mixed", i);
                store.put(&ctx, &key, value.clone()).await.unwrap();
            }
        });

        b.to_async(&rt).iter(|| {
            let store = store.clone();
            let value = value.clone();

            let ctx = test_ctx();
            async move {
                for i in 0..config.operation_count {
                    let key = test_key("mixed", i);

                    // 80% reads, 20% writes
                    if i % 5 == 0 {
                        // Write
                        store.put(&ctx, &key, value.clone()).await.unwrap();
                    } else {
                        // Read
                        let _ = black_box(store.get(&ctx, &key).await);
                    }
                }
            }
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK 4: KEY PATTERNS (PREFIX SCANS, LIST OPERATIONS)
// =============================================================================

/// Benchmark prefix scan (list with prefix)
fn bench_prefix_scan(c: &mut Criterion) {
    let mut group = c.benchmark_group("prefix_scan");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    // Test different result set sizes
    for &count in &[10, 100, 1000] {
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::new("InMemory", count), &count, |b, &count| {
            let store = Arc::new(InMemoryKVStore::new());
            let value = test_value(config.small_value_size);

            // Pre-populate with multiple prefixes
            let ctx = test_ctx();
            rt.block_on(async {
                for i in 0..count {
                    let key = format!("prefix-a:{:08}", i);
                    store.put(&ctx, &key, value.clone()).await.unwrap();

                    let key = format!("prefix-b:{:08}", i);
                    store.put(&ctx, &key, value.clone()).await.unwrap();
                }
            });

            b.to_async(&rt).iter(|| {
                let store = store.clone();
                let ctx = test_ctx();

                async move {
                    let keys = store.list(&ctx, "prefix-a:").await.unwrap();
                    black_box(keys);
                }
            });
        });
    }

    group.finish();
}

// =============================================================================
// BENCHMARK 5: TTL OPERATIONS
// =============================================================================

/// Benchmark put with TTL
fn bench_put_with_ttl(c: &mut Criterion) {
    let mut group = c.benchmark_group("put_with_ttl");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.bench_function("InMemory", |b| {
        let store = Arc::new(InMemoryKVStore::new());
        let value = test_value(config.small_value_size);
        let ttl = Duration::from_secs(60);

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let store = store.clone();
            let value = value.clone();
            let key = test_key("ttl", counter);
            counter += 1;

            async move {
                let ctx = test_ctx();
                store.put_with_ttl(&ctx, &key, value, ttl).await.unwrap();
            }
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION GROUPS
// =============================================================================

criterion_group!(
    single_operations,
    bench_single_put,
    bench_single_get,
    bench_single_delete,
    bench_single_exists
);

criterion_group!(batch_operations, bench_bulk_put, bench_bulk_get);

criterion_group!(
    concurrent_access,
    bench_concurrent_reads,
    bench_mixed_workload
);

criterion_group!(key_patterns, bench_prefix_scan);

criterion_group!(ttl_operations, bench_put_with_ttl);

criterion_main!(
    single_operations,
    batch_operations,
    concurrent_access,
    key_patterns,
    ttl_operations
);
