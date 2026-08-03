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

//! WebSocket-first transport clients with gRPC fallback.
//!
//! # Purpose
//! `WsActorTransportClient` and `WsNodeTransportClient` implement the
//! `ActorTransportClient` and `NodeTransportClient` traits. They check
//! `WsRegistry::is_connected(node_id)` first, routing via the active WS session
//! if present, otherwise delegating to a gRPC fallback client.
//!
//! # Ask correlation
//! For ask-reply operations over WS, the response arrives asynchronously on the
//! same WS connection. A `pending_asks` map is keyed by `request_id`; the WS
//! message loop resolves the matching `oneshot::Sender` when the
//! `AskReplyResponse` frame arrives.
//!
//! # Design decisions
//! - `pending_asks` uses a `DashMap`-free plain `RwLock<HashMap>` — contention is
//!   low because entries are inserted at ask time and removed when answered.
//! - Timeout cleanup: entries that exceed the timeout are pruned during ask
//!   resolution and on WS disconnect.
//! - The gRPC fallback path is the existing `GrpcActorTransportClient` so there is
//!   no duplication of the gRPC routing logic.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use plexspaces_proto::actor::v1::{
    AskReplyRequest, AskReplyResponse, SendMessageRequest, SendMessageResponse,
};
use plexspaces_proto::node::v1::{
    PingReqRequest, PingReqResponse, PingRequest, PingResponse, SyncMembershipRequest,
    SyncMembershipResponse,
};
use plexspaces_proto::transport::ws::v1::{ws_frame, WsFrame};
use plexspaces_service_traits::{ActorTransportClient, NodeTransportClient};
use tokio::sync::{oneshot, RwLock};
use ulid::Ulid;

use crate::ws_registry::WsRegistry;

// ─────────────────────────────────────────────────────────────────────────────
// Shared pending ask registry
// ─────────────────────────────────────────────────────────────────────────────

/// Shared state for in-flight ask requests over WebSocket.
///
/// Two indexes under separate `RwLock`s for efficient per-operation access:
/// - `by_request`: request_id → (node_id, sender) — O(1) resolve per ask
/// - `by_node`:    node_id   → Vec<request_id>    — O(k) disconnect cancellation
///
/// All public method signatures are identical to the previous single-map version.
pub struct PendingAsks {
    by_request: RwLock<HashMap<String, (String, oneshot::Sender<AskReplyResponse>)>>,
    by_node: RwLock<HashMap<String, Vec<String>>>,
}

impl PendingAsks {
    /// Create a new empty `PendingAsks` registry.
    pub fn new() -> Self {
        Self {
            by_request: RwLock::new(HashMap::new()),
            by_node: RwLock::new(HashMap::new()),
        }
    }

    /// Register a pending ask for the given node. Returns the receiver end.
    pub async fn register(
        &self,
        node_id: String,
        request_id: String,
    ) -> oneshot::Receiver<AskReplyResponse> {
        let (tx, rx) = oneshot::channel();
        // Update by_request first, then by_node. Both are separate locks; order is
        // insert-before-index so a concurrent resolve never finds a dangling index entry.
        self.by_request
            .write()
            .await
            .insert(request_id.clone(), (node_id.clone(), tx));
        self.by_node
            .write()
            .await
            .entry(node_id)
            .or_default()
            .push(request_id);
        rx
    }

    /// Resolve a pending ask with the given response. No-op if not registered.
    pub async fn resolve(&self, request_id: &str, response: AskReplyResponse) {
        if let Some((node_id, tx)) = self.by_request.write().await.remove(request_id) {
            let _ = tx.send(response);
            // Remove from the node index; ignore if already absent (e.g., cancel_for_node ran first).
            if let Some(reqs) = self.by_node.write().await.get_mut(&node_id) {
                reqs.retain(|r| r != request_id);
            }
        }
    }

    /// Cancel only the pending asks that belong to `node_id` (on WS disconnect).
    /// O(k) where k = number of in-flight asks for this node.
    /// Asks for other connected nodes are not affected.
    pub async fn cancel_for_node(&self, node_id: &str, error_message: &str) {
        let req_ids = self
            .by_node
            .write()
            .await
            .remove(node_id)
            .unwrap_or_default();
        if req_ids.is_empty() {
            return;
        }
        let mut by_req = self.by_request.write().await;
        for req_id in req_ids {
            if let Some((_, tx)) = by_req.remove(&req_id) {
                let _ = tx.send(AskReplyResponse {
                    request_id: req_id,
                    success: false,
                    error_message: error_message.to_string(),
                    ..Default::default()
                });
            }
        }
    }

    /// Remove a single pending ask without resolving it (e.g., on timeout).
    pub async fn remove(&self, request_id: &str) {
        if let Some((node_id, _)) = self.by_request.write().await.remove(request_id) {
            if let Some(reqs) = self.by_node.write().await.get_mut(&node_id) {
                reqs.retain(|r| r != request_id);
            }
        }
    }

