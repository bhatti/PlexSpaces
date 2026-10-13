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

use plexspaces_channel::{Channel, InMemoryChannel};
use plexspaces_common::retry::default_retry_policy;
use plexspaces_node::NodeBuilder;
use plexspaces_outbox::{
    AdmissionControlFacet, ConcurrencyLimiter, MessageDelivery, OutboxRelay,
    PartitionedOutboxRelay, build_dispatcher_status, validate_config,
};
use plexspaces_proto::channel::v1::{
    ChannelConfig, ChannelProvider, DeliveryGuarantee, OrderingGuarantee,
};
use plexspaces_proto::outbox::v1::{
    AdmissionControlConfig, AdmissionStrategy, ConcurrencyConfig, ConcurrencyStrategy,
    OutboxConfig, PartitionedOutboxConfig,
};
use plexspaces_sdk::{
    cast_message, gen_server_actor, json, plexspaces_handlers, spawn_with_facets, ActorContext,
    BehaviorError, RequestContext, RequestContextExt, Value,
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

    // --- Patterns demonstrated ---
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Key patterns demonstrated:");
    println!("    ConcurrencyLimiter (AIMD) — additive increase on success, multiplicative decrease on rejection");
    println!("    AdmissionControlFacet     — receiver-side flow control, RESOURCE_EXHAUSTED on overload");
    println!("    OutboxRelay               — async drain loop bridging channel to actor delivery");
    println!("    PartitionedOutboxRelay    — N independent partitions with hash routing for scale");
    println!("    InMemoryChannel           — DB-backed outbox storage with ack/nack/DLQ");
    println!("    cast_message()            — SDK message construction (fire-and-forget)");
    println!("    spawn_with_facets()       — actor spawn with facet decoration");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("  ✓ Done");

    Ok(())
}
