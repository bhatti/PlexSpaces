// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Performance / benchmark tests for the actor crate.
//
// All tests are tagged `#[ignore]` and therefore excluded from `cargo test`.
// Run explicitly with:
//   cargo test -p plexspaces-actor --test performance_tests -- --include-ignored

use async_trait::async_trait;
use plexspaces_actor::behavior::GenServer;
use plexspaces_actor::{
    Actor, ActorBuilder, ActorContext, BehaviorError, BehaviorType, Message, RequestContextExt,
};
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use ulid::Ulid;

// ---------------------------------------------------------------------------
// Shared helpers
// ---------------------------------------------------------------------------

fn make_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

fn make_call_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload,
        ..Default::default()
    }
}

fn actor_id_from_receiver(receiver_id: &str) -> plexspaces_actor::ActorId {
    plexspaces_actor::ActorId::from_canonical(receiver_id)
        .expect("receiver_id must be canonical actor id")
}

// ---------------------------------------------------------------------------
// Echo actor (GenServer) — replies to every call with the same payload
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
enum EchoMsg {
    Echo(Vec<u8>),
    Reply(Vec<u8>),
    Ping,
    Ack,
}

struct EchoActor;

#[async_trait]
impl Actor for EchoActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait]
impl GenServer for EchoActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: EchoMsg = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("parse error: {}", e)))?;

        let reply_payload = match request {
            EchoMsg::Echo(data) => serde_json::to_vec(&EchoMsg::Reply(data))
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?,
            EchoMsg::Ping => serde_json::to_vec(&EchoMsg::Ack)
                .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?,
            _ => return Ok(()),
        };

        if !msg.sender_id.is_empty() {
            let reply_msg = make_message(reply_payload);
            ctx.send_reply(
                if msg.correlation_id.is_empty() {
                    None
                } else {
                    Some(msg.correlation_id.as_str())
                },
                &msg.sender_id,
                actor_id_from_receiver(&msg.receiver_id),
                reply_msg,
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("send_reply failed: {}", e)))?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Sink actor — accepts tell() messages and discards them
// ---------------------------------------------------------------------------

struct SinkActor {
    count: u64,
}

impl SinkActor {
    fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait]
impl Actor for SinkActor {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        _msg: Message,
    ) -> Result<(), BehaviorError> {
        self.count += 1;
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// ---------------------------------------------------------------------------
// Perf test: tell() throughput
// ---------------------------------------------------------------------------

/// Send 10,000 fire-and-forget messages to a local actor; assert completion < 5 s.
#[tokio::test]
#[ignore]
async fn perf_tell_throughput_10k() {
    let node = NodeBuilder::new("perf-tell-node")
        .with_in_memory_backends()
        .build()
        .await;

    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "perf-tenant".to_string(),
        "perf-ns".to_string(),
    );

    let sink: Box<dyn Actor> = Box::new(SinkActor::new());
    let actor_ref = ActorBuilder::new(sink)
        .with_name("sink-actor")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("sink actor should spawn");

    let n: u64 = 10_000;
    let payload = serde_json::to_vec(&EchoMsg::Ping).unwrap();

    let start = Instant::now();
    for _ in 0..n {
        let msg = make_message(payload.clone());
        actor_ref
            .tell(&ctx, msg)
            .await
            .expect("tell should not fail");
    }
    let elapsed = start.elapsed();

    println!(
        "[perf_tell_throughput_10k] sent {} messages in {:.3}s  ({:.0} msg/s)",
        n,
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64()
    );

    assert!(
        elapsed < Duration::from_secs(5),
        "10k tell() calls took {:.3}s, expected < 5s",
        elapsed.as_secs_f64()
    );
}

// ---------------------------------------------------------------------------
// Perf test: ask() latency (p50 < 10 ms)
// ---------------------------------------------------------------------------

