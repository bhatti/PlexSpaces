// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for NodeService ConnectNodes and DisconnectNodes over gRPC.
// Uses an in-process tonic Server with NodeServiceImpl so no separate process is required.

use plexspaces_core::ObjectRegistry as CoreObjectRegistry;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::node::v1::{
    node_service_client::NodeServiceClient,
    ConnectNodesRequest, DisconnectNodesRequest, ListConnectedNodesRequest,
};
use plexspaces_services::node_service::NodeServiceImpl;
use std::sync::Arc;
use tonic::transport::Server;
use tonic::Request;

/// Build NodeRegistry with SQLite backend for tests
async fn create_test_node_registry(node_id: &str) -> Arc<plexspaces_services::node_registry::NodeRegistry> {
    use plexspaces_services::node_registry::NodeRegistry;
    let object_repo = Arc::new(SqliteObjectRegistryRepository::new(":memory:").await.unwrap());
    let object_registry: Arc<dyn CoreObjectRegistry> =
        Arc::new(ObjectRegistryImpl::new(object_repo));
    Arc::new(NodeRegistry::new_simple(
        object_registry,
        node_id.to_string(),
        Some(60),
        Some(false),
        None,
        None,
    ))
}

/// Spawn in-process gRPC server with NodeService; returns bound_addr. Server runs until test exits.
async fn start_node_service_server(
    node_id: &str,
    cluster_name: &str,
) -> Result<std::net::SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let service_locator = Arc::new(plexspaces_services::ServiceLocatorImpl::new());
    let node_registry = create_test_node_registry(node_id).await;
    service_locator.register_node_registry(node_registry).await;

    if !cluster_name.is_empty() {
        let config = plexspaces_proto::node::v1::NodeConfig {
            id: node_id.to_string(),
            cluster_name: cluster_name.to_string(),
            ..Default::default()
        };
        service_locator.register_node_config(config).await;
    }

    let node_service = NodeServiceImpl::new(service_locator, node_id.to_string());
    let node_svc = plexspaces_proto::node::v1::node_service_server::NodeServiceServer::new(node_service)
        .max_decoding_message_size(5 * 1024 * 1024)
        .max_encoding_message_size(5 * 1024 * 1024);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let bound = listener.local_addr()?;
    let server = Server::builder()
        .add_service(node_svc)
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));
    tokio::spawn(server);
    Ok(bound)
}

#[tokio::test]
async fn node_service_list_connected_nodes_empty() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = start_node_service_server("node-list-test", "").await?;
    let endpoint = format!("http://127.0.0.1:{}", addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    let req = Request::new(ListConnectedNodesRequest {
        cluster: String::new(),
        page_size: 100,
        page_token: String::new(),
        include_health: false,
    });
    let res = client.list_connected_nodes(req).await?;
    let inner = res.into_inner();
    assert!(inner.nodes.is_empty(), "Expected no connected nodes");
    Ok(())
}

#[tokio::test]
async fn node_service_connect_nodes_unreachable_returns_failed() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = start_node_service_server("node-connect-test", "").await?;
    let endpoint = format!("http://127.0.0.1:{}", addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    let req = Request::new(ConnectNodesRequest {
        node_addresses: vec!["192.0.2.1:7999".to_string()],
        cluster: String::new(),
        timeout: Some(prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
    });
    let res = client.connect_nodes(req).await?;
    let inner = res.into_inner();
    assert!(inner.connected.is_empty());
    assert_eq!(inner.failed.len(), 1);
    assert!(inner.failed.contains_key("192.0.2.1:7999"));
    Ok(())
}

#[tokio::test]
async fn node_service_disconnect_nodes_unknown_idempotent() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = start_node_service_server("node-disconnect-test", "").await?;
    let endpoint = format!("http://127.0.0.1:{}", addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    let req = Request::new(DisconnectNodesRequest {
        node_ids: vec!["nonexistent-node".to_string()],
        notify_remote: false,
    });
    let res = client.disconnect_nodes(req).await?;
    let inner = res.into_inner();
    assert!(
        inner.disconnected.contains(&"nonexistent-node".to_string()) || inner.failed.contains_key("nonexistent-node"),
        "Idempotent: unknown node should be in disconnected or failed"
    );
    Ok(())
}
