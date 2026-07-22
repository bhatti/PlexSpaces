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

//! WebSocket endpoint for PlexSpaces thin-client connections.
//!
//! # Endpoint
//! `GET /ws` — WebSocket upgrade with JWT authentication.
//!
//! # Auth
//! JWT is validated at upgrade time via:
//! 1. `?token=<jwt>` query parameter (browser-friendly)
//! 2. `Authorization: Bearer <jwt>` header (standard)
//!
//! The upgrade is rejected with 401 if auth is enabled and neither is valid.
//!
//! # Protocol flow
//! 1. Upgrade → first binary frame MUST be `WsFrame { node_register: NodeRegistration }`
//! 2. Server responds `WsFrame { node_register_ack: WsNodeRegisterAck }`
//! 3. Client sends tell/ask frames; server routes them via `ActorServiceImpl`
//! 4. Ask responses arrive back as `WsFrame { ask_response: AskReplyResponse }`
//!    with the same `request_id` the client set in the ask frame.
//! 5. Heartbeat frames keep the session alive.
//! 6. Socket close: session unregistered from WsRegistry.
//!
//! # Wire format
//! All frames: binary protobuf (`WsFrame` via prost). No JSON.

use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use prost::Message as ProstMessage;
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};
use ulid::Ulid;

pub use plexspaces_proto::transport::ws::v1::WsFrame;
use plexspaces_proto::transport::ws::v1::{ws_frame, WsError, WsNodeRegisterAck};
use plexspaces_actor::ServiceLocator;
use plexspaces_services::actor_service::ActorServiceImpl;

use plexspaces_actor::{NodeRegistryTrait, RequestContext, RequestContextExt};

use crate::ws_registry::{WsRegistry, WsSession};
use crate::ws_transport_client::PendingAsks;
use crate::http_jwt::JwtKeyPair;

// ─────────────────────────────────────────────────────────────────────────────
// State
// ─────────────────────────────────────────────────────────────────────────────

/// Shared state for the `/ws` endpoint.
#[derive(Clone)]
pub struct WsRouteState {
    pub actor_service: Arc<ActorServiceImpl>,
    pub ws_registry: Arc<WsRegistry>,
    pub pending_asks: Arc<PendingAsks>,
    pub service_locator: Arc<dyn ServiceLocator>,
    pub node_registry: Option<Arc<dyn NodeRegistryTrait>>,
    pub auth_disabled: bool,
    pub jwt_key_pair: Option<Arc<JwtKeyPair>>,
}

/// Query parameters for the `/ws` upgrade.
#[derive(Deserialize)]
struct WsQueryParams {
    /// JWT bearer token (alternative to Authorization header, browser-friendly).
    token: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router
// ─────────────────────────────────────────────────────────────────────────────

/// Create the `/ws` router with the given state.
pub fn ws_router(state: WsRouteState) -> Router {
    Router::new()
        .route("/ws", get(ws_upgrade_handler))
        .with_state(state)
}

// ─────────────────────────────────────────────────────────────────────────────
// Upgrade handler
// ─────────────────────────────────────────────────────────────────────────────

async fn ws_upgrade_handler(
    State(state): State<WsRouteState>,
    Query(params): Query<WsQueryParams>,
    headers: axum::http::HeaderMap,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    // Validate JWT before upgrading — reject early to avoid wasting a connection.
    let tenant_id = match extract_tenant_id(state.auth_disabled, state.jwt_key_pair.as_deref(), &params, &headers) {
        Ok(tid) => tid,
        Err(msg) => {
            warn!("WS upgrade rejected: {}", msg);
            return (StatusCode::UNAUTHORIZED, msg).into_response();
        }
    };

    ws.on_upgrade(move |socket| handle_ws_connection(socket, tenant_id, state))
        .into_response()
}

/// Extract tenant_id from JWT token (query param or Authorization header).
fn extract_tenant_id(
    auth_disabled: bool,
    jwt_key_pair: Option<&JwtKeyPair>,
    params: &WsQueryParams,
    headers: &axum::http::HeaderMap,
) -> Result<String, String> {
    if auth_disabled {
        // Extract tenant from x-tenant-id header if present, empty string otherwise.
        let tenant = headers
            .get("x-tenant-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if tenant.is_empty() && tracing::enabled!(tracing::Level::DEBUG) {
            debug!("WS upgrade with auth disabled and no x-tenant-id header");
        }
        return Ok(tenant);
    }

    let kp = jwt_key_pair
        .ok_or_else(|| "Auth enabled but JWT key not configured".to_string())?;

    // Prefer ?token= query param (browser WebSocket can't set headers).
    // validate_bearer_token_with_keypair expects "Bearer <token>" format.
    let auth_header: String = if let Some(ref t) = params.token {
        format!("Bearer {}", t)
    } else {
        headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| "Missing JWT: supply ?token= or Authorization: Bearer".to_string())?
    };

