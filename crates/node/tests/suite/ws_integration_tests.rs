// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for WebSocket transport.
//!
//! # Coverage
//! - Auth: valid tenant header accepted; auth-disabled skips JWT check
//! - Registration handshake: WsNodeRegister → WsNodeRegisterAck
//! - WsRegistry: session appears in `session_count()` and `list_thin_nodes()`
//! - ServiceLocator: `get_ws_registry()` returns the live registry
//! - Disconnect: session removed after close
//! - Heartbeat: WsHeartbeat → WsHeartbeatAck
//! - Tell: WsTell dispatched, no crash
//! - request_id: echoed back in AskResponse
//!
//! # Design
//! - Uses `tokio-tungstenite` to drive a raw WS connection.
//! - Port is reserved with `TcpListener::bind("127.0.0.1:0")`, then released
//!   before `NodeBuilder` is given the fixed address. This avoids port conflicts.
//! - Node is started in background via `tokio::spawn`. A TCP ready-loop waits
//!   for the port to be listening before the WebSocket is upgraded.
//! - All nodes run with auth disabled.

use std::net::TcpListener as StdTcpListener;
use std::sync::Arc;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use plexspaces_actor::{NodeRegistryTrait, RequestContext, RequestContextExt, ServiceLocator};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::node::v1::{NodeRegistration, PingRequest, SendHeartbeatRequest};
use plexspaces_proto::transport::ws::v1::{ws_frame, WsFrame};
use prost::Message as ProstMessage;
use tokio::time::Instant;
use tokio_tungstenite::{connect_async, tungstenite::Message as TungsteniteMessage};

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Reserve a free TCP port, then release it for the node to bind.
fn reserve_free_port() -> u16 {
    let l = StdTcpListener::bind("127.0.0.1:0").expect("bind port 0");
    let port = l.local_addr().unwrap().port();
    drop(l);
    port
}

/// Build a node on `port` with auth disabled, start it in background, and wait
/// until the port is accepting TCP connections.
async fn start_node_on_port(name: &str, port: u16) -> Arc<Node> {
    let addr = format!("127.0.0.1:{}", port);
    let node = Arc::new(
        NodeBuilder::new(name)
            .with_listen_addr(&addr)
            .with_auth_disabled()
            .build()
            .await,
    );

    let node_clone = node.clone();
    tokio::spawn(async move {
        let _ = node_clone.start().await;
    });

    // Wait until the TCP port is accepting connections.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if std::net::TcpStream::connect(&addr).is_ok() {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Node {} did not listen on {} within 8s", name, addr);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    node
}

/// Encode a `WsFrame` to a binary tungstenite `Message`.
fn encode_ws_frame(frame: &WsFrame) -> TungsteniteMessage {
    TungsteniteMessage::Binary(frame.encode_to_vec().into())
}

/// Decode a binary tungstenite `Message` into a `WsFrame`. Panics on failure.
fn decode_ws_message(msg: TungsteniteMessage) -> WsFrame {
    match msg {
        TungsteniteMessage::Binary(b) => WsFrame::decode(b.as_ref()).expect("decode WsFrame"),
        other => panic!("Expected binary WS frame, got {:?}", other),
    }
}

