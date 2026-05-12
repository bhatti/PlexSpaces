// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Rust-only integration tests for named virtual actor definitions (name != actor_type).
//
// These tests mirror the TypeScript abstractions example Step-8 without WASM:
//   - Named child spec "ephemeral" (name != actor_type) with initial_count=5 in args
//   - Increment count, stop explicitly, reactivate → count resets to 5 (not 7)

use super::test_helpers::{registry_ask, spawn_actor_helper};
use async_trait::async_trait;
use plexspaces_actor::behavior_factory::BehaviorRegistry;
use plexspaces_actor::{Actor, ActorBuilder};
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType,
    InitializableServiceLocator, Message, RequestContextExt, ServiceLocator,
};
use plexspaces_behavior::GenServer;
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_node::NodeBuilder;
use plexspaces_proto::actor::v1::ActorSpawnSpec;
use plexspaces_proto::common::v1::{ActorIdentity, Facet};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// Use "default" tenant/namespace so registry_ask (which hardcodes "default") can access actors.
const TENANT: &str = "default";
const NAMESPACE: &str = "default";
const ASK_TIMEOUT: Duration = Duration::from_secs(5);

fn call_msg(payload: Vec<u8>) -> Message {
    Message {
        id: ulid::Ulid::new().to_string(),
        payload,
        message_type: "call".to_string(),
        ..Default::default()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Cmd {
    Status,
    Increment { amount: i64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StatusReply {
    count: i64,
}

/// Counter actor initialized with count from wasm_init_payload args.
/// Mirrors the AbstractionsActor onInit path in abstractions_app.ts.
struct CounterActor {
    count: i64,
}

impl CounterActor {
    fn new() -> Self {
        Self { count: 0 }
    }
    fn from_init_payload(payload: &[u8]) -> Self {
        let count = if payload.is_empty() {
            0
        } else if let Ok(val) = serde_json::from_slice::<serde_json::Value>(payload) {
            // wasm_init_payload: {"actor_id":"…","args":{"initial_count":"5",…},…}
            val.get("args")
                .unwrap_or(&val)
                .get("initial_count")
                .and_then(|v| v.as_str())
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0)
        } else {
            0
        };
        Self { count }
    }
}

#[async_trait]
impl ActorTrait for CounterActor {
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
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let receiver_id = msg.receiver_id.clone();
        let correlation_id = msg.correlation_id.clone();
        let sender_id = msg.sender_id.clone();

        let cmd: Cmd = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        let reply_bytes = match cmd {
            Cmd::Status => serde_json::to_vec(&StatusReply { count: self.count }).unwrap(),
            Cmd::Increment { amount } => {
                self.count += amount;
                serde_json::to_vec(&StatusReply { count: self.count }).unwrap()
            }
        };

        if !sender_id.is_empty() {
            let reply = Message {
                id: ulid::Ulid::new().to_string(),
                payload: reply_bytes,
                correlation_id: correlation_id.clone(),
                ..Default::default()
            };
            let self_id = ActorId::from_canonical(&receiver_id)
                .map_err(|e| BehaviorError::ProcessingError(format!("bad receiver_id: {e}")))?;
            let _ = ctx
                .send_reply(
                    if correlation_id.is_empty() {
                        None
                    } else {
                        Some(correlation_id.as_str())
                    },
                    &sender_id,
                    self_id,
                    reply,
                )
                .await;
        }
        Ok(())
    }
}

/// Register CounterActor — constructor parses initial_count from wasm_init_payload args.
async fn register_counter_behavior(node: &plexspaces_node::Node, actor_type: &str) {
    let registry = BehaviorRegistry::new();
    let t = actor_type.to_string();
    registry
        .register(&t, |args: &[u8]| {
            let payload = args.to_vec();
            Box::pin(async move {
                Ok(Box::new(CounterActor::from_init_payload(&payload))
                    as Box<dyn plexspaces_actor::Actor>)
            })
        })
        .await;
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;
}

fn virtual_facet() -> Box<VirtualActorFacet> {
    Box::new(VirtualActorFacet::new(
        serde_json::json!({ "activation_strategy": "lazy" }),
        100,
    ))
}

/// Named definition spec (name != actor_type) with initial_count in args.
fn named_def_spec(name: &str, actor_type: &str, initial_count: i64) -> ActorSpawnSpec {
    ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: name.to_string(),
            actor_type: actor_type.to_string(),
        }),
        namespace: NAMESPACE.to_string(),
        tenant_id: TENANT.to_string(),
        behavior_kind: "GenServer".to_string(),
        args: HashMap::from([("initial_count".to_string(), initial_count.to_string())]),
        facets: vec![Facet {
            r#type: "virtual_actor".to_string(),
            config: HashMap::from([("activation_strategy".to_string(), "lazy".to_string())]),
            priority: 0,
            state: HashMap::new(),
            metadata: None,
        }],
        ..Default::default()
    }
}

