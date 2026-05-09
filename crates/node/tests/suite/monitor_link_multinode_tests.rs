// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Multi-node monitor / demonitor: `Node::monitor` / `Node::demonitor` with a peer
// registered in `NodeRegistry` (same path production uses for remote RPC).

use std::sync::Arc;
use std::time::Duration;

use plexspaces_actor::{ActorRegistry, ExitReason, RequestContext, ServiceLocator, ServiceLocatorBase, RequestContextExt};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::actor::v1::actor_service_server::ActorServiceServer;
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::node::v1::NodeRegistration;
use plexspaces_services::actor_service::ActorServiceImpl;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;
use tonic::transport::Server;

use super::test_helpers::{register_actor_with_message_sender, test_runtime_actor_id};

/// Start `ActorService` for `node` on an ephemeral port; returns `host:port` (no scheme).
async fn start_actor_grpc_server(node: Arc<plexspaces_node::Node>) -> String {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl::new(node.service_locator(), node.id().as_str().to_string());

    let listener = TcpListener::bind(addr).await.unwrap();
    let bound_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve_with_incoming(TcpListenerStream::new(listener))
            .await
            .expect("ActorService test server failed");
    });

    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    bound_addr.to_string()
}

/// Poll mailbox for a `__DOWN__` (same contract as `distributed_supervision` tests).
async fn wait_for_down(
    mailbox: &plexspaces_mailbox::Mailbox,
    deadline: Duration,
) -> Option<Message> {
    let start = tokio::time::Instant::now();
    loop {
        let remaining = deadline.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            return None;
        }
        let poll_timeout = remaining.min(Duration::from_millis(50));
        if let Some(msg) = mailbox.dequeue_with_timeout(Some(poll_timeout)).await {
            if msg.message_type == "__DOWN__"
                || msg.headers.get("type").map_or(false, |v| v == "__DOWN__")
            {
                return Some(msg);
            }
        }
        if start.elapsed() >= deadline {
            return None;
        }
    }
}

#[tokio::test]
async fn test_remote_monitor_then_demonitor_clears_registry_on_worker_node() {
    let node1 = Arc::new(NodeBuilder::new("node1").with_auth_disabled().build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").with_auth_disabled().build().await);

    let node2_listen = start_actor_grpc_server(node2.clone()).await;

    let worker_id = test_runtime_actor_id("remote-w", "node2");
    let sup_id = test_runtime_actor_id("remote-s", "node1");

    let sup_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), sup_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node1, &sup_id, sup_mailbox).await;

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node2, &worker_id, worker_mailbox).await;

    let ctx = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let node_registry = node1
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry should be available for remote monitor/demonitor");
    node_registry
        .register_node(
            &ctx,
            NodeRegistration {
                node_id: "node2".to_string(),
                node_address: format!("http://{}", node2_listen),
                ..Default::default()
            },
        )
        .await
        .expect("register peer node2");

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let reg2: Arc<ActorRegistry> = node2
        .service_locator()
        .actor_registry()
        .await
        .expect("node2 ActorRegistry");
    assert!(
        reg2.actor_monitor()
            .get_monitors(&worker_id)
            .await
            .is_empty(),
        "precondition: no monitors on worker"
    );

    let monitor_ref = node1
        .monitor(&ctx, &worker_id, &sup_id)
        .await
        .expect("remote MonitorActor RPC");

    assert_eq!(
        reg2.actor_monitor().get_monitors(&worker_id).await.len(),
        1,
        "monitor entry should exist on the node hosting the worker"
    );

    node1
        .demonitor(&ctx, &worker_id, &sup_id, &monitor_ref)
        .await
        .expect("remote DemonitorActor RPC");

    assert!(
        reg2.actor_monitor()
            .get_monitors(&worker_id)
            .await
            .is_empty(),
        "demonitor should remove the monitor on the remote registry"
    );
}

