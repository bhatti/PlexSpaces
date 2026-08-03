// SPDX-License-Identifier: AGPL-3.0-or-later
// PerfActor — Rust embedded actor for PlexSpaces load testing.
//
// Provides two actor types for testing:
//   PerfActor         — regular (pre-spawned) actor, tests steady-state throughput
//   VirtualPerfActor  — virtual actor (spawns on first request), tests activation overhead
//
// These are the BASELINE: native Tokio actors with zero WASM overhead.
// Compare latency/throughput against WASM variants to measure sandbox cost.

use plexspaces_actor::{
    behavior_factory::BehaviorRegistry, Actor as ActorTrait, InitializableServiceLocator,
    RequestContext,
};
use plexspaces_node::NodeBuilder;
use plexspaces_sdk::{
    gen_server_actor, json, plexspaces_handlers, spawn_with_facets, ActorContext,
    BehaviorError, Message, RequestContextExt, Value,
};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

// ─── Lucas-Lehmer Mersenne prime check ────────────────────────────────────────

fn is_mersenne_prime(p: u32) -> bool {
    if p == 2 {
        return true;
    }
    if p < 2 || p > 62 {
        return false;
    }
    let mp: u128 = (1u128 << p) - 1;
    let mut s: u128 = 4;
    for _ in 0..(p - 2) {
        s = (s.wrapping_mul(s).wrapping_sub(2)) % mp;
    }
    s == 0
}

fn gradient_step(values: &[f64], lr: f64) -> Value {
    if values.is_empty() {
        return json!({ "gradient": 0.0, "count": 0 });
    }
    let n = values.len() as f64;
    let mean: f64 = values.iter().sum::<f64>() / n;
    let gradient: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let sample: Vec<f64> = values.iter().take(3).map(|v| v - lr * (v - mean)).collect();
    json!({ "gradient": gradient, "count": values.len(), "mean": mean, "sample": sample })
}

fn parse_or_default<T: serde::de::DeserializeOwned + Default>(payload: &[u8]) -> T {
    serde_json::from_slice(payload).unwrap_or_default()
}

// ─── Regular actor (pre-spawned, tests steady-state throughput) ───────────────

// virtual_actor facet: unknown actor IDs auto-activate on first request, deactivate when idle.
// Pre-warmed pool (perf-vuN) tests steady-state. "virtual-vuN" tests on-demand activation cost.
#[gen_server_actor(facets = ["virtual_actor"])]
struct PerfActor {
    echo_count: u64,
    compute_count: u64,
    kv_count: u64,
    shard_count: u64,
    // in-memory kv store (mirrors what WASM actors do via host-kv)
    kv_store: std::collections::HashMap<String, String>,
}