    crate::http_jwt::validate_bearer_token_with_keypair(kp, Some(&auth_header))
        .map(|claims| claims.tenant_id)
        .map_err(|e| format!("Invalid JWT: {}", e))
}

// ─────────────────────────────────────────────────────────────────────────────
// Connection handler
// ─────────────────────────────────────────────────────────────────────────────

const WS_CHANNEL_CAPACITY: usize = 256;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

async fn handle_ws_connection(
    socket: WebSocket,
    tenant_id: String,
    state: WsRouteState,
) {
    let (mut ws_sender, mut ws_receiver) = StreamExt::split(socket);
    let (tx, mut rx) = mpsc::channel::<WsFrame>(WS_CHANNEL_CAPACITY);

    // ── Step 1: Wait for NodeRegistration handshake frame ─────────────────────
    // ws_sender is passed so error frames can be flushed before the writer task starts.
    let (node_id, node_role, node_registration, ack_request_id) = match await_registration(&mut ws_receiver, &mut ws_sender, &tenant_id).await {
        Ok(result) => result,
        Err(_) => return, // error already sent directly to client
    };

    // ── Step 2: Reject duplicate node_id (prevent session takeover) ─────────
    // A reconnecting client that sends the same node_id as an existing live session
    // could hijack in-flight asks on the old session. Reject duplicates; the client
    // must wait for the old session to close (TCP close propagates the unregister)
    // or use a new node_id.
    if state.ws_registry.is_connected(&node_id).await {
        warn!("WS duplicate node_id '{}' rejected (session takeover prevention)", node_id);
        let err = WsFrame {
            request_id: ack_request_id,
            payload: Some(ws_frame::Payload::Error(WsError {
                request_id: ulid::Ulid::new().to_string(),
                code: 9,
                message: format!("node_id '{}' already registered", node_id),
            })),
        };
        let _ = ws_sender.send(Message::Binary(err.encode_to_vec().into())).await;
        return;
    }

    // ── Step 2b: Register session in WsRegistry BEFORE sending ack ──────────
    // Registering first ensures the client only proceeds once server state is consistent;
    // any test or code that checks is_connected()/session_count() after receiving the ack
    // will see the session already present with no sleep required.
    let session = WsSession {
        node_id: node_id.clone(),
        sender: tx.clone(),
        role: node_role,
        tenant_id: tenant_id.clone(),
        connected_at: std::time::Instant::now(),
        last_heartbeat: std::time::Instant::now(),
    };
    state.ws_registry.register(session).await;

    // Send ack — writer task not yet running so send directly.
    let ack_frame = WsFrame {
        request_id: ack_request_id,
        payload: Some(ws_frame::Payload::NodeRegisterAck(WsNodeRegisterAck {
            success: true,
            assigned_node_id: node_id.clone(),
            error_message: String::new(),
        })),
    };
    let ack_bytes = ack_frame.encode_to_vec();
    let _ = ws_sender.send(Message::Binary(ack_bytes.into())).await;

    info!("WS session registered: node_id={}, tenant={}", node_id, tenant_id);

    // ── Step 3: Register thin node in NodeRegistry for cluster membership ───────
    // This allows the node to appear in list_connected_nodes and be correctly excluded
    // from SWIM indirect ping intermediary selection.
    let node_registry_ctx = RequestContext::new_without_auth(
        tenant_id.clone(),
        String::new(),
    );
    if let Some(ref nr) = state.node_registry {
        match nr.register_node(&node_registry_ctx, node_registration).await {
            Ok(_) => {
                metrics::counter!("plexspaces_ws_thin_node_registered_total").increment(1);
            }
            Err(e) => {
                warn!("Failed to register thin node {} in NodeRegistry: {}", node_id, e);
                metrics::counter!("plexspaces_ws_thin_node_registration_errors_total").increment(1);
            }
        }
    }

    // ── Step 4: Writer task — drains the mpsc channel to the WS socket ──────
    let writer_handle = tokio::spawn(async move {
        while let Some(frame) = rx.recv().await {
            let bytes = frame.encode_to_vec();
            if ws_sender.send(Message::Binary(bytes.into())).await.is_err() {
                break;
            }
        }
    });

    // ── Step 5: Reader loop — processes incoming frames ───────────────────────
    let node_id_for_cleanup = node_id.clone();
    loop {
        let msg = match ws_receiver.next().await {
            Some(Ok(msg)) => msg,
            _ => break, // socket closed or error
        };

        match msg {
            Message::Binary(data) => {
                match WsFrame::decode(data.as_ref()) {
                    Ok(frame) => {
                        dispatch_frame(frame, &node_id, &tenant_id, &state).await;
                    }
                    Err(e) => {
                        warn!("WS decode error from {}: {}", node_id, e);
                        // No request_id available — frame couldn't be decoded.
                        let _ = send_error(&tx, "", 13, &format!("Decode error: {}", e)).await;
                    }
                }
            }
            Message::Close(_) => break,
            Message::Ping(_) => {
                // Axum handles Ping/Pong at the transport layer automatically.
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!("WS ping from {}", node_id);
                }
            }
            _ => {} // Text frames not supported
        }
    }

    // ── Cleanup ──────────────────────────────────────────────────────────────
    state.ws_registry.unregister(&node_id_for_cleanup).await;
    if let Some(ref nr) = state.node_registry {
        match nr.unregister_node(&node_registry_ctx, &node_id_for_cleanup).await {
            Ok(_) => {
                metrics::counter!("plexspaces_ws_thin_node_unregistered_total").increment(1);
            }
            Err(e) => {
                warn!("Failed to unregister thin node {} from NodeRegistry: {}", node_id_for_cleanup, e);
                metrics::counter!("plexspaces_ws_thin_node_unregistration_errors_total").increment(1);
            }
        }
    }
    // Cancel only asks for this session so other connected thin nodes are not affected.
    state.pending_asks.cancel_for_node(&node_id_for_cleanup, "WS session closed").await;
    writer_handle.abort();
    info!("WS session closed: node_id={}", node_id_for_cleanup);
}