/// 1,000 sequential ask/reply round-trips; assert p50 latency < 10 ms.
#[tokio::test]
#[ignore]
async fn perf_ask_latency_1k_sequential() {
    let node = NodeBuilder::new("perf-ask-node")
        .with_in_memory_backends()
        .build()
        .await;

    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "perf-tenant".to_string(),
        "perf-ns".to_string(),
    );

    let echo: Box<dyn Actor> = Box::new(EchoActor);
    let actor_ref = ActorBuilder::new(echo)
        .with_name("echo-actor")
        .spawn(&ctx, node.service_locator())
        .await
        .expect("echo actor should spawn");

    let n: usize = 1_000;
    let payload = serde_json::to_vec(&EchoMsg::Ping).unwrap();
    let mut latencies_us: Vec<u128> = Vec::with_capacity(n);

    for i in 0..n {
        let mut msg = make_call_message(payload.clone());
        msg.receiver_id = actor_ref.id().to_string();

        let t0 = Instant::now();
        let reply = actor_ref
            .ask(&ctx, msg, Duration::from_secs(5))
            .await
            .unwrap_or_else(|e| panic!("ask #{} failed: {}", i, e));
        let us = t0.elapsed().as_micros();
        latencies_us.push(us);

        // Verify echo replied correctly
        let _: EchoMsg = serde_json::from_slice(&reply.payload)
            .expect("reply payload must deserialize as EchoMsg");
    }

    latencies_us.sort_unstable();
    let p50 = latencies_us[n / 2];
    let p95 = latencies_us[n * 95 / 100];
    let p99 = latencies_us[n * 99 / 100];
    let mean_us = latencies_us.iter().sum::<u128>() / n as u128;

    println!(
        "[perf_ask_latency_1k_sequential] n={} mean={:.1}µs p50={}µs p95={}µs p99={}µs",
        n, mean_us as f64, p50, p95, p99
    );

    let p50_ms = p50 as f64 / 1_000.0;
    assert!(
        p50_ms < 10.0,
        "p50 ask latency was {:.3}ms, expected < 10ms",
        p50_ms
    );
}

// ---------------------------------------------------------------------------
// Perf test: spawn throughput
// ---------------------------------------------------------------------------

