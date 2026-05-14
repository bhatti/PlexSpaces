// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for node heartbeat and registration functionality
//!
//! ## Purpose
//! Verifies that:
//! 1. A node registers itself in ObjectRegistry during start
//! 2. Heartbeat background task updates the registration timestamp
//! 3. A node registered in a cluster namespace is visible in that namespace
//! 4. A node only sees registrations from its own namespace (cluster isolation)

use plexspaces_actor::{NodeRegistryTrait, ServiceLocator, ServiceLocatorBase};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Helper: spawn start() in background then poll until the node appears in ObjectRegistry.
///
/// The node registers itself in ObjectRegistry inside `start()`, before the gRPC server
/// begins serving — so the spawned task will always complete registration before blocking.
async fn start_node_background(node: Arc<Node>) {
    let n = node.clone();
    tokio::spawn(async move {
        let _ = n.start().await;
    });

    let node_id = node.id().as_str().to_string();
    let cluster_name = {
        let c = node.config().cluster_name.clone();
        if c.is_empty() { None } else { Some(c) }
    };

    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        sleep(Duration::from_millis(50)).await;
        if tokio::time::Instant::now() >= deadline {
            panic!("Node {} did not register within 8s", node_id);
        }
        let Some(obj_reg) = node.service_locator().get_object_registry().await else {
            continue;
        };
        let ctx = if let Some(ref cluster) = cluster_name {
            node.service_locator()
                .request_context_for_system_operations_with_namespace(cluster.clone())
                .await
        } else {
            node.service_locator()
                .request_context_for_system_operations()
                .await
        };
        if obj_reg
            .lookup_full(&ctx, ObjectType::ObjectTypeNode, &node_id)
            .await
            .ok()
            .flatten()
            .is_some()
        {
            break;
        }
    }
    // Extra wait to ensure heartbeat has run at least once
    sleep(Duration::from_millis(150)).await;
}

/// Test that a node registers itself and its heartbeat timestamp advances.
#[tokio::test]
async fn test_node_registration_and_heartbeat() {
    let node = Arc::new(
        NodeBuilder::new("test-node-1")
            .with_in_memory_backends()
            .with_listen_addr("127.0.0.1:0")
            .with_heartbeat_interval_ms(100)
            .build()
            .await,
    );

    start_node_background(node.clone()).await;

    let object_registry = node
        .service_locator()
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be available");

    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;

    let registration = object_registry
        .lookup_full(&ctx, ObjectType::ObjectTypeNode, "test-node-1")
        .await
        .expect("lookup should not error")
        .expect("node should be registered");

    assert_eq!(registration.object_id, "test-node-1");
    assert_eq!(registration.object_type, ObjectType::ObjectTypeNode as i32);

    let node_registry = node
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry should be available");
    let (visible_nodes, _) = node_registry.list_nodes(&ctx, None, 100, "").await.unwrap();
    assert!(
        visible_nodes
            .iter()
            .any(|entry| entry.node_id == "test-node-1"),
        "node should appear in NodeRegistry list"
    );

    let initial_heartbeat = registration.last_heartbeat.clone();

    // Wait for at least one more heartbeat cycle
    sleep(Duration::from_millis(400)).await;

    let updated = object_registry
        .lookup_full(&ctx, ObjectType::ObjectTypeNode, "test-node-1")
        .await
        .expect("lookup should not error")
        .expect("node should still be registered");

    assert!(
        updated.last_heartbeat.is_some(),
        "heartbeat timestamp should be set"
    );
    if let (Some(initial), Some(after)) = (initial_heartbeat, &updated.last_heartbeat) {
        assert!(
            after.seconds > initial.seconds
                || (after.seconds == initial.seconds && after.nanos > initial.nanos),
            "heartbeat timestamp should advance: initial={:?} after={:?}",
            initial,
            after
        );
    }

    node.shutdown(Duration::from_secs(5))
        .await
        .expect("node should shut down cleanly");
}