// ============================================================================
// Step-8 equivalent test
// ============================================================================

/// Mirrors TypeScript abstractions Step-8:
/// After explicit stop, next poll on "ephemeral:session-1" must return count=5
/// (from definition args), not count=7 (stale from stopped session).
#[tokio::test]
async fn test_non_durable_named_actor_reactivates_with_definition_args() {
    let node_id = "test-node-named-reactivation";
    let node = Arc::new(
        NodeBuilder::new(node_id)
            .with_in_memory_backends()
            .build()
            .await,
    );
    let actor_type = "abstractions_wasm";

    register_counter_behavior(&node, actor_type).await;

    let manager = node
        .service_locator()
        .virtual_actor_manager()
        .await
        .unwrap();

    // Register named definition: "ephemeral" → actor_type="abstractions_wasm", initial_count=5
    manager
        .register_virtual_actor_definition(named_def_spec("ephemeral", actor_type, 5))
        .await
        .unwrap();

    // Spawn instance "session-1"
    let actor_id = ActorId::new("session-1", actor_type, NAMESPACE, node_id).unwrap();
    let def_meta = manager
        .get_virtual_actor_definition(NAMESPACE, "ephemeral")
        .await
        .unwrap();
    manager
        .prime_instance_from_definition(&actor_id, &def_meta)
        .await;

    let mut actor_struct = ActorBuilder::new(Box::new(CounterActor::new()))
        .with_name(actor_id.name().to_string())
        .build()
        .await
        .unwrap();
    actor_struct.attach_facet(virtual_facet()).await.unwrap();
    spawn_actor_helper(&node, actor_struct).await.unwrap();
    sleep(Duration::from_millis(200)).await;

    // Trigger activation and verify initial count = 5
    let status = registry_ask(
        &node,
        &actor_id,
        call_msg(serde_json::to_vec(&Cmd::Status).unwrap()),
        ASK_TIMEOUT,
    )
    .await;
    assert!(
        status.is_ok(),
        "initial status should succeed: {:?}",
        status.err()
    );
    let s: StatusReply = serde_json::from_slice(&status.unwrap().payload).unwrap();
    assert_eq!(
        s.count, 5,
        "initial count must be 5 from definition args, got {}",
        s.count
    );

    // Increment by 2 → count = 7
    registry_ask(
        &node,
        &actor_id,
        call_msg(serde_json::to_vec(&Cmd::Increment { amount: 2 }).unwrap()),
        ASK_TIMEOUT,
    )
    .await
    .expect("increment must succeed");
    sleep(Duration::from_millis(100)).await;

    let status = registry_ask(
        &node,
        &actor_id,
        call_msg(serde_json::to_vec(&Cmd::Status).unwrap()),
        ASK_TIMEOUT,
    )
    .await
    .unwrap();
    let s: StatusReply = serde_json::from_slice(&status.payload).unwrap();
    assert_eq!(s.count, 7, "count should be 7 after increment");

    // Explicit stop (simulates controller.stop_actor in the abstractions example)
    let factory = node.service_locator().get_actor_factory().await.unwrap();
    let stop_ctx = plexspaces_actor::RequestContext::new_without_auth(
        TENANT.to_string(),
        NAMESPACE.to_string(),
    );
    factory.stop_actor(&stop_ctx, &actor_id).await.unwrap();
    sleep(Duration::from_millis(200)).await;

    // is_virtual must still be true so routing can trigger reactivation
    assert!(
        manager.is_virtual(&actor_id).await,
        "actor must still be virtual after stop"
    );

    // prime_instance_from_definition re-seeds from definition (initial_count=5)
    // In production, canonical_actor_id_from_client_target does this on every request.
    manager
        .prime_instance_from_definition(&actor_id, &def_meta)
        .await;

    // Send status → triggers reactivation with fresh init payload (initial_count=5)
    let _ = registry_ask(
        &node,
        &actor_id,
        call_msg(serde_json::to_vec(&Cmd::Status).unwrap()),
        ASK_TIMEOUT,
    )
    .await;
    sleep(Duration::from_millis(300)).await;

    // Poll until count=5 (mirrors test.sh wait_for_json_field pattern)
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let reply = registry_ask(
            &node,
            &actor_id,
            call_msg(serde_json::to_vec(&Cmd::Status).unwrap()),
            ASK_TIMEOUT,
        )
        .await;
        if let Ok(r) = reply {
            if let Ok(s) = serde_json::from_slice::<StatusReply>(&r.payload) {
                if s.count == 5 {
                    break; // ✓ Step-8 passed
                }
                assert!(
                    std::time::Instant::now() <= deadline,
                    "Step-8 reactivation bug: expected count=5 after reactivation, got {}. \
                     prime_instance_from_definition must refresh spec from definition args on stop.",
                    s.count
                );
            }
        }
        sleep(Duration::from_millis(100)).await;
    }
}

