// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// MPI Collectives Example
//
// Demonstrates scatter/gather and map/reduce-style patterns using PlexSpaces:
// - Broadcast / AllReduce: ProcessGroupRegistry::publish_to_group()
// - Scatter / Gather / Reduce / Barrier: TupleSpace (real write/read/take/barrier)
//
// Multi-tenancy: Uses RequestContext with explicit tenant/namespace (no internal()).
// In production, use RequestContext::from_auth(tenant_from_jwt, namespace_from_request, ...)
// or extract from gRPC/HTTP (e.g. request_context_from_grpc_request). Auth token evaluation
// can be added when JWT/mTLS is enabled.
//
// Use Case: Distributed computing, MPI/Hadoop-style collective operations

use plexspaces_keyvalue::SqliteKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::{ActorId, RequestContext};
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField, tuple};
use plexspaces_node::CoordinationComputeTracker;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;
use anyhow::Result;

// =============================================================================
// Helpers: build pattern for TupleSpace (pattern! uses PatternField::from; we build manually for wildcards)
// =============================================================================

fn pattern_scatter_task() -> Pattern {
    Pattern::new(vec![
        PatternField::Exact(TupleField::String("scatter_task".to_string())),
        PatternField::Wildcard,
        PatternField::Wildcard,
        PatternField::Wildcard,
    ])
}

fn pattern_gather_result() -> Pattern {
    Pattern::new(vec![
        PatternField::Exact(TupleField::String("gather_result".to_string())),
        PatternField::Wildcard,
        PatternField::Wildcard,
    ])
}

fn pattern_partial_sum() -> Pattern {
    Pattern::new(vec![
        PatternField::Exact(TupleField::String("partial_sum".to_string())),
        PatternField::Wildcard,
        PatternField::Wildcard,
    ])
}

/// Extract f64 from tuple field (for partial_sum and similar).
fn tuple_field_as_f64(f: &TupleField) -> f64 {
    match f {
        TupleField::Float(o) => o.get(),
        _ => 0.0,
    }
}

/// Extract Vec<u8> from tuple field (for scatter/gather payloads).
fn tuple_field_as_binary(f: &TupleField) -> Vec<u8> {
    match f {
        TupleField::Binary(v) => v.clone(),
        _ => vec![],
    }
}