/// Wait for the mandatory first NodeRegistration frame (with timeout).
///
/// Takes the raw `ws_sender` split half so that error frames can be flushed to the
/// client before the writer task is spawned. The writer task must not start until
/// this function returns — if it did, error frames sent during a failed handshake
/// would sit in the mpsc buffer and never be drained.
///
/// Returns `(node_id, node_role, registration)` on success.
async fn await_registration(
    ws_receiver: &mut futures_util::stream::SplitStream<WebSocket>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    tenant_id: &str,
) -> Result<(String, plexspaces_proto::node::v1::NodeRole, plexspaces_proto::node::v1::NodeRegistration, String), ()> {
    // Helper: send an error frame directly on the socket (writer task not yet running).
    // request_id is empty for pre-registration errors (no request to correlate against).
    async fn send_ws_error(
        ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
        code: u32,
        message: &str,
    ) {
        let frame = WsFrame {
            request_id: ulid::Ulid::new().to_string(),
            payload: Some(ws_frame::Payload::Error(WsError {
                request_id: ulid::Ulid::new().to_string(),
                code,
                message: message.to_string(),
            })),
        };
        let bytes = frame.encode_to_vec();
        let _ = ws_sender.send(Message::Binary(bytes.into())).await;
    }

    let result = tokio::time::timeout(HANDSHAKE_TIMEOUT, async {
        loop {
            match ws_receiver.next().await {
                Some(Ok(Message::Binary(data))) => {
                    return WsFrame::decode(data.as_ref()).map_err(|e| format!("Decode: {}", e));
                }
                Some(Ok(Message::Close(_))) => {
                    return Err("Client closed before handshake".to_string());
                }
                Some(Err(e)) => return Err(format!("Socket error: {}", e)),
                None => return Err("Socket closed".to_string()),
                _ => {} // Skip non-binary frames
            }
        }
    })
    .await;

    let frame = match result {
        Ok(Ok(f)) => f,
        Ok(Err(e)) => {
            send_ws_error(ws_sender, 9, &e).await;
            warn!("WS handshake failed for tenant {}: {}", tenant_id, e);
            return Err(());
        }
        Err(_elapsed) => {
            send_ws_error(ws_sender, 4, "Handshake timeout").await;
            warn!("WS handshake timeout for tenant {}", tenant_id);
            return Err(());
        }
    };

    // Expect node_register payload
    let reg = match frame.payload {
        Some(ws_frame::Payload::NodeRegister(r)) => r,
        _ => {
            send_ws_error(ws_sender, 9, "First frame must be node_register").await;
            return Err(());
        }
    };

    let node_id = if reg.node_id.is_empty() {
        Ulid::new().to_string()
    } else {
        reg.node_id.clone()
    };

    // The /ws endpoint is exclusively for thin (WS-only) clients. Full nodes always connect
    // via gRPC. Always assign NodeRoleThin regardless of what the client declared so that
    // NodeRegistry, SWIM, and WsRegistry.list_thin_nodes() all see the correct role.
    let node_role = plexspaces_proto::node::v1::NodeRole::NodeRoleThin;

    // Build the canonical registration with the resolved node_id (may differ if client sent empty).
    let registration = plexspaces_proto::node::v1::NodeRegistration {
        node_id: node_id.clone(),
        node_address: reg.node_address.clone(),
        node_role: plexspaces_proto::node::v1::NodeRole::NodeRoleThin as i32,
        capabilities: reg.capabilities.clone(),
        ..Default::default()
    };

    // Return the ack request_id without sending — caller must register in WsRegistry first,
    // then send the ack so the client only proceeds after the server state is consistent.
    Ok((node_id, node_role, registration, frame.request_id))
}

