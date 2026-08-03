// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Performance / benchmark tests for plexspaces-node.
//
// All tests are tagged #[ignore] so they do NOT run during normal `cargo test`.
// Run explicitly with:
//   cargo test -p plexspaces-node --test performance_tests -- --include-ignored

use async_trait::async_trait;
use plexspaces_actor::behavior::GenServer;
use plexspaces_actor::{
    ActorBuilder, ActorContext, ActorId, BehaviorError, BehaviorType, Message, RequestContext,
    RequestContextExt,
};
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Shared helper: a minimal GenServer actor that handles call/reply
// ---------------------------------------------------------------------------

struct EchoActor;

#[async_trait]
impl plexspaces_actor::Actor for EchoActor {
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
        // Echo the payload back as the reply
        let reply = Message {
            id: ulid::Ulid::new().to_string(),
            payload: msg.payload.clone(),
            ..Default::default()
        };
        if !msg.sender_id.is_empty() {
            let receiver_id = ActorId::from_canonical(&msg.receiver_id)
                .map_err(|e| BehaviorError::ProcessingError(format!("bad receiver_id: {}", e)))?;
            let correlation_id = if msg.correlation_id.is_empty() {
                None
            } else {
                Some(msg.correlation_id.as_str())
            };
            ctx.send_reply(correlation_id, &msg.sender_id, receiver_id, reply)
                .await
                .map_err(|e| BehaviorError::ProcessingError(format!("send_reply failed: {}", e)))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helper to spawn EchoActor on a fresh node
// ---------------------------------------------------------------------------

async fn spawn_echo_actor(node: &plexspaces_node::Node, name: &str) -> plexspaces_actor::ActorRef {
    use plexspaces_actor::ActorFactoryImpl;

    let actor = ActorBuilder::new(Box::new(EchoActor))
        .with_name(name)
        .build()
        .await
        .expect("ActorBuilder failed");

    let actor_factory = node
        .service_locator()
        .get_actor_factory()
        .await
        .expect("ActorFactory not available");

    let ctx = RequestContext::new_without_auth("default".into(), "default".into());

    let factory_impl = actor_factory
        .as_any()
        .downcast_ref::<ActorFactoryImpl>()
        .expect("not ActorFactoryImpl");

    factory_impl
        .spawn_built_actor_impl(
            &ctx,
            Arc::new(actor),
            "GenServer".to_string(),
            vec![],
            std::collections::HashMap::new(),
        )
        .await
        .expect("spawn_built_actor_impl failed")
}

// ---------------------------------------------------------------------------
// Test 1: local actor routing — 1 000 tell() messages
// ---------------------------------------------------------------------------

/// Spawn one local actor and fire 1 000 `tell()` messages. Asserts completion
/// in < 10 s and prints throughput.
#[tokio::test]
#[ignore]
async fn perf_local_actor_routing_1k() {
    const N: u64 = 1_000;

    let node = Arc::new(NodeBuilder::new("perf-node-routing").build().await);
    let actor_ref = spawn_echo_actor(&node, "perf-echo").await;

    // Warm-up: one message before timing
    let warmup = Message {
        id: ulid::Ulid::new().to_string(),
        payload: b"warmup".to_vec(),
        ..Default::default()
    };
    let ctx = RequestContext::new_without_auth("default".into(), "default".into());
    actor_ref
        .tell(&ctx, warmup)
        .await
        .expect("warmup tell failed");

    // Timed loop
    let start = Instant::now();
    for i in 0..N {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: format!("msg-{}", i).into_bytes(),
            ..Default::default()
        };
        actor_ref.tell(&ctx, msg).await.expect("tell failed");
    }
    let elapsed = start.elapsed();

    let throughput = N as f64 / elapsed.as_secs_f64();
    println!(
        "[perf_local_actor_routing_1k] {} messages in {:.3}s  ({:.0} msg/s)",
        N,
        elapsed.as_secs_f64(),
        throughput,
    );

    assert!(
        elapsed.as_secs() < 10,
        "Expected < 10s, got {:.3}s",
        elapsed.as_secs_f64()
    );
}

// ---------------------------------------------------------------------------
// Test 2: actor spawn throughput — 50 sequential spawns
// ---------------------------------------------------------------------------

/// Spawn 50 actors sequentially. Asserts completion in < 5 s and prints
/// spawn/sec.
#[tokio::test]
#[ignore]
async fn perf_actor_spawn_throughput_50() {
    const N: usize = 50;

    let node = Arc::new(NodeBuilder::new("perf-node-spawn").build().await);

    let start = Instant::now();
    for i in 0..N {
        let name = format!("perf-spawn-{}", i);
        let _actor_ref = spawn_echo_actor(&node, &name).await;
    }
    let elapsed = start.elapsed();

    let throughput = N as f64 / elapsed.as_secs_f64();
    println!(
        "[perf_actor_spawn_throughput_50] {} spawns in {:.3}s  ({:.1} spawn/s)",
        N,
        elapsed.as_secs_f64(),
        throughput,
    );

    assert!(
        elapsed.as_secs() < 5,
        "Expected < 5s, got {:.3}s",
        elapsed.as_secs_f64()
    );
}

// ---------------------------------------------------------------------------
// Test 3: ask/reply latency — 100 sequential roundtrips
// ---------------------------------------------------------------------------

/// 100 sequential ask/reply round-trips over local node routing.
/// Asserts p50 < 50 ms and prints p50 / p95 / p99.
#[tokio::test]
#[ignore]
async fn perf_ask_reply_latency_100() {
    use std::time::Duration;

    const N: usize = 100;

    let node = Arc::new(NodeBuilder::new("perf-node-ask").build().await);
    let actor_ref = spawn_echo_actor(&node, "perf-ask-echo").await;

    let ctx = RequestContext::new_without_auth("default".into(), "default".into());

    // Warm-up round
    let warmup = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: b"warmup".to_vec(),
        ..Default::default()
    };
    let _ = actor_ref
        .ask(&ctx, warmup, Duration::from_secs(5))
        .await
        .expect("warmup ask failed");

    // Timed round-trips
    let mut latencies_us: Vec<u128> = Vec::with_capacity(N);
    for i in 0..N {
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            message_type: "call".to_string(),
            payload: format!("rtt-{}", i).into_bytes(),
            ..Default::default()
        };
        let t0 = Instant::now();
        let _ = actor_ref
            .ask(&ctx, msg, Duration::from_secs(5))
            .await
            .expect("ask failed");
        latencies_us.push(t0.elapsed().as_micros());
    }

    latencies_us.sort_unstable();
    let p50 = latencies_us[N / 2];
    let p95 = latencies_us[(N as f64 * 0.95) as usize];
    let p99 = latencies_us[(N as f64 * 0.99) as usize];

    println!(
        "[perf_ask_reply_latency_100] {} round-trips  p50={:.3}ms  p95={:.3}ms  p99={:.3}ms",
        N,
        p50 as f64 / 1_000.0,
        p95 as f64 / 1_000.0,
        p99 as f64 / 1_000.0,
    );

    assert!(
        p50 < 50_000,
        "p50 latency {} µs exceeds 50 ms threshold",
        p50
    );
}