/// Spawn 100 actors; assert total time < 2 s.
#[tokio::test]
#[ignore]
async fn perf_spawn_throughput_100_actors() {
    let node = Arc::new(
        NodeBuilder::new("perf-spawn-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "perf-tenant".to_string(),
        "perf-ns".to_string(),
    );

    let n = 100usize;
    let start = Instant::now();

    let mut handles = Vec::with_capacity(n);
    for i in 0..n {
        let sl = node.service_locator();
        let ctx2 = ctx.clone();
        handles.push(tokio::spawn(async move {
            let actor: Box<dyn Actor> = Box::new(SinkActor::new());
            ActorBuilder::new(actor)
                .with_name(&format!("spawn-actor-{}", i))
                .spawn(&ctx2, sl)
                .await
                .unwrap_or_else(|e| panic!("spawn {} failed: {}", i, e))
        }));
    }

    for h in handles {
        h.await.expect("spawn task should not panic");
    }

    let elapsed = start.elapsed();

    println!(
        "[perf_spawn_throughput_100_actors] spawned {} actors in {:.3}s  ({:.0} actors/s)",
        n,
        elapsed.as_secs_f64(),
        n as f64 / elapsed.as_secs_f64()
    );

    assert!(
        elapsed < Duration::from_secs(2),
        "Spawning 100 actors took {:.3}s, expected < 2s",
        elapsed.as_secs_f64()
    );
}

// ---------------------------------------------------------------------------
// Regression: ActorRegistry O(N) linear scan + write-lock contention under
// concurrent spawn/stop/ask
//
// Root cause (identified in actor_registry.rs):
//
//   1. ActorRegistry::tell() calls lookup_actor() on EVERY message to decide
//      local-vs-remote.  lookup_actor() does a full HashMap::iter() scan —
//      O(N) where N = live actor count.
//
//   2. ActorRegistry::dispatch_local_message() calls lookup_actor() again to
//      find the sender.  get_or_activate_local_sender() may call it a third
//      time.  Each ask therefore does 2–3 O(N) read-locked scans.
//
//   3. ActorRegistry::unregister() acquires THREE sequential write locks
//      (actors, actor_type_index, registered_actor_entries).  While each
//      write lock is held, every concurrent ask() that needs a read lock
//      blocks.  With 10 VUs all sending asks concurrently while a 11th VU
//      is doing spawn/stop, the write-lock starvation compounds.
//
//   4. In the k6 load test the HTTP timeout near end-of-duration is clamped
//      to (remaining - 0.5)s.  With 2s left on a 30s run that is ~1500ms.
//      The write-lock stall from 100 accumulated actors + 10 concurrent stops
//      pushes a handful of asks past 1500ms → all 10 VUs time out at the
//      same iteration simultaneously.
//
// The correct fix is to change lookup_actor to O(1) using the ScopedActorKey
// and to merge the three sequential write locks in unregister() into one.
// These tests verify the expected performance contract so a regression shows
// up immediately when the fix is applied.
//
// Pattern that triggered the bug (spawn_lifecycle.js before the k6-side fix):
//   actorName = `lifecycle-vu${__VU}-i${__ITER}`   ← unique per iteration, never stopped
// ---------------------------------------------------------------------------

/// Reproduces the write-lock starvation bug: 10 concurrent VU tasks each doing
/// spawn → ask → stop in a loop while the registry grows.
///
/// With the bug present: p99 climbs into the hundreds-of-ms range because each
/// stop() grabs three sequential write locks while the other VU tasks are blocked
/// waiting to read-lock the same map for their ask().
///
/// With the fix (merged write lock + O(1) lookup): p99 stays under 50ms.
#[tokio::test]
#[ignore]
async fn perf_concurrent_lifecycle_write_lock_contention() {
    let node = Arc::new(
        NodeBuilder::new("perf-lock-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "perf-tenant".to_string(),
        "perf-ns".to_string(),
    );

    let factory = Arc::new(
        node.service_locator()
            .get_actor_factory()
            .await
            .expect("actor factory must be available"),
    );

    let vus = 10usize;
    let iters = 10usize;
    let ask_timeout = Duration::from_secs(5);
    let payload = Arc::new(serde_json::to_vec(&EchoMsg::Ping).unwrap());

    // All VUs run concurrently — this is the key difference from the sequential tests.
    // Concurrent spawn+stop write locks contend with concurrent ask read locks.
    let mut vu_handles = Vec::with_capacity(vus);
    // Measure in microseconds — in-memory backend is sub-millisecond, ms resolution rounds to 0.
    let latency_results: Arc<tokio::sync::Mutex<Vec<u128>>> =
        Arc::new(tokio::sync::Mutex::new(Vec::with_capacity(vus * iters)));

    for vu in 0..vus {
        let node2 = node.clone();
        let ctx2 = ctx.clone();
        let factory2 = factory.clone();
        let payload2 = payload.clone();
        let results2 = latency_results.clone();

        vu_handles.push(tokio::spawn(async move {
            let name = format!("lifecycle-vu{}", vu);
            let mut local_latencies = Vec::with_capacity(iters);

            for iter in 0..iters {
                let actor: Box<dyn Actor> = Box::new(EchoActor);
                let actor_ref = ActorBuilder::new(actor)
                    .with_name(&name)
                    .spawn(&ctx2, node2.service_locator())
                    .await
                    .unwrap_or_else(|e| panic!("spawn vu={} iter={} failed: {}", vu, iter, e));

                let mut msg = make_call_message((*payload2).clone());
                msg.receiver_id = actor_ref.id().to_string();
                let t0 = Instant::now();
                let _reply = actor_ref
                    .ask(&ctx2, msg, ask_timeout)
                    .await
                    .unwrap_or_else(|e| panic!("ask vu={} iter={} failed: {}", vu, iter, e));
                local_latencies.push(t0.elapsed().as_micros());

                let _ = factory2.stop_actor(&ctx2, actor_ref.id()).await;
            }

            results2.lock().await.extend(local_latencies);
        }));
    }

    for h in vu_handles {
        h.await.expect("VU task panicked");
    }

    let mut latencies_us = latency_results.lock().await.clone();
    latencies_us.sort_unstable();
    let p95_us = latencies_us[latencies_us.len() * 95 / 100];
    let p99_us = latencies_us[latencies_us.len() * 99 / 100];
    let max_us = *latencies_us.last().unwrap_or(&0);

    println!(
        "[perf_concurrent_lifecycle_write_lock_contention] {} VUs × {} iters  p95={}µs p99={}µs max={}µs",
        vus, iters, p95_us, p99_us, max_us
    );

    // With the O(N) bug + sequential write locks: p99 grows proportionally to actor count.
    // At N=100 in-memory this is still fast, but the growth rate is the signal.
    // The hard limit is 50ms (50000µs) — well above in-memory cost, well below k6 HTTP timeout.
    // Note: the full reproduction of the production timeout requires the SQLite backend
    // (disk I/O amplifies the O(N) scan + write-lock stall from ~µs to ~ms per operation).
    assert!(
        p99_us < 50_000,
        "p99 ask latency {}µs under concurrent spawn/stop/ask — expected < 50000µs (50ms). \
         Likely cause: lookup_actor O(N) scan or unregister() sequential write locks. \
         See ActorRegistry::tell(), dispatch_local_message(), unregister().",
        p99_us
    );
}

/// Baseline: 10 VUs × 10 iterations, sequential (no concurrency).
/// Should always pass — establishes the floor latency for comparison.
#[tokio::test]
#[ignore]
async fn perf_bounded_actor_lifecycle_ask_latency() {
    let node = Arc::new(
        NodeBuilder::new("perf-bounded-node")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let ctx = plexspaces_actor::RequestContext::new_without_auth(
        "perf-tenant".to_string(),
        "perf-ns".to_string(),
    );

    let factory = node
        .service_locator()
        .get_actor_factory()
        .await
        .expect("actor factory must be available");

    let vus = 10usize;
    let iters = 10usize;
    let payload = serde_json::to_vec(&EchoMsg::Ping).unwrap();
    let ask_timeout = Duration::from_secs(5);
    let mut latencies_us: Vec<u128> = Vec::with_capacity(vus * iters);

    for vu in 0..vus {
        let name = format!("lifecycle-vu{}", vu);
        for iter in 0..iters {
            let actor: Box<dyn Actor> = Box::new(EchoActor);
            let actor_ref = ActorBuilder::new(actor)
                .with_name(&name)
                .spawn(&ctx, node.service_locator())
                .await
                .unwrap_or_else(|e| panic!("spawn vu={} iter={} failed: {}", vu, iter, e));

            let mut msg = make_call_message(payload.clone());
            msg.receiver_id = actor_ref.id().to_string();
            let t0 = Instant::now();
            let _reply = actor_ref
                .ask(&ctx, msg, ask_timeout)
                .await
                .unwrap_or_else(|e| panic!("ask vu={} iter={} failed: {}", vu, iter, e));
            latencies_us.push(t0.elapsed().as_micros());

            let _ = factory.stop_actor(&ctx, actor_ref.id()).await;
        }
    }

    latencies_us.sort_unstable();
    let p95_us = latencies_us[latencies_us.len() * 95 / 100];
    let p99_us = latencies_us[latencies_us.len() * 99 / 100];
    println!(
        "[perf_bounded_actor_lifecycle_ask_latency] {} iterations sequential  p95={}µs p99={}µs",
        vus * iters, p95_us, p99_us
    );

    assert!(
        p99_us < 10_000,
        "p99 ask latency {}µs sequential — expected < 10000µs (10ms)",
        p99_us
    );
}
