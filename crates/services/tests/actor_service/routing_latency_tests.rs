// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Baseline latency measurements for actor routing.
//! Run: cargo test -p plexspaces-services --test routing_latency_tests -- --nocapture
//! Each test has a hard 25-second deadline; hangs = immediate failure.

use async_trait::async_trait;
use plexspaces_actor::behavior::GenServer;
use plexspaces_actor::behavior_factory::BehaviorRegistry;
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType,
    InitializableServiceLocator, Message, RequestContext, RequestContextExt,
};
use plexspaces_node::{Node, NodeBuilder, ReleaseSpec};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, AskReplyRequest,
};
use plexspaces_proto::node::v1::RuntimeConfig;
use plexspaces_proto::storage::v1::SharedDbConfig;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::process_group_service::ProcessGroupServiceImpl;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tonic::Request;

// ── Echo actor ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct EchoActor;

#[async_trait]
impl ActorTrait for EchoActor {
    async fn handle_message(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }
    fn behavior_type(&self) -> BehaviorType { BehaviorType::GenServer }
}

#[async_trait]
impl GenServer for EchoActor {
    async fn handle_request(&mut self, ctx: &ActorContext, msg: Message) -> Result<(), BehaviorError> {
        if !msg.sender_id.is_empty() {
            let correlation_id = if msg.correlation_id.is_empty() { None } else { Some(msg.correlation_id.as_str()) };
            ctx.send_reply(
                correlation_id,
                &msg.sender_id,
                ctx.actor_id().clone(),
                Message { id: ulid::Ulid::new().to_string(), payload: msg.payload, ..Default::default() },
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("reply: {e}")))?;
        }
        Ok(())
    }
}

// ── Infrastructure ────────────────────────────────────────────────────────────

fn writable_test_db_url(temp_dir: &TempDir) -> String {
    let db_path: PathBuf = temp_dir.path().join("db").join("plexspaces.db");
    std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
    format!("sqlite://{}?mode=rwc", db_path.display())
}

struct TestNode {
    node: Node,
    actor_service: Arc<ActorServiceImpl>,
    _temp_dir: TempDir,
}

