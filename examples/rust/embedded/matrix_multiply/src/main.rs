// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Matrix Multiplication Example (Parallel with Actors)
//
// Demonstrates parallel matrix multiplication using PlexSpaces:
// - SDK annotations for actor definition
// - Scatter-gather pattern (call for distribution and collection - ensures completion)
// - CoordinationComputeTracker metrics
//
// Use Case: Scientific computing, ML inference, graphics, signal processing

use matrix_multiply::MatrixWorker;
use plexspaces_sdk::{spawn, GenServerRef, RequestContext, json};
use plexspaces_node::{NodeBuilder, CoordinationComputeTracker};
use std::time::{Duration, Instant};
use tracing::info;
use anyhow::Result;

// =============================================================================
// Main - Demonstrates scatter-gather pattern for parallel matrix multiplication
// =============================================================================

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("matrix_multiply=info,plexspaces=warn"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║       Matrix Multiplication with Actor Workers                 ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    info!("SDK: gen_server_actor, spawn(), GenServerRef.call()");
    info!("Pattern: Scatter-gather (call for distribution and collection - ensures completion)");
    println!();

    // Configuration: Use non-trivial data sizes (run for several seconds to show real metrics)
    let matrix_size = 1000; // 1000×1000 matrices (increased for substantial computation)
    let num_workers = 8; // 8 worker actors (increased parallelism)
    
    info!("Configuration:");
    info!("  Matrix size: {}×{}", matrix_size, matrix_size);
    info!("  Workers: {}", num_workers);
    info!("  Total operations: {} (2×{}³)", 2 * matrix_size * matrix_size * matrix_size, matrix_size);
    println!();

    // Create metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("matrix-multiply".to_string());
    let total_start = Instant::now();

    // RequestContext: explicit tenant/namespace for multi-tenancy
    let tenant_id = "matrix-compute";
    let namespace = "multiply";
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Setup: Node
    let node = NodeBuilder::new("matrix-node".to_string())
        .build_started()
        .await;
    let service_locator = node.service_locator();

    // =========================================================================
    // Step 1: Spawn worker actors
    // =========================================================================
    info!("Step 1: Spawn {} worker actors", num_workers);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut worker_refs: Vec<GenServerRef> = Vec::new();

    for worker_id in 0..num_workers {
        let actor = MatrixWorker::new(worker_id);
        let actor_name = format!("worker-{}", worker_id);

        let actor_ref = spawn(&ctx, service_locator.clone(), actor_name, namespace, actor).await
            .map_err(|e| anyhow::anyhow!("Failed to spawn worker {}: {}", worker_id, e))?;

        let worker_ref = GenServerRef::new(actor_ref);
        worker_refs.push(worker_ref);

        if worker_id < 3 || worker_id == num_workers - 1 {
            info!("  Spawned worker-{}", worker_id);
        }
    }
    println!();

    // =========================================================================
    // Step 2: Generate test matrices
    // =========================================================================
    info!("Step 2: Generate {}×{} test matrices", matrix_size, matrix_size);
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let matrix_gen_start = Instant::now();
    let matrix_a: Vec<Vec<f64>> = (0..matrix_size)
        .map(|i| (0..matrix_size).map(|j| (i * matrix_size + j) as f64).collect())
        .collect();
    let matrix_b: Vec<Vec<f64>> = (0..matrix_size)
        .map(|i| (0..matrix_size).map(|j| ((i + j) % matrix_size) as f64).collect())
        .collect();
    let matrix_gen_time = matrix_gen_start.elapsed();
    
    info!("  Generated matrices in {:.2}ms", matrix_gen_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 3: SCATTER-GATHER - Distribute work and collect results via call()
    // =========================================================================
    // Design: Use call() for compute_rows to ensure computation completes before proceeding
    // This avoids deadlock where get_result() is called before compute_rows() finishes
    // Pattern: Request-reply ensures sequential processing and result availability
    info!("Step 3: SCATTER-GATHER work via GenServerRef::call()");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let rows_per_worker = matrix_size / num_workers;
    let scatter_gather_start = Instant::now();
    let mut all_results: Vec<(usize, Vec<Vec<f64>>)> = Vec::new();
    let mut total_compute_time_ms = 0u64;
    
    metrics_tracker.start_coordinate();
    for (i, worker_ref) in worker_refs.iter().enumerate() {
        let start_row = i * rows_per_worker;
        let end_row = if i == num_workers - 1 { matrix_size } else { start_row + rows_per_worker };
        
        let work_request = json!({
            "start_row": start_row,
            "end_row": end_row,
            "matrix_a": matrix_a,
            "matrix_b": matrix_b,
        });
        
        if i < 3 || i == num_workers - 1 {
            info!("  call(worker-{}, compute_rows, rows {}..{})", i, start_row, end_row - 1);
        }
        
        // Use call() instead of cast() to ensure computation completes before proceeding
        // This prevents deadlock where get_result() is called before compute_rows() finishes
        metrics_tracker.end_coordinate();
        metrics_tracker.start_compute();
        
        let result_start = Instant::now();
        let result: serde_json::Value = worker_ref.call("compute_rows", &work_request).await
            .map_err(|e| anyhow::anyhow!("compute_rows failed for worker {}: {}", i, e))?;
        let result_time = result_start.elapsed();
        
        metrics_tracker.end_compute();
        metrics_tracker.start_coordinate();
        
        let start_row_result = result["start_row"].as_u64().unwrap_or(0) as usize;
        let rows: Vec<Vec<f64>> = serde_json::from_value(
            result["rows"].clone()
        ).map_err(|e| anyhow::anyhow!("Invalid result rows from worker {}: {}", i, e))?;
        
        let num_rows = rows.len();
        
        if let Some(compute_ms) = result["compute_time_ms"].as_u64() {
            total_compute_time_ms += compute_ms;
        }
        
        all_results.push((start_row_result, rows));
        
        if i < 3 || i == num_workers - 1 {
            info!("  ✓ worker-{} computed {} rows in {:.2}ms", i, num_rows, result_time.as_secs_f64() * 1000.0);
        }
        
        metrics_tracker.increment_message();
    }
    let scatter_gather_time = scatter_gather_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Completed scatter-gather for {} workers in {:.2}ms", num_workers, scatter_gather_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 5: Assemble final result matrix
    // =========================================================================
    info!("Step 5: Assemble final result matrix");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let assemble_start = Instant::now();
    all_results.sort_by_key(|(start_row, _)| *start_row);
    let mut result_matrix: Vec<Vec<f64>> = Vec::with_capacity(matrix_size);
    for (_, rows) in all_results {
        result_matrix.extend(rows);
    }
    let assemble_time = assemble_start.elapsed();
    
    info!("  Assembled {}×{} result matrix in {:.2}ms", matrix_size, matrix_size, assemble_time.as_secs_f64() * 1000.0);
    
    // Verify result (sample check)
    if matrix_size <= 10 {
        info!("  Result matrix (first 3×3):");
        for i in 0..3.min(matrix_size) {
            info!("    {:?}", &result_matrix[i][..3.min(matrix_size)]);
        }
    } else {
        info!("  Result matrix sample: C[0][0]={:.2}, C[{}][{}]={:.2}", 
              result_matrix[0][0], matrix_size-1, matrix_size-1, result_matrix[matrix_size-1][matrix_size-1]);
    }
    println!();

    // =========================================================================
    // Step 6: Performance metrics
    // =========================================================================
    info!("Step 6: Performance Metrics");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();

    let coordinate_time = Duration::from_millis(metrics.coordinate_duration_ms);
    let compute_time = Duration::from_millis(metrics.compute_duration_ms);
    
    // Calculate benchmark metrics
    let total_ops = 2.0 * (matrix_size as f64).powi(3); // 2×n³ operations (multiply + add per element)
    let gflops = (total_ops / total_time.as_secs_f64()) / 1e9;
    let data_size_mb = (matrix_size * matrix_size * 3 * 8) as f64 / (1024.0 * 1024.0); // 3 matrices × 8 bytes per f64
    let throughput_mbps = data_size_mb / total_time.as_secs_f64();

    info!("Execution Summary:");
    info!("  Total execution time: {:.2}ms ({:.2}s)", 
          total_time.as_secs_f64() * 1000.0,
          total_time.as_secs_f64());
    info!("  Matrix size: {}×{}", matrix_size, matrix_size);
    info!("  Workers: {}", num_workers);
    info!("  Total operations: {:.0} (2×{}³)", total_ops, matrix_size);
    println!();

    info!("Coordination vs Computation Breakdown:");
    info!("  Coordination time: {:.2}ms ({:.1}%)", 
          coordinate_time.as_secs_f64() * 1000.0,
          (coordinate_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Computation time: {:.2}ms ({:.1}%)",
          compute_time.as_secs_f64() * 1000.0,
          (compute_time.as_secs_f64() / total_time.as_secs_f64()) * 100.0);
    info!("  Efficiency (compute/total): {:.1}%", metrics.efficiency * 100.0);
    println!();

    info!("Message & Barrier Metrics:");
    info!("  Total messages sent: {}", metrics.message_count);
    if metrics.message_count > 0 {
        let avg_latency_ms = compute_time.as_secs_f64() * 1000.0 / metrics.message_count as f64;
        info!("  Average latency per message: {:.2}ms", avg_latency_ms);
        let throughput = metrics.message_count as f64 / total_time.as_secs_f64();
        info!("  Message throughput: {:.1} msg/s", throughput);
    }
    println!();

    info!("Benchmark Metrics:");
    info!("  Performance: {:.2} GFLOPS", gflops);
    info!("  Data processed: {:.2} MB", data_size_mb);
    info!("  Throughput: {:.2} MB/s", throughput_mbps);
    println!();

    info!("Granularity Analysis:");
    if coordinate_time.as_secs_f64() > 0.0 {
        info!("  Granularity ratio (compute/coordinate): {:.2}", metrics.granularity_ratio);
        if metrics.granularity_ratio >= 100.0 {
            info!("  ✅ Excellent granularity (coordination overhead is negligible)");
        } else if metrics.granularity_ratio >= 10.0 {
            info!("  ✅ Good granularity (coordination overhead is low)");
        } else if metrics.granularity_ratio >= 1.0 {
            info!("  ⚠️  Moderate granularity (coordination overhead is noticeable)");
        } else {
            info!("  ❌ Poor granularity (coordination overhead dominates)");
        }
    } else {
        info!("  Granularity ratio: N/A (no coordination overhead)");
    }
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Matrix Multiplication Example Complete");
    println!();

    Ok(())
}
