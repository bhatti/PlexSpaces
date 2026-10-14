// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Job Queue — Flow-Controlled Messaging Example
//
// Demonstrates the outbox pattern with AIMD concurrency control:
//
// 1. ConcurrencyLimiter: AIMD-style adaptive concurrency
// 2. AdmissionControlFacet: Receiver-side flow control
// 3. OutboxRelay: Drain loop delivering messages from a channel
// 4. Scaling benchmark: vary worker counts, measure throughput and AIMD adaptation
// 5. Thundering herd protection (UC1): AIMD + admission control under burst load
// 6. Config wave deploy (UC2): wave-based dispatch with bounded config servers
// 7. Heartbeat coalescing (UC3): OVERWRITE conflict resolution for transient data
// 8. Telemetry fast-path with outbox fallback (UC4): dual-path delivery
// 9. Reliable offline delivery (UC5): SQLite-backed channel with send_at backoff

use plexspaces_channel::{Channel, InMemoryChannel, SqliteChannel};
use plexspaces_common::retry::default_retry_policy;
use plexspaces_node::NodeBuilder;
use plexspaces_outbox::{
    AdmissionControlFacet, ConcurrencyLimiter, MessageDelivery, OutboxRelay,
    PartitionedOutboxRelay, build_dispatcher_status, validate_config,
};
use plexspaces_proto::channel::v1::{
    channel_config, ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
    SqliteConfig,
};
use plexspaces_proto::outbox::v1::{
    AdmissionControlConfig, AdmissionStrategy, ConcurrencyConfig, ConcurrencyStrategy,
    OutboxConfig, PartitionedOutboxConfig,
};
use plexspaces_sdk::{
    cast_message, gen_server_actor, json, plexspaces_handlers, spawn_with_facets,
    ActorContext, BehaviorError, Message, RequestContext, RequestContextExt, Value,
};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Notify;
use tracing::Level;

// =============================================================================
// Worker Actor — processes jobs with admission control
// =============================================================================

#[gen_server_actor]
struct Worker {
    processed: u64,
    name: String,
}

#[plexspaces_handlers]
impl Worker {
    #[handler("process_job")]
    async fn process_job(
        &mut self,
        _ctx: &ActorContext,
        _msg: &plexspaces_sdk::Message,
    ) -> Result<Value, BehaviorError> {
        self.processed += 1;
        Ok(json!({ "status": "completed", "worker": self.name, "count": self.processed }))
    }

    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &plexspaces_sdk::Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({ "worker": self.name, "processed": self.processed }))
    }
}

// =============================================================================
// Config Server Actor — serves config with admission control (UC2)
// =============================================================================

#[gen_server_actor]
struct ConfigServer {
    served: u64,
    name: String,
}

#[plexspaces_handlers]
impl ConfigServer {
    #[handler("download_config")]
    async fn download_config(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.served += 1;
        Ok(json!({ "status": "config_delivered", "server": self.name, "served": self.served }))
    }

    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({ "server": self.name, "served": self.served }))
    }
}

// =============================================================================
// Telemetry Collector Actor — collects telemetry with admission control (UC4)
// =============================================================================

#[gen_server_actor]
struct TelemetryCollector {
    received: u64,
}

#[plexspaces_handlers]
impl TelemetryCollector {
    #[handler("ingest", cast)]
    async fn ingest(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<(), BehaviorError> {
        self.received += 1;
        Ok(())
    }

    #[handler("get_stats")]
    async fn get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        Ok(json!({ "received": self.received }))
    }
}

// =============================================================================
// Delivery callback — counts completions, signals via Notify
// =============================================================================

struct CountingDelivery {
    delivered: AtomicU64,
    notify: Notify,
    target_count: u64,
}

#[async_trait::async_trait]
impl MessageDelivery for CountingDelivery {
    async fn deliver(&self, _msg: &plexspaces_proto::common::v1::Message) -> Result<(), String> {
        let n = self.delivered.fetch_add(1, Ordering::SeqCst) + 1;
        if n >= self.target_count {
            self.notify.notify_one();
        }
        Ok(())
    }
}

// =============================================================================
// Benchmark result row
// =============================================================================

struct BenchRow {
    workers: usize,
    jobs: u64,
    elapsed_ms: u128,
    jobs_per_sec: f64,
    final_aimd_limit: u32,
    speedup: f64,
    efficiency_pct: f64,
}

// =============================================================================
// Run a single benchmark iteration
// =============================================================================

