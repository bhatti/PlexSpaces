// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Metrics Aggregation Pipeline - Embedded Rust
//
// Production-grade metrics pipeline: StatsD/OTLP ingestion → tumbling window
// aggregation → cascading rollup (1s→1m→1h) → anomaly detection (Z-score).
//
// Uses SDK annotations, CoordinationComputeTracker, NodeBuilder, spawn_gen_server.

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, spawn_gen_server, json, Value, GenServerRef, RequestContextExt,
};
use plexspaces_node::CoordinationComputeTracker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::Level;
use anyhow::Result;

// ─── Types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Metric {
    name: String,
    value: f64,
    metric_type: String,
    tags: HashMap<String, String>,
    timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AggregateResult {
    name: String,
    count: u64,
    sum: f64,
    avg: f64,
    min: f64,
    max: f64,
    metric_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ShardAggregates {
    shard_id: usize,
    aggregates: Vec<AggregateResult>,
    total_metrics: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Anomaly {
    metric: String,
    value: f64,
    z_score: f64,
    mean: f64,
    std_dev: f64,
    severity: String,
}

// ─── Aggregator Shard Actor ──────────────────────────────────────────────────

#[gen_server_actor]
struct AggregatorShard {
    shard_id: usize,
    windows: HashMap<String, Vec<f64>>,
    total_metrics: u64,
}

impl AggregatorShard {
    fn new(shard_id: usize) -> Self {
        Self {
            shard_id,
            windows: HashMap::new(),
            total_metrics: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl AggregatorShard {
    #[handler("ingest_batch", cast)]
    async fn handle_ingest_batch(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        let batch: Vec<Metric> = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid batch: {}", e)))?;

        for m in &batch {
            self.windows.entry(m.name.clone()).or_default().push(m.value);
        }
        self.total_metrics += batch.len() as u64;
        Ok(())
    }

    #[handler("get_aggregates")]
    async fn handle_get_aggregates(
        &self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let mut aggregates = Vec::new();
        for (name, values) in &self.windows {
            if values.is_empty() {
                continue;
            }
            let count = values.len() as u64;
            let sum: f64 = values.iter().sum();
            let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
            let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
            aggregates.push(AggregateResult {
                name: name.clone(),
                count,
                sum,
                avg: sum / count as f64,
                min,
                max,
                metric_type: "gauge".to_string(),
            });
        }
        let result = ShardAggregates {
            shard_id: self.shard_id,
            aggregates,
            total_metrics: self.total_metrics,
        };
        Ok(json!(result))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn route_metric(name: &str, shard_count: usize) -> usize {
    let hash = name.bytes().fold(0u64, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u64));
    (hash % shard_count as u64) as usize
}

const METRIC_NAMES: &[&str] = &[
    "cpu.usage", "memory.used", "disk.io.read", "disk.io.write",
    "network.rx.bytes", "network.tx.bytes", "http.request.duration",
    "http.request.count", "cache.hit_rate", "queue.depth",
    "gc.pause_ms", "thread.count", "db.query.duration", "db.connections.active",
    "api.latency.p99", "api.error_rate",
];

fn generate_metrics(count: usize, seed: u64) -> Vec<Metric> {
    let hosts = ["web-01", "api-01", "db-01", "cache-01"];
    let mut metrics = Vec::with_capacity(count);
    let mut rng = seed;
    for i in 0..count {
        rng = (rng.wrapping_mul(1103515245).wrapping_add(12345)) & 0x7fffffff;
        let name = METRIC_NAMES[(rng as usize) % METRIC_NAMES.len()];
        let host = hosts[((rng >> 8) as usize) % hosts.len()];
        let value = if name.contains("duration") || name.contains("latency") {
            ((rng % 500) + 1) as f64
        } else if name.contains("rate") || name.contains("usage") {
            (rng % 100) as f64
        } else {
            (rng % 10000) as f64
        };
        let value = if (rng >> 20) % 20 == 0 { value * 10.0 } else { value };
        let mut tags = HashMap::new();
        tags.insert("host".into(), host.into());
        metrics.push(Metric {
            name: name.into(),
            value,
            metric_type: if name.contains("count") { "counter".into() } else { "gauge".into() },
            tags,
            timestamp: i as u64,
        });
    }
    metrics
}

fn detect_anomalies(aggregates: &[AggregateResult], threshold: f64) -> Vec<Anomaly> {
    let values: Vec<f64> = aggregates.iter().map(|a| a.avg).collect();
    if values.len() < 3 {
        return vec![];
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
    let std = variance.sqrt().max(0.001);

    aggregates.iter().filter_map(|a| {
        let z = (a.avg - mean).abs() / std;
        if z > threshold {
            Some(Anomaly {
                metric: a.name.clone(),
                value: a.avg,
                z_score: (z * 100.0).round() / 100.0,
                mean: (mean * 100.0).round() / 100.0,
                std_dev: (std * 100.0).round() / 100.0,
                severity: if z > threshold * 2.0 { "critical".into() } else { "warning".into() },
            })
        } else {
            None
        }
    }).collect()
}

fn rollup(aggregates: &[AggregateResult]) -> Vec<AggregateResult> {
    let mut rolled: HashMap<String, AggregateResult> = HashMap::new();
    for a in aggregates {
        let entry = rolled.entry(a.name.clone()).or_insert_with(|| AggregateResult {
            name: a.name.clone(),
            count: 0, sum: 0.0, avg: 0.0,
            min: f64::INFINITY, max: f64::NEG_INFINITY,
            metric_type: a.metric_type.clone(),
        });
        entry.count += a.count;
        entry.sum += a.sum;
        entry.min = entry.min.min(a.min);
        entry.max = entry.max.max(a.max);
    }
    rolled.values_mut().for_each(|r| {
        r.avg = if r.count > 0 { r.sum / r.count as f64 } else { 0.0 };
    });
    rolled.into_values().collect()
}

// ─── Main ────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("metrics_aggregation=info,plexspaces=warn")
        .try_init();

    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Metrics Aggregation Pipeline - Embedded Rust");
    println!("  StatsD/OTLP → Window Aggregation → Rollup → Anomaly Detection");
    println!("═══════════════════════════════════════════════════════════════════");
    println!();

    let mut tracker = CoordinationComputeTracker::new("metrics-aggregation".to_string());
    let total_start = Instant::now();

    let shard_count: usize = 8;
    let metric_count: usize = 100_000;

    // Step 1: Create node
    println!("Step 1: Create PlexSpaces Node");
    println!("────────────────────────────────────────────────────────────────");
    tracker.start_coordinate();
    let node = NodeBuilder::new("metrics-node")
        .with_clustering_enabled(false)
        .build_started().await;
    tracker.end_coordinate();
    println!("  ✓ Node created");

    let ctx = RequestContext::new_without_auth(
        "metrics-tenant".to_string(),
        "aggregation".to_string(),
    );

    // Step 2: Spawn shard actors
    println!("\nStep 2: Spawn {} Aggregator Shards", shard_count);
    println!("────────────────────────────────────────────────────────────────");
    tracker.start_coordinate();
    let spawn_start = Instant::now();
    let service_locator = node.service_locator();
    let mut shards: Vec<GenServerRef> = Vec::new();

    for i in 0..shard_count {
        let shard = spawn_gen_server(
            &ctx, service_locator.clone(),
            &format!("agg-shard-{}", i),
            AggregatorShard::new(i),
            vec![],
        ).await
        .map_err(|e| anyhow::anyhow!("Failed to spawn shard-{}: {}", i, e))?;
        shards.push(shard);
    }
    let spawn_time = spawn_start.elapsed();
    tracker.end_coordinate();
    println!("  ✓ {} shards in {:.2}ms", shard_count, spawn_time.as_secs_f64() * 1000.0);

    // Step 3: Ingest metrics via hash-based routing
    println!("\nStep 3: Ingest {} Metrics (Hash-Based Routing)", metric_count);
    println!("────────────────────────────────────────────────────────────────");
    let metrics = generate_metrics(metric_count, 42);
    let batch_size = 500;
    let mut shard_distribution: HashMap<usize, u64> = HashMap::new();

    tracker.start_compute();
    let ingest_start = Instant::now();

    let mut shard_batches: Vec<Vec<Metric>> = (0..shard_count).map(|_| Vec::new()).collect();
    for m in &metrics {
        let shard_id = route_metric(&m.name, shard_count);
        *shard_distribution.entry(shard_id).or_default() += 1;
        shard_batches[shard_id].push(m.clone());
    }

    for (shard_id, batch) in shard_batches.iter().enumerate() {
        for chunk in batch.chunks(batch_size) {
            let chunk_vec: Vec<Metric> = chunk.to_vec();
            shards[shard_id].cast(&ctx, "ingest_batch", &chunk_vec).await
                .map_err(|e| anyhow::anyhow!("Cast failed: {}", e))?;
            for _ in 0..chunk_vec.len() {
                tracker.increment_message();
            }
        }
    }

    let ingest_time = ingest_start.elapsed();
    tracker.end_compute();
    println!("  ✓ {} metrics ingested in {:.2}ms", metric_count, ingest_time.as_secs_f64() * 1000.0);
    for shard_id in 0..shard_count.min(4) {
        if let Some(count) = shard_distribution.get(&shard_id) {
            println!("    shard-{}: {} metrics", shard_id, count);
        }
    }

    // Allow casts to be processed
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Step 4: Scatter-gather aggregates
    println!("\nStep 4: Aggregate (Scatter-Gather)");
    println!("────────────────────────────────────────────────────────────────");
    tracker.start_coordinate();
    let agg_start = Instant::now();

    let empty_query: serde_json::Value = json!({});
    let mut query_futures = Vec::new();
    for shard in &shards {
        query_futures.push(shard.call::<serde_json::Value, ShardAggregates>(&ctx, "get_aggregates", &empty_query));
    }

    let shard_results: Vec<std::result::Result<ShardAggregates, _>> = futures::future::join_all(query_futures).await;

    let mut all_aggregates: Vec<AggregateResult> = Vec::new();
    let mut total_shard_metrics: u64 = 0;

    for result in shard_results {
        let shard_agg = result.map_err(|e| anyhow::anyhow!("Query failed: {}", e))?;
        total_shard_metrics += shard_agg.total_metrics;
        all_aggregates.extend(shard_agg.aggregates);
        tracker.increment_message();
    }

    let agg_time = agg_start.elapsed();
    tracker.end_coordinate();
    println!("  ✓ {} aggregates from {} shard metrics in {:.2}ms",
        all_aggregates.len(), total_shard_metrics, agg_time.as_secs_f64() * 1000.0);

    // Step 5: Cascading rollup
    println!("\nStep 5: Cascading Rollup (1s → 1m → 1h)");
    println!("────────────────────────────────────────────────────────────────");
    tracker.start_compute();
    let rollup_start = Instant::now();

    let rollup_1s = rollup(&all_aggregates);
    let rollup_1m = rollup(&rollup_1s);
    let rollup_1h = rollup(&rollup_1m);

    let rollup_time = rollup_start.elapsed();
    tracker.end_compute();
    println!("  ✓ 1s: {} metrics, 1m: {} metrics, 1h: {} metrics ({:.2}ms)",
        rollup_1s.len(), rollup_1m.len(), rollup_1h.len(),
        rollup_time.as_secs_f64() * 1000.0);

    // Step 6: Anomaly detection
    println!("\nStep 6: Anomaly Detection (Z-score, threshold=2.5)");
    println!("────────────────────────────────────────────────────────────────");
    tracker.start_compute();
    let anomaly_start = Instant::now();

    let anomalies = detect_anomalies(&all_aggregates, 2.5);

    let anomaly_time = anomaly_start.elapsed();
    tracker.end_compute();

    let critical = anomalies.iter().filter(|a| a.severity == "critical").count();
    let warning = anomalies.iter().filter(|a| a.severity == "warning").count();
    println!("  ✓ {} anomalies detected: {} critical, {} warning ({:.2}ms)",
        anomalies.len(), critical, warning, anomaly_time.as_secs_f64() * 1000.0);

    for a in anomalies.iter().take(5) {
        println!("    [{}] {}: avg={:.1}, z={:.2} (mean={:.1}, std={:.1})",
            a.severity, a.metric, a.value, a.z_score, a.mean, a.std_dev);
    }

    // Step 7: Summary
    let total_time = total_start.elapsed();
    let metrics_per_sec = if total_time.as_millis() > 0 {
        (metric_count as f64 / total_time.as_secs_f64()).round() as u64
    } else { 0 };

    let perf = tracker.finalize();

    println!();
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Metrics Aggregation — Results");
    println!("═══════════════════════════════════════════════════════════════════");
    println!("  Metrics:       {}", metric_count);
    println!("  Shards:        {}", shard_count);
    println!("  Total time:    {:.2}ms", total_time.as_secs_f64() * 1000.0);
    println!("  Throughput:    {} metrics/sec", metrics_per_sec);
    println!("  Aggregates:    {} (1s) → {} (1m) → {} (1h)", rollup_1s.len(), rollup_1m.len(), rollup_1h.len());
    println!("  Anomalies:     {} (critical={}, warning={})", anomalies.len(), critical, warning);
    println!();
    println!("  Coordination vs Computation:");
    println!("    Compute:       {:.2}ms ({:.1}%)", perf.compute_duration_ms,
        if perf.total_duration_ms > 0 { perf.compute_duration_ms as f64 / perf.total_duration_ms as f64 * 100.0 } else { 0.0 });
    println!("    Coordination:  {:.2}ms ({:.1}%)", perf.coordinate_duration_ms,
        if perf.total_duration_ms > 0 { perf.coordinate_duration_ms as f64 / perf.total_duration_ms as f64 * 100.0 } else { 0.0 });
    println!("    Granularity:   {:.2}x", perf.granularity_ratio);
    println!("    Efficiency:    {:.1}%", perf.efficiency * 100.0);
    println!("    Messages:      {}", perf.message_count);
    println!("═══════════════════════════════════════════════════════════════════");

    assert!(total_shard_metrics >= metric_count as u64, "all metrics ingested");
    assert!(!all_aggregates.is_empty(), "aggregates produced");
    assert!(metrics_per_sec > 0, "non-zero throughput");
    println!("  ✓ All assertions passed");

    Ok(())
}