// =============================================================================
// Main - Demonstrates MPI collectives using TupleSpace + ProcessGroupRegistry
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing - ensure INFO level for metrics output
    // Use try_init() to avoid panic if already initialized (e.g., in tests)
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║     MPI Collectives with TupleSpace + ProcessGroupRegistry     ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    info!("TupleSpace: scatter, gather, reduce, barrier (real APIs)");
    info!("ProcessGroupRegistry: broadcast, all-reduce");
    println!();

    // Configuration: Use non-trivial data sizes (run for 2+ seconds)
    let num_workers = 8;
    let data_size = 100_000; // 100k elements per worker = 800k total
    let full_data_size = num_workers * data_size;

    info!("Configuration:");
    info!("  Workers: {}", num_workers);
    info!("  Data size per worker: {} elements", data_size);
    info!("  Total data size: {} elements", full_data_size);
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("mpi-collectives".to_string());
    let total_start = Instant::now();

    // -------------------------------------------------------------------------
    // RequestContext: explicit tenant/namespace for multi-tenancy.
    // In production: use RequestContext::from_auth(tenant_from_jwt, namespace, ...)
    // or extract from gRPC/HTTP; add auth token when JWT/mTLS is enabled.
    // -------------------------------------------------------------------------
    let tenant_id = "mpi-tenant";
    let namespace = "collectives";
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Setup: SQLite :memory: for process groups; TupleSpace with same tenant/namespace
    let kv_store = Arc::new(SqliteKVStore::new(":memory:").await?);
    let registry = ProcessGroupRegistry::new("mpi-node", kv_store);
    let tuplespace = TupleSpace::with_tenant_namespace(tenant_id, namespace);

    // =========================================================================
    // Step 1: BROADCAST via ProcessGroupRegistry
    // =========================================================================
    info!("Step 1: BROADCAST via ProcessGroupRegistry");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: registry.publish_to_group(&ctx, group, None, data)");
    println!();

    metrics_tracker.start_coordinate();
    let broadcast_start = Instant::now();

    registry.create_group(&ctx, "workers").await?;
    let workers: Vec<ActorId> = (0..num_workers)
        .map(|i| ActorId::from(format!("worker-{}@mpi-node", i)))
        .collect();
    for worker in &workers {
        registry.join_group(&ctx, "workers", worker, vec![]).await?;
        info!("  {} joined 'workers' group", worker);
    }

    let config_data = b"learning_rate=0.01,epochs=100".to_vec();
    let recipients = registry.publish_to_group(&ctx, "workers", None, config_data).await?;
    metrics_tracker.increment_message();
    
    let broadcast_time = broadcast_start.elapsed();
    metrics_tracker.end_coordinate();
    
    println!();
    info!("  Broadcast config to {} workers:", recipients.len());
    for r in &recipients {
        info!("    -> {} received config", r);
    }
    info!("  Broadcast time: {:.2}ms", broadcast_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 2: SCATTER via TupleSpace (coordinator writes tasks; workers take)
    // =========================================================================
    info!("Step 2: SCATTER via TupleSpace (write tasks, take by workers)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: tuplespace.write(tuple); tuplespace.take(pattern)");
    println!();

    metrics_tracker.start_coordinate();
    let scatter_start = Instant::now();

    // Generate non-trivial data (100k elements per worker)
    let mut full_data: Vec<f64> = Vec::with_capacity(full_data_size);
    for i in 0..full_data_size {
        full_data.push((i as f64) * 0.001);
    }
    let chunk_size = full_data.len() / num_workers;

    info!("  Full data: {} elements (first 10: {:?})", full_data.len(), &full_data[0..10.min(full_data.len())]);
    info!("  Coordinator writes one tuple per worker (scatter_task, worker_id, chunk_index, payload):");

    for (i, worker) in workers.iter().enumerate() {
        let start = i * chunk_size;
        let end = if i == num_workers - 1 {
            full_data.len()
        } else {
            start + chunk_size
        };
        let chunk = &full_data[start..end];
        let payload = serde_json::to_vec(chunk)?;
        let t = Tuple::new(vec![
            TupleField::String("scatter_task".to_string()),
            TupleField::String(worker.to_string()),
            TupleField::Integer(i as i64),
            TupleField::Binary(payload),
        ]);
        tuplespace.write(t).await?;
        metrics_tracker.increment_message();
        if i < 3 || i == num_workers - 1 {
            info!("    write([\"scatter_task\", \"{}\", {}, <{} elements>])", worker, i, chunk.len());
        }
    }
    
    let scatter_time = scatter_start.elapsed();
    metrics_tracker.end_coordinate();
    info!("  Scatter time: {:.2}ms", scatter_time.as_secs_f64() * 1000.0);
    println!();

    // Simulate workers: each takes one scatter_task, processes, writes gather_result
    info!("  Workers take their task (take pattern), process, write gather_result:");
    metrics_tracker.start_compute();
    let compute_start = Instant::now();
    
    for _ in 0..num_workers {
        metrics_tracker.start_coordinate();
        let taken = tuplespace.take(pattern_scatter_task()).await?;
        metrics_tracker.end_coordinate();
        
        if let Some(t) = taken {
            let worker_id = match t.fields().get(1) {
                Some(TupleField::String(s)) => s.clone(),
                _ => String::new(),
            };
            let payload = t.fields().get(3).map(tuple_field_as_binary).unwrap_or_default();
            let chunk: Vec<f64> = serde_json::from_slice(&payload).unwrap_or_default();
            
            // Actual computation: sum the chunk
            let local_sum: f64 = chunk.iter().sum();
            
            let result_bytes = serde_json::to_vec(&local_sum)?;
            
            metrics_tracker.start_coordinate();
            tuplespace
                .write(Tuple::new(vec![
                    TupleField::String("gather_result".to_string()),
                    TupleField::String(worker_id),
                    TupleField::Binary(result_bytes),
                ]))
                .await?;
            metrics_tracker.increment_message();
            metrics_tracker.end_coordinate();
            
            info!("    take(scatter_task) -> process -> write(gather_result, {:.2})", local_sum);
        }
    }
    let compute_time = compute_start.elapsed();
    metrics_tracker.end_compute();
    info!("  Compute time: {:.2}ms", compute_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 3: GATHER via TupleSpace (read_all results)
    // =========================================================================
    info!("Step 3: GATHER via TupleSpace (read_all gather_result)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: tuplespace.read_all(pattern)");
    println!();

    metrics_tracker.start_coordinate();
    let gather_start = Instant::now();
    
    let results = tuplespace.read_all(pattern_gather_result()).await?;
    let mut gathered: Vec<f64> = Vec::new();
    for t in &results {
        if let Some(TupleField::Binary(b)) = t.fields().get(2) {
            if let Ok(v) = serde_json::from_slice::<f64>(b) {
                gathered.push(v);
            }
        }
    }
    gathered.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let gather_time = gather_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Gathered partial sums (first 5): {:?}", &gathered[0..5.min(gathered.len())]);
    info!("  Total gathered: {} results", gathered.len());
    info!("  Gather time: {:.2}ms", gather_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 4: REDUCE via TupleSpace (workers write partial_sum; coordinator read_all and sum)
    // =========================================================================
    info!("Step 4: REDUCE via TupleSpace (write partial_sum; read_all and aggregate)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: tuplespace.write(partial_sum); tuplespace.read_all(pattern)");
    println!();

    metrics_tracker.start_coordinate();
    let reduce_start = Instant::now();
    
    // Use gathered results as partial sums
    for (i, &sum) in gathered.iter().enumerate() {
        tuplespace
            .write(Tuple::new(vec![
                TupleField::String("partial_sum".to_string()),
                TupleField::String(format!("worker-{}", i)),
                TupleField::Float(plexspaces_tuplespace::OrderedFloat::new(sum)),
            ]))
            .await?;
        metrics_tracker.increment_message();
        if i < 3 || i == gathered.len() - 1 {
            info!("    write([\"partial_sum\", \"worker-{}\", {:.2}])", i, sum);
        }
    }
    println!();

    let partial_tuples = tuplespace.read_all(pattern_partial_sum()).await?;
    let mut global_sum = 0.0;
    for t in &partial_tuples {
        if let Some(f) = t.fields().get(2) {
            global_sum += tuple_field_as_f64(f);
        }
    }
    
    let reduce_time = reduce_start.elapsed();
    metrics_tracker.end_coordinate();
    info!("  Coordinator read_all(partial_sum) -> global sum = {:.2}", global_sum);
    info!("  Reduce time: {:.2}ms", reduce_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 5: BARRIER via TupleSpace (register barrier; workers write and wait)
    // =========================================================================
    info!("Step 5: BARRIER via TupleSpace (barrier name + pattern; write then recv)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: tuplespace.barrier(name, pattern, count); write(tuple); rx.recv()");
    println!();

    metrics_tracker.start_coordinate();
    let barrier_start = Instant::now();
    metrics_tracker.increment_barrier();
    
    let barrier_id = "iteration_1";
    let barrier_pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("barrier".to_string())),
        PatternField::Exact(TupleField::String(barrier_id.to_string())),
    ]);

    let mut barrier_rxs = Vec::new();
    for _ in 0..num_workers {
        let rx = tuplespace
            .barrier(barrier_id.to_string(), barrier_pattern.clone(), num_workers)
            .await;
        barrier_rxs.push(rx);
    }
    for i in 0..num_workers {
        tuplespace
            .write(tuple!("barrier", barrier_id))
            .await?;
        metrics_tracker.increment_message();
        if i < 3 || i == num_workers - 1 {
            info!("    worker-{} writes barrier tuple", i);
        }
    }
    for (i, rx) in barrier_rxs.iter_mut().enumerate() {
        let _ = rx.recv().await;
        if i < 3 || i == num_workers - 1 {
            info!("    worker-{} barrier released", i);
        }
    }
    
    let barrier_time = barrier_start.elapsed();
    metrics_tracker.end_coordinate();
    info!("  All workers arrived - barrier released!");
    info!("  Barrier time: {:.2}ms", barrier_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 6: ALL_REDUCE = Reduce + Broadcast (result to all via ProcessGroup)
    // =========================================================================
    info!("Step 6: ALL_REDUCE = Reduce + Broadcast");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("  API: Reduce (TupleSpace, above) + publish_to_group (ProcessGroupRegistry)");
    println!();

    metrics_tracker.start_coordinate();
    let allreduce_start = Instant::now();
    
    let result_data = format!("{}", global_sum).into_bytes();
    let recipients = registry.publish_to_group(&ctx, "workers", None, result_data).await?;
    metrics_tracker.increment_message();
    
    let allreduce_time = allreduce_start.elapsed();
    metrics_tracker.end_coordinate();
    
    for r in &recipients {
        info!("     -> {} now has global_sum = {:.2}", r, global_sum);
    }
    info!("  AllReduce time: {:.2}ms", allreduce_time.as_secs_f64() * 1000.0);
    println!();

    // Cleanup
    registry.delete_group(&ctx, "workers").await?;

    // Finalize metrics
    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();
    
    // Calculate benchmark metrics
    let total_ops = full_data_size; // Sum operations
    let total_time_secs = total_time.as_secs_f64();
    let ops_per_sec = if total_time_secs > 0.0 {
        total_ops as f64 / total_time_secs
    } else {
        0.0
    };
    let throughput_mb = if total_time_secs > 0.0 {
        (full_data_size * 8) as f64 / 1_000_000.0 / total_time_secs
    } else {
        0.0
    };
    
    // Print metrics prominently with clear coordination vs computation breakdown
    info!("\n{}", "=".repeat(80));
    info!("📊 PERFORMANCE METRICS & BENCHMARKS");
    info!("{}", "=".repeat(80));
    
    info!("\nProblem Size:");
    info!("  Total data: {} elements", full_data_size);
    info!("  Workers: {} ({} elements/worker)", num_workers, data_size);
    info!("  Total operations: {} (sum operations)", total_ops);
    
    info!("\n{}", "─".repeat(80));
    info!("⚡ LATENCY BREAKDOWN (Coordination vs Computation)");
    info!("{}", "─".repeat(80));
    info!("  Broadcast:  {:>12.2} ms (coordination)", broadcast_time.as_secs_f64() * 1000.0);
    info!("  Scatter:    {:>12.2} ms (coordination)", scatter_time.as_secs_f64() * 1000.0);
    info!("  Compute:     {:>12.2} ms (computation)", compute_time.as_secs_f64() * 1000.0);
    info!("  Gather:      {:>12.2} ms (coordination)", gather_time.as_secs_f64() * 1000.0);
    info!("  Reduce:      {:>12.2} ms (coordination)", reduce_time.as_secs_f64() * 1000.0);
    info!("  Barrier:     {:>12.2} ms (coordination)", barrier_time.as_secs_f64() * 1000.0);
    info!("  AllReduce:   {:>12.2} ms (coordination)", allreduce_time.as_secs_f64() * 1000.0);
    info!("  {}", "─".repeat(30));
    info!("  Coordination: {:>10.2} ms (total)", metrics.coordinate_duration_ms as f64);
    info!("  Computation:  {:>10.2} ms (total)", metrics.compute_duration_ms as f64);
    info!("  Total Time:    {:>10.2} ms ({:.2} seconds)", 
        total_time.as_secs_f64() * 1000.0, total_time_secs);
    
    info!("\n{}", "─".repeat(80));
    info!("📈 COORDINATION vs COMPUTATION ANALYSIS");
    info!("{}", "─".repeat(80));
    info!("  Computation time:      {:>12.2} ms", metrics.compute_duration_ms as f64);
    info!("  Coordination time:    {:>12.2} ms", metrics.coordinate_duration_ms as f64);
    info!("  Granularity ratio:     {:>12.2}× (compute/coordinate)", metrics.granularity_ratio);
    info!("  Efficiency:            {:>12.2}% (compute/total)", metrics.efficiency * 100.0);
    info!("  Message count:         {:>12}", metrics.message_count);
    info!("  Barrier count:         {:>12}", metrics.barrier_count);
    
    // Cost analysis - show percentage breakdown
    let coord_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    let compute_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    info!("\n  Cost Breakdown:");
    info!("    Coordination overhead: {:>8.2}% of total time", coord_cost_pct);
    info!("    Computation:           {:>8.2}% of total time", compute_cost_pct);
    
    info!("\n{}", "─".repeat(80));
    info!("🚀 BENCHMARK METRICS");
    info!("{}", "─".repeat(80));
    info!("  Throughput:        {:>12.2} M ops/s", ops_per_sec / 1_000_000.0);
    info!("  Data throughput:   {:>12.2} MB/s", throughput_mb);
    info!("  Operations/sec:    {:>12.2} ops/s", ops_per_sec);
    
    info!("\n{}", "─".repeat(80));
    info!("💡 ANALYSIS & RECOMMENDATIONS");
    info!("{}", "─".repeat(80));
    if metrics.granularity_ratio < 10.0 {
        info!("  �⚠️  WARNING: Overhead too high! Consider:");
        info!("     - Larger problem size (more elements)");
        info!("     - Fewer workers (coarser granularity)");
        info!("     - Current ratio: {:.2}× (should be >= 10×)", metrics.granularity_ratio);
    } else if metrics.granularity_ratio < 100.0 {
        info!("  ✓  ACCEPTABLE: Reasonable granularity for this problem size");
        info!("     - Ratio: {:.2}× (good for small-medium problems)", metrics.granularity_ratio);
    } else {
        info!("  ✓  EXCELLENT: Good compute/coordinate ratio");
        info!("     - Ratio: {:.2}× (ideal for parallel efficiency)", metrics.granularity_ratio);
    }
    
    if coord_cost_pct > 20.0 {
        info!("  ⚠️  Coordination overhead is {:.1}% - consider larger problem size", coord_cost_pct);
    } else {
        info!("  ✓  Coordination overhead is {:.1}% - acceptable", coord_cost_pct);
    }
    
    info!("{}", "=".repeat(80));

    // =========================================================================
    // Summary
    // =========================================================================
    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("MPI Collectives Example Complete");
    println!();
    info!("PlexSpaces API Mapping (TupleSpace + ProcessGroupRegistry):");
    println!();
    info!("  ┌─────────────┬────────────────────────────────────────────┐");
    info!("  │ MPI         │ PlexSpaces API                             │");
    info!("  ├─────────────┼────────────────────────────────────────────┤");
    info!("  │ Broadcast   │ ProcessGroupRegistry::publish_to_group()   │");
    info!("  │ Scatter     │ TupleSpace::write(tasks); take(pattern)    │");
    info!("  │ Gather      │ TupleSpace::read_all(pattern)              │");
    info!("  │ Reduce      │ TupleSpace::write(partial); read_all; sum  │");
    info!("  │ Barrier     │ TupleSpace::barrier(); write(); recv()     │");
    info!("  │ AllReduce   │ Reduce (TupleSpace) + Broadcast (PG)       │");
    info!("  └─────────────┴────────────────────────────────────────────┘");
    println!();
    info!("Use Cases: Distributed ML (AllReduce), MapReduce, Monte Carlo, consensus");
    println!();

    Ok(())
}
