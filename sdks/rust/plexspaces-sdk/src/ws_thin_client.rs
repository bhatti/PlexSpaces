// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 PlexSpaces Contributors
//
// WsThinClient — Rust thin-node WebSocket client skeleton.
//
// # TODO: Full Implementation
// Implement WsFrame protobuf encoding via `prost` over `tokio-tungstenite`.
// The wire protocol is defined in:
//   proto/plexspaces/v1/transport/websocket.proto
//
// NodeClient (node_client.rs) uses gRPC; this is the WebSocket equivalent
// for thin nodes that cannot accept inbound connections.
//
// # Protocol
// 1. Connect to ws://{host}:{port}/ws?token={jwt}
// 2. Send WsFrame { node_register: NodeRegistration { node_role: NODE_ROLE_THIN } }
// 3. Receive WsFrame { node_register_ack: WsNodeRegisterAck { assigned_node_id } }
// 4. Exchange tell/ask frames (binary prost-encoded WsFrame)
// 5. Send WsFrame { heartbeat: SendHeartbeatRequest } every ~25s

/// Options for connecting a thin node via WebSocket.
pub struct WsThinClientOptions {
    /// WebSocket URL, e.g. "ws://localhost:8091/ws"
    pub ws_url: String,
    /// JWT Bearer token (appended as ?token=<jwt>). None if auth is disabled.
    pub jwt_token: Option<String>,
    /// ULID preferred. Server assigns one if omitted or collision detected.
    pub node_id: Option<String>,
    pub tenant: String,
    pub namespace: String,
}

/// Resource hints from a PingResponse (proto fields 9–11 of PingResponse).
/// Maps to the cpu_percent / memory_available_mb / available_cores fields
/// added to proto/plexspaces/v1/node/node.proto.
pub struct ThinNodePingResult {
    pub node_id: String,
    pub cpu_percent: f32,
    pub memory_available_mb: u64,
    pub available_cores: u32,
}

/// WebSocket thin-node client.
///
/// Stub implementation — all methods return `todo!()`.
/// See the TypeScript implementation (`sdks/typescript/src/ws_thin_client.ts`)
/// for the reference wire-protocol encoding.
pub struct WsThinClient;

impl WsThinClient {
    /// Connect and complete the NodeRegistration handshake.
    /// Returns the server-assigned node_id on success.
    pub async fn connect(_opts: WsThinClientOptions) -> anyhow::Result<Self> {
        todo!("implement WsFrame prost encoding over tokio-tungstenite")
    }

    /// Fire-and-forget tell to a canonical actor ID.
    pub async fn tell(
        &self,
        _actor_id: &str,
        _msg_type: &str,
        _payload: serde_json::Value,
    ) -> anyhow::Result<()> {
        todo!()
    }

    /// Request-reply ask. Returns the response payload as JSON.
    pub async fn ask(
        &self,
        _actor_id: &str,
        _msg_type: &str,
        _payload: serde_json::Value,
        _timeout: std::time::Duration,
    ) -> anyhow::Result<serde_json::Value> {
        todo!()
    }

    /// Register a handler for incoming tell frames addressed to this thin node.
    pub fn on_message<F>(&self, _handler: F)
    where
        F: Fn(&str, &str, serde_json::Value) + Send + Sync + 'static,
    {
        todo!()
    }

    /// The server-assigned node_id (available after connect()).
    pub fn node_id(&self) -> &str {
        todo!()
    }

    /// Build a canonical actor ID on this thin node.
    /// Format: {name}//{type}::{namespace}@{node_id}
    pub fn local_actor_id(&self, name: &str, actor_type: &str, namespace: &str) -> String {
        format!("{}//{actor_type}::{namespace}@{}", name, self.node_id())
    }

    /// Send a node ping and return resource hints from the PingResponse.
    pub async fn ping_node(&self, _target_node_id: &str) -> anyhow::Result<ThinNodePingResult> {
        todo!()
    }

    /// Send a heartbeat frame to keep the WS session alive.
    pub async fn heartbeat(&self) -> anyhow::Result<()> {
        todo!()
    }

    /// Disconnect and clean up.
    pub async fn disconnect(self) -> anyhow::Result<()> {
        todo!()
    }
}