/// Dispatch a decoded `WsFrame` to the appropriate handler.
async fn dispatch_frame(
    frame: WsFrame,
    node_id: &str,
    tenant_id: &str,
    state: &WsRouteState,
) {
    let request_id = frame.request_id.clone();
    match frame.payload {
        Some(ws_frame::Payload::Tell(req)) => {
            handle_tell(request_id, req, node_id, tenant_id, state).await;
        }
        Some(ws_frame::Payload::Ask(req)) => {
            handle_ask(request_id, req, node_id, tenant_id, state).await;
        }
        Some(ws_frame::Payload::AskResponse(resp)) => {
            // Response from a remote node to an outbound ask
            state.pending_asks.resolve(&request_id, resp).await;
        }
        Some(ws_frame::Payload::Heartbeat(_hb)) => {
            if let Some(tx) = state.ws_registry.update_heartbeat_and_get_sender(node_id).await {
                let ack = WsFrame {
                    request_id,
                    payload: Some(ws_frame::Payload::HeartbeatAck(
                        plexspaces_proto::node::v1::SendHeartbeatResponse {
                            request_id: ulid::Ulid::new().to_string(),
                            acknowledged: true,
                            server_time: None,
                        },
                    )),
                };
                // try_send: if the channel is full the client is backed up; drop the ack.
                // Heartbeats are advisory; the client will retry after the next interval.
                if tx.try_send(ack).is_err() {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("WS heartbeat ack dropped: channel full for {}", node_id);
                    }
                }
            }
            // Forward to NodeRegistry so SWIM treats the thin node as alive while connected.
            if let Some(ref nr) = state.node_registry {
                let ctx = RequestContext::new_without_auth(tenant_id.to_string(), String::new());
                if let Err(e) = nr.send_heartbeat(&ctx, node_id, None).await {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("WS heartbeat → NodeRegistry failed for {}: {}", node_id, e);
                    }
                }
            }
        }
        Some(ws_frame::Payload::NodePing(req)) => {
            if let Some(tx) = state.ws_registry.get_sender(node_id).await {
                let resources = {
                    let available_cores = std::thread::available_parallelism()
                        .map(|n| n.get() as u32)
                        .unwrap_or(0);
                    let mut sys = sysinfo::System::new_all();
                    sys.refresh_all();
                    let cpu_percent = sys.global_cpu_info().cpu_usage();
                    let memory_available_mb = sys.available_memory() / (1024 * 1024);
                    Some(plexspaces_proto::node::v1::NodeResourceHints {
                        cpu_percent,
                        memory_available_mb,
                        available_cores,
                    })
                };
                let pong = WsFrame {
                    request_id,
                    payload: Some(ws_frame::Payload::NodePingResponse(
                        plexspaces_proto::node::v1::PingResponse {
                            node_id: node_id.to_string(),
                            sequence_number: req.sequence_number,
                            resources,
                            ..Default::default()
                        },
                    )),
                };
                // try_send: ping-pong is best-effort; drop if channel full.
                if tx.try_send(pong).is_err() {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        debug!("WS node-ping pong dropped: channel full for {}", node_id);
                    }
                }
            }
        }
        Some(ws_frame::Payload::MetricsRequest(_)) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS metrics request from {} (not yet implemented)", node_id);
            }
        }
        Some(ws_frame::Payload::NodeRegister(_)) => {
            warn!("Duplicate node_register frame from {}", node_id);
        }
        None | Some(ws_frame::Payload::Error(_)) | Some(ws_frame::Payload::NodeRegisterAck(_))
        | Some(ws_frame::Payload::TellResponse(_)) | Some(ws_frame::Payload::HeartbeatAck(_))
        | Some(ws_frame::Payload::NodePingResponse(_)) | Some(ws_frame::Payload::MetricsResponse(_)) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS: ignored server-to-client or unknown frame from {}", node_id);
            }
        }
    }
}