/// Open a WS connection and perform the node-register handshake.
/// Returns `(write_half, read_half, assigned_node_id)`.
async fn connect_and_register(
    port: u16,
    client_node_id: &str,
) -> (
    impl SinkExt<TungsteniteMessage, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    impl StreamExt<Item = Result<TungsteniteMessage, tokio_tungstenite::tungstenite::Error>> + Unpin,
    String,
) {
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (ws_stream, _) = connect_async(&url)
        .await
        .unwrap_or_else(|e| panic!("WS connect to {} failed: {}", url, e));

    let (mut write, mut read) = ws_stream.split();

    // Send NodeRegister handshake.
    let reg_frame = WsFrame {
        request_id: "handshake-1".to_string(),
        payload: Some(ws_frame::Payload::NodeRegister(NodeRegistration {
            node_id: client_node_id.to_string(),
            node_address: String::new(),
            capabilities: std::collections::HashMap::new(),
            ..Default::default()
        })),
    };
    write
        .send(encode_ws_frame(&reg_frame))
        .await
        .expect("send NodeRegister");

    // Receive ack.
    let ack_msg = read
        .next()
        .await
        .expect("expected ack")
        .expect("ack recv error");
    let ack = decode_ws_message(ack_msg);

    // Verify request_id is echoed back in the outer WsFrame envelope.
    assert_eq!(ack.request_id, "handshake-1", "ack must echo request_id");

    let assigned_node_id = match ack.payload {
        Some(ws_frame::Payload::NodeRegisterAck(a)) => {
            assert!(a.success, "NodeRegisterAck.success must be true");
            if client_node_id.is_empty() {
                assert!(
                    !a.assigned_node_id.is_empty(),
                    "Server must assign a node_id when client sends empty"
                );
            } else {
                assert_eq!(a.assigned_node_id, client_node_id);
            }
            a.assigned_node_id
        }
        other => panic!("Expected NodeRegisterAck, got {:?}", other),
    };

    (write, read, assigned_node_id)
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

/// Auth disabled: upgrade with no auth headers should succeed.
#[tokio::test]
async fn test_ws_upgrade_auth_disabled() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-auth-disabled", port).await;

    let url = format!("ws://127.0.0.1:{}/ws", port);
    let result = connect_async(&url).await;
    assert!(result.is_ok(), "WS upgrade should succeed with auth disabled");
}

/// After upgrading and sending the register frame, the server sends back a
/// `NodeRegisterAck` with `success=true` and the client's node_id.
#[tokio::test]
async fn test_ws_registration_handshake() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-reg-handshake", port).await;

    let (_write, _read, assigned_id) =
        connect_and_register(port, "thin-client-1").await;
    assert_eq!(assigned_id, "thin-client-1");
}

/// If the client sends an empty node_id, the server assigns a ULID and returns it.
#[tokio::test]
async fn test_ws_server_assigns_node_id_when_empty() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-auto-id", port).await;

    let (_write, _read, assigned_id) = connect_and_register(port, "").await;
    assert!(!assigned_id.is_empty(), "Server must assign a non-empty node_id");
    // A server-assigned ID should be a valid ULID (26 chars, uppercase)
    assert_eq!(assigned_id.len(), 26, "Server-assigned ID must be a 26-char ULID");
}

/// After registration, the session appears in `WsRegistry` via `get_ws_registry()`.
#[tokio::test]
async fn test_ws_registry_accessible_via_service_locator() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-sl-registry", port).await;

    let (_write, _read, _) =
        connect_and_register(port, "thin-client-sl").await;

    // Register-before-ack: WsRegistry is already updated when connect_and_register returns.
    let ws_reg = node
        .service_locator()
        .get_ws_registry()
        .await
        .expect("WsRegistry must be registered in ServiceLocator after start()");

    let count = ws_reg.session_count().await;
    assert_eq!(count, 1, "Expected 1 active WS session");
}

/// Registered sessions appear in `list_thin_nodes()`.
#[tokio::test]
async fn test_ws_registry_list_thin_nodes() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-thin-list", port).await;

    let (_write, _read, _) =
        connect_and_register(port, "thin-node-list-1").await;

    let ws_reg = node
        .service_locator()
        .get_ws_registry()
        .await
        .expect("WsRegistry");

    let thin = ws_reg.list_thin_nodes().await;
    assert!(
        thin.contains(&"thin-node-list-1".to_string()),
        "thin-node-list-1 must appear in list_thin_nodes(); got {:?}",
        thin
    );
}

