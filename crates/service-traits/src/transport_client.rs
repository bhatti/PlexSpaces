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

//! Transport client traits for protocol-agnostic node-to-node communication.
//!
//! # Purpose
//! Abstracts the underlying transport (gRPC or WebSocket) so that high-level
//! services — `ActorServiceImpl`, `NodeRegistry`, SWIM — call these traits
//! instead of building gRPC clients directly. The transport layer then routes
//! via WebSocket if the target node is connected via WS, falling back to gRPC.
//!
//! # Architecture
//! ```text
//! ActorServiceImpl::route_message()
//!     └─ ActorTransportClient::send_message(node_id, req)
//!             ├─ WsActorTransportClient  (WS-first, gRPC-fallback)
//!             └─ GrpcActorTransportClient (gRPC-only, used pre-WS)
//!
//! NodeRegistry::direct_ping()
//!     └─ NodeTransportClient::ping(node_id, address, req, timeout)
//!             ├─ WsNodeTransportClient  (WS-first, gRPC-fallback)
//!             └─ GrpcNodeTransportClient (gRPC-only, used pre-WS)
//! ```
//!
//! # Design
//! Traits live in `plexspaces-service-traits` (not in `plexspaces-actor` or
//! `plexspaces-node`) to avoid circular dependencies: both `plexspaces-actor`
//! and `plexspaces-node` implement these traits, and `plexspaces-services`
//! consumes them via `ServiceLocator`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use plexspaces_proto::actor::v1::{AskReplyRequest, AskReplyResponse, SendMessageRequest, SendMessageResponse};
use plexspaces_proto::node::v1::{PingRequest, PingReqRequest, PingReqResponse, PingResponse, SyncMembershipRequest, SyncMembershipResponse};

// ─────────────────────────────────────────────────────────────────────────────
// ActorTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// Transport-agnostic client for actor operations on a remote node.
///
/// # Purpose
/// Called by `ActorServiceImpl::route_message()` instead of building gRPC
/// clients directly. Implementations choose WebSocket or gRPC based on
/// whether the target node has an active WS session.
///
/// # Contract
/// - Both methods are infallible at the routing level: transport errors are
///   mapped to `tonic::Status` so callers see a uniform error type.
/// - `node_id` is the canonical node ID of the target node (not its address).
#[async_trait]
pub trait ActorTransportClient: Send + Sync {
    /// Send a fire-and-forget message to an actor on `node_id`.
    async fn send_message(
        &self,
        node_id: &str,
        request: tonic::Request<SendMessageRequest>,
    ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status>;

    /// Send a request-reply message to an actor on `node_id`.
    async fn ask_reply(
        &self,
        node_id: &str,
        request: tonic::Request<AskReplyRequest>,
    ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status>;
}

// ─────────────────────────────────────────────────────────────────────────────
// NodeTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// Transport-agnostic client for node-level operations (SWIM, membership).
///
/// # Purpose
/// Called by `NodeRegistry::direct_ping()`, `indirect_ping()`, and
/// `NodeServiceHandler::notify_disconnect()` instead of building gRPC clients
/// directly. WS-connected thin nodes are pinged via the WS connection;
/// full nodes use gRPC.
///
/// # Contract
/// - Errors are boxed `std::error::Error` to match the existing call-site
///   signatures in node_registry/mod.rs.
#[async_trait]
pub trait NodeTransportClient: Send + Sync {
    /// Send a SWIM direct ping to the node at `node_id` / `address`.
    async fn ping(
        &self,
        node_id: &str,
        address: &str,
        request: PingRequest,
        timeout: Duration,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>>;

    /// Ask an intermediary node to send an indirect ping to a target.
    async fn ping_req(
        &self,
        node_id: &str,
        address: &str,
        request: PingReqRequest,
        timeout: Duration,
    ) -> Result<PingReqResponse, Box<dyn std::error::Error + Send + Sync>>;

    /// Perform a full membership state exchange with a peer node.
    async fn sync_membership(
        &self,
        node_id: &str,
        address: &str,
        request: SyncMembershipRequest,
        timeout: Duration,
    ) -> Result<SyncMembershipResponse, Box<dyn std::error::Error + Send + Sync>>;
}

// ─────────────────────────────────────────────────────────────────────────────
// Arc delegation — lets Arc<dyn Trait> implement the trait
// ─────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl ActorTransportClient for Arc<dyn ActorTransportClient> {
    async fn send_message(
        &self,
        node_id: &str,
        request: tonic::Request<SendMessageRequest>,
    ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
        (**self).send_message(node_id, request).await
    }

    async fn ask_reply(
        &self,
        node_id: &str,
        request: tonic::Request<AskReplyRequest>,
    ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
        (**self).ask_reply(node_id, request).await
    }
}

#[async_trait]
impl NodeTransportClient for Arc<dyn NodeTransportClient> {
    async fn ping(
        &self,
        node_id: &str,
        address: &str,
        request: PingRequest,
        timeout: Duration,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        (**self).ping(node_id, address, request, timeout).await
    }

    async fn ping_req(
        &self,
        node_id: &str,
        address: &str,
        request: PingReqRequest,
        timeout: Duration,
    ) -> Result<PingReqResponse, Box<dyn std::error::Error + Send + Sync>> {
        (**self).ping_req(node_id, address, request, timeout).await
    }

    async fn sync_membership(
        &self,
        node_id: &str,
        address: &str,
        request: SyncMembershipRequest,
        timeout: Duration,
    ) -> Result<SyncMembershipResponse, Box<dyn std::error::Error + Send + Sync>> {
        (**self).sync_membership(node_id, address, request, timeout).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    struct MockActorTransport;

    #[async_trait]
    impl ActorTransportClient for MockActorTransport {
        async fn send_message(
            &self,
            _node_id: &str,
            request: tonic::Request<SendMessageRequest>,
        ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
            let req_id = request.into_inner().request_id.clone();
            Ok(tonic::Response::new(SendMessageResponse {
                request_id: req_id,
                success: true,
                message_id: "test-msg-id".to_string(),
                ..Default::default()
            }))
        }

        async fn ask_reply(
            &self,
            _node_id: &str,
            request: tonic::Request<AskReplyRequest>,
        ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
            let req_id = request.into_inner().request_id.clone();
            Ok(tonic::Response::new(AskReplyResponse {
                request_id: req_id,
                success: true,
                payload: b"pong".to_vec(),
                ..Default::default()
            }))
        }
    }

    #[tokio::test]
    async fn test_send_message_propagates_request_id() {
        let transport = MockActorTransport;
        let req = SendMessageRequest {
            request_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            actor_type: "counter".to_string(),
            ..Default::default()
        };
        let resp = transport
            .send_message("node-1", tonic::Request::new(req))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().request_id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[tokio::test]
    async fn test_ask_reply_propagates_request_id() {
        let transport = MockActorTransport;
        let req = AskReplyRequest {
            request_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".to_string(),
            actor_type: "counter".to_string(),
            ..Default::default()
        };
        let resp = transport
            .ask_reply("node-1", tonic::Request::new(req))
            .await
            .unwrap();
        assert_eq!(resp.into_inner().request_id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
    }

    #[tokio::test]
    async fn test_arc_delegation_works() {
        let transport: Arc<dyn ActorTransportClient> = Arc::new(MockActorTransport);
        let req = SendMessageRequest {
            request_id: "req-123".to_string(),
            actor_type: "echo".to_string(),
            ..Default::default()
        };
        let resp = transport
            .send_message("node-2", tonic::Request::new(req))
            .await
            .unwrap();
        assert!(resp.into_inner().success);
    }
}
