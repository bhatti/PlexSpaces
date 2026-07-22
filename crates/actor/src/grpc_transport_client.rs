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

//! gRPC-only implementations of `ActorTransportClient` and `NodeTransportClient`.
//!
//! # Purpose
//! These implementations delegate directly to the existing `GrpcConnectionManager`
//! pool. They are the default transport used when no WebSocket registry is registered.
//! When WS support is active, `WsActorTransportClient` / `WsNodeTransportClient`
//! (in `plexspaces-node`) wrap these as gRPC fallback.
//!
//! # Architecture
//! Both types hold a `GrpcConnectionManager` and a `ServiceLocator` reference for
//! node-address resolution. They do NOT own the address resolution logic — they call
//! the same helpers that `ServiceLocatorImpl::get_actor_service_client` uses, so the
//! ObjectRegistry → NodeRegistry lookup chain is DRY.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use plexspaces_proto::actor::v1::actor_service_client::ActorServiceClient;
use plexspaces_proto::actor::v1::{AskReplyRequest, AskReplyResponse, SendMessageRequest, SendMessageResponse};
use plexspaces_proto::node::v1::node_service_client::NodeServiceClient;
use plexspaces_proto::node::v1::{PingRequest, PingReqRequest, PingReqResponse, PingResponse, SyncMembershipRequest, SyncMembershipResponse};
use plexspaces_service_traits::{ActorTransportClient, NodeTransportClient};

use crate::service_locator_trait::ServiceLocator;

// ─────────────────────────────────────────────────────────────────────────────
// GrpcActorTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// gRPC implementation of `ActorTransportClient`.
///
/// # Purpose
/// Used when there is no active WebSocket session for the target node.
/// Routes actor calls through the pooled gRPC channels managed by
/// `GrpcConnectionManager`.
///
/// # Lifecycle
/// Constructed at node startup and registered via
/// `InitializableServiceLocator::register_actor_transport_client()`.
pub struct GrpcActorTransportClient {
    service_locator: Arc<dyn ServiceLocator>,
}

impl GrpcActorTransportClient {
    /// Create a new client backed by the given `ServiceLocator`.
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }
}

#[async_trait]
impl ActorTransportClient for GrpcActorTransportClient {
    async fn send_message(
        &self,
        node_id: &str,
        request: tonic::Request<SendMessageRequest>,
    ) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
        let channel = self
            .service_locator
            .get_actor_service_client(node_id)
            .await
            .map_err(|e| tonic::Status::unavailable(format!("Cannot connect to node '{}': {}", node_id, e)))?;
        ActorServiceClient::new(channel)
            .send_message(request)
            .await
    }

    async fn ask_reply(
        &self,
        node_id: &str,
        request: tonic::Request<AskReplyRequest>,
    ) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
        let channel = self
            .service_locator
            .get_actor_service_client(node_id)
            .await
            .map_err(|e| tonic::Status::unavailable(format!("Cannot connect to node '{}': {}", node_id, e)))?;
        ActorServiceClient::new(channel).ask_reply(request).await
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GrpcNodeTransportClient
// ─────────────────────────────────────────────────────────────────────────────

/// gRPC implementation of `NodeTransportClient`.
///
/// # Purpose
/// Used by the SWIM protocol (`NodeRegistry::direct_ping`, `indirect_ping`) and
/// `NodeService::notify_disconnect` when no WS session exists for the target node.
pub struct GrpcNodeTransportClient {
    service_locator: Arc<dyn ServiceLocator>,
}

impl GrpcNodeTransportClient {
    /// Create a new client backed by the given `ServiceLocator`.
    pub fn new(service_locator: Arc<dyn ServiceLocator>) -> Self {
        Self { service_locator }
    }

    async fn node_channel(
        &self,
        node_id: &str,
        address: &str,
    ) -> Result<tonic::transport::Channel, Box<dyn std::error::Error + Send + Sync>> {
        use crate::grpc_connection_manager::ServiceType;
        let conn_manager = self
            .service_locator
            .get_grpc_connection_manager()
            .await
            .ok_or("GrpcConnectionManager not available")?;
        conn_manager
            .get_connection(ServiceType::ServiceNameNodeService, node_id, address)
            .await
            .map_err(|e| format!("Failed to get channel for node '{}': {}", node_id, e).into())
    }
}

#[async_trait]
impl NodeTransportClient for GrpcNodeTransportClient {
    async fn ping(
        &self,
        node_id: &str,
        address: &str,
        request: PingRequest,
        timeout: Duration,
    ) -> Result<PingResponse, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.node_channel(node_id, address).await?;
        let mut client = NodeServiceClient::new(channel);
        let response = tokio::time::timeout(timeout, client.ping(tonic::Request::new(request)))
            .await
            .map_err(|_| "Ping timeout")?
            .map_err(|e| format!("Ping to '{}' failed: {}", node_id, e))?;
        Ok(response.into_inner())
    }

    async fn ping_req(
        &self,
        node_id: &str,
        address: &str,
        request: PingReqRequest,
        timeout: Duration,
    ) -> Result<PingReqResponse, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.node_channel(node_id, address).await?;
        let mut client = NodeServiceClient::new(channel);
        let response =
            tokio::time::timeout(timeout, client.ping_req(tonic::Request::new(request)))
                .await
                .map_err(|_| "PingReq timeout")?
                .map_err(|e| format!("PingReq to '{}' failed: {}", node_id, e))?;
        Ok(response.into_inner())
    }

    async fn sync_membership(
        &self,
        node_id: &str,
        address: &str,
        request: SyncMembershipRequest,
        timeout: Duration,
    ) -> Result<SyncMembershipResponse, Box<dyn std::error::Error + Send + Sync>> {
        let channel = self.node_channel(node_id, address).await?;
        let mut client = NodeServiceClient::new(channel);
        let response = tokio::time::timeout(
            timeout,
            client.sync_membership(tonic::Request::new(request)),
        )
        .await
        .map_err(|_| "SyncMembership timeout")?
        .map_err(|e| format!("SyncMembership to '{}' failed: {}", node_id, e))?;
        Ok(response.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    // Verify the types are Send + Sync (compile-time check — no runtime assertion)
    fn assert_send_sync<T: Send + Sync>() {}

    #[test]
    fn grpc_actor_transport_client_is_send_sync() {
        // Would fail to compile if GrpcActorTransportClient is not Send+Sync
        assert_send_sync::<GrpcActorTransportClient>();
    }

    #[test]
    fn grpc_node_transport_client_is_send_sync() {
        assert_send_sync::<GrpcNodeTransportClient>();
    }

    #[test]
    fn trait_objects_are_arc_compatible() {
        // Verify dyn upcasting compiles — these are API surface tests
        fn accepts_actor(_: Arc<dyn ActorTransportClient>) {}
        fn accepts_node(_: Arc<dyn NodeTransportClient>) {}

        // These function signatures compile only if the bounds are correct
        let _ = accepts_actor;
        let _ = accepts_node;
    }
}