/// Handle a tell frame — fire-and-forget actor message.
async fn handle_tell(
    request_id: String,
    req: plexspaces_proto::actor::v1::SendMessageRequest,
    node_id: &str,
    tenant_id: &str,
    state: &WsRouteState,
) {
    use plexspaces_proto::actor::v1::actor_service_server::ActorService as ActorServiceTrait;
    use tonic::metadata::MetadataValue;

    let namespace = req.namespace.clone();
    let actor_type = req.actor_type.clone();
    let mut grpc_req = tonic::Request::new(req);
    if let Ok(v) = MetadataValue::try_from(tenant_id) {
        grpc_req.metadata_mut().insert("x-tenant-id", v);
    }
    if let Ok(v) = MetadataValue::try_from(namespace.as_str()) {
        grpc_req.metadata_mut().insert("x-namespace", v);
    }

    match ActorServiceTrait::send_message(&*state.actor_service, grpc_req).await {
        Ok(_) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS tell dispatched: actor_type={}, request_id={}", actor_type, request_id);
            }
        }
        Err(status) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS tell failed: actor_type={}, status={}", actor_type, status.message());
            }
            let sender = state.ws_registry.get_sender(node_id).await;
            if let Some(tx) = sender {
                let _ = send_error(&tx, &request_id, status.code() as u32, status.message()).await;
            }
        }
    }
}