/// After closing the WS connection, the session is removed from the registry.
#[tokio::test]
async fn test_ws_session_removed_on_disconnect() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-disconnect", port).await;

    let (mut write, _read, _) =
        connect_and_register(port, "thin-disc-1").await;

    // Register-before-ack: session is already in registry when connect_and_register returns.
    let ws_reg = node
        .service_locator()
        .get_ws_registry()
        .await
        .expect("WsRegistry");
    assert_eq!(ws_reg.session_count().await, 1);

    // Close the connection and poll until the server detects it (up to 2s).
    let _ = write.close().await;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if ws_reg.session_count().await == 0 {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Session was not removed within 2s after WS close");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// WsHeartbeat → WsHeartbeatAck.
#[tokio::test]
async fn test_ws_heartbeat_ack() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-hb", port).await;

    let (mut write, mut read, _) =
        connect_and_register(port, "thin-hb-1").await;

    let hb_frame = WsFrame {
        request_id: "hb-req-1".to_string(),
        payload: Some(ws_frame::Payload::Heartbeat(SendHeartbeatRequest {
            request_id: "hb-req-1".to_string(),
            ..Default::default()
        })),
    };
    write.send(encode_ws_frame(&hb_frame)).await.expect("send heartbeat");

    let resp_msg = read
        .next()
        .await
        .expect("expected heartbeat ack")
        .expect("heartbeat ack recv error");
    let resp = decode_ws_message(resp_msg);

    match resp.payload {
        Some(ws_frame::Payload::HeartbeatAck(ack)) => {
            assert!(ack.acknowledged, "HeartbeatAck.acknowledged must be true");
        }
        other => panic!("Expected HeartbeatAck, got {:?}", other),
    }
    assert_eq!(resp.request_id, "hb-req-1", "request_id must be echoed back");
}

/// `is_connected()` returns true for a connected session, false for unknown nodes.
#[tokio::test]
async fn test_ws_registry_is_connected() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-is-connected", port).await;

    let (_write, _read, _) =
        connect_and_register(port, "thin-conn-check").await;

    let ws_reg = node
        .service_locator()
        .get_ws_registry()
        .await
        .expect("WsRegistry");

    assert!(ws_reg.is_connected("thin-conn-check").await);
    assert!(!ws_reg.is_connected("no-such-node").await);
}

/// Multiple simultaneous connections all appear in the registry.
#[tokio::test]
async fn test_ws_multiple_sessions() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-multi-sess", port).await;

    let (_w1, _r1, _) = connect_and_register(port, "thin-multi-1").await;
    let (_w2, _r2, _) = connect_and_register(port, "thin-multi-2").await;
    let (_w3, _r3, _) = connect_and_register(port, "thin-multi-3").await;

    // All three acks received means all three are already in WsRegistry (register-before-ack).
    let ws_reg = node
        .service_locator()
        .get_ws_registry()
        .await
        .expect("WsRegistry");

    assert_eq!(ws_reg.session_count().await, 3);

    let mut all = ws_reg.list_all_nodes().await;
    all.sort();
    assert_eq!(all, vec!["thin-multi-1", "thin-multi-2", "thin-multi-3"]);
}

