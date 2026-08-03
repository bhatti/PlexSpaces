// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Performance / benchmark tests for plexspaces-sdk.
//
// All tests are tagged #[ignore] so they do NOT run during normal `cargo test`.
// Run explicitly with:
//   cargo test -p plexspaces-sdk --test sdk_performance_tests -- --include-ignored

use plexspaces_actor::{RequestContext, RequestContextExt};
use plexspaces_node::NodeBuilder;
use plexspaces_sdk::{
    call_message, gen_server_actor, json, plexspaces_handlers, spawn, ActorContext, BehaviorError,
    Message, Value,
};
use std::sync::Arc;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Counter actor used for all SDK perf tests
// ---------------------------------------------------------------------------

#[gen_server_actor]
struct SdkPerfCounter {
    count: i64,
}

impl SdkPerfCounter {
    fn new() -> Self {
        Self { count: 0 }
    }
}

#[plexspaces_handlers]
impl SdkPerfCounter {
    #[handler("increment")]
    async fn increment(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        self.count += 1;
        Ok(json!({ "count": self.count }))
    }

    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "count": self.count }))
    }
}

// ---------------------------------------------------------------------------
// Test 1: 1 000 sequential call_message + ask round-trips via SDK
// ---------------------------------------------------------------------------

/// Spawn a counter actor via SDK helpers, then send 1 000 sequential
/// `call_message` + `ask` round-trips. Asserts completion in < 10 s and
/// prints throughput.
#[tokio::test]
#[ignore]
async fn perf_call_message_ask_1k() {
    const N: u64 = 1_000;

    let node = Arc::new(
        NodeBuilder::new("perf-sdk-node")
            .with_in_memory_backends()
            .build()
            .await,
    );
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("perf-tenant".into(), "perf-ns".into());

    let actor_ref = spawn(
        &ctx,
        service_locator,
        "perf-counter",
        "perf-ns",
        SdkPerfCounter::new(),
    )
    .await
    .expect("spawn failed");

    // Warm-up round
    let warmup = call_message(json!({ "action": "get" }));
    let _ = actor_ref
        .ask(&ctx, warmup, Duration::from_secs(5))
        .await
        .expect("warmup ask failed");

    // Timed loop
    let start = Instant::now();
    for _ in 0..N {
        let msg = call_message(json!({ "action": "increment" }));
        let _ = actor_ref
            .ask(&ctx, msg, Duration::from_secs(5))
            .await
            .expect("ask failed");
    }
    let elapsed = start.elapsed();

    let throughput = N as f64 / elapsed.as_secs_f64();
    println!(
        "[perf_call_message_ask_1k] {} round-trips in {:.3}s  ({:.0} rtt/s)",
        N,
        elapsed.as_secs_f64(),
        throughput,
    );

    // Verify final count
    let get_msg = call_message(json!({ "action": "get" }));
    let reply = actor_ref
        .ask(&ctx, get_msg, Duration::from_secs(5))
        .await
        .expect("final get failed");
    let body: Value = serde_json::from_slice(&reply.payload).unwrap_or(json!({}));
    assert_eq!(
        body.get("count").and_then(|v| v.as_i64()),
        Some(N as i64),
        "counter should equal N"
    );

    assert!(
        elapsed.as_secs() < 10,
        "Expected < 10s, got {:.3}s",
        elapsed.as_secs_f64()
    );
}