    /// Returns the number of pending ask requests.
    pub async fn len(&self) -> usize {
        self.by_request.read().await.len()
    }
}

impl Default for PendingAsks {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WsActorTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// WebSocket-first implementation of `ActorTransportClient`.
///
/// # Routing logic
/// 1. Check `WsRegistry::is_connected(node_id)`.
/// 2. If connected: wrap request in a `WsFrame` and send through the WS session.
/// 3. If not connected: delegate to `grpc_fallback`.
///
/// # Ask over WS
/// - Register the `request_id` in `pending_asks` before enqueuing the frame.
/// - Wait for the WS receive loop to call `pending_asks.resolve()`.
/// - The receive loop must call `pending_asks.resolve(request_id, response)` when
///   it receives a `WsFrame.ask_response` frame.
pub struct WsActorTransportClient {
    ws_registry: Arc<WsRegistry>,
    pending_asks: Arc<PendingAsks>,
    grpc_fallback: Arc<dyn ActorTransportClient>,
    default_ask_timeout: Duration,
}

impl WsActorTransportClient {
    /// Create a new `WsActorTransportClient` with a WebSocket registry, pending-ask tracker, and gRPC fallback.
    pub fn new(
        ws_registry: Arc<WsRegistry>,
        pending_asks: Arc<PendingAsks>,
        grpc_fallback: Arc<dyn ActorTransportClient>,
    ) -> Self {
        Self {
            ws_registry,
            pending_asks,
            grpc_fallback,
            default_ask_timeout: Duration::from_secs(30),
        }
    }

    /// Return a reference to the `PendingAsks` map so the WS receive loop can
    /// resolve in-flight asks when `AskReplyResponse` frames arrive.
    pub fn pending_asks(&self) -> Arc<PendingAsks> {
        self.pending_asks.clone()
    }
}

#[async_trait]
impl ActorTransportClient for WsActorTransportClient {
    async fn send_message(
        &self,
        node_id: &str,
        request: tonic::Request<SendMessageRequest>,
    ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
        let inner = request.into_inner();
        let request_id = if inner.request_id.is_empty() {
            Ulid::new().to_string()
        } else {
            inner.request_id.clone()
        };

        if let Some(sender) = self.ws_registry.get_sender(node_id).await {
            let frame = WsFrame {
                request_id: request_id.clone(),
                payload: Some(ws_frame::Payload::Tell(inner)),
            };
            sender.send(frame).await.map_err(|_| {
                tonic::Status::unavailable(format!("WS session to '{}' closed", node_id))
            })?;

            // Fire-and-forget: return immediate success
            Ok(tonic::Response::new(SendMessageResponse {
                request_id,
                success: true,
                ..Default::default()
            }))
        } else {
            // gRPC fallback — reconstruct request
            self.grpc_fallback
                .send_message(node_id, tonic::Request::new(inner))
                .await
        }
    }

