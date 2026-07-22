// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// Integration tests for process-group fan-out via TypeScript chat_room WASM.
// Verifies that pg_join + pg_broadcast delivers messages through the Rust host to SessionActors.

use plexspaces_actor::{ApplicationManager, ServiceLocator};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, AskReplyRequest,
};
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, DeployApplicationRequest,
};
use plexspaces_proto::wasm::v1::WasmModule;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::application_service::ApplicationServiceImpl;
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic::metadata::MetadataValue;
use tonic::Request;

async fn create_chat_room_test_node() -> Arc<Node> {
    Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_addr("127.0.0.1:0")
            .with_in_memory_backends()
            .with_auth_disabled()
            .build()
            .await,
    )
}

fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn build_typescript_chat_room_wasm() -> Vec<u8> {
    let root = repo_root();
    let example_dir = root.join("examples/typescript/apps/chat_room");
    let output_path = root.join("target/examples/typescript/chat_room/chat_room_actor.wasm");

    if !output_path.exists() {
        let status = Command::new("bash")
            .arg("build.sh")
            .current_dir(&example_dir)
            .status()
            .expect("TypeScript chat_room build.sh should start");
        assert!(
            status.success(),
            "TypeScript chat_room build.sh should succeed"
        );
    }

    std::fs::read(&output_path).unwrap_or_else(|e| {
        panic!(
            "Failed to load built TypeScript chat_room wasm {}: {}",
            output_path.display(),
            e
        )
    })
}

fn build_go_chat_room_wasm() -> Vec<u8> {
    let root = repo_root();
    let example_dir = root.join("examples/go/apps/chat_room");
    let output_path = root.join("target/examples/go/chat_room/chat_room_actor.wasm");

    if !output_path.exists() {
        let status = Command::new("bash")
            .arg("build.sh")
            .current_dir(&example_dir)
            .env("GOCACHE", "/tmp/plexspaces-go-cache")
            .status()
            .expect("Go chat_room build.sh should start");
        assert!(status.success(), "Go chat_room build.sh should succeed");
    }

    std::fs::read(&output_path).unwrap_or_else(|e| {
        panic!(
            "Failed to load built Go chat_room wasm {}: {}",
            output_path.display(),
            e
        )
    })
}

fn load_app_spec(config_path: &std::path::Path, app_id: &str) -> plexspaces_proto::application::v1::ApplicationSpec {
    let config_text = std::fs::read_to_string(config_path).unwrap_or_else(|e| {
        panic!("Failed to read config {}: {}", config_path.display(), e)
    });
    plexspaces_node::wasm_apps_loader::parse_app_config_toml(&config_text, app_id)
        .unwrap_or_else(|e| {
            panic!("Failed to parse config {}: {}", config_path.display(), e)
        })
}

fn app_request<T: Send>(body: T, tenant_id: &str, namespace: &str) -> Request<T> {
    let mut request = Request::new(body);
    request
        .metadata_mut()
        .insert("x-tenant-id", MetadataValue::try_from(tenant_id).unwrap());
    request
        .metadata_mut()
        .insert("x-namespace", MetadataValue::try_from(namespace).unwrap());
    request
}

async fn actor_ask(
    actor_service: &ActorServiceImpl,
    tenant_id: &str,
    namespace: &str,
    actor_type_and_name: &str,
    payload: serde_json::Value,
) -> serde_json::Value {
    let request = AskReplyRequest {
        request_id: ulid::Ulid::new().to_string(),
        namespace: namespace.to_string(),
        actor_type: actor_type_and_name.to_string(),
        actor_name: String::new(),
        http_method: "POST".to_string(),
        payload: serde_json::to_vec(&payload).expect("payload should serialize"),
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: format!("/api/v1/actors/{}/{}/ask", namespace, actor_type_and_name),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    };

    let response = ActorServiceTrait::ask_reply(
        actor_service,
        app_request(request, tenant_id, namespace),
    )
    .await
    .expect("actor ask should succeed")
    .into_inner();

    assert!(
        response.success,
        "actor ask should succeed for {}, got: {}",
        actor_type_and_name,
        response.error_message
    );

    serde_json::from_slice(&response.payload)
        .unwrap_or_else(|e| panic!("payload should be JSON for {}: {}", actor_type_and_name, e))
}

async fn actor_tell(
    actor_service: &ActorServiceImpl,
    tenant_id: &str,
    namespace: &str,
    actor_type_and_name: &str,
    payload: serde_json::Value,
) {
    // Use ask with cast message_type to send fire-and-forget (we just ignore result)
    let _ = actor_ask(actor_service, tenant_id, namespace, actor_type_and_name, payload).await;
}