async fn build_test_node(node_id: &str) -> TestNode {
    let temp_dir = TempDir::new().unwrap();
    let node = NodeBuilder::new(node_id)
        .with_sqlite_journaling()
        .with_release_spec(ReleaseSpec {
            name: format!("lat-{node_id}"),
            version: "0.0.0-test".to_string(),
            runtime: Some(RuntimeConfig {
                db: Some(SharedDbConfig {
                    connection_string: writable_test_db_url(&temp_dir),
                    auto_migrate: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .build()
        .await;

    let pg: Arc<dyn plexspaces_actor::ProcessGroupService> =
        Arc::new(ProcessGroupServiceImpl::new(node.service_locator(), node_id.to_string()));
    node.service_locator().register_process_group_service(pg).await;

    let registry = BehaviorRegistry::new();
    registry
        .register("echo".to_string(), |_| Box::pin(async { Ok(Box::new(EchoActor) as Box<dyn ActorTrait>) }))
        .await;
    node.service_locator().register_behavior_registry(Arc::new(registry)).await;

    let actor_service = Arc::new(ActorServiceImpl::new(node.service_locator().clone(), node_id.to_string()));
    TestNode { node, actor_service, _temp_dir: temp_dir }
}

async fn spawn_echo(node: &TestNode, namespace: &str, name: &str) {
    use plexspaces_sdk::spawn_with_facets;
    let ctx = RequestContext::new_without_auth("test-tenant".to_string(), namespace.to_string());
    spawn_with_facets(&ctx, node.node.service_locator(), name, namespace, EchoActor, vec![])
        .await
        .expect("spawn_with_facets");
}

async fn timed_ask(svc: &ActorServiceImpl, namespace: &str, name: &str, actor_type: &str) -> Duration {
    let mut req = Request::new(AskReplyRequest {
        request_id: ulid::Ulid::new().to_string(),
        namespace: namespace.to_string(),
        actor_type: actor_type.to_string(),
        actor_name: name.to_string(),
        http_method: "POST".to_string(),
        payload: serde_json::to_vec(&serde_json::json!({"op":"echo","payload":"x"})).unwrap(),
        message_type: "call".to_string(),
        timeout: Some(prost_types::Duration { seconds: 5, nanos: 0 }),
        headers: HashMap::new(),
        ..Default::default()
    });
    req.metadata_mut().insert("x-tenant-id", "test-tenant".parse().unwrap());
    req.metadata_mut().insert("x-namespace", namespace.parse().unwrap());
    let t = Instant::now();
    ActorServiceTrait::ask_reply(svc, req).await.expect("ask_reply");
    t.elapsed()
}

fn print_stats(label: &str, samples: &[Duration]) {
    let mut us: Vec<u128> = samples.iter().map(|d| d.as_micros()).collect();
    us.sort_unstable();
    let n = us.len();
    let mean = us.iter().sum::<u128>() / n as u128;
    let p50 = us[n / 2];
    let p95 = us[(n as f64 * 0.95) as usize];
    let p99 = us[(n as f64 * 0.99) as usize];
    let max = us[n - 1];
    println!("[latency] {label:<45} n={n:>3}  mean={mean:>5}µs  p50={p50:>5}µs  p95={p95:>5}µs  p99={p99:>5}µs  max={max:>5}µs");
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// Pre-warmed actor: O(1) registry lookup + mailbox enqueue + reply.
/// Budget: p99 < 5ms.
#[tokio::test(flavor = "multi_thread")]
async fn regular_actor_latency() {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let node = build_test_node("lat-regular").await;
        let ns = "lat-ns";
        spawn_echo(&node, ns, "echo-0").await;
        // warm-up
        timed_ask(&node.actor_service, ns, "echo-0", "gen_server").await;

        const N: usize = 50;
        let mut samples = Vec::with_capacity(N);
        for _ in 0..N {
            samples.push(timed_ask(&node.actor_service, ns, "echo-0", "gen_server").await);
        }
        print_stats("regular_actor (pre-warmed)    (budget: <2ms) ", &samples);

        let mut us: Vec<u128> = samples.iter().map(|d| d.as_micros()).collect();
        us.sort_unstable();
        let p99 = us[(N as f64 * 0.99) as usize];
        assert!(p99 < 2_000, "regular actor p99 {p99}µs > 2ms");
    })
    .await;
    result.expect("regular_actor_latency timed out after 10s");
}

/// Virtual actor (registered type, no live instance): first call activates; second is warm.
/// Steady-state p99 budget: < 2ms.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_actor_latency() {
    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let node = build_test_node("lat-virtual").await;
        let ns = "lat-vns";

        // Register virtual actor type
        let mgr = node.node.service_locator().virtual_actor_manager().await.unwrap();
        mgr.register_virtual_actor_type(
            "echo".to_string(),
            None,
            ns.to_string(),
            serde_json::json!({"virtual_actor": {"idle_timeout": "5m", "activation_strategy": "lazy"}}),
            None,
            None,
        )
        .await
        .unwrap();

        // First call activates the virtual actor (one-time spawn cost)
        let first = timed_ask(&node.actor_service, ns, "virt-0", "echo").await;
        println!("[latency] virtual_actor first-activation (budget: <2ms)     n=  1  latency={}µs  (one-time spawn cost)", first.as_micros());

        const N: usize = 20;
        let mut samples = Vec::with_capacity(N);
        for _ in 0..N {
            samples.push(timed_ask(&node.actor_service, ns, "virt-0", "echo").await);
        }
        print_stats("virtual_actor steady-state    (budget: <2ms) ", &samples);

        let mut us: Vec<u128> = samples.iter().map(|d| d.as_micros()).collect();
        us.sort_unstable();
        let p99 = us[(N as f64 * 0.99) as usize];
        assert!(p99 < 2_000, "virtual actor steady-state p99 {p99}µs > 2ms");
    })
    .await;
    result.expect("virtual_actor_latency timed out after 10s — activation path is broken");
}

/// Break down where virtual actor first-activation time goes.
/// Times each step of the activation path directly against the ServiceLocator.
#[tokio::test(flavor = "multi_thread")]
async fn virtual_actor_activation_breakdown() {
    use plexspaces_actor::{ActorId, InitializableServiceLocator};

    let result = tokio::time::timeout(Duration::from_secs(10), async {
        let node = build_test_node("lat-breakdown").await;
        let ns = "bd-ns";
        let sl = node.node.service_locator();

        // Step A: register_virtual_actor_type
        let t = Instant::now();
        let mgr = sl.virtual_actor_manager().await.unwrap();
        mgr.register_virtual_actor_type(
            "echo".to_string(),
            None,
            ns.to_string(),
            serde_json::json!({"virtual_actor": {"idle_timeout": "5m", "activation_strategy": "lazy"}}),
            None,
            None,
        )
        .await
        .unwrap();
        println!("[breakdown] register_virtual_actor_type         {}µs", t.elapsed().as_micros());

        let actor_id = ActorId::new("bd-0", "echo", ns, "lat-breakdown").unwrap();

        // Step B: prime_instance_from_definition (what routing.rs does before first message)
        let t = Instant::now();
        let type_meta = mgr.get_virtual_actor_type("echo").await.unwrap();
        mgr.prime_instance_from_definition(&actor_id, &type_meta).await;
        println!("[breakdown] prime_instance_from_definition       {}µs", t.elapsed().as_micros());

        // Step C: is_virtual check
        let t = Instant::now();
        let _ = mgr.is_virtual(&actor_id).await;
        println!("[breakdown] is_virtual                           {}µs", t.elapsed().as_micros());

        // Step D: is_active check (false — not yet spawned)
        let t = Instant::now();
        let _ = mgr.is_active(&actor_id).await;
        println!("[breakdown] is_active (before spawn)             {}µs", t.elapsed().as_micros());

        // Step E: evict_lru_if_needed
        let t = Instant::now();
        let _ = mgr.evict_lru_if_needed("echo", Some(sl.clone())).await;
        println!("[breakdown] evict_lru_if_needed                  {}µs", t.elapsed().as_micros());

        // Step F: get_metadata (instance-level lookup)
        let t = Instant::now();
        let _ = mgr.get_metadata(&actor_id).await;
        println!("[breakdown] get_metadata                         {}µs", t.elapsed().as_micros());

        // Step G: get_virtual_actor_type (type-level lookup for idle_timeout)
        let t = Instant::now();
        let _ = mgr.get_virtual_actor_type("echo").await;
        println!("[breakdown] get_virtual_actor_type               {}µs", t.elapsed().as_micros());

        // Step H: spawn_with_facets directly (no routing overhead, just actor creation)
        // NOTE: H1 is always "cold" (first spawn warms registries). H2+ are warm and comparable.
        {
            use plexspaces_sdk::spawn_with_facets;
            use plexspaces_journaling::{VirtualActorFacet, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY};
            let ctx2 = RequestContext::new_without_auth("test-tenant".to_string(), ns.to_string());

            // H0: warm-up spawn (discarded) so H1/H2 are on equal footing
            spawn_with_facets(&ctx2, node.node.service_locator(), "bd-warmup", ns, EchoActor, vec![])
                .await
                .expect("warmup spawn");

            // H1: regular actor, no virtual facet (warm registry baseline)
            let t = Instant::now();
            spawn_with_facets(&ctx2, node.node.service_locator(), "bd-direct", ns, EchoActor, vec![])
                .await
                .expect("spawn_with_facets native");
            println!("[breakdown] spawn_with_facets native (no virtual)  {}µs  (warm)", t.elapsed().as_micros());

            // H2: regular actor WITH virtual_actor facet (warm, same as activate_virtual_actor does internally)
            let eager_facet_config = serde_json::json!({
                "idle_timeout": "5m",
                "activation_strategy": "eager"
            });
            let virtual_facet = VirtualActorFacet::new(eager_facet_config, VIRTUAL_ACTOR_FACET_DEFAULT_PRIORITY);
            let t = Instant::now();
            spawn_with_facets(&ctx2, node.node.service_locator(), "bd-virtual-facet", ns, EchoActor, vec![Box::new(virtual_facet)])
                .await
                .expect("spawn_with_facets with virtual facet");
            println!("[breakdown] spawn_with_facets+virtual_facet        {}µs  (warm; same facet as virtual path)", t.elapsed().as_micros());

            // H3: Mailbox::new cost (spawns background task — likely dominant)
            {
                use plexspaces_mailbox::{mailbox_config_default, Mailbox};
                let t = Instant::now();
                for i in 0..5 {
                    let _ = Mailbox::new(
                        mailbox_config_default(),
                        format!("bench-mailbox-{i}"),
                        "test-tenant".to_string(),
                        ns.to_string(),
                        None,
                    ).await.unwrap();
                }
                println!("[breakdown] Mailbox::new x5                       {}µs  (avg={}µs)", t.elapsed().as_micros(), t.elapsed().as_micros()/5);
            }

            // H4: tokio::spawn overhead (bare task)
            {
                let t = Instant::now();
                for _ in 0..5 {
                    let h = tokio::spawn(async { std::hint::black_box(42u64) });
                    let _ = h.await;
                }
                println!("[breakdown] tokio::spawn+await x5                 {}µs  (avg={}µs)", t.elapsed().as_micros(), t.elapsed().as_micros()/5);
            }
        }

        // Step I: full first ask_reply (routing + all above + spawn + mailbox + reply)
        let t_full = Instant::now();
        timed_ask(&node.actor_service, ns, "bd-0", "echo").await;
        let full_us = t_full.elapsed().as_micros();
        println!("[breakdown] full ask_reply (first activation)    {}µs", full_us);

        // Step J: second ask_reply (actor already live)
        let t_warm = Instant::now();
        timed_ask(&node.actor_service, ns, "bd-0", "echo").await;
        let warm_us = t_warm.elapsed().as_micros();
        println!("[breakdown] full ask_reply (warm)                {}µs", warm_us);

        println!("[breakdown] activation overhead = {}µs  (first={}µs warm={}µs)", full_us.saturating_sub(warm_us), full_us, warm_us);
        println!("[breakdown] NOTE: metrics now use tenant_id/namespace/actor_type labels — bounded cardinality, no per-actor DashMap growth");
    })
    .await;
    result.expect("virtual_actor_activation_breakdown timed out after 10s");
}