/// A second client attempting to register with the same node_id as an existing
/// live session receives a WsError frame (code 9) and no NodeRegisterAck.
/// This prevents session takeover of in-flight asks on the first session.
#[tokio::test]
async fn test_ws_duplicate_node_id_rejected() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-dup-id", port).await;

    // First client registers successfully.
    let (_w1, _r1, _) = connect_and_register(port, "thin-dup-node").await;

    // Second client tries to register with the same node_id.
    let url = format!("ws://127.0.0.1:{}/ws", port);
    let (ws_stream2, _) = connect_async(&url).await.expect("second WS connect");
    let (mut write2, mut read2) = ws_stream2.split();

    let reg_frame = WsFrame {
        request_id: "dup-handshake".to_string(),
        payload: Some(ws_frame::Payload::NodeRegister(NodeRegistration {
            node_id: "thin-dup-node".to_string(),
            ..Default::default()
        })),
    };
    write2.send(encode_ws_frame(&reg_frame)).await.expect("send duplicate NodeRegister");

    let resp_msg = read2
        .next()
        .await
        .expect("expected error response")
        .expect("recv error");
    let resp = decode_ws_message(resp_msg);

    match resp.payload {
        Some(ws_frame::Payload::Error(err)) => {
            assert_eq!(err.code, 9, "Duplicate registration must return error code 9");
            assert!(
                err.message.contains("already registered"),
                "Error message must mention 'already registered'; got: {}",
                err.message
            );
        }
        other => panic!("Expected WsError for duplicate node_id, got {:?}", other),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Thin-node lifecycle: NodeRegistry / SWIM integration
// ─────────────────────────────────────────────────────────────────────────────

/// After the WS handshake the thin node must be visible in NodeRegistry with
/// `node_role = NodeRoleThin`.
#[tokio::test]
async fn test_ws_connect_registers_in_node_registry() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-nr-register", port).await;

    let (_write, _read, node_id) = connect_and_register(port, "thin-nr-reg-1").await;

    let nr = node
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry must be available");
    let ctx = RequestContext::new_without_auth("default".into(), String::new());

    // Poll briefly — registration is synchronous on the server side, so this
    // should be immediate but we allow a couple of retries for scheduling.
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut reg = None;
    while Instant::now() < deadline {
        reg = nr.lookup_node(&ctx, &node_id).await.ok().flatten();
        if reg.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    let reg = reg.expect("thin node must appear in NodeRegistry after WS handshake");
    assert_eq!(
        reg.node_role,
        plexspaces_proto::node::v1::NodeRole::NodeRoleThin as i32,
        "node_role must be NodeRoleThin"
    );
}

/// A NodePing frame sent over WS must receive a NodePingResponse carrying real
/// CPU and memory resource hints (non-zero available_cores on any real machine).
#[tokio::test]
async fn test_ws_node_ping_returns_resource_hints() {
    let port = reserve_free_port();
    let _node = start_node_on_port("ws-ping-res", port).await;

    let (mut write, mut read, _) = connect_and_register(port, "thin-ping-1").await;

    let ping_frame = WsFrame {
        request_id: "ping-req-1".to_string(),
        payload: Some(ws_frame::Payload::NodePing(PingRequest {
            request_id: "ping-req-1".to_string(),
            source_node_id: "thin-ping-1".to_string(),
            sequence_number: 42,
            ..Default::default()
        })),
    };
    write.send(encode_ws_frame(&ping_frame)).await.expect("send NodePing");

    let resp_msg = read
        .next()
        .await
        .expect("expected NodePingResponse")
        .expect("recv error");
    let resp = decode_ws_message(resp_msg);

    match resp.payload {
        Some(ws_frame::Payload::NodePingResponse(pr)) => {
            assert_eq!(pr.sequence_number, 42, "sequence_number must be echoed");
            assert!(!pr.node_id.is_empty(), "node_id must be set in PingResponse");
            let res = pr.resources.expect("resources must be populated");
            assert!(
                res.available_cores > 0,
                "available_cores must be > 0 on any real machine; got {}",
                res.available_cores
            );
            // cpu_percent and memory_available_mb may be 0 in CI but the field must be present.
            let _ = res.cpu_percent;
            let _ = res.memory_available_mb;
        }
        other => panic!("Expected NodePingResponse, got {:?}", other),
    }
}

/// A WS heartbeat must (a) return a HeartbeatAck AND (b) keep the thin node
/// visible in NodeRegistry (i.e. `lookup_node` still returns `Some` after the heartbeat).
#[tokio::test]
async fn test_ws_heartbeat_keeps_node_alive_in_registry() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-hb-alive", port).await;

    let (mut write, mut read, node_id) = connect_and_register(port, "thin-hb-alive-1").await;

    // Send a heartbeat.
    let hb_frame = WsFrame {
        request_id: "hb-alive-1".to_string(),
        payload: Some(ws_frame::Payload::Heartbeat(SendHeartbeatRequest {
            request_id: "hb-alive-1".to_string(),
            ..Default::default()
        })),
    };
    write.send(encode_ws_frame(&hb_frame)).await.expect("send heartbeat");

    // Drain the HeartbeatAck.
    let ack_msg = read.next().await.expect("expected ack").expect("ack recv");
    let ack = decode_ws_message(ack_msg);
    assert!(
        matches!(ack.payload, Some(ws_frame::Payload::HeartbeatAck(_))),
        "expected HeartbeatAck"
    );

    // After the heartbeat the node must still be registered.
    let nr = node
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry");
    let ctx = RequestContext::new_without_auth("default".into(), String::new());
    let reg = nr
        .lookup_node(&ctx, &node_id)
        .await
        .expect("lookup_node must not error")
        .expect("thin node must still be in NodeRegistry after heartbeat");
    assert_eq!(
        reg.node_role,
        plexspaces_proto::node::v1::NodeRole::NodeRoleThin as i32
    );
}

