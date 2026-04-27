// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Wrapper so NodeServiceServer can use Arc<NodeServiceImpl> (same instance registered as NodeConnectivity).

use async_trait::async_trait;
use std::sync::Arc;
use tonic::{Request, Response, Status};

use plexspaces_proto::node::v1::{
    node_service_server::NodeService, CalculateCapacityRequest, ConnectNodesRequest,
    ConnectNodesResponse, DisconnectNodesRequest, DisconnectNodesResponse, GetHealthRequest,
    GetHealthResponse, GetMetricsRequest, GetReleaseSpecRequest, GetReleaseSpecResponse,
    ListConnectedNodesRequest, ListConnectedNodesResponse, ListNodeApplicationsRequest,
    ListNodeApplicationsResponse, NodeCapacity, NodeMetrics, PingRequest, RegisterNodesRequest,
    RegisterNodesResponse, SendHeartbeatRequest, SendHeartbeatResponse,
    StreamConnectedNodesRequest, UnregisterNodeRequest, UnregisterNodeResponse,
};
use plexspaces_services::node_service::NodeServiceImpl;

/// Wraps Arc<NodeServiceImpl> so the gRPC server and NodeConnectivity can share the same instance.
pub struct NodeServiceHandler(pub Arc<NodeServiceImpl>);

#[async_trait]
impl NodeService for NodeServiceHandler {
    type StreamConnectedNodesStream = <NodeServiceImpl as NodeService>::StreamConnectedNodesStream;

    async fn get_release_spec(
        &self,
        request: Request<GetReleaseSpecRequest>,
    ) -> Result<Response<GetReleaseSpecResponse>, Status> {
        self.0.get_release_spec(request).await
    }
    async fn register_nodes(
        &self,
        request: Request<RegisterNodesRequest>,
    ) -> Result<Response<RegisterNodesResponse>, Status> {
        self.0.register_nodes(request).await
    }
    async fn unregister_node(
        &self,
        request: Request<UnregisterNodeRequest>,
    ) -> Result<Response<UnregisterNodeResponse>, Status> {
        self.0.unregister_node(request).await
    }
    async fn list_connected_nodes(
        &self,
        request: Request<ListConnectedNodesRequest>,
    ) -> Result<Response<ListConnectedNodesResponse>, Status> {
        self.0.list_connected_nodes(request).await
    }
    async fn stream_connected_nodes(
        &self,
        request: Request<StreamConnectedNodesRequest>,
    ) -> Result<Response<Self::StreamConnectedNodesStream>, Status> {
        self.0.stream_connected_nodes(request).await
    }
    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<NodeMetrics>, Status> {
        self.0.get_metrics(request).await
    }
    async fn calculate_capacity(
        &self,
        request: Request<CalculateCapacityRequest>,
    ) -> Result<Response<NodeCapacity>, Status> {
        self.0.calculate_capacity(request).await
    }
    async fn list_node_applications(
        &self,
        request: Request<ListNodeApplicationsRequest>,
    ) -> Result<Response<ListNodeApplicationsResponse>, Status> {
        self.0.list_node_applications(request).await
    }
    async fn get_health(
        &self,
        request: Request<GetHealthRequest>,
    ) -> Result<Response<GetHealthResponse>, Status> {
        self.0.get_health(request).await
    }
    async fn send_heartbeat(
        &self,
        request: Request<SendHeartbeatRequest>,
    ) -> Result<Response<SendHeartbeatResponse>, Status> {
        self.0.send_heartbeat(request).await
    }
    async fn ping(
        &self,
        request: Request<PingRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::PingResponse>, Status> {
        self.0.ping(request).await
    }
    async fn ping_req(
        &self,
        request: Request<plexspaces_proto::node::v1::PingReqRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::PingReqResponse>, Status> {
        self.0.ping_req(request).await
    }
    async fn sync_membership(
        &self,
        request: Request<plexspaces_proto::node::v1::SyncMembershipRequest>,
    ) -> Result<Response<plexspaces_proto::node::v1::SyncMembershipResponse>, Status> {
        self.0.sync_membership(request).await
    }
    async fn connect_nodes(
        &self,
        request: Request<ConnectNodesRequest>,
    ) -> Result<Response<ConnectNodesResponse>, Status> {
        self.0.connect_nodes(request).await
    }
    async fn disconnect_nodes(
        &self,
        request: Request<DisconnectNodesRequest>,
    ) -> Result<Response<DisconnectNodesResponse>, Status> {
        self.0.disconnect_nodes(request).await
    }
}