/// Handle an ask frame — request-reply actor message, send response back.
async fn handle_ask(
    request_id: String,
    req: plexspaces_proto::actor::v1::AskReplyRequest,
    node_id: &str,
    tenant_id: &str,
    state: &WsRouteState,
) {
    use plexspaces_proto::actor::v1::actor_service_server::ActorService as ActorServiceTrait;
    use tonic::metadata::MetadataValue;

    let namespace = req.namespace.clone();
    let actor_type = req.actor_type.clone();
    let req_id = request_id.clone();
    let mut grpc_req = tonic::Request::new(req);
    if let Ok(v) = MetadataValue::try_from(tenant_id) {
        grpc_req.metadata_mut().insert("x-tenant-id", v);
    }
    if let Ok(v) = MetadataValue::try_from(namespace.as_str()) {
        grpc_req.metadata_mut().insert("x-namespace", v);
    }

    let sender = state.ws_registry.get_sender(node_id).await;
    let tx = match sender {
        Some(tx) => tx,
        None => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS ask response: session for {} already gone", node_id);
            }
            return;
        }
    };

    match ActorServiceTrait::ask_reply(&*state.actor_service, grpc_req).await {
        Ok(grpc_resp) => {
            let mut resp = grpc_resp.into_inner();
            // Override request_id with the WS-layer request_id (the gRPC layer may set its own).
            resp.request_id = req_id;
            let frame = WsFrame {
                request_id: request_id.clone(),
                payload: Some(ws_frame::Payload::AskResponse(resp)),
            };
            let _ = tx.send(frame).await;
        }
        Err(status) => {
            if tracing::enabled!(tracing::Level::DEBUG) {
                debug!("WS ask failed: actor_type={}, status={}", actor_type, status.message());
            }
            let resp = plexspaces_proto::actor::v1::AskReplyResponse {
                request_id: req_id,
                success: false,
                error_message: status.message().to_string(),
                ..Default::default()
            };
            let frame = WsFrame {
                request_id: request_id.clone(),
                payload: Some(ws_frame::Payload::AskResponse(resp)),
            };
            let _ = tx.send(frame).await;
        }
    }
}

/// Send an error frame to the client.
/// `request_id` echoes the frame that triggered the error so clients can correlate
/// it to an in-flight ask/tell on a multiplexed connection.
async fn send_error(tx: &mpsc::Sender<WsFrame>, request_id: &str, code: u32, message: &str) -> Result<(), ()> {
    let frame = WsFrame {
        request_id: request_id.to_string(),
        payload: Some(ws_frame::Payload::Error(WsError {
            request_id: request_id.to_string(),
            code,
            message: message.to_string(),
        })),
    };
    tx.send(frame).await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_tenant_id_with_auth_disabled_uses_header() {
        let params = WsQueryParams { token: None };
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-tenant-id", "my-tenant".parse().unwrap());

        let result = extract_tenant_id(true, None, &params, &headers);
        assert_eq!(result.unwrap(), "my-tenant");
    }

    #[test]
    fn extract_tenant_id_with_auth_disabled_no_header_returns_empty() {
        let params = WsQueryParams { token: None };
        let headers = axum::http::HeaderMap::new();

        let result = extract_tenant_id(true, None, &params, &headers);
        assert_eq!(result.unwrap(), "");
    }

    #[test]
    fn extract_tenant_id_with_auth_enabled_no_jwt_key_returns_error() {
        let params = WsQueryParams { token: Some("some-token".to_string()) };
        let headers = axum::http::HeaderMap::new();

        let result = extract_tenant_id(false, None, &params, &headers);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not configured"));
    }

    #[test]
    fn extract_tenant_id_with_auth_enabled_no_token_returns_error() {
        let params = WsQueryParams { token: None };
        let headers = axum::http::HeaderMap::new();

        let result = extract_tenant_id(false, None, &params, &headers);
        assert!(result.is_err());
    }

    #[test]
    fn extract_tenant_id_with_valid_jwt() {
        use plexspaces_grpc_middleware::{JwtClaims, JwtKeyPair};

        let kp = JwtKeyPair::from_secret("test-secret");
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let claims = JwtClaims {
            sub: "user-1".to_string(),
            exp: now + 3600,
            iat: now,
            iss: "test".to_string(),
            aud: vec!["plexspaces-api".to_string()],
            tenant_id: "tenant-1".to_string(),
            roles: vec![],
            groups: vec![],
            is_admin: false,
            jti: None,
        };
        let jwt = plexspaces_grpc_middleware::sign_jwt_with_keypair(&kp, &claims)
            .expect("sign JWT");
        let params = WsQueryParams { token: Some(jwt) };
        let headers = axum::http::HeaderMap::new();

        let result = extract_tenant_id(false, Some(&kp), &params, &headers);
        assert_eq!(result.unwrap(), "tenant-1");
    }
}