/// Test that a node with a cluster name registers under the cluster namespace.
///
/// With in-memory (isolated) backends, each node has its own registry, so we
/// verify per-node state only — shared-registry cross-node visibility requires
/// an external DB and is covered by higher-level integration suites.
#[tokio::test]
async fn test_node_registers_in_cluster_namespace() {
    let cluster_name = "test-cluster";

    let node1: Arc<Node> = Arc::new(
        NodeBuilder::new("node-1")
            .with_in_memory_backends()
            .with_listen_addr("127.0.0.1:0")
            .with_cluster_name(cluster_name.to_string())
            .with_heartbeat_interval_ms(100)
            .build()
            .await,
    );

    let node2: Arc<Node> = Arc::new(
        NodeBuilder::new("node-2")
            .with_in_memory_backends()
            .with_listen_addr("127.0.0.1:0")
            .with_cluster_name(cluster_name.to_string())
            .with_heartbeat_interval_ms(100)
            .build()
            .await,
    );

    // Start both nodes in background
    let n1 = node1.clone();
    tokio::spawn(async move {
        let _ = n1.start().await;
    });
    let n2 = node2.clone();
    tokio::spawn(async move {
        let _ = n2.start().await;
    });
    sleep(Duration::from_millis(400)).await;

    // Each node should see itself registered under the cluster namespace
    for (node, node_id) in [(&node1, "node-1"), (&node2, "node-2")] {
        let ctx = node
            .service_locator()
            .request_context_for_system_operations_with_namespace(cluster_name.to_string())
            .await;

        let registry = node
            .service_locator()
            .get_object_registry()
            .await
            .expect("ObjectRegistry should be available");

        let reg: ObjectRegistration = registry
            .lookup_full(&ctx, ObjectType::ObjectTypeNode, node_id)
            .await
            .expect("lookup should not error")
            .unwrap_or_else(|| panic!("{} should be registered under cluster namespace", node_id));

        assert_eq!(reg.object_id, node_id);
        assert_eq!(reg.namespace, cluster_name);
        assert!(reg.last_heartbeat.is_some(), "heartbeat should be set");
    }

    node1
        .shutdown(Duration::from_secs(5))
        .await
        .expect("node1 should shut down");
    node2
        .shutdown(Duration::from_secs(5))
        .await
        .expect("node2 should shut down");
}

/// Test that cluster namespace isolation works: a node in cluster-1 cannot see
/// registrations from cluster-2 within its own (isolated) ObjectRegistry.
#[tokio::test]
async fn test_cluster_isolation() {
    let cluster1 = "cluster-1";
    let cluster2 = "cluster-2";

    let node1: Arc<Node> = Arc::new(
        NodeBuilder::new("node-cluster1")
            .with_in_memory_backends()
            .with_listen_addr("127.0.0.1:0")
            .with_cluster_name(cluster1.to_string())
            .with_heartbeat_interval_ms(100)
            .build()
            .await,
    );

    let node2: Arc<Node> = Arc::new(
        NodeBuilder::new("node-cluster2")
            .with_in_memory_backends()
            .with_listen_addr("127.0.0.1:0")
            .with_cluster_name(cluster2.to_string())
            .with_heartbeat_interval_ms(100)
            .build()
            .await,
    );

    let n1 = node1.clone();
    tokio::spawn(async move {
        let _ = n1.start().await;
    });
    let n2 = node2.clone();
    tokio::spawn(async move {
        let _ = n2.start().await;
    });
    sleep(Duration::from_millis(400)).await;

    // node1's own registry, scoped to cluster1
    let registry1 = node1
        .service_locator()
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be available");

    let ctx1 = node1
        .service_locator()
        .request_context_for_system_operations_with_namespace(cluster1.to_string())
        .await;

    // node-cluster2 must NOT appear in node1's cluster1-scoped registry
    let cross_lookup: Option<ObjectRegistration> = registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-cluster2")
        .await
        .expect("lookup should not error");
    assert!(
        cross_lookup.is_none(),
        "node-cluster2 must not be visible under cluster1 namespace"
    );

    // node1 must see itself
    let self_lookup: ObjectRegistration = registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-cluster1")
        .await
        .expect("lookup should not error")
        .expect("node-cluster1 should see itself");

    assert_eq!(self_lookup.object_id, "node-cluster1");
    assert_eq!(self_lookup.namespace, cluster1);

    node1
        .shutdown(Duration::from_secs(5))
        .await
        .expect("node1 should shut down");
    node2
        .shutdown(Duration::from_secs(5))
        .await
        .expect("node2 should shut down");
}