impl PerfActor {
    fn new() -> Self {
        PerfActor {
            echo_count: 0,
            compute_count: 0,
            kv_count: 0,
            shard_count: 0,
            kv_store: std::collections::HashMap::new(),
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl PerfActor {
    #[handler("echo")]
    async fn handle_echo(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        self.echo_count += 1;
        let payload: Value = serde_json::from_slice(&msg.payload).unwrap_or(json!({}));
        Ok(json!({ "ok": true, "echo": payload, "count": self.echo_count }))
    }

    #[handler("compute")]
    async fn handle_compute(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize, Default)]
        struct Req { p: Option<u32> }
        let req: Req = parse_or_default(&msg.payload);
        let p = req.p.unwrap_or(7);
        let result = is_mersenne_prime(p);
        self.compute_count += 1;
        Ok(json!({ "ok": true, "p": p, "is_mersenne_prime": result, "count": self.compute_count }))
    }

    #[handler("kv_put")]
    async fn handle_kv_put(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize, Default)]
        struct Req { key: Option<String>, value: Option<String> }
        let req: Req = parse_or_default(&msg.payload);
        let key = req.key.unwrap_or_else(|| "perf_key".to_string());
        let value = req.value.unwrap_or_else(|| "perf_val".to_string());
        self.kv_store.insert(key.clone(), value);
        self.kv_count += 1;
        Ok(json!({ "ok": true, "key": key, "count": self.kv_count }))
    }

    #[handler("kv_get")]
    async fn handle_kv_get(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize, Default)]
        struct Req { key: Option<String> }
        let req: Req = parse_or_default(&msg.payload);
        let key = req.key.unwrap_or_else(|| "perf_key".to_string());
        let value = self.kv_store.get(&key).cloned();
        Ok(json!({ "ok": true, "key": key, "value": value }))
    }

    #[handler("shard_task")]
    async fn handle_shard_task(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        #[derive(Deserialize, Default)]
        struct Req { shard_index: Option<u64>, lr: Option<f64>, values: Option<Vec<f64>> }
        let req: Req = parse_or_default(&msg.payload);
        let shard_index = req.shard_index.unwrap_or(0);
        let lr = req.lr.unwrap_or(0.01);
        let values = req.values.unwrap_or_else(|| (0..100).map(|i| i as f64).collect());
        let mut stats = gradient_step(&values, lr);
        self.shard_count += 1;
        stats["ok"] = json!(true);
        stats["shard_index"] = json!(shard_index);
        stats["count"] = json!(self.shard_count);
        Ok(stats)
    }

    #[handler("get_stats")]
    async fn handle_get_stats(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({
            "ok": true,
            "echo_count": self.echo_count,
            "compute_count": self.compute_count,
            "kv_count": self.kv_count,
            "shard_count": self.shard_count,
        }))
    }
}

// Note: virtual_actor facet (on-demand activation) is tested by k6 using
// actor instance names not in the pre-warmed pool (e.g. "virtual-vuN").
// PerfActor with virtual_actor facet handles both: pre-warmed pool + on-demand.

// ─── Entry point ──────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(8091);

    let node = NodeBuilder::new("perf-embedded-node")
        .with_listen_addr(format!("0.0.0.0:{port}"))
        .with_clustering_enabled(false)
        .build_started()
        .await;

    let ctx = RequestContext::new_without_auth("default".to_string(), "perf-embedded".to_string());
    let sl = node.service_locator();

    let warm_count = std::env::var("PERF_WARM_COUNT")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);

    // Pre-spawn regular actors (known IDs, warm mailboxes)
    for i in 0..warm_count {
        let actor_name = format!("perf-vu{i}");
        spawn_with_facets(&ctx, sl.clone(), actor_name, "perf-embedded", PerfActor::new(), vec![])
            .await
            .unwrap_or_else(|e| panic!("failed to pre-spawn perf actor {i}: {e}"));
    }

    // Register BehaviorRegistry so the factory can instantiate PerfActor on-demand
    // for virtual actor activation (unknown instance names like "virtual-vuN").
    let registry = BehaviorRegistry::new();
    registry
        .register("gen_server".to_string(), |_| {
            Box::pin(async { Ok(Box::new(PerfActor::new()) as Box<dyn ActorTrait>) })
        })
        .await;
    sl.register_behavior_registry(Arc::new(registry)).await;

    // Register "gen_server" as a virtual actor type so the HTTP routing layer
    // auto-activates any unknown instance name on first request (e.g. "virtual-vuN").
    if let Some(mgr) = sl.virtual_actor_manager().await {
        mgr.register_virtual_actor_type(
            "gen_server".to_string(),
            None,
            "perf-embedded".to_string(),
            serde_json::json!({ "virtual_actor": { "idle_timeout": "10m", "activation_strategy": "lazy" } }),
            None,
            None,
        )
        .await
        .unwrap_or_else(|e| panic!("failed to register virtual actor type: {e}"));
    }

    info!("PerfActor embedded node ready on port {port}");
    info!("  Pre-warmed actors:   POST .../perf-embedded/perf-vuN:gen_server/ask");
    info!("  Virtual (on-demand): POST .../perf-embedded/virtual-vuN:gen_server/ask  (auto-activates)");

    tokio::signal::ctrl_c().await.ok();
    node.shutdown(std::time::Duration::from_secs(5)).await;
}
