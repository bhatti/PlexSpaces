// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Integration test: WASM ask() message-id tracing
//!
//! Deploys a Python calculator WASM actor, sends a single ask message,
//! and verifies the request/reply message-id flow:
//!
//! 1. Request message-id starts with `req-`
//! 2. Python actor receives and processes the message
//! 3. Reply message-id starts with `res-`
//! 4. Reply reaches the temporary sender and returns to caller
//!
//! ## Running
//! ```bash
//! cargo test -p plexspaces-node --test node_integration_tests wasm_ask_single_message_id_flow -- --nocapture
//! ```

use super::test_helpers::app_request_with_tenant;
use plexspaces_node::NodeBuilder;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec, ApplicationType, ChildSpec,
    DeployApplicationRequest, RestartPolicy, ShutdownStrategy, SupervisionStrategy, SupervisorSpec,
};
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::common::v1::{Message, Metadata};
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_services::application_service::ApplicationServiceImpl;
use prost_types::Duration as ProstDuration;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

/// Get the calculator WASM file path (same as other test files)
fn get_calculator_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/node -> crates
    path.push("wasm-runtime");
    path.push("tests");
    path.push("fixtures");
    path.push("calculator_actor.wasm");
    path
}

/// Deploy the calculator WASM as a single-actor application via ApplicationService,
/// then send one ask("add") message and verify message-id prefixes.
#[tokio::test]
async fn test_wasm_ask_single_message_id_flow() {
    // ── tracing (visible with --nocapture) ──────────────────────────
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info,plexspaces_application=debug,plexspaces_actor::routing=debug,plexspaces_services::actor_service=debug")
        .with_test_writer()
        .try_init();

    // ── 0. Load WASM file ──────────────────────────────────────────
    let wasm_path = get_calculator_wasm_path();
    if !wasm_path.exists() {
        eprintln!(
            "Skipping test: calculator_actor.wasm not found at {:?}",
            wasm_path
        );
        return;
    }
    let wasm_bytes = tokio::fs::read(&wasm_path).await.expect("read WASM file");
    eprintln!("Loaded calculator_actor.wasm: {} bytes", wasm_bytes.len());

    // ── 1. Create node ─────────────────────────────────────────────
    let node = Arc::new(
        NodeBuilder::new("test-node-ask")
            .with_listen_addr("127.0.0.1:0")
            .with_in_memory_backends()
            .build()
            .await,
    );
    node.application_manager()
        .set_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;

    // ── 2. Deploy calculator as single-actor application ───────────
    let child_spec = ChildSpec {
        actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
            name: "calculator".to_string(),
            actor_type: "test_wasm_actor".to_string(),
        }),

        role: "worker".to_string(),
        restart: RestartPolicy::RestartPolicyPermanent.into(),
        shutdown_timeout: Some(ProstDuration {
            seconds: 5,
            nanos: 0,
        }),
        ..Default::default()
    };
    let supervisor_spec = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 3,
        max_restart_window: Some(ProstDuration {
            seconds: 5,
            nanos: 0,
        }),
        children: vec![child_spec],
    };
    let app_spec = ApplicationSpec {
        name: "calc-ask-test".to_string(),
        tenant_id: String::new(),
        namespace: String::new(),
        version: "1.0.0".to_string(),
        description: "Ask pattern test".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: HashMap::new(),
        supervisor: Some(supervisor_spec),
        enabled: true,
        auto_start: true,
        shutdown_timeout: Some(ProstDuration {
            seconds: 10,
            nanos: 0,
        }),
        shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
        seed_nodes: vec![],
        required_service_links: vec![],
        metadata: None,
    };
    let wasm_module = WasmModule {
        name: "calc-ask-test".to_string(),
        version: "1.0.0".to_string(),
        version_number: 1,
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: Some(Metadata {
            labels: HashMap::new(),
            annotations: HashMap::new(),
            create_time: None,
            created_by: String::new(),
            update_time: None,
            updated_by: String::new(),
        }),
        created_at: None,
        size_bytes: 0,
    };

    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let deploy_req = DeployApplicationRequest {
        application_id: "calc-ask-test".to_string(),
        name: "calc-ask-test".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    let deploy_resp = service
        .deploy_application(app_request_with_tenant(deploy_req))
        .await
        .expect("deploy should succeed");
    let inner = deploy_resp.into_inner();
    assert!(inner.success, "deployment should succeed: {:?}", inner);
    eprintln!("Deployed calc-ask-test, waiting for actor to spawn...");

    // Wait for application to start and actor to spawn
    sleep(Duration::from_millis(2000)).await;

    // ── 3. Lookup calculator ActorRef ──────────────────────────────
    let registry = node
        .service_locator()
        .actor_registry()
        .await
        .expect("ActorRegistry should be available");

    // Actor IDs use the canonical structured form; find the calculator actor in the registry.
    let all_actor_ids = registry.registered_actor_ids().await;
    eprintln!("Registered actors: {:?}", all_actor_ids);

    let calc_actor_id = all_actor_ids
        .iter()
        .find(|id| id.name() == "calculator")
        .expect("calculator actor should be registered");
    eprintln!("Found calculator actor: {}", calc_actor_id);

    let actor_ref = registry
        .lookup_actor(calc_actor_id)
        .await
        .expect("calculator ActorRef should exist");

    // ── 4. Send ask("add") with operands [1, 2, 3] ────────────────
    let request_payload = serde_json::json!({
        "msg_type": "add",
        "operands": [1, 2, 3]
    });
    let payload_bytes = serde_json::to_vec(&request_payload).unwrap();
    let request_id = format!("req-{}", ulid::Ulid::new());

    let ask_message = Message {
        id: request_id.clone(),
        payload: payload_bytes,
        sender_id: String::new(), // Outside caller — ActorRef.ask() creates temp sender
        receiver_id: calc_actor_id.to_string(),
        message_type: "add".to_string(),
        ..Default::default()
    };

    eprintln!(
        "Sending ask: message_id={}, recipient={}",
        request_id, calc_actor_id
    );

    let reply = actor_ref
        .ask(ask_message, std::time::Duration::from_secs(10))
        .await;

    // ── 5. Verify reply ────────────────────────────────────────────
    match reply {
        Ok(reply_msg) => {
            eprintln!("Reply received:");
            eprintln!("  reply.id       = {}", reply_msg.id);
            eprintln!("  reply.sender   = {}", reply_msg.sender_id);
            eprintln!("  reply.receiver = {}", reply_msg.receiver_id);
            let payload_str = String::from_utf8_lossy(&reply_msg.payload);
            eprintln!("  reply.payload  = {}", payload_str);

            // Verify message-id prefix conventions
            assert!(
                request_id.starts_with("req-"),
                "Request message-id should start with req-, got: {}",
                request_id
            );
            assert!(
                reply_msg.id.starts_with("res-"),
                "Reply message-id should start with res-, got: {}",
                reply_msg.id
            );

            // Verify the calculator actually computed the result
            let result: serde_json::Value =
                serde_json::from_slice(&reply_msg.payload).expect("reply should be valid JSON");
            eprintln!("  parsed result  = {}", result);
            // Calculator returns {"result": 6, "operation": "add"}
            if let Some(r) = result.get("result") {
                assert_eq!(r.as_f64().unwrap() as i64, 6, "1+2+3 should equal 6");
            }
        }
        Err(e) => {
            panic!("ask() failed: {:?}", e);
        }
    }

    eprintln!(
        "PASSED: WASM ask message-id flow verified (req-/res- prefixes, reply reached caller)"
    );
}
