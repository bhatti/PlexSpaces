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

//! Performance benchmarks for WASM Runtime
//!
//! ## Purpose
//! Measures performance characteristics of WASM actor execution to validate
//! the < 10% overhead target (CLAUDE.md Phase 6 success criteria).
//!
//! ## Benchmarks
//! 1. **Module Operations**: Compilation, caching, deployment
//! 2. **Actor Operations**: Instantiation, message processing
//! 3. **Throughput**: Concurrent actor instantiation, message throughput
//! 4. **Resource Limits**: Memory limits, execution timeouts, cleanup
//! 5. **Deployment**: End-to-end deployment latency
//!
//! ## Running Benchmarks
//! ```bash
//! # Run all benchmarks
//! cargo bench -p plexspaces-wasm-runtime
//!
//! # Run specific benchmark group
//! cargo bench -p plexspaces-wasm-runtime -- module_operations
//!
//! # Save baseline for comparison
//! cargo bench -p plexspaces-wasm-runtime -- --save-baseline main
//!
//! # Compare against baseline
//! cargo bench -p plexspaces-wasm-runtime -- --baseline main
//!
//! # Generate detailed report
//! cargo bench -p plexspaces-wasm-runtime -- --verbose
//! ```
//!
//! ## Performance Targets
//! Based on PlexSpaces performance goals (CLAUDE.md):
//!
//! ### Module Operations
//! - Module compilation: < 100ms for small modules (< 1MB)
//! - Module cache hit: < 1μs (microsecond)
//! - Module cache miss: < 100ms (compile + cache)
//!
//! ### Actor Operations
//! - Actor instantiation: < 10ms from cached module
//! - Message processing: < 10% overhead vs native
//! - State serialization: < 1ms for small state (< 10KB)
//!
//! ### Throughput
//! - Concurrent instantiation: > 100 actors/sec
//! - Message throughput: > 10K msg/sec per actor
//! - Concurrent actors: > 1K active actors
//!
//! ### Resource Limits
//! - Memory limit enforcement: < 100μs overhead
//! - Timeout enforcement: < 100μs overhead
//! - Resource cleanup: < 10ms per actor
//!
//! ## Interpreting Results
//!
//! Criterion outputs three key metrics:
//! - **time**: Mean execution time (target: WASM < native * 1.10)
//! - **thrpt**: Throughput (operations/sec)
//! - **change**: % change from baseline (< ±5% = stable)
//!
//! ### Success Criteria
//! - ✅ WASM actor instantiation < 10ms
//! - ✅ WASM message processing overhead < 10% vs native
//! - ✅ Module compilation < 100ms for typical modules
//! - ✅ Concurrent throughput > 100 actors/sec
//!
//! ### Red Flags
//! - ❌ > 15% overhead vs native (exceeds target)
//! - ❌ > 20% regression from baseline (performance degradation)
//! - ❌ < 50 actors/sec (insufficient throughput)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use plexspaces_wasm_runtime::deployment_service::WasmDeploymentService;
use plexspaces_wasm_runtime::{WasmConfig, WasmRuntime};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Runtime;

// =============================================================================
// TEST WASM MODULES
// =============================================================================

/// Simple WASM module: (module (func (export "test") (result i32) i32.const 42))
const SIMPLE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
    0x01, 0x00, 0x00, 0x00, // Version: 1
    0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section: () -> i32
    0x03, 0x02, 0x01, 0x00, // Function section: 1 function
    0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // Export "test"
    0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code: i32.const 42, end
];

/// Medium complexity WASM module with multiple functions
/// (module
///   (func (export "add") (param i32 i32) (result i32)
///     local.get 0
///     local.get 1
///     i32.add)
///   (func (export "mul") (param i32 i32) (result i32)
///     local.get 0
///     local.get 1
///     i32.mul))
const MEDIUM_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, // Magic + version
    0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01, 0x7f, // Type: (i32, i32) -> i32
    0x03, 0x03, 0x02, 0x00, 0x00, // Function section: 2 functions
    0x07, 0x0b, 0x02, 0x03, 0x61, 0x64, 0x64, 0x00, 0x00, 0x03, 0x6d, 0x75, 0x6c, 0x00,
    0x01, // Exports: "add", "mul"
    0x0a, 0x11, 0x02, 0x07, 0x00, 0x20, 0x00, 0x20, 0x01, 0x6a, 0x0b, 0x07, 0x00, 0x20, 0x00,
    0x20, 0x01, 0x6c, 0x0b, // Code: add/mul implementations
];