async fn deploy_chat_room_app(
    service: &ApplicationServiceImpl,
    app_id: &str,
    tenant_id: &str,
    wasm_bytes: Vec<u8>,
    config_path: &std::path::Path,
) {
    let app_spec = load_app_spec(config_path, app_id);
    let wasm_module = WasmModule {
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(),
        ..Default::default()
    };

    let deploy_request = DeployApplicationRequest {
        request_id: ulid::Ulid::new().to_string(),
        application_id: app_id.to_string(),
        name: app_id.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_spec),
        initial_state: vec![],
    };

    let deploy_response = service
        .deploy_application(app_request(deploy_request, tenant_id, app_id))
        .await
        .expect("deployment should succeed");
    assert!(
        deploy_response.into_inner().success,
        "deployment should succeed for {}",
        app_id
    );

    // Allow actors with eager activation to start
    sleep(Duration::from_millis(500)).await;
}

async fn run_chat_room_fanout_test(
    actor_service: &ActorServiceImpl,
    tenant_id: &str,
    app_id: &str,
) {
    // Step 1: Create guild channel
    let create_ch = actor_ask(
        actor_service,
        tenant_id,
        app_id,
        "GuildActor:guild-acme",
        serde_json::json!({"op": "create_channel", "channel_id": "general"}),
    )
    .await;
    assert!(
        create_ch.get("error").is_none(),
        "create_channel should succeed: {:?}",
        create_ch
    );

    // Step 2: Connect Alice and Bob (they join process groups in onConnect)
    let connect_alice = actor_ask(
        actor_service,
        tenant_id,
        app_id,
        "SessionActor:alice-mobile",
        serde_json::json!({
            "op": "connect",
            "user_id": "alice",
            "guild_id": "guild-acme",
            "channels": ["general"],
            "ttl_ms": 30000
        }),
    )
    .await;
    assert_eq!(
        connect_alice["status"], "connected",
        "alice should connect: {:?}",
        connect_alice
    );

    let connect_bob = actor_ask(
        actor_service,
        tenant_id,
        app_id,
        "SessionActor:bob-desktop",
        serde_json::json!({
            "op": "connect",
            "user_id": "bob",
            "guild_id": "guild-acme",
            "channels": ["general"],
            "ttl_ms": 30000
        }),
    )
    .await;
    assert_eq!(
        connect_bob["status"], "connected",
        "bob should connect: {:?}",
        connect_bob
    );

    // Step 3: Alice sends a channel message (triggers pg_broadcast via FanoutActor)
    let send_msg = actor_ask(
        actor_service,
        tenant_id,
        app_id,
        "SessionActor:alice-mobile",
        serde_json::json!({
            "op": "send_channel_message",
            "channel_id": "general",
            "text": "hello from alice"
        }),
    )
    .await;
    assert_eq!(
        send_msg["status"], "ok",
        "send_channel_message should succeed: {:?}",
        send_msg
    );

    // Step 4: Wait for fan-out to reach Bob (fire-and-forget path through pg_broadcast)
    let mut bob_got_message = false;
    for _ in 0..30 {
        sleep(Duration::from_millis(100)).await;
        let inbox = actor_ask(
            actor_service,
            tenant_id,
            app_id,
            "SessionActor:bob-desktop",
            serde_json::json!({"op": "inbox"}),
        )
        .await;
        let events = inbox["delivered_events"].as_array();
        if let Some(events) = events {
            if events
                .iter()
                .any(|e| e["from_user"].as_str() == Some("alice"))
            {
                bob_got_message = true;
                break;
            }
        }
    }

    assert!(
        bob_got_message,
        "Bob should receive Alice's message via pg_broadcast fan-out. \
         Check that: (1) SessionActors join the channel process group on connect, \
         (2) FanoutActor receives deliver_channel_event and calls pg_broadcast, \
         (3) pg_broadcast in Rust host delivers to all group members."
    );
}

#[tokio::test]
async fn test_typescript_chat_room_pg_broadcast_fanout() {
    let node = create_chat_room_test_node().await;
    node.application_manager()
        .set_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;

    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let app_id = "ts-chat-room-fanout-it";
    let tenant_id = "";
    let wasm_bytes = build_typescript_chat_room_wasm();
    let config_path = repo_root().join("examples/typescript/apps/chat_room/app-config.toml");

    deploy_chat_room_app(&service, app_id, tenant_id, wasm_bytes, &config_path).await;

    run_chat_room_fanout_test(&actor_service, tenant_id, app_id).await;
}

#[tokio::test]
async fn test_go_chat_room_pg_broadcast_fanout() {
    let node = create_chat_room_test_node().await;
    node.application_manager()
        .set_node_context(node.clone() as Arc<dyn plexspaces_application::ApplicationNode>)
        .await;

    let service = ApplicationServiceImpl::new(node.service_locator().clone(), None);
    let actor_service =
        ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let app_id = "go-chat-room-fanout-it";
    let tenant_id = "";
    let wasm_bytes = build_go_chat_room_wasm();
    let config_path = repo_root().join("examples/go/apps/chat_room/app-config.toml");

    deploy_chat_room_app(&service, app_id, tenant_id, wasm_bytes, &config_path).await;

    run_chat_room_fanout_test(&actor_service, tenant_id, app_id).await;
}