/// Namespace isolation: same actor_type in different namespace is NOT virtual.
#[tokio::test]
async fn test_named_definition_namespace_isolation() {
    let node_id = "test-node-ns-isolation";
    let node = Arc::new(
        NodeBuilder::new(node_id)
            .with_in_memory_backends()
            .build()
            .await,
    );
    let actor_type = "isolated_wasm";

    let manager = node
        .service_locator()
        .virtual_actor_manager()
        .await
        .unwrap();
    let def_spec = ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: "worker".to_string(),
            actor_type: actor_type.to_string(),
        }),
        namespace: "ns-a".to_string(),
        tenant_id: TENANT.to_string(),
        behavior_kind: "GenServer".to_string(),
        facets: vec![Facet {
            r#type: "virtual_actor".to_string(),
            config: HashMap::new(),
            priority: 0,
            state: HashMap::new(),
            metadata: None,
        }],
        ..Default::default()
    };
    manager
        .register_virtual_actor_definition(def_spec)
        .await
        .unwrap();

    let in_a = ActorId::new("w1", actor_type, "ns-a", node_id).unwrap();
    assert!(
        manager.is_virtual(&in_a).await,
        "actor in ns-a must be virtual"
    );

    let in_b = ActorId::new("w1", actor_type, "ns-b", node_id).unwrap();
    assert!(
        !manager.is_virtual(&in_b).await,
        "same actor_type in ns-b must NOT be virtual"
    );
}

/// Durable actors keep their instance record after stop.
#[tokio::test]
async fn test_durable_actor_instance_retained_after_stop() {
    let node_id = "test-node-durable-retain";
    let node = Arc::new(
        NodeBuilder::new(node_id)
            .with_in_memory_backends()
            .build()
            .await,
    );
    let actor_type = "durable_wasm";

    register_counter_behavior(&node, actor_type).await;

    let manager = node
        .service_locator()
        .virtual_actor_manager()
        .await
        .unwrap();
    let def_spec = ActorSpawnSpec {
        identity: Some(ActorIdentity {
            name: "cart".to_string(),
            actor_type: actor_type.to_string(),
        }),
        namespace: NAMESPACE.to_string(),
        tenant_id: TENANT.to_string(),
        behavior_kind: "GenServer".to_string(),
        args: HashMap::from([("initial_count".to_string(), "3".to_string())]),
        facets: vec![
            Facet {
                r#type: "virtual_actor".to_string(),
                config: HashMap::from([("activation_strategy".to_string(), "lazy".to_string())]),
                priority: 0,
                state: HashMap::new(),
                metadata: None,
            },
            Facet {
                r#type: "durability".to_string(),
                config: HashMap::new(),
                priority: 0,
                state: HashMap::new(),
                metadata: None,
            },
        ],
        ..Default::default()
    };
    manager
        .register_virtual_actor_definition(def_spec)
        .await
        .unwrap();

    let actor_id = ActorId::new("cart-1", actor_type, NAMESPACE, node_id).unwrap();
    let def_meta = manager
        .get_virtual_actor_definition(NAMESPACE, "cart")
        .await
        .unwrap();
    manager
        .prime_instance_from_definition(&actor_id, &def_meta)
        .await;

    let mut actor_struct = ActorBuilder::new(Box::new(CounterActor::new()))
        .with_name(actor_id.name().to_string())
        .build()
        .await
        .unwrap();
    actor_struct.attach_facet(virtual_facet()).await.unwrap();
    spawn_actor_helper(&node, actor_struct).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    let factory = node.service_locator().get_actor_factory().await.unwrap();
    let req_ctx = plexspaces_actor::RequestContext::new_without_auth(
        TENANT.to_string(),
        NAMESPACE.to_string(),
    );
    factory.stop_actor(&req_ctx, &actor_id).await.unwrap();
    sleep(Duration::from_millis(100)).await;

    assert!(
        manager.get_metadata(&actor_id).await.is_some(),
        "durable actor instance must be retained after stop"
    );
}