/// Large WASM module (simulated) - repeat SIMPLE_WASM multiple times
fn large_wasm_module() -> Vec<u8> {
    let mut large = Vec::new();
    // Repeat simple module pattern to create ~100KB module
    for _ in 0..5000 {
        large.extend_from_slice(SIMPLE_WASM);
    }
    large
}

// =============================================================================
// BENCHMARK HELPERS
// =============================================================================

/// Benchmark configuration
struct BenchConfig {
    /// Number of operations for throughput tests
    operation_count: usize,
    /// Number of concurrent actors for scaling tests
    concurrent_actors: usize,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            operation_count: 100,
            concurrent_actors: 10,
        }
    }
}

// =============================================================================
// BENCHMARK 1: MODULE OPERATIONS (COMPILE, CACHE, DEPLOY)
// =============================================================================

/// Benchmark module compilation (cold start)
fn bench_module_compilation(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_compilation");
    let rt = Runtime::new().unwrap();

    // Benchmark different module sizes
    for (name, wasm_bytes) in &[
        ("simple", SIMPLE_WASM),
        ("medium", MEDIUM_WASM),
        ("large", &large_wasm_module()[..]),
    ] {
        group.throughput(Throughput::Bytes(wasm_bytes.len() as u64));

        group.bench_with_input(BenchmarkId::new("compile", name), wasm_bytes, |b, wasm| {
            b.to_async(&rt).iter(|| async {
                let runtime = WasmRuntime::new().await.unwrap();
                runtime
                    .load_module(black_box("test"), black_box("1.0.0"), black_box(wasm))
                    .await
                    .unwrap();
            });
        });
    }

    group.finish();
}