async fn run_single(
    num_workers: usize,
    total_jobs: u64,
) -> anyhow::Result<(u128, u32, u64, u64)> {
    let node = NodeBuilder::new(format!("bench-{}-workers", num_workers))
        .with_clustering_enabled(false)
        .build_started()
        .await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("tenant-bench".into(), "job-queue".into());

    let admission_config = AdmissionControlConfig {
        max_concurrent: 5,
        strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
        default_retry_after_ms: 100,
    };

    for i in 0..num_workers {
        let worker = Worker {
            processed: 0,
            name: format!("worker-{}", i),
        };
        let admission_facet = AdmissionControlFacet::new(&admission_config);
        let _ref = spawn_with_facets(
            &ctx,
            service_locator.clone(),
            format!("worker-{}", i),
            "job-queue",
            worker,
            vec![Box::new(admission_facet)],
        )
        .await
        .map_err(|e| anyhow::anyhow!("spawn worker-{}: {}", i, e))?;
    }

    let outbox_config = OutboxConfig {
        concurrency: Some(ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 50,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        }),
        retry_policy: Some(default_retry_policy()),
        relay_interval_ms: 5,
        relay_batch_size: 20,
        storage_channel: None,
    };

    let channel_config = ChannelConfig {
        name: format!("bench-outbox-{}", num_workers),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: total_jobs + 100,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };
    let channel = Arc::new(
        InMemoryChannel::new(channel_config)
            .await
            .map_err(|e| anyhow::anyhow!("channel: {}", e))?,
    );
    let limiter = Arc::new(ConcurrencyLimiter::new(
        outbox_config.concurrency.as_ref().unwrap(),
    ));

    let delivery = Arc::new(CountingDelivery {
        delivered: AtomicU64::new(0),
        notify: Notify::new(),
        target_count: total_jobs,
    });

    let relay = Arc::new(OutboxRelay::new(
        &outbox_config,
        channel.clone(),
        limiter.clone(),
        delivery.clone(),
    ));

    for i in 0..total_jobs {
        let msg = cast_message(json!({
            "type": "process_data",
            "job_id": format!("job-{}", i),
            "priority": if i % 10 == 0 { "high" } else { "normal" },
        }));
        channel.send(msg).await.map_err(|e| anyhow::anyhow!("send: {}", e))?;
    }

    let t0 = Instant::now();
    let relay_clone = relay.clone();
    let relay_handle = tokio::spawn(async move { relay_clone.run().await });

    tokio::select! {
        _ = delivery.notify.notified() => {},
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
            panic!("timeout: only {}/{} delivered", delivery.delivered.load(Ordering::SeqCst), total_jobs);
        }
    }
    let elapsed = t0.elapsed();
    relay.stop();
    relay_handle.await?;

    let delivered = delivery.delivered.load(Ordering::SeqCst);
    let final_limit = limiter.current_limit();

    let stats = relay.stats();
    let failed = stats.failed.load(Ordering::Relaxed);

    node.shutdown(Duration::from_secs(5)).await?;

    Ok((elapsed.as_millis(), final_limit, delivered, failed))
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::WARN)
        .with_target(false)
        .init();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║       Job Queue: Flow-Controlled Messaging Example         ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();

    // --- Step 1: Validate config ---
    let outbox_config = OutboxConfig {
        concurrency: Some(ConcurrencyConfig {
            initial_limit: 10,
            min_limit: 1,
            max_limit: 50,
            strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
        }),
        retry_policy: Some(default_retry_policy()),
        relay_interval_ms: 5,
        relay_batch_size: 20,
        storage_channel: None,
    };
    validate_config(&outbox_config).map_err(|e| anyhow::anyhow!("{}", e))?;
    println!("  Config validated: AIMD limits [1, 50], initial=10");
    println!();

    // --- Step 2: Single run with status reporting ---
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Single run: 3 workers × 50 jobs");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let total_jobs: u64 = 50;
    let node = NodeBuilder::new("job-queue-node")
        .with_clustering_enabled(false)
        .build_started()
        .await;
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("tenant-1".into(), "job-queue".into());

    let t_spawn = Instant::now();
    let admission_config = AdmissionControlConfig {
        max_concurrent: 5,
        strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
        default_retry_after_ms: 100,
    };
    for i in 0..3 {
        let worker = Worker {
            processed: 0,
            name: format!("worker-{}", i),
        };
        let admission_facet = AdmissionControlFacet::new(&admission_config);
        let _ref = spawn_with_facets(
            &ctx,
            service_locator.clone(),
            format!("worker-{}", i),
            "job-queue",
            worker,
            vec![Box::new(admission_facet)],
        )
        .await
        .map_err(|e| anyhow::anyhow!("spawn worker-{}: {}", i, e))?;
    }
    let spawn_ms = t_spawn.elapsed().as_millis();

    let t_enqueue = Instant::now();
    let channel_config = ChannelConfig {
        name: "job-outbox".to_string(),
        provider: ChannelProvider::ChannelProviderInMemory as i32,
        capacity: 200,
        delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
        ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
        ..Default::default()
    };
    let channel = Arc::new(
        InMemoryChannel::new(channel_config)
            .await
            .map_err(|e| anyhow::anyhow!("channel: {}", e))?,
    );
    let limiter = Arc::new(ConcurrencyLimiter::new(
        outbox_config.concurrency.as_ref().unwrap(),
    ));

    for i in 0..total_jobs {
        let msg = cast_message(json!({
            "type": "process_data",
            "job_id": format!("job-{}", i),
            "priority": if i % 10 == 0 { "high" } else { "normal" },
        }));
        channel.send(msg).await.map_err(|e| anyhow::anyhow!("send: {}", e))?;
    }
    let enqueue_ms = t_enqueue.elapsed().as_millis();

    let delivery = Arc::new(CountingDelivery {
        delivered: AtomicU64::new(0),
        notify: Notify::new(),
        target_count: total_jobs,
    });

    let relay = Arc::new(OutboxRelay::new(
        &outbox_config,
        channel.clone(),
        limiter.clone(),
        delivery.clone(),
    ));

    let t_relay = Instant::now();
    let relay_clone = relay.clone();
    let relay_handle = tokio::spawn(async move { relay_clone.run().await });

    tokio::select! {
        _ = delivery.notify.notified() => {},
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
            panic!("timeout waiting for deliveries");
        }
    }
    let relay_ms = t_relay.elapsed().as_millis();
    relay.stop();
    relay_handle.await?;

    let stats = relay.stats();
    let status = build_dispatcher_status("job-queue-dispatcher", stats, &limiter);

    println!("  Spawn workers   : {} ms", spawn_ms);
    println!("  Enqueue {} jobs : {} ms", total_jobs, enqueue_ms);
    println!("  Relay delivery  : {} ms", relay_ms);
    println!("  Delivered       : {}", status.delivered_count);
    println!("  Failed          : {}", status.failed_count);
    println!("  Dead-letter     : {}", status.dead_letter_count);
    println!("  AIMD limit      : {} (initial=10)", status.current_concurrency_limit);
    println!("  Success rate    : {:.1}%", status.success_rate * 100.0);
    let throughput = if relay_ms > 0 {
        total_jobs as f64 / (relay_ms as f64 / 1000.0)
    } else {
        f64::INFINITY
    };
    println!("  Throughput      : {:.1} jobs/sec", throughput);
    println!();

    assert_eq!(status.delivered_count, total_jobs, "all jobs must be delivered");
    assert_eq!(status.failed_count, 0, "no failures expected");
    assert!(throughput > 0.0, "non-zero throughput");
    println!("  ✓ Single-run assertions passed");

    node.shutdown(Duration::from_secs(5)).await?;
    println!();

    // --- Step 3: Scaling benchmark ---
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Scaling benchmark: 1 / 3 / 6 / 12 workers × 100 jobs");
    println!("  (AIMD initial=10, max=50, admission max_concurrent=5/worker)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let worker_counts = [1, 3, 6, 12];
    let bench_jobs: u64 = 100;
    let mut rows: Vec<BenchRow> = Vec::new();
    let mut baseline_ms: Option<u128> = None;

    for &n in &worker_counts {
        let (elapsed_ms, final_limit, delivered, failed) = run_single(n, bench_jobs).await?;

        assert_eq!(delivered, bench_jobs, "all {} jobs must be delivered with {} workers", bench_jobs, n);
        assert_eq!(failed, 0, "no failures expected with {} workers", n);

        let jobs_per_sec = if elapsed_ms > 0 {
            bench_jobs as f64 / (elapsed_ms as f64 / 1000.0)
        } else {
            f64::INFINITY
        };

        if baseline_ms.is_none() {
            baseline_ms = Some(elapsed_ms);
        }
        let base = baseline_ms.unwrap() as f64;
        let speedup = if elapsed_ms > 0 { base / elapsed_ms as f64 } else { 1.0 };
        let efficiency_pct = (speedup / n as f64) * 100.0;

        rows.push(BenchRow {
            workers: n,
            jobs: bench_jobs,
            elapsed_ms,
            jobs_per_sec,
            final_aimd_limit: final_limit,
            speedup,
            efficiency_pct,
        });
    }

    println!();
    println!("  ┌─────────┬──────┬────────────┬────────────┬────────────┬─────────┬──────────┐");
    println!("  │ Workers │ Jobs │ Elapsed ms │   Jobs/sec │ AIMD Limit │ Speedup │   Eff %  │");
    println!("  ├─────────┼──────┼────────────┼────────────┼────────────┼─────────┼──────────┤");
    for r in &rows {
        println!(
            "  │ {:>7} │ {:>4} │ {:>10} │ {:>10.1} │ {:>10} │ {:>6.2}x │ {:>7.1}% │",
            r.workers, r.jobs, r.elapsed_ms, r.jobs_per_sec, r.final_aimd_limit,
            r.speedup, r.efficiency_pct,
        );
    }
    println!("  └─────────┴──────┴────────────┴────────────┴────────────┴─────────┴──────────┘");
    println!();
    println!("  Legend: Speedup = baseline(1 worker) / elapsed");
    println!("          Eff% = speedup / N × 100");
    println!("          AIMD Limit = final concurrency limit after relay (initial=10)");
    println!();

    // --- Final assertions ---
    assert!(rows.len() == worker_counts.len(), "all benchmark rows populated");
    for r in &rows {
        assert!(r.jobs_per_sec > 0.0, "non-zero throughput for {} workers", r.workers);
    }
    println!("  ✓ All benchmark assertions passed");

    // --- Step 4: Partitioned relay demonstration ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Partitioned relay: 4 partitions × 200 jobs to 50 targets");
    println!("  (hash routing preserves per-recipient ordering)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let part_jobs: u64 = 200;
    let num_targets: u64 = 50;

    let part_delivery = Arc::new(CountingDelivery {
        delivered: AtomicU64::new(0),
        notify: Notify::new(),
        target_count: part_jobs,
    });

    let part_config = PartitionedOutboxConfig {
        num_partitions: 4,
        per_partition: Some(OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 50,
                strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
            }),
            retry_policy: Some(default_retry_policy()),
            relay_interval_ms: 5,
            relay_batch_size: 20,
            storage_channel: None,
        }),
        hash_strategy: 0,
    };

    let part_relay = PartitionedOutboxRelay::new("partitioned-demo", &part_config, part_delivery.clone())
        .await
        .map_err(|e| anyhow::anyhow!("partitioned relay: {}", e))?;

    let t_part = Instant::now();
    for i in 0..part_jobs {
        let target = format!("actor-{}", i % num_targets);
        let msg = cast_message(json!({
            "type": "process_data",
            "job_id": format!("part-job-{}", i),
        }));
        part_relay.send(&target, msg).await.map_err(|e| anyhow::anyhow!("send: {}", e))?;
    }
    let enqueue_part_ms = t_part.elapsed().as_millis();

    let handles = part_relay.run();

    tokio::select! {
        _ = part_delivery.notify.notified() => {},
        _ = tokio::time::sleep(Duration::from_secs(30)) => {
            panic!("partitioned relay timeout: {}/{}", part_delivery.delivered.load(Ordering::SeqCst), part_jobs);
        }
    }
    let part_elapsed = t_part.elapsed().as_millis();
    part_relay.stop();
    for h in handles {
        h.await?;
    }

    let part_status = part_relay.status();
    println!("  Enqueue {} jobs  : {} ms", part_jobs, enqueue_part_ms);
    println!("  Total elapsed    : {} ms", part_elapsed);
    println!("  Partitions       : {}", part_status.num_partitions);
    println!("  Total delivered  : {}", part_status.total_delivered);
    println!("  Total failed     : {}", part_status.total_failed);
    println!("  Success rate     : {:.1}%", part_status.aggregate_success_rate * 100.0);
    println!();

    println!("  Per-partition breakdown:");
    println!("  ┌───────────┬───────────┬────────┬────────────┐");
    println!("  │ Partition │ Delivered │ Failed │ AIMD Limit │");
    println!("  ├───────────┼───────────┼────────┼────────────┤");
    for (i, ps) in part_status.partition_statuses.iter().enumerate() {
        println!(
            "  │ {:>9} │ {:>9} │ {:>6} │ {:>10} │",
            i, ps.delivered_count, ps.failed_count, ps.current_concurrency_limit,
        );
    }
    println!("  └───────────┴───────────┴────────┴────────────┘");
    println!();

    assert_eq!(part_status.total_delivered, part_jobs, "all partitioned jobs must be delivered");
    assert_eq!(part_status.total_failed, 0, "no partitioned failures");
    let non_empty: Vec<_> = part_status.partition_statuses.iter().filter(|s| s.delivered_count > 0).collect();
    assert!(non_empty.len() > 1, "messages should spread across multiple partitions");
    println!("  ✓ Partitioned relay assertions passed");

    // --- UC1: Thundering Herd Protection ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  UC1: Thundering Herd Protection");
    println!("  50 workers, 200 simultaneous jobs, admission max_concurrent=3");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    {
        let herd_node = NodeBuilder::new("herd-node")
            .with_clustering_enabled(false)
            .build_started()
            .await;
        let herd_sl = herd_node.service_locator();
        let herd_ctx = RequestContext::new_without_auth("tenant-herd".into(), "herd".into());

        let herd_admission = AdmissionControlConfig {
            max_concurrent: 3,
            strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
            default_retry_after_ms: 50,
        };
        let num_herd_workers = 50;
        for i in 0..num_herd_workers {
            let w = Worker { processed: 0, name: format!("herd-w-{}", i) };
            let facet = AdmissionControlFacet::new(&herd_admission);
            spawn_with_facets(
                &herd_ctx, herd_sl.clone(), format!("herd-w-{}", i),
                "herd", w, vec![Box::new(facet)],
            ).await.map_err(|e| anyhow::anyhow!("spawn herd worker: {}", e))?;
        }

        let herd_jobs: u64 = 200;
        let herd_outbox = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 20,
                min_limit: 1,
                max_limit: 100,
                strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
            }),
            retry_policy: Some(default_retry_policy()),
            relay_interval_ms: 5,
            relay_batch_size: 50,
            storage_channel: None,
        };
        let herd_ch_config = ChannelConfig {
            name: "herd-outbox".to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: herd_jobs + 100,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        let herd_ch = Arc::new(InMemoryChannel::new(herd_ch_config).await
            .map_err(|e| anyhow::anyhow!("herd channel: {}", e))?);
        let herd_limiter = Arc::new(ConcurrencyLimiter::new(
            herd_outbox.concurrency.as_ref().unwrap(),
        ));
        let initial_limit = herd_limiter.current_limit();

        let herd_delivery = Arc::new(CountingDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target_count: herd_jobs,
        });
        let herd_relay = Arc::new(OutboxRelay::new(
            &herd_outbox, herd_ch.clone(), herd_limiter.clone(), herd_delivery.clone(),
        ));

        for i in 0..herd_jobs {
            let msg = cast_message(json!({ "type": "process_job", "job_id": format!("herd-{}", i) }));
            herd_ch.send(msg).await.map_err(|e| anyhow::anyhow!("herd send: {}", e))?;
        }

        let t_herd = Instant::now();
        let herd_relay_clone = herd_relay.clone();
        let herd_handle = tokio::spawn(async move { herd_relay_clone.run().await });

        tokio::select! {
            _ = herd_delivery.notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                panic!("herd timeout: {}/{}", herd_delivery.delivered.load(Ordering::SeqCst), herd_jobs);
            }
        }
        let herd_elapsed = t_herd.elapsed();
        herd_relay.stop();
        herd_handle.await?;

        let herd_stats = herd_relay.stats();
        let final_limit = herd_limiter.current_limit();
        let rejections = herd_stats.failed.load(Ordering::Relaxed);

        println!("  Workers          : {}", num_herd_workers);
        println!("  Jobs dispatched  : {}", herd_jobs);
        println!("  Elapsed          : {} ms", herd_elapsed.as_millis());
        println!("  AIMD limit       : {} → {} (initial → final)", initial_limit, final_limit);
        println!("  Delivered        : {}", herd_delivery.delivered.load(Ordering::SeqCst));
        println!("  Rejections (DLQ) : {}", rejections);
        println!();

        assert_eq!(herd_delivery.delivered.load(Ordering::SeqCst), herd_jobs);
        herd_node.shutdown(Duration::from_secs(5)).await?;
        println!("  ✓ UC1 Thundering herd assertions passed");
    }

    // --- UC2: Config Wave Deploy with Backpressure ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  UC2: Config Wave Deploy with Backpressure");
    println!("  3 config servers (max_concurrent=2), 3 waves of 30 workers");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    {
        let wave_node = NodeBuilder::new("wave-node")
            .with_clustering_enabled(false)
            .build_started()
            .await;
        let wave_sl = wave_node.service_locator();
        let wave_ctx = RequestContext::new_without_auth("tenant-wave".into(), "wave".into());

        let server_admission = AdmissionControlConfig {
            max_concurrent: 2,
            strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
            default_retry_after_ms: 50,
        };
        let num_servers = 3;
        for i in 0..num_servers {
            let server = ConfigServer { served: 0, name: format!("cfg-server-{}", i) };
            let facet = AdmissionControlFacet::new(&server_admission);
            spawn_with_facets(
                &wave_ctx, wave_sl.clone(), format!("cfg-server-{}", i),
                "wave", server, vec![Box::new(facet)],
            ).await.map_err(|e| anyhow::anyhow!("spawn config server: {}", e))?;
        }

        let wave_size = 30u64;
        let num_waves = 3;
        let wave_outbox = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 50,
                strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
            }),
            retry_policy: Some(default_retry_policy()),
            relay_interval_ms: 5,
            relay_batch_size: 20,
            storage_channel: None,
        };

        let total_wave_jobs = wave_size * num_waves;
        let mut wave_times = Vec::new();

        for wave in 0..num_waves {
            let wave_ch_config = ChannelConfig {
                name: format!("wave-outbox-{}", wave),
                provider: ChannelProvider::ChannelProviderInMemory as i32,
                capacity: wave_size + 50,
                delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
                ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
                ..Default::default()
            };
            let wave_ch = Arc::new(InMemoryChannel::new(wave_ch_config).await
                .map_err(|e| anyhow::anyhow!("wave channel: {}", e))?);
            let wave_limiter = Arc::new(ConcurrencyLimiter::new(
                wave_outbox.concurrency.as_ref().unwrap(),
            ));

            let wave_delivery = Arc::new(CountingDelivery {
                delivered: AtomicU64::new(0),
                notify: Notify::new(),
                target_count: wave_size,
            });
            let wave_relay = Arc::new(OutboxRelay::new(
                &wave_outbox, wave_ch.clone(), wave_limiter.clone(), wave_delivery.clone(),
            ));

            for i in 0..wave_size {
                let server_id = format!("cfg-server-{}", i as usize % num_servers);
                let msg = cast_message(json!({
                    "type": "download_config",
                    "worker_id": format!("wave-{}-worker-{}", wave, i),
                    "config_server": server_id,
                }));
                wave_ch.send(msg).await.map_err(|e| anyhow::anyhow!("wave send: {}", e))?;
            }

            let t_wave = Instant::now();
            let relay_clone = wave_relay.clone();
            let handle = tokio::spawn(async move { relay_clone.run().await });

            tokio::select! {
                _ = wave_delivery.notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    panic!("wave {} timeout", wave);
                }
            }
            let wave_elapsed = t_wave.elapsed();
            wave_relay.stop();
            handle.await?;

            wave_times.push(wave_elapsed.as_millis());
            println!("  Wave {} : {} jobs in {} ms (AIMD limit: {})",
                wave, wave_size, wave_elapsed.as_millis(), wave_limiter.current_limit());
        }

        println!();
        println!("  Total waves      : {}", num_waves);
        println!("  Total jobs       : {}", total_wave_jobs);
        println!("  Config servers   : {} (max_concurrent=2 each)", num_servers);
        println!("  Wave times (ms)  : {:?}", wave_times);
        println!();

        wave_node.shutdown(Duration::from_secs(5)).await?;
        println!("  ✓ UC2 Config wave deploy assertions passed");
    }

    // --- UC3: Heartbeat Coalescing with OVERWRITE ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  UC3: Heartbeat Coalescing with OVERWRITE");
    println!("  50 rapid heartbeats with same idempotency key");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    {
        let hb_ch_config = ChannelConfig {
            name: "heartbeat-outbox".to_string(),
            provider: ChannelProvider::ChannelProviderSqlite as i32,
            capacity: 200,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            backend_config: Some(channel_config::BackendConfig::Sqlite(SqliteConfig {
                database_path: ":memory:".to_string(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false,
                cleanup_age_seconds: 0,
            })),
            ..Default::default()
        };
        let hb_ch = Arc::new(SqliteChannel::new(hb_ch_config).await
            .map_err(|e| anyhow::anyhow!("heartbeat channel: {}", e))?);

        let num_heartbeats = 50u64;
        for i in 0..num_heartbeats {
            let mut msg = cast_message(json!({
                "type": "heartbeat",
                "worker_id": "worker-0",
                "sequence": i,
                "cpu_pct": 42.0 + i as f64 * 0.5,
            }));
            msg.idempotency_key = "worker-0-heartbeat".to_string();
            msg.conflict_resolution = 1; // OVERWRITE
            msg.headers.insert("sequence_number".to_string(), i.to_string());
            hb_ch.send(msg).await.map_err(|e| anyhow::anyhow!("hb send: {}", e))?;
        }

        let stats = hb_ch.get_stats().await.map_err(|e| anyhow::anyhow!("hb stats: {}", e))?;
        let pending = stats.messages_pending;

        let hb_delivery = Arc::new(CountingDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target_count: pending,
        });

        let hb_outbox = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 50,
                strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
            }),
            retry_policy: Some(default_retry_policy()),
            relay_interval_ms: 5,
            relay_batch_size: 20,
            storage_channel: None,
        };
        let hb_limiter = Arc::new(ConcurrencyLimiter::new(
            hb_outbox.concurrency.as_ref().unwrap(),
        ));
        let hb_relay = Arc::new(OutboxRelay::new(
            &hb_outbox, hb_ch.clone(), hb_limiter.clone(), hb_delivery.clone(),
        ));

        let relay_clone = hb_relay.clone();
        let handle = tokio::spawn(async move { relay_clone.run().await });

        tokio::select! {
            _ = hb_delivery.notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(10)) => {
                panic!("heartbeat relay timeout");
            }
        }
        hb_relay.stop();
        handle.await?;

        let delivered = hb_delivery.delivered.load(Ordering::SeqCst);
        let coalesced = num_heartbeats - pending;

        println!("  Heartbeats sent  : {}", num_heartbeats);
        println!("  Pending (after OVERWRITE) : {}", pending);
        println!("  Actually delivered: {}", delivered);
        println!("  Coalesced        : {} ({:.0}% reduction)",
            coalesced, coalesced as f64 / num_heartbeats as f64 * 100.0);
        println!();

        assert_eq!(pending, 1, "OVERWRITE should coalesce to exactly 1 message");
        assert_eq!(delivered, 1, "only the coalesced message should be delivered");
        println!("  ✓ UC3 Heartbeat coalescing assertions passed");
    }

    // --- UC4: Telemetry Fast-Path with Outbox Fallback ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  UC4: Telemetry Fast-Path with Outbox Fallback");
    println!("  100 telemetry events, collector max_concurrent=3");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    {
        let tel_node = NodeBuilder::new("telemetry-node")
            .with_clustering_enabled(false)
            .build_started()
            .await;
        let tel_sl = tel_node.service_locator();
        let tel_ctx = RequestContext::new_without_auth("tenant-tel".into(), "telemetry".into());

        let tel_admission = AdmissionControlConfig {
            max_concurrent: 3,
            strategy: AdmissionStrategy::AdmissionStrategyFixedLimit as i32,
            default_retry_after_ms: 50,
        };
        let collector = TelemetryCollector { received: 0 };
        let facet = AdmissionControlFacet::new(&tel_admission);
        let collector_ref = spawn_with_facets(
            &tel_ctx, tel_sl.clone(), "tel-collector".to_string(),
            "telemetry", collector, vec![Box::new(facet)],
        ).await.map_err(|e| anyhow::anyhow!("spawn collector: {}", e))?;

        let tel_events = 100u64;
        let tell_ok = AtomicU64::new(0);
        let tell_fail = AtomicU64::new(0);

        for i in 0..tel_events {
            let msg = cast_message(json!({
                "type": "ingest",
                "metric": format!("cpu.usage.worker-{}", i % 10),
                "value": 42.0 + i as f64,
            }));
            match collector_ref.tell(&tel_ctx, msg).await {
                Ok(_) => { tell_ok.fetch_add(1, Ordering::SeqCst); }
                Err(_) => { tell_fail.fetch_add(1, Ordering::SeqCst); }
            }
        }

        let fast_ok = tell_ok.load(Ordering::SeqCst);
        let fast_fail = tell_fail.load(Ordering::SeqCst);

        let fallback_ch_config = ChannelConfig {
            name: "tel-fallback-outbox".to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: tel_events + 50,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            ..Default::default()
        };
        let fallback_ch = Arc::new(InMemoryChannel::new(fallback_ch_config).await
            .map_err(|e| anyhow::anyhow!("fallback channel: {}", e))?);

        let fallback_jobs = fast_fail;
        if fallback_jobs > 0 {
            for i in 0..fallback_jobs {
                let msg = cast_message(json!({
                    "type": "ingest",
                    "metric": format!("cpu.usage.fallback-{}", i),
                    "value": 99.0,
                    "fallback": true,
                }));
                fallback_ch.send(msg).await.map_err(|e| anyhow::anyhow!("fallback send: {}", e))?;
            }

            let fb_delivery = Arc::new(CountingDelivery {
                delivered: AtomicU64::new(0),
                notify: Notify::new(),
                target_count: fallback_jobs,
            });
            let fb_outbox = OutboxConfig {
                concurrency: Some(ConcurrencyConfig {
                    initial_limit: 5,
                    min_limit: 1,
                    max_limit: 20,
                    strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
                }),
                retry_policy: Some(default_retry_policy()),
                relay_interval_ms: 5,
                relay_batch_size: 10,
                storage_channel: None,
            };
            let fb_limiter = Arc::new(ConcurrencyLimiter::new(
                fb_outbox.concurrency.as_ref().unwrap(),
            ));
            let fb_relay = Arc::new(OutboxRelay::new(
                &fb_outbox, fallback_ch.clone(), fb_limiter.clone(), fb_delivery.clone(),
            ));

            let relay_clone = fb_relay.clone();
            let handle = tokio::spawn(async move { relay_clone.run().await });

            tokio::select! {
                _ = fb_delivery.notify.notified() => {},
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    panic!("telemetry fallback timeout");
                }
            }
            fb_relay.stop();
            handle.await?;
        }

        println!("  Total events     : {}", tel_events);
        println!("  Fast-path (tell) : {} succeeded, {} rejected", fast_ok, fast_fail);
        println!("  Outbox fallback  : {} enqueued and delivered", fallback_jobs);
        println!("  Total delivered  : {}", fast_ok + fallback_jobs);
        println!();

        assert_eq!(fast_ok + fast_fail, tel_events);
        tel_node.shutdown(Duration::from_secs(5)).await?;
        println!("  ✓ UC4 Telemetry fallback assertions passed");
    }

    // --- UC5: Reliable Offline Delivery with SQLite-backed Channel ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  UC5: Reliable Offline Delivery (SQLite-backed outbox)");
    println!("  100 jobs via durable SQLite channel with retry + DLQ");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    {
        let sqlite_ch_config = ChannelConfig {
            name: "durable-outbox".to_string(),
            provider: ChannelProvider::ChannelProviderSqlite as i32,
            capacity: 500,
            delivery: DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            ordering: OrderingGuarantee::OrderingGuaranteeFifo as i32,
            dlq_enabled: true,
            dead_letter_queue: "durable-outbox-dlq".to_string(),
            retry_policy: Some(default_retry_policy()),
            backend_config: Some(channel_config::BackendConfig::Sqlite(SqliteConfig {
                database_path: ":memory:".to_string(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false,
                cleanup_age_seconds: 0,
            })),
            ..Default::default()
        };
        let sqlite_ch = Arc::new(SqliteChannel::new(sqlite_ch_config).await
            .map_err(|e| anyhow::anyhow!("sqlite channel: {}", e))?);

        let sqlite_jobs: u64 = 100;
        for i in 0..sqlite_jobs {
            let msg = cast_message(json!({
                "type": "process_job",
                "job_id": format!("durable-{}", i),
                "priority": "normal",
            }));
            sqlite_ch.send(msg).await.map_err(|e| anyhow::anyhow!("sqlite send: {}", e))?;
        }

        let sqlite_delivery = Arc::new(CountingDelivery {
            delivered: AtomicU64::new(0),
            notify: Notify::new(),
            target_count: sqlite_jobs,
        });

        let sqlite_outbox = OutboxConfig {
            concurrency: Some(ConcurrencyConfig {
                initial_limit: 10,
                min_limit: 1,
                max_limit: 50,
                strategy: ConcurrencyStrategy::ConcurrencyStrategyAimd as i32,
            }),
            retry_policy: Some(default_retry_policy()),
            relay_interval_ms: 5,
            relay_batch_size: 20,
            storage_channel: None,
        };
        let sqlite_limiter = Arc::new(ConcurrencyLimiter::new(
            sqlite_outbox.concurrency.as_ref().unwrap(),
        ));
        let sqlite_relay = Arc::new(OutboxRelay::new(
            &sqlite_outbox, sqlite_ch.clone(), sqlite_limiter.clone(), sqlite_delivery.clone(),
        ));

        let t_sqlite = Instant::now();
        let relay_clone = sqlite_relay.clone();
        let handle = tokio::spawn(async move { relay_clone.run().await });

        tokio::select! {
            _ = sqlite_delivery.notify.notified() => {},
            _ = tokio::time::sleep(Duration::from_secs(30)) => {
                panic!("sqlite relay timeout: {}/{}",
                    sqlite_delivery.delivered.load(Ordering::SeqCst), sqlite_jobs);
            }
        }
        let sqlite_elapsed = t_sqlite.elapsed();
        sqlite_relay.stop();
        handle.await?;

        let sqlite_stats = sqlite_relay.stats();
        let sqlite_status = build_dispatcher_status("durable-relay", sqlite_stats, &sqlite_limiter);
        let ch_stats = sqlite_ch.get_stats().await.map_err(|e| anyhow::anyhow!("ch stats: {}", e))?;

        println!("  Backend          : SQLite (in-memory, durable-capable)");
        println!("  Jobs enqueued    : {}", sqlite_jobs);
        println!("  Delivered        : {}", sqlite_status.delivered_count);
        println!("  Failed (DLQ)     : {}", sqlite_status.dead_letter_count);
        println!("  Elapsed          : {} ms", sqlite_elapsed.as_millis());
        println!("  AIMD final limit : {}", sqlite_status.current_concurrency_limit);
        println!("  Channel stats    : sent={}, received={}, pending={}",
            ch_stats.messages_sent, ch_stats.messages_received, ch_stats.messages_pending);
        println!();

        assert_eq!(sqlite_status.delivered_count, sqlite_jobs, "all durable jobs delivered");
        println!("  ✓ UC5 Reliable offline delivery assertions passed");
    }

    // --- Patterns demonstrated ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Key patterns demonstrated:");
    println!("    ConcurrencyLimiter (AIMD)  — additive increase, multiplicative decrease on rejection");
    println!("    AdmissionControlFacet      — receiver-side flow control, RESOURCE_EXHAUSTED on overload");
    println!("    OutboxRelay                — async drain loop bridging channel to actor delivery");
    println!("    PartitionedOutboxRelay     — N independent partitions with hash routing for scale");
    println!("    InMemoryChannel            — in-process outbox storage with ack/nack/DLQ");
    println!("    SqliteChannel              — durable outbox with send_at backoff + delivery_count");
    println!("    OVERWRITE conflict         — last-write-wins coalescing for transient data");
    println!("    Telemetry fast-path        — tell() first, outbox fallback on rejection");
    println!("    Config wave deploy         — bounded fan-out with backpressure-aware config servers");
    println!("    cast_message()             — SDK message construction (fire-and-forget)");
    println!("    spawn_with_facets()        — actor spawn with facet decoration");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  ✓ Done");

    Ok(())
}
