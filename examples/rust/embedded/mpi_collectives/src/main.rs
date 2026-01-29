// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// MPI Collectives Example
//
// Demonstrates collective communication patterns using PlexSpaces:
// - Broadcast: ProcessGroupRegistry::publish_to_group()
// - Scatter/Gather: Actor tell/ask patterns
// - Reduce: TupleSpace coordination
//
// Use Case: Distributed computing, parallel algorithms

use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_process_groups::ProcessGroupRegistry;
use plexspaces_core::{ActorId, RequestContext};
use plexspaces_tuplespace::TupleSpace;
use std::sync::Arc;

// =============================================================================
// Main - Demonstrates each collective using PlexSpaces APIs
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║           MPI Collectives with PlexSpaces APIs                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Mapping MPI operations to PlexSpaces abstractions:");
    println!("  - Broadcast  → ProcessGroupRegistry::publish_to_group()");
    println!("  - Scatter    → ActorRef::tell() to each worker");
    println!("  - Gather     → ActorRef::ask() from each worker");
    println!("  - Reduce     → TupleSpace write/read coordination");
    println!("  - Barrier    → TupleSpace barrier synchronization");
    println!();

    // Setup
    let kv_store = Arc::new(InMemoryKVStore::new());
    let registry = ProcessGroupRegistry::new("mpi-node", kv_store);
    let tuplespace = TupleSpace::with_tenant_namespace("mpi-tenant", "collectives");
    
    let tenant_id = "mpi-tenant";
    let namespace = "collectives";
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // =========================================================================
    // Step 1: BROADCAST via Process Groups
    // =========================================================================
    println!("Step 1: BROADCAST via ProcessGroupRegistry");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: registry.publish_to_group(&ctx, group, None, data)");
    println!();

    // Create worker group
    registry.create_group(&ctx, "workers").await?;
    
    // Workers join the group
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

    // Broadcast config to all workers
    let config_data = b"learning_rate=0.01,epochs=100".to_vec();
    let recipients = registry.publish_to_group(&ctx, "workers", None, config_data).await?;
    
    println!();
    println!("  Broadcast config to {} workers:", recipients.len());
    for r in &recipients {
        println!("    -> {} received config", r);
    }
    println!();

    // =========================================================================
    // Step 2: SCATTER via Individual tell() Calls
    // =========================================================================
    println!("Step 2: SCATTER via ActorRef::tell()");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: actor_ref.tell(partition_data).await");
    println!();

    let full_data: Vec<f64> = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
    let chunk_size = full_data.len() / workers.len();
    
    println!("  Full data: {:?}", full_data);
    println!("  Partitioning into {} chunks of size {}:", workers.len(), chunk_size);
    println!();

    for (i, worker) in workers.iter().enumerate() {
        let start = i * chunk_size;
        let end = if i == workers.len() - 1 { full_data.len() } else { start + chunk_size };
        let chunk = &full_data[start..end];
        
        // In real code: actor_ref.tell(Message::json(&chunk)?).await?
        println!("  tell({}, {:?})", worker, chunk);
    }
    println!();

    // =========================================================================
    // Step 3: GATHER via ask() Calls
    // =========================================================================
    println!("Step 3: GATHER via ActorRef::ask()");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: actor_ref.ask(request, timeout).await");
    println!();

    println!("  Gathering results from all workers:");
    let mut gathered: Vec<f64> = vec![];
    for (i, worker) in workers.iter().enumerate() {
        // Simulate: let response = actor_ref.ask(GetResultRequest, timeout).await?
        let start = i * chunk_size;
        let end = if i == workers.len() - 1 { full_data.len() } else { start + chunk_size };
        let worker_result = &full_data[start..end];
        
        println!("  ask({}) -> {:?}", worker, worker_result);
        gathered.extend(worker_result);
    }
    println!();
    println!("  Gathered: {:?}", gathered);
    println!();

    // =========================================================================
    // Step 4: REDUCE via TupleSpace Coordination
    // =========================================================================
    println!("Step 4: REDUCE via TupleSpace");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.write() / tuplespace.read()");
    println!();

    // Each worker writes partial result to TupleSpace
    let partial_sums: Vec<f64> = vec![3.0, 7.0, 11.0, 15.0]; // Simulated local sums
    
    println!("  Workers write partial sums to TupleSpace:");
    for (i, &sum) in partial_sums.iter().enumerate() {
        // In real code: tuplespace.write(&ctx, tuple!["partial_sum", worker_id, sum]).await?
        println!("    tuplespace.write([\"partial_sum\", \"worker-{}\", {}])", i, sum);
    }
    println!();

    // Coordinator reads and aggregates
    println!("  Coordinator reads partial sums:");
    let mut global_sum = 0.0;
    for (i, &sum) in partial_sums.iter().enumerate() {
        // In real code: let tuple = tuplespace.read(&ctx, pattern!["partial_sum", _, _]).await?
        println!("    tuplespace.read([\"partial_sum\", \"worker-{}\", _]) -> {}", i, sum);
        global_sum += sum;
    }
    println!();
    println!("  Global sum (reduce): {}", global_sum);
    println!();

    // =========================================================================
    // Step 5: BARRIER via TupleSpace
    // =========================================================================
    println!("Step 5: BARRIER via TupleSpace");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: tuplespace.barrier(name, count).await");
    println!();

    println!("  Each worker signals arrival at barrier:");
    for worker in &workers {
        // In real code: tuplespace.write(&ctx, tuple!["barrier", "iteration_1", worker]).await?
        println!("    {} -> tuplespace.write([\"barrier\", \"iteration_1\", \"{}\"])", worker, worker);
    }
    println!();
    println!("  Coordinator waits for all {} workers...", workers.len());
    println!("  All workers arrived - barrier released!");
    println!();

    // =========================================================================
    // Step 6: ALL_REDUCE = Reduce + Broadcast
    // =========================================================================
    println!("Step 6: ALL_REDUCE = Reduce + Broadcast");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  API: Combine reduce (TupleSpace) + broadcast (ProcessGroup)");
    println!();

    println!("  1. Reduce: Aggregate partial sums (see Step 4)");
    println!("     Global sum = {}", global_sum);
    println!();
    
    println!("  2. Broadcast result to all workers:");
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
    println!("PlexSpaces API Mapping:");
    println!();
    println!("  ┌─────────────┬────────────────────────────────────────────┐");
    println!("  │ MPI         │ PlexSpaces API                             │");
    println!("  ├─────────────┼────────────────────────────────────────────┤");
    println!("  │ Broadcast   │ ProcessGroupRegistry::publish_to_group()   │");
    println!("  │ Scatter     │ ActorRef::tell() to each worker            │");
    println!("  │ Gather      │ ActorRef::ask() from each worker           │");
    println!("  │ Reduce      │ TupleSpace write/read coordination         │");
    println!("  │ Barrier     │ TupleSpace barrier / write+read            │");
    println!("  │ AllReduce   │ Reduce (TupleSpace) + Broadcast (PG)       │");
    println!("  └─────────────┴────────────────────────────────────────────┘");
    println!();
    println!("Use Cases:");
    println!("  - Distributed ML training (gradient AllReduce)");
    println!("  - Monte Carlo simulations (reduce samples)");
    println!("  - MapReduce (scatter map, gather reduce)");
    println!("  - Consensus protocols (barrier + broadcast)");
    println!();

    // Keep TupleSpace in scope
    drop(tuplespace);

    Ok(())
}