/// Benchmark module cache hit (warm start)
fn bench_module_cache_hit(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_cache_hit");
    let rt = Runtime::new().unwrap();

    group.bench_function("cache_hit", |b| {
        // Pre-compile and cache module
        let runtime = rt.block_on(async {
            let runtime = Arc::new(WasmRuntime::new().await.unwrap());
            runtime
                .load_module("test", "1.0.0", SIMPLE_WASM)
                .await
                .unwrap();
            runtime
        });

        b.to_async(&rt).iter(|| {
            let runtime = Arc::clone(&runtime);
            async move {
                // This should hit cache
                runtime
                    .load_module(black_box("test"), black_box("1.0.0"), black_box(SIMPLE_WASM))
                    .await
                    .unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark end-to-end deployment (network transfer simulation + compile + cache)
fn bench_module_deployment(c: &mut Criterion) {
    let mut group = c.benchmark_group("module_deployment");
    let rt = Runtime::new().unwrap();

    group.bench_function("deploy_simple", |b| {
        let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });
        let service = Arc::new(WasmDeploymentService::new(runtime.clone()));

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let service = Arc::clone(&service);
            let name = format!("module-{}", counter);
            counter += 1;

            async move {
                service
                    .deploy_module(
                        black_box(&name),
                        black_box("1.0.0"),
                        black_box(SIMPLE_WASM),
                    )
                    .await
                    .unwrap();
            }
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK 2: ACTOR OPERATIONS (INSTANTIATE, MESSAGE PROCESSING)
// =============================================================================

/// Benchmark actor instantiation from cached module
fn bench_actor_instantiation(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_instantiation");
    let rt = Runtime::new().unwrap();

    group.bench_function("instantiate_from_cache", |b| {
        // Setup: Pre-deploy module
        let (runtime, _service) = rt.block_on(async {
            let runtime = Arc::new(WasmRuntime::new().await.unwrap());
            let service = Arc::new(WasmDeploymentService::new(runtime.clone()));
            service
                .deploy_module("test-actor", "1.0.0", SIMPLE_WASM)
                .await
                .unwrap();
            (runtime, service)
        });

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let runtime = Arc::clone(&runtime);
            let actor_id = format!("actor-{}", counter);
            counter += 1;

            async move {
                let module = runtime
                    .resolve_module(black_box("test-actor@1.0.0"))
                    .await
                    .unwrap();
                runtime
                    .instantiate(
                        black_box(module),
                        black_box(actor_id),
                        black_box(&[]),
                        black_box(WasmConfig::default()),
                    )
                    .await
                    .unwrap();
            }
        });
    });

    group.finish();
}

/// Benchmark actor instantiation with different state sizes
fn bench_actor_instantiation_with_state(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_instantiation_with_state");
    let rt = Runtime::new().unwrap();

    let state_sizes = vec![
        ("empty", vec![]),
        ("small_1kb", vec![0u8; 1024]),
        ("medium_10kb", vec![0u8; 10 * 1024]),
        ("large_100kb", vec![0u8; 100 * 1024]),
    ];

    for (name, state) in state_sizes {
        group.throughput(Throughput::Bytes(state.len() as u64));

        group.bench_with_input(BenchmarkId::new("with_state", name), &state, |b, state| {
            // Setup: Pre-deploy module
            let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });

            rt.block_on(async {
                let service = WasmDeploymentService::new(runtime.clone());
                service
                    .deploy_module("test-actor", "1.0.0", SIMPLE_WASM)
                    .await
                    .unwrap();
            });

            let mut counter = 0;
            b.to_async(&rt).iter(|| {
                let runtime = Arc::clone(&runtime);
                let actor_id = format!("actor-{}", counter);
                let state = state.clone();
                counter += 1;

                async move {
                    let module = runtime.resolve_module("test-actor@1.0.0").await.unwrap();
                    runtime
                        .instantiate(
                            black_box(module),
                            black_box(actor_id),
                            black_box(&state),
                            black_box(WasmConfig::default()),
                        )
                        .await
                        .unwrap();
                }
            });
        });
    }

    group.finish();
}

// =============================================================================
// BENCHMARK 3: THROUGHPUT (CONCURRENT OPERATIONS)
// =============================================================================

/// Benchmark concurrent actor instantiation
fn bench_concurrent_instantiation(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_instantiation");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.throughput(Throughput::Elements(config.concurrent_actors as u64));

    group.bench_function("concurrent_actors", |b| {
        // Setup: Pre-deploy module
        let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });

        rt.block_on(async {
            let service = WasmDeploymentService::new(runtime.clone());
            service
                .deploy_module("test-actor", "1.0.0", SIMPLE_WASM)
                .await
                .unwrap();
        });

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let runtime = Arc::clone(&runtime);
            let base_counter = counter;
            counter += config.concurrent_actors;

            async move {
                let mut handles = vec![];
                for i in 0..config.concurrent_actors {
                    let runtime = Arc::clone(&runtime);
                    let actor_id = format!("actor-{}", base_counter + i);

                    handles.push(tokio::spawn(async move {
                        let module = runtime.resolve_module("test-actor@1.0.0").await.unwrap();
                        runtime
                            .instantiate(module, actor_id, &[], WasmConfig::default())
                            .await
                            .unwrap();
                    }));
                }

                for handle in handles {
                    handle.await.unwrap();
                }
            }
        });
    });

    group.finish();
}

/// Benchmark module deployment throughput
fn bench_deployment_throughput(c: &mut Criterion) {
    let mut group = c.benchmark_group("deployment_throughput");
    let rt = Runtime::new().unwrap();
    let config = BenchConfig::default();

    group.throughput(Throughput::Elements(config.operation_count as u64));

    group.bench_function("bulk_deployment", |b| {
        let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });
        let service = Arc::new(WasmDeploymentService::new(runtime.clone()));

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let service = Arc::clone(&service);
            let base_counter = counter;
            counter += config.operation_count;

            async move {
                for i in 0..config.operation_count {
                    let name = format!("module-{}", base_counter + i);
                    service
                        .deploy_module(&name, "1.0.0", black_box(SIMPLE_WASM))
                        .await
                        .unwrap();
                }
            }
        });
    });

    group.finish();
}

// =============================================================================
// BENCHMARK 4: RESOURCE LIMITS (MEMORY, TIMEOUT OVERHEAD)
// =============================================================================