    async fn ask_reply(
        &self,
        node_id: &str,
        request: tonic::Request<AskReplyRequest>,
    ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
        let inner = request.into_inner();
        let request_id = if inner.request_id.is_empty() {
            Ulid::new().to_string()
        } else {
            inner.request_id.clone()
        };

        if let Some(sender) = self.ws_registry.get_sender(node_id).await {
            // Register before sending so the response can't arrive before we register.
            // node_id is stored with the entry so cancel_for_node() only cancels this session.
            let rx = self
                .pending_asks
                .register(node_id.to_string(), request_id.clone())
                .await;

            let timeout_duration = inner
                .timeout
                .as_ref()
                .map(|t| Duration::new(t.seconds as u64, t.nanos as u32))
                .unwrap_or(self.default_ask_timeout);

            let frame = WsFrame {
                request_id: request_id.clone(),
                payload: Some(ws_frame::Payload::Ask(inner)),
            };
            if let Err(_) = sender.send(frame).await {
                // Session closed between registry lookup and send — clean up so the
                // pending entry doesn't leak until the disconnect cancel_for_node runs.
                self.pending_asks.remove(&request_id).await;
                return Err(tonic::Status::unavailable(format!(
                    "WS session to '{}' closed",
                    node_id
                )));
            }

            match tokio::time::timeout(timeout_duration, rx).await {
                Ok(Ok(resp)) => Ok(tonic::Response::new(resp)),
                Ok(Err(_)) => Err(tonic::Status::internal(format!(
                    "Ask channel closed for request '{}'",
                    request_id
                ))),
                Err(_) => {
                    // Timeout — clean up the pending entry
                    self.pending_asks.remove(&request_id).await;
                    Err(tonic::Status::deadline_exceeded(format!(
                        "Ask to '{}' timed out (request_id: {})",
                        node_id, request_id
                    )))
                }
            }
        } else {
            self.grpc_fallback
                .ask_reply(node_id, tonic::Request::new(inner))
                .await
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// WsNodeTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// WebSocket-first implementation of `NodeTransportClient`.
///
/// All three trait methods delegate to the gRPC fallback: thin nodes are excluded
/// from SWIM indirect ping selection (Phase 7), so the WS path for node pings is
/// never exercised. The `ws_registry` field is retained for future WS-native pings.
pub struct WsNodeTransportClient {
    #[allow(dead_code)]
    ws_registry: Arc<WsRegistry>,
    grpc_fallback: Arc<dyn NodeTransportClient>,
}

impl WsNodeTransportClient {
    /// Create a new `WsNodeTransportClient` with a WebSocket registry, pending-ask tracker (unused), and gRPC fallback.
    pub fn new(
        ws_registry: Arc<WsRegistry>,
        _pending_asks: Arc<PendingAsks>,
        grpc_fallback: Arc<dyn NodeTransportClient>,
    ) -> Self {
        Self {
            ws_registry,
            grpc_fallback,
        }
    }
}

#[async_trait]
impl NodeTransportClient for WsNodeTransportClient {
    async fn ping(
        &self,
        node_id: &str,
        address: &str,
        request: PingRequest,
        timeout: Duration,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Thin nodes (WS-only) don't serve the NodeService gRPC interface, so pings
        // from SWIM to a WS-connected node go through gRPC if available, or are skipped
        // by the SWIM exclusion logic in Phase 7. Full round-trip WS pings will be added
        // when PendingPings infrastructure is in place.
        self.grpc_fallback
            .ping(node_id, address, request, timeout)
            .await
    }

    async fn ping_req(
        &self,
        node_id: &str,
        address: &str,
        request: PingReqRequest,
        timeout: Duration,
    ) -> Result<PingReqResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Thin nodes don't serve as SWIM intermediaries — always use gRPC fallback
        self.grpc_fallback
            .ping_req(node_id, address, request, timeout)
            .await
    }

    async fn sync_membership(
        &self,
        node_id: &str,
        address: &str,
        request: SyncMembershipRequest,
        timeout: Duration,
    ) -> Result<SyncMembershipResponse, Box<dyn std::error::Error + Send + Sync>> {
        // Full membership sync goes over gRPC for reliability
        self.grpc_fallback
            .sync_membership(node_id, address, request, timeout)
            .await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::actor::v1::{AskReplyRequest, SendMessageRequest};
    use plexspaces_proto::node::v1::NodeRole;
    use plexspaces_proto::transport::ws::v1::ws_frame;
    use plexspaces_service_traits::ActorTransportClient;
    use std::sync::Arc;
    use tokio::sync::mpsc;

    // ─── Mock gRPC fallback ────────────────────────────────────────────────

    struct MockGrpcActorFallback;

    #[async_trait]
    impl ActorTransportClient for MockGrpcActorFallback {
        async fn send_message(
            &self,
            _node_id: &str,
            request: tonic::Request<SendMessageRequest>,
        ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
            Ok(tonic::Response::new(SendMessageResponse {
                request_id: request.into_inner().request_id,
                success: true,
                message_id: "grpc-msg".to_string(),
                ..Default::default()
            }))
        }

        async fn ask_reply(
            &self,
            _node_id: &str,
            request: tonic::Request<AskReplyRequest>,
        ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
            Ok(tonic::Response::new(AskReplyResponse {
                request_id: request.into_inner().request_id,
                success: true,
                payload: b"grpc-reply".to_vec(),
                ..Default::default()
            }))
        }
    }

    // ─── Helper ───────────────────────────────────────────────────────────

    fn make_ws_transport(
        registry: Arc<WsRegistry>,
        pending: Arc<PendingAsks>,
    ) -> WsActorTransportClient {
        WsActorTransportClient::new(registry, pending, Arc::new(MockGrpcActorFallback))
    }

    #[tokio::test]
    async fn test_send_message_uses_ws_when_connected() {
        let registry = Arc::new(WsRegistry::new());
        let pending = Arc::new(PendingAsks::new());
        let transport = make_ws_transport(registry.clone(), pending);

        let (tx, mut rx) = mpsc::channel(16);
        let session = crate::ws_registry::WsSession {
            node_id: "ws-node".to_string(),
            sender: tx,
            role: NodeRole::NodeRoleThin,
            tenant_id: "tenant-1".to_string(),
            connected_at: std::time::Instant::now(),
            last_heartbeat: std::time::Instant::now(),
        };
        registry.register(session).await;

        let req = SendMessageRequest {
            request_id: "req-001".to_string(),
            actor_type: "counter".to_string(),
            ..Default::default()
        };
        let resp = transport
            .send_message("ws-node", tonic::Request::new(req))
            .await
            .unwrap();
        assert!(resp.into_inner().success);

        // Verify the WS frame was enqueued
        let frame = rx.recv().await.expect("Expected WS frame");
        assert_eq!(frame.request_id, "req-001");
        assert!(matches!(frame.payload, Some(ws_frame::Payload::Tell(_))));
    }

    #[tokio::test]
    async fn test_send_message_falls_back_to_grpc_when_not_connected() {
        let registry = Arc::new(WsRegistry::new());
        let pending = Arc::new(PendingAsks::new());
        let transport = make_ws_transport(registry, pending);

        let req = SendMessageRequest {
            request_id: "req-grpc".to_string(),
            actor_type: "echo".to_string(),
            ..Default::default()
        };
        let resp = transport
            .send_message("grpc-only-node", tonic::Request::new(req))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(inner.success);
        assert_eq!(inner.message_id, "grpc-msg");
    }

    #[tokio::test]
    async fn test_ask_reply_over_ws_with_correlation() {
        let registry = Arc::new(WsRegistry::new());
        let pending = Arc::new(PendingAsks::new());
        let transport = make_ws_transport(registry.clone(), pending.clone());

        let (tx, mut rx) = mpsc::channel::<WsFrame>(16);
        let session = crate::ws_registry::WsSession {
            node_id: "ws-ask-node".to_string(),
            sender: tx,
            role: NodeRole::NodeRoleThin,
            tenant_id: "tenant-1".to_string(),
            connected_at: std::time::Instant::now(),
            last_heartbeat: std::time::Instant::now(),
        };
        registry.register(session).await;

        // Spawn a task that acts as the WS receive loop — resolves the ask
        let pending2 = pending.clone();
        tokio::spawn(async move {
            if let Some(frame) = rx.recv().await {
                // Extract request_id from frame and resolve
                pending2
                    .resolve(
                        &frame.request_id,
                        AskReplyResponse {
                            request_id: frame.request_id.clone(),
                            success: true,
                            payload: b"ws-pong".to_vec(),
                            ..Default::default()
                        },
                    )
                    .await;
            }
        });

        let req = AskReplyRequest {
            request_id: "ask-001".to_string(),
            actor_type: "echo".to_string(),
            ..Default::default()
        };
        let resp = transport
            .ask_reply("ws-ask-node", tonic::Request::new(req))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(inner.success);
        assert_eq!(inner.payload, b"ws-pong");
    }

    #[tokio::test]
    async fn test_ask_reply_falls_back_to_grpc() {
        let registry = Arc::new(WsRegistry::new());
        let pending = Arc::new(PendingAsks::new());
        let transport = make_ws_transport(registry, pending);

        let req = AskReplyRequest {
            request_id: "ask-grpc".to_string(),
            actor_type: "echo".to_string(),
            ..Default::default()
        };
        let resp = transport
            .ask_reply("grpc-node", tonic::Request::new(req))
            .await
            .unwrap();
        let inner = resp.into_inner();
        assert!(inner.success);
        assert_eq!(inner.payload, b"grpc-reply");
    }

    #[tokio::test]
    async fn test_pending_asks_resolve_and_cancel_for_node() {
        let pending = Arc::new(PendingAsks::new());

        // node-a has two in-flight asks, node-b has one
        let rx_a1 = pending
            .register("node-a".to_string(), "id-a1".to_string())
            .await;
        let rx_a2 = pending
            .register("node-a".to_string(), "id-a2".to_string())
            .await;
        let rx_b1 = pending
            .register("node-b".to_string(), "id-b1".to_string())
            .await;
        assert_eq!(pending.len().await, 3);

        // Resolve one ask from node-a normally
        pending
            .resolve(
                "id-a1",
                AskReplyResponse {
                    request_id: "id-a1".to_string(),
                    success: true,
                    ..Default::default()
                },
            )
            .await;
        assert_eq!(pending.len().await, 2);

        let resp_a1 = rx_a1.await.unwrap();
        assert!(resp_a1.success);

        // Disconnect node-a: only node-a's remaining asks are cancelled
        pending.cancel_for_node("node-a", "node disconnected").await;
        assert_eq!(pending.len().await, 1); // node-b's ask must survive

        let resp_a2 = rx_a2.await.unwrap();
        assert!(!resp_a2.success);
        assert_eq!(resp_a2.request_id, "id-a2"); // request_id echoed for correlation
        assert!(resp_a2.error_message.contains("node disconnected"));

        // node-b's ask is still pending
        assert_eq!(pending.len().await, 1);
        pending.cancel_for_node("node-b", "test teardown").await;
        let resp_b1 = rx_b1.await.unwrap();
        assert!(!resp_b1.success);
        assert_eq!(pending.len().await, 0);
    }
}