#[tokio::test]
async fn test_cross_node_link_registers_on_both_actor_registries() {
    let node1 = Arc::new(NodeBuilder::new("node1").with_auth_disabled().build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").with_auth_disabled().build().await);

    let node1_listen = start_actor_grpc_server(node1.clone()).await;
    let node2_listen = start_actor_grpc_server(node2.clone()).await;

    let a_id = test_runtime_actor_id("link-a", "node1");
    let b_id = test_runtime_actor_id("link-b", "node2");

    let ma = Arc::new(
        Mailbox::new(MailboxConfig::default(), a_id.to_string())
            .await
            .unwrap(),
    );
    let mb = Arc::new(
        Mailbox::new(MailboxConfig::default(), b_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node1, &a_id, ma).await;
    register_actor_with_message_sender(&node2, &b_id, mb).await;

    let ctx_sys = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let nr1 = node1
        .service_locator()
        .get_node_registry()
        .await
        .expect("node1 NodeRegistry");
    nr1.register_node(
        &ctx_sys,
        NodeRegistration {
            node_id: "node1".to_string(),
            node_address: format!("http://{}", node1_listen),
            ..Default::default()
        },
    )
    .await
    .expect("register node1 on node1 (lookup for local link RPC)");
    nr1.register_node(
        &ctx_sys,
        NodeRegistration {
            node_id: "node2".to_string(),
            node_address: format!("http://{}", node2_listen),
            ..Default::default()
        },
    )
    .await
    .expect("register node2 on node1");

    let ctx_sys2 = node2
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let nr2 = node2
        .service_locator()
        .get_node_registry()
        .await
        .expect("node2 NodeRegistry");
    nr2.register_node(
        &ctx_sys2,
        NodeRegistration {
            node_id: "node1".to_string(),
            node_address: format!("http://{}", node1_listen),
            ..Default::default()
        },
    )
    .await
    .expect("register node1 on node2");
    nr2.register_node(
        &ctx_sys2,
        NodeRegistration {
            node_id: "node2".to_string(),
            node_address: format!("http://{}", node2_listen),
            ..Default::default()
        },
    )
    .await
    .expect("register node2 on node2 (lookup for local link RPC)");

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let ctx = RequestContext::new_without_auth("default".to_string(), "default".to_string());
    node1
        .link(&ctx, &a_id, &b_id)
        .await
        .expect("cross-node link");

    let reg1: Arc<ActorRegistry> = node1.service_locator().actor_registry().await.unwrap();
    let reg2: Arc<ActorRegistry> = node2.service_locator().actor_registry().await.unwrap();

    let links_a = reg1.get_links(&a_id).await;
    assert!(
        links_a.contains(&b_id),
        "node1 registry should record link from A to B: {:?}",
        links_a
    );
    let links_b = reg2.get_links(&b_id).await;
    assert!(
        links_b.contains(&a_id),
        "node2 registry should record link from B to A: {:?}",
        links_b
    );
}

/// Remote monitor: when the **worker node** runs `handle_actor_termination` for the worker,
/// the supervisor on the **other node** receives a `__DOWN__` in its mailbox (via registry `tell`
/// routing to the supervisor's canonical `ActorId`).
#[tokio::test]
async fn test_remote_monitor_down_delivered_to_supervisor_mailbox() {
    let node1 = Arc::new(NodeBuilder::new("node1").with_auth_disabled().build().await);
    let node2 = Arc::new(NodeBuilder::new("node2").with_auth_disabled().build().await);

    let node1_listen = start_actor_grpc_server(node1.clone()).await;
    let node2_listen = start_actor_grpc_server(node2.clone()).await;

    let worker_id = test_runtime_actor_id("down-worker", "node2");
    let supervisor_id = test_runtime_actor_id("down-supervisor", "node1");

    let supervisor_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), supervisor_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node1, &supervisor_id, supervisor_mailbox.clone()).await;

    let worker_mailbox = Arc::new(
        Mailbox::new(MailboxConfig::default(), worker_id.to_string())
            .await
            .unwrap(),
    );
    register_actor_with_message_sender(&node2, &worker_id, worker_mailbox.clone()).await;

    let ctx1 = node1
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let nr1 = node1
        .service_locator()
        .get_node_registry()
        .await
        .expect("node1 NodeRegistry");
    for reg in [
        NodeRegistration {
            node_id: "node1".to_string(),
            node_address: format!("http://{}", node1_listen),
            ..Default::default()
        },
        NodeRegistration {
            node_id: "node2".to_string(),
            node_address: format!("http://{}", node2_listen),
            ..Default::default()
        },
    ] {
        nr1.register_node(&ctx1, reg)
            .await
            .expect("node1 nr register");
    }

    let ctx2 = node2
        .service_locator()
        .request_context_for_system_operations()
        .await;
    let nr2 = node2
        .service_locator()
        .get_node_registry()
        .await
        .expect("node2 NodeRegistry");
    for reg in [
        NodeRegistration {
            node_id: "node1".to_string(),
            node_address: format!("http://{}", node1_listen),
            ..Default::default()
        },
        NodeRegistration {
            node_id: "node2".to_string(),
            node_address: format!("http://{}", node2_listen),
            ..Default::default()
        },
    ] {
        nr2.register_node(&ctx2, reg)
            .await
            .expect("node2 nr register");
    }

    tokio::time::sleep(Duration::from_millis(50)).await;

    node1
        .monitor(&ctx1, &worker_id, &supervisor_id)
        .await
        .expect("establish remote monitor on node2");

    let reg2: Arc<ActorRegistry> = node2.service_locator().actor_registry().await.unwrap();
    assert_eq!(reg2.actor_monitor().get_monitors(&worker_id).await.len(), 1);

    reg2.handle_actor_termination(&worker_id, ExitReason::Normal)
        .await;

    let down = wait_for_down(&supervisor_mailbox, Duration::from_secs(3))
        .await
        .expect("supervisor on node1 should receive __DOWN__ after remote worker termination");
    assert!(
        down.message_type == "__DOWN__"
            || down.headers.get("type").map_or(false, |v| v == "__DOWN__"),
        "expected __DOWN__ message, got type {:?}",
        down.message_type
    );
}