/// Benchmark memory limit enforcement overhead
fn bench_memory_limits(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_limits");
    let rt = Runtime::new().unwrap();

    let memory_limits = vec![
        ("no_limit", None),
        ("limit_1mb", Some(1024 * 1024)),
        ("limit_10mb", Some(10 * 1024 * 1024)),
        ("limit_100mb", Some(100 * 1024 * 1024)),
    ];

    for (name, limit) in memory_limits {
        group.bench_with_input(BenchmarkId::new("with_limit", name), &limit, |b, &limit| {
            // Setup: Pre-deploy module
            let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });

            rt.block_on(async {
                let service = WasmDeploymentService::new(runtime.clone());
                service
                    .deploy_module("test-actor", "1.0.0", SIMPLE_WASM)
                    .await
                    .unwrap();
            });

            let mut counter = 0;
            b.to_async(&rt).iter(|| {
                let runtime = Arc::clone(&runtime);
                let actor_id = format!("actor-{}", counter);
                counter += 1;

                async move {
                    let mut config = WasmConfig::default();
                    if let Some(limit) = limit {
                        config.limits.max_memory_bytes = limit;
                    }

                    let module = runtime.resolve_module("test-actor@1.0.0").await.unwrap();
                    runtime
                        .instantiate(black_box(module), black_box(actor_id), &[], black_box(config))
                        .await
                        .unwrap();
                }
            });
        });
    }

    group.finish();
}

/// Benchmark execution timeout enforcement overhead
fn bench_execution_timeouts(c: &mut Criterion) {
    let mut group = c.benchmark_group("execution_timeouts");
    let rt = Runtime::new().unwrap();

    let timeout_configs = vec![
        ("no_timeout", None),
        ("timeout_100ms", Some(Duration::from_millis(100))),
        ("timeout_1s", Some(Duration::from_secs(1))),
        ("timeout_10s", Some(Duration::from_secs(10))),
    ];

    for (name, timeout) in timeout_configs {
        group.bench_with_input(
            BenchmarkId::new("with_timeout", name),
            &timeout,
            |b, &timeout| {
                // Setup: Pre-deploy module
                let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });

                rt.block_on(async {
                    let service = WasmDeploymentService::new(runtime.clone());
                    service
                        .deploy_module("test-actor", "1.0.0", SIMPLE_WASM)
                        .await
                        .unwrap();
                });

                let mut counter = 0;
                b.to_async(&rt).iter(|| {
                    let runtime = Arc::clone(&runtime);
                    let actor_id = format!("actor-{}", counter);
                    counter += 1;

                    async move {
                        let mut config = WasmConfig::default();
                        if let Some(timeout) = timeout {
                            config.limits.max_execution_time = Some(timeout);
                        }

                        let module = runtime.resolve_module("test-actor@1.0.0").await.unwrap();
                        runtime
                            .instantiate(
                                black_box(module),
                                black_box(actor_id),
                                &[],
                                black_box(config),
                            )
                            .await
                            .unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

// =============================================================================
// BENCHMARK 5: END-TO-END DEPLOYMENT LATENCY
// =============================================================================

/// Benchmark complete deployment workflow (deploy → instantiate)
fn bench_e2e_deployment(c: &mut Criterion) {
    let mut group = c.benchmark_group("e2e_deployment");
    let rt = Runtime::new().unwrap();

    group.bench_function("deploy_and_instantiate", |b| {
        let runtime = rt.block_on(async { Arc::new(WasmRuntime::new().await.unwrap()) });
        let service = Arc::new(WasmDeploymentService::new(runtime.clone()));

        let mut counter = 0;
        b.to_async(&rt).iter(|| {
            let service = Arc::clone(&service);
            let runtime = Arc::clone(&runtime);
            let name = format!("module-{}", counter);
            let actor_id = format!("actor-{}", counter);
            counter += 1;

            async move {
                // Deploy module
                let _hash = service
                    .deploy_module(black_box(&name), black_box("1.0.0"), black_box(SIMPLE_WASM))
                    .await
                    .unwrap();

                // Instantiate actor
                let module_ref = format!("{}@1.0.0", name);
                let module = runtime.resolve_module(&module_ref).await.unwrap();
                runtime
                    .instantiate(
                        black_box(module),
                        black_box(actor_id),
                        black_box(&[]),
                        black_box(WasmConfig::default()),
                    )
                    .await
                    .unwrap();
            }
        });
    });

    group.finish();
}

// =============================================================================
// CRITERION CONFIGURATION
// =============================================================================

criterion_group! {
    name = benches;
    config = Criterion::default()
        .measurement_time(Duration::from_secs(10))
        .sample_size(100)
        .warm_up_time(Duration::from_secs(3));
    targets =
        bench_module_compilation,
        bench_module_cache_hit,
        bench_module_deployment,
        bench_actor_instantiation,
        bench_actor_instantiation_with_state,
        bench_concurrent_instantiation,
        bench_deployment_throughput,
        bench_memory_limits,
        bench_execution_timeouts,
        bench_e2e_deployment,
}

criterion_main!(benches);
