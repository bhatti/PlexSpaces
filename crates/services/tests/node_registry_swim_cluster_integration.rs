// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration: when a peer's PingResponse omits cluster_name but the observer has
// NodeConfig.cluster_name set, SWIM reconciliation must label the peer so
// ListConnectedNodes (and from_registry placement) see it under that cluster.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plexspaces_core::{
    GrpcConnectionManager, NodeRegistryTrait, ObjectRegistry as CoreObjectRegistry, ServiceLocator,
};
use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};
use plexspaces_proto::node::v1::{
    node_service_client::NodeServiceClient, node_service_server::NodeServiceServer,
    ConnectNodesRequest, ListConnectedNodesRequest, NodeConfig, ReleaseSpec, SecurityConfig,
};
use plexspaces_services::node_registry::{NodeRegistry, NodeRegistryConfig};
use plexspaces_services::node_service::NodeServiceImpl;
use plexspaces_services::ServiceLocatorImpl;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;
use tonic::Request;

struct SpawnedGossipNode {
    grpc_addr: std::net::SocketAddr,
    node_registry: Arc<NodeRegistry>,
}

/// In-process NodeService with SWIM-capable NodeRegistry and gRPC connection manager.
async fn spawn_gossip_node(
    node_id: &str,
    listen_addr: &str,
    cluster_name_for_ping: &str,
    release_cluster: Option<&str>,
) -> Result<SpawnedGossipNode, Box<dyn std::error::Error + Send + Sync>> {
    let listener = TcpListener::bind(listen_addr).await?;
    let grpc_addr = listener.local_addr()?;
    let local_http_addr = format!("http://127.0.0.1:{}", grpc_addr.port());

    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .map_err(|e| e.to_string())?,
    );
    let object_registry: Arc<dyn CoreObjectRegistry> =
        Arc::new(ObjectRegistryImpl::new(object_repo));

    let mut nr_config = NodeRegistryConfig::default();
    nr_config.gossip_enabled = true;
    nr_config.swim_config.protocol_period = Duration::from_millis(20);
    nr_config.swim_config.probe_timeout = Duration::from_secs(2);
    nr_config.swim_config.anti_entropy_interval = Duration::from_secs(3600);

    let node_registry = Arc::new(NodeRegistry::new(
        object_registry,
        node_id.to_string(),
        local_http_addr.clone(),
        nr_config,
        None,
    ));

    let service_locator: Arc<ServiceLocatorImpl> = Arc::new(ServiceLocatorImpl::new());
    service_locator
        .register_security_config(SecurityConfig {
            disable_auth: true,
            ..Default::default()
        })
        .await;
    let grpc_mgr = Arc::new(GrpcConnectionManager::new(Some(2)));
    ServiceLocator::register_grpc_connection_manager(service_locator.as_ref(), grpc_mgr).await;
    service_locator
        .register_node_registry(node_registry.clone())
        .await;

    let node_config = NodeConfig {
        id: node_id.to_string(),
        listen_addr: format!("127.0.0.1:{}", grpc_addr.port()),
        cluster_name: cluster_name_for_ping.to_string(),
        ..Default::default()
    };
    service_locator.register_node_config(node_config).await;

    let node_service: NodeServiceImpl = if let Some(cluster) = release_cluster {
        let release = ReleaseSpec {
            name: "integration".into(),
            version: "1.0.0".into(),
            description: String::new(),
            node: Some(NodeConfig {
                id: node_id.to_string(),
                cluster_name: cluster.to_string(),
                listen_addr: format!("127.0.0.1:{}", grpc_addr.port()),
                ..Default::default()
            }),
            ..Default::default()
        };
        NodeServiceImpl::with_release_spec(service_locator.clone(), node_id.to_string(), release)
    } else {
        NodeServiceImpl::new(service_locator.clone(), node_id.to_string())
    };

    node_registry
        .set_service_locator(service_locator.clone() as Arc<dyn ServiceLocator>)
        .await;

    let node_svc = NodeServiceServer::new(node_service)
        .max_decoding_message_size(5 * 1024 * 1024)
        .max_encoding_message_size(5 * 1024 * 1024);

    tokio::spawn(
        Server::builder()
            .add_service(node_svc)
            .serve_with_incoming(TcpListenerStream::new(listener)),
    );

    Ok(SpawnedGossipNode {
        grpc_addr,
        node_registry,
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn swim_reconcile_stamps_local_cluster_when_peer_ping_omits_cluster_name(
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let peer = spawn_gossip_node(
        "node-b",
        "127.0.0.1:0",
        "",
        None, // no ReleaseSpec → connect seeds without cluster on this side only matters for B's view
    )
    .await?;

    let observer = spawn_gossip_node("node-a", "127.0.0.1:0", "heat", Some("heat")).await?;

    let endpoint = format!("http://127.0.0.1:{}", observer.grpc_addr.port());
    let mut client = NodeServiceClient::connect(endpoint).await?;

    client
        .connect_nodes(Request::new(ConnectNodesRequest {
            node_addresses: vec![format!("127.0.0.1:{}", peer.grpc_addr.port())],
            cluster: "heat".into(),
            timeout: Some(prost_types::Duration {
                seconds: 5,
                nanos: 0,
            }),
        }))
        .await?;

    observer.node_registry.start_gossip_protocol();

    let deadline = Instant::now() + Duration::from_secs(6);
    let mut ok = false;
    while Instant::now() < deadline {
        let list = client
            .list_connected_nodes(Request::new(ListConnectedNodesRequest {
                cluster: "heat".into(),
                page_size: 100,
                page_token: String::new(),
                include_health: false,
            }))
            .await?
            .into_inner();

        if list.nodes.iter().any(|n| {
            n.node_id == "node-b"
                && n.capabilities.get("cluster").map(String::as_str) == Some("heat")
        }) {
            ok = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }

    assert!(
        ok,
        "ListConnectedNodes(cluster=heat) should include node-b with cluster=heat after SWIM ping \
         (peer returns empty cluster_name in PingResponse)"
    );

    Ok(())
}
