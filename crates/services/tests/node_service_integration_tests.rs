// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for NodeService ConnectNodes and DisconnectNodes over gRPC.
// Uses an in-process tonic Server with NodeServiceImpl so no separate process is required.

use plexspaces_actor::{
    InitializableServiceLocator, NodeRegistryTrait, ObjectRegistry as CoreObjectRegistry,
    ServiceLocator,
};
use plexspaces_common::RequestContextExt;
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::node::v1::{
    node_service_client::NodeServiceClient, ConnectNodesRequest, DisconnectNodesRequest,
    ListConnectedNodesRequest, SecurityConfig,
};
use plexspaces_services::node_service::NodeServiceImpl;
use std::sync::Arc;
use tonic::transport::Server;
use tonic::Request;

fn system_request<T>(message: T) -> Request<T> {
    let mut request = Request::new(message);
    request.metadata_mut().insert(
        "x-tenant-id",
        tonic::metadata::MetadataValue::try_from("test-tenant").expect("valid tenant header"),
    );
    request.metadata_mut().insert(
        "x-namespace",
        tonic::metadata::MetadataValue::try_from("heat").expect("valid namespace header"),
    );
    request
}

/// Build NodeRegistry with SQLite backend for tests
async fn create_test_node_registry(
    node_id: &str,
) -> Arc<plexspaces_services::node_registry::NodeRegistry> {
    use plexspaces_services::node_registry::NodeRegistry;
    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
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
    service_locator
        .register_security_config(SecurityConfig {
            disable_auth: true,
            ..Default::default()
        })
        .await;
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
    let node_svc =
        plexspaces_proto::node::v1::node_service_server::NodeServiceServer::new(node_service)
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

async fn start_node_service_server_with_registry(
    node_id: &str,
    cluster_name: &str,
) -> Result<
    (
        std::net::SocketAddr,
        Arc<plexspaces_services::node_registry::NodeRegistry>,
    ),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let service_locator = Arc::new(plexspaces_services::ServiceLocatorImpl::new());
    service_locator
        .register_security_config(SecurityConfig {
            disable_auth: true,
            ..Default::default()
        })
        .await;
    let node_registry = create_test_node_registry(node_id).await;
    service_locator
        .register_node_registry(node_registry.clone())
        .await;

    if !cluster_name.is_empty() {
        let config = plexspaces_proto::node::v1::NodeConfig {
            id: node_id.to_string(),
            cluster_name: cluster_name.to_string(),
            ..Default::default()
        };
        service_locator.register_node_config(config).await;
    }

    let node_service = NodeServiceImpl::new(service_locator, node_id.to_string());
    let node_svc =
        plexspaces_proto::node::v1::node_service_server::NodeServiceServer::new(node_service)
            .max_decoding_message_size(5 * 1024 * 1024)
            .max_encoding_message_size(5 * 1024 * 1024);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let bound = listener.local_addr()?;
    let server = Server::builder()
        .add_service(node_svc)
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));
    tokio::spawn(server);
    Ok((bound, node_registry))
}

#[tokio::test]
async fn node_service_list_connected_nodes_empty(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
async fn node_service_connect_nodes_registers_seed_addresses(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
    assert_eq!(inner.connected.len(), 1);
    assert!(inner.failed.is_empty());

    let list = client
        .list_connected_nodes(Request::new(ListConnectedNodesRequest {
            cluster: String::new(),
            page_size: 100,
            page_token: String::new(),
            include_health: false,
        }))
        .await?
        .into_inner();
    assert_eq!(list.nodes.len(), 1);
    assert_eq!(list.nodes[0].node_address, "http://192.0.2.1:7999");
    Ok(())
}

#[tokio::test]
async fn node_service_connect_nodes_registers_seed_addresses_in_cluster_view(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = start_node_service_server("node-connect-cluster-test", "heat").await?;
    let endpoint = format!("http://127.0.0.1:{}", addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    client
        .connect_nodes(Request::new(ConnectNodesRequest {
            node_addresses: vec!["192.0.2.2:7999".to_string()],
            cluster: "heat".to_string(),
            timeout: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000,
            }),
        }))
        .await?;

    let list = client
        .list_connected_nodes(Request::new(ListConnectedNodesRequest {
            cluster: "heat".to_string(),
            page_size: 100,
            page_token: String::new(),
            include_health: false,
        }))
        .await?
        .into_inner();
    assert_eq!(list.nodes.len(), 1);
    assert_eq!(list.nodes[0].node_address, "http://192.0.2.2:7999");
    assert_eq!(
        list.nodes[0].capabilities.get("cluster"),
        Some(&"heat".to_string())
    );
    Ok(())
}

#[tokio::test]
async fn node_service_seed_registration_upserts_to_concrete_node_and_supports_id_and_address_lookup(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (addr, registry) =
        start_node_service_server_with_registry("node-connect-upsert", "heat").await?;
    let endpoint = format!("http://127.0.0.1:{}", addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    client
        .connect_nodes(system_request(ConnectNodesRequest {
            node_addresses: vec!["localhost:8093".to_string()],
            cluster: "heat".to_string(),
            timeout: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000,
            }),
        }))
        .await?;

    let ctx =
        plexspaces_common::RequestContext::new_without_auth(String::new(), "heat".to_string())
            .with_admin(true);
    registry
        .register_node(
            &ctx,
            plexspaces_proto::node::v1::NodeRegistration {
                node_id: "test-node-8093".to_string(),
                node_address: "http://0.0.0.0:8093".to_string(),
                capabilities: std::collections::HashMap::from([(
                    "cluster".to_string(),
                    "heat".to_string(),
                )]),
                ..Default::default()
            },
        )
        .await?;

    let by_id = registry
        .lookup_node(&ctx, "test-node-8093")
        .await?
        .expect("concrete node should be discoverable by id");
    assert_eq!(by_id.node_address, "http://0.0.0.0:8093");

    let by_address = registry
        .lookup_node(&ctx, "http://localhost:8093")
        .await?
        .expect("concrete node should be discoverable by canonical address");
    assert_eq!(by_address.node_id, "test-node-8093");

    let list = client
        .list_connected_nodes(system_request(ListConnectedNodesRequest {
            cluster: "heat".to_string(),
            page_size: 100,
            page_token: String::new(),
            include_health: false,
        }))
        .await?
        .into_inner();
    assert_eq!(list.nodes.len(), 1);
    assert_eq!(list.nodes[0].node_id, "test-node-8093");

    Ok(())
}

#[tokio::test]
async fn node_service_disconnect_nodes_unknown_idempotent(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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
        inner.disconnected.contains(&"nonexistent-node".to_string())
            || inner.failed.contains_key("nonexistent-node"),
        "Idempotent: unknown node should be in disconnected or failed"
    );
    Ok(())
}
