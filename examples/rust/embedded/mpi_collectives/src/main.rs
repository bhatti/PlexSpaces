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
use std::sync::Arc;

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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     MPI Collectives with TupleSpace + ProcessGroupRegistry     ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    println!("TupleSpace: scatter, gather, reduce, barrier (real APIs)");
    println!("ProcessGroupRegistry: broadcast, all-reduce");
    println!();

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
    println!("Step 1: BROADCAST via ProcessGroupRegistry");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: registry.publish_to_group(&ctx, group, None, data)");
    println!();

    registry.create_group(&ctx, "workers").await?;
    let workers = vec![
        ActorId::from("worker-0@mpi-node"),
        ActorId::from("worker-1@mpi-node"),
        ActorId::from("worker-2@mpi-node"),
        ActorId::from("worker-3@mpi-node"),
    ];
    for worker in &workers {
        registry.join_group(&ctx, "workers", worker, vec![]).await?;
        println!("  {} joined 'workers' group", worker);
    }

    let config_data = b"learning_rate=0.01,epochs=100".to_vec();
    let recipients = registry.publish_to_group(&ctx, "workers", None, config_data).await?;
    println!();
    println!("  Broadcast config to {} workers:", recipients.len());
    for r in &recipients {
        println!("    -> {} received config", r);
    }
    println!();

    // =========================================================================
    // Step 2: SCATTER via TupleSpace (coordinator writes tasks; workers take)
    // =========================================================================
    println!("Step 2: SCATTER via TupleSpace (write tasks, take by workers)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.write(tuple); tuplespace.take(pattern)");
    println!();

    let full_data: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let num_workers = workers.len();
    let chunk_size = full_data.len() / num_workers;

    println!("  Full data: {:?}", full_data);
    println!("  Coordinator writes one tuple per worker (scatter_task, worker_id, chunk_index, payload):");

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
        println!("    write([\"scatter_task\", \"{}\", {}, <chunk>])", worker, i);
    }
    println!();

    // Simulate workers: each takes one scatter_task, processes, writes gather_result
    println!("  Workers take their task (take pattern), process, write gather_result:");
    for _ in 0..num_workers {
        let taken = tuplespace.take(pattern_scatter_task()).await?;
        if let Some(t) = taken {
            let worker_id = match t.fields().get(1) {
                Some(TupleField::String(s)) => s.clone(),
                _ => String::new(),
            };
            let payload = t.fields().get(3).map(tuple_field_as_binary).unwrap_or_default();
            let chunk: Vec<f64> = serde_json::from_slice(&payload).unwrap_or_default();
            let local_sum: f64 = chunk.iter().sum();
            let result_bytes = serde_json::to_vec(&local_sum)?;
            tuplespace
                .write(Tuple::new(vec![
                    TupleField::String("gather_result".to_string()),
                    TupleField::String(worker_id),
                    TupleField::Binary(result_bytes),
                ]))
                .await?;
            println!("    take(scatter_task) -> process -> write(gather_result, {})", local_sum);
        }
    }
    println!();

    // =========================================================================
    // Step 3: GATHER via TupleSpace (read_all results)
    // =========================================================================
    println!("Step 3: GATHER via TupleSpace (read_all gather_result)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.read_all(pattern)");
    println!();

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
    println!("  Gathered partial sums (sorted): {:?}", gathered);
    println!();

    // =========================================================================
    // Step 4: REDUCE via TupleSpace (workers write partial_sum; coordinator read_all and sum)
    // =========================================================================
    println!("Step 4: REDUCE via TupleSpace (write partial_sum; read_all and aggregate)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.write(partial_sum); tuplespace.read_all(pattern)");
    println!();

    let partial_sums: Vec<f64> = vec![3.0, 7.0, 11.0, 15.0];
    for (i, &sum) in partial_sums.iter().enumerate() {
        tuplespace
            .write(Tuple::new(vec![
                TupleField::String("partial_sum".to_string()),
                TupleField::String(format!("worker-{}", i)),
                TupleField::Float(plexspaces_tuplespace::OrderedFloat::new(sum)),
            ]))
            .await?;
        println!("    write([\"partial_sum\", \"worker-{}\", {}])", i, sum);
    }
    println!();

    let partial_tuples = tuplespace.read_all(pattern_partial_sum()).await?;
    let mut global_sum = 0.0;
    for t in &partial_tuples {
        if let Some(f) = t.fields().get(2) {
            global_sum += tuple_field_as_f64(f);
        }
    }
    println!("  Coordinator read_all(partial_sum) -> global sum = {}", global_sum);
    println!();

    // =========================================================================
    // Step 5: BARRIER via TupleSpace (register barrier; workers write and wait)
    // =========================================================================
    println!("Step 5: BARRIER via TupleSpace (barrier name + pattern; write then recv)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.barrier(name, pattern, count); write(tuple); rx.recv()");
    println!();

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
        println!("    worker-{} writes barrier tuple", i);
    }
    for (i, rx) in barrier_rxs.iter_mut().enumerate() {
        let _ = rx.recv().await;
        println!("    worker-{} barrier released", i);
    }
    println!("  All workers arrived - barrier released!");
    println!();

    // =========================================================================
    // Step 6: ALL_REDUCE = Reduce + Broadcast (result to all via ProcessGroup)
    // =========================================================================
    println!("Step 6: ALL_REDUCE = Reduce + Broadcast");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: Reduce (TupleSpace, above) + publish_to_group (ProcessGroupRegistry)");
    println!();

    let result_data = format!("{}", global_sum).into_bytes();
    let recipients = registry.publish_to_group(&ctx, "workers", None, result_data).await?;
    for r in &recipients {
        println!("     -> {} now has global_sum = {}", r, global_sum);
    }
    println!();

    // Cleanup
    registry.delete_group(&ctx, "workers").await?;

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("MPI Collectives Example Complete");
    println!();
    println!("PlexSpaces API Mapping (TupleSpace + ProcessGroupRegistry):");
    println!();
    println!("  ┌─────────────┬────────────────────────────────────────────┐");
    println!("  │ MPI         │ PlexSpaces API                             │");
    println!("  ├─────────────┼────────────────────────────────────────────┤");
    println!("  │ Broadcast   │ ProcessGroupRegistry::publish_to_group()   │");
    println!("  │ Scatter     │ TupleSpace::write(tasks); take(pattern)    │");
    println!("  │ Gather      │ TupleSpace::read_all(pattern)              │");
    println!("  │ Reduce      │ TupleSpace::write(partial); read_all; sum  │");
    println!("  │ Barrier     │ TupleSpace::barrier(); write(); recv()     │");
    println!("  │ AllReduce   │ Reduce (TupleSpace) + Broadcast (PG)       │");
    println!("  └─────────────┴────────────────────────────────────────────┘");
    println!();
    println!("Use Cases: Distributed ML (AllReduce), MapReduce, Monte Carlo, consensus");
    println!();

    Ok(())
}