/// After the WS connection is closed the thin node must be removed from
/// NodeRegistry (lookup returns None) within a short window.
#[tokio::test]
async fn test_ws_disconnect_unregisters_from_node_registry() {
    let port = reserve_free_port();
    let node = start_node_on_port("ws-nr-unreg", port).await;

    let (mut write, _read, node_id) = connect_and_register(port, "thin-nr-unreg-1").await;

    // Verify node is registered first.
    let nr = node
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry");
    let ctx = RequestContext::new_without_auth("default".into(), String::new());
    let initial = nr
        .lookup_node(&ctx, &node_id)
        .await
        .expect("lookup_node must not error");
    assert!(initial.is_some(), "node must be registered before disconnect");

    // Close the WS connection.
    let _ = write.close().await;

    // Poll until NodeRegistry stops returning the thin node (up to 3s).
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let reg = nr
            .lookup_node(&ctx, &node_id)
            .await
            .expect("lookup_node must not error");
        if reg.is_none() {
            break;
        }
        if Instant::now() >= deadline {
            panic!("Thin node '{}' was not unregistered from NodeRegistry within 3s after WS close", node_id);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

/// Ask frames dispatched over WS are forwarded to `ActorServiceImpl::ask_reply`.
/// Because no actor is registered, the response must be a WsAskResponse carrying
/// a non-success flag (actor not found) — not a raw WsError.
/// This verifies the ask-reply round trip works end-to-end over the WS transport.
#[tokio::test]
async fn test_ws_ask_reply_round_trip() {
    use plexspaces_proto::actor::v1::AskReplyRequest;

    let port = reserve_free_port();
    let _node = start_node_on_port("ws-ask-rt", port).await;

    let (mut write, mut read, _) = connect_and_register(port, "thin-ask-1").await;

    // Send an ask for a non-existent actor — expect an AskResponse (not an Error frame).
    let ask_req = AskReplyRequest {
        request_id: "ask-rt-001".to_string(),
        actor_type: "non-existent-actor".to_string(),
        namespace: "default".to_string(),
        ..Default::default()
    };
    let ask_frame = WsFrame {
        request_id: "ask-rt-001".to_string(),
        payload: Some(ws_frame::Payload::Ask(ask_req)),
    };
    write.send(encode_ws_frame(&ask_frame)).await.expect("send ask frame");

    let resp_msg = read
        .next()
        .await
        .expect("expected ask response")
        .expect("recv error");
    let resp = decode_ws_message(resp_msg);

    // The response must be an AskResponse frame, not an Error frame.
    match resp.payload {
        Some(ws_frame::Payload::AskResponse(ar)) => {
            // request_id is echoed back for correlation.
            assert_eq!(ar.request_id, "ask-rt-001", "request_id must be echoed");
            // Actor not found means success=false, but the transport itself worked.
            assert!(!ar.success, "Expect success=false for non-existent actor");
        }
        other => panic!("Expected AskResponse, got {:?}", other),
    }
}
