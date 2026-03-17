// SPDX-License-Identifier: LGPL-2.1-or-later
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

//! Integration tests for node heartbeat functionality
//!
//! ## Purpose
//! Verifies that:
//! 1. Nodes register themselves in ObjectRegistry using internal context
//! 2. Nodes heartbeat to update their own registration (not to other nodes)
//! 3. Heartbeat updates are visible across nodes in the same cluster
//! 4. Cluster isolation works (nodes in different clusters don't see each other)

use plexspaces_core::NodeRegistryTrait;
use plexspaces_core::RequestContext;
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::object_registry::v1::ObjectType;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Test that node registers and heartbeats correctly
#[tokio::test]
async fn test_node_registration_and_heartbeat() {
    let node = Arc::new(
        NodeBuilder::new("test-node-1")
            .with_in_memory_backends()
            .with_heartbeat_interval_ms(100) // Fast heartbeat for tests
            .build()
            .await,
    );

    // Start node (clone Arc since start() takes ownership)
    node.clone().start().await.expect("Node should start");

    // Wait for registration to complete
    sleep(Duration::from_millis(500)).await;

    // Verify node is registered in ObjectRegistry
    let object_registry = node
        .service_locator()
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be available");

    // Use internal context (same as registration)
    let ctx = node
        .service_locator()
        .request_context_for_system_operations()
        .await;

    // Lookup the node
    let registration = object_registry
        .lookup_full(&ctx, ObjectType::ObjectTypeNode, "test-node-1")
        .await
        .expect("Should be able to lookup node")
        .expect("Node should be registered");

    assert_eq!(registration.object_id, "test-node-1");
    assert_eq!(registration.object_type, ObjectType::ObjectTypeNode as i32);

    let node_registry = node
        .service_locator()
        .get_node_registry()
        .await
        .expect("NodeRegistry should be available");
    let (visible_nodes, _) = node_registry.list_nodes(&ctx, None, 100, "").await.unwrap();
    assert!(visible_nodes
        .iter()
        .any(|entry| entry.node_id == "test-node-1"));

    // Get initial heartbeat timestamp
    let initial_heartbeat = registration.last_heartbeat.clone();

    // Wait for heartbeat interval (100ms) plus buffer for test reliability
    // Reduced from 6000ms to 300ms for faster tests
    sleep(Duration::from_millis(300)).await;

    // Verify heartbeat timestamp was updated
    let updated_registration = object_registry
        .lookup_full(&ctx, ObjectType::ObjectTypeNode, "test-node-1")
        .await
        .expect("Should be able to lookup node")
        .expect("Node should still be registered");

    assert!(updated_registration.last_heartbeat.is_some());
    if let (Some(initial), Some(updated)) =
        (initial_heartbeat, &updated_registration.last_heartbeat)
    {
        // Updated timestamp should be later than initial
        assert!(
            updated.seconds > initial.seconds
                || (updated.seconds == initial.seconds && updated.nanos > initial.nanos),
            "Heartbeat timestamp should be updated"
        );
    }

    // Shutdown
    node.shutdown(Duration::from_secs(5))
        .await
        .expect("Node should shutdown");
}

/// Test that nodes in the same cluster can see each other's heartbeats
#[tokio::test]
async fn test_heartbeat_across_nodes_same_cluster() {
    let cluster_name = "test-cluster";

    // Create two nodes in the same cluster with fast heartbeat for tests
    let node1: Arc<Node> = Arc::new(
        NodeBuilder::new("node-1")
            .with_in_memory_backends()
            .with_cluster_name(cluster_name.to_string())
            .with_heartbeat_interval_ms(100) // Fast heartbeat for tests
            .build()
            .await,
    );

    let node2: Arc<Node> = Arc::new(
        NodeBuilder::new("node-2")
            .with_in_memory_backends()
            .with_cluster_name(cluster_name.to_string())
            .with_heartbeat_interval_ms(100) // Fast heartbeat for tests
            .build()
            .await,
    );

    // Start both nodes (clone Arc since start() takes ownership)
    node1.clone().start().await.expect("Node1 should start");
    node2.clone().start().await.expect("Node2 should start");

    // Wait for registration
    sleep(Duration::from_millis(500)).await;

    // Node1 should be able to see Node2's registration
    let object_registry1 = node1
        .service_locator()
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be available");

    // Use internal context with cluster_name as namespace (same as registration)
    let ctx1 = node1
        .service_locator()
        .request_context_for_system_operations_with_namespace(cluster_name.to_string())
        .await;

    let node2_registration = object_registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-2")
        .await
        .expect("Should be able to lookup node2")
        .expect("Node2 should be registered");

    assert_eq!(node2_registration.object_id, "node-2");
    assert_eq!(node2_registration.namespace, cluster_name);

    // Get initial heartbeat timestamp for node2
    let initial_heartbeat = node2_registration.last_heartbeat.clone();

    // Wait for heartbeat interval (100ms) plus buffer for test reliability
    // Reduced from 6000ms to 300ms for faster tests
    sleep(Duration::from_millis(300)).await;

    // Node1 should see updated heartbeat for Node2
    let updated_registration = object_registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-2")
        .await
        .expect("Should be able to lookup node2")
        .expect("Node2 should still be registered");

    assert!(updated_registration.last_heartbeat.is_some());
    if let (Some(initial), Some(updated)) =
        (initial_heartbeat, &updated_registration.last_heartbeat)
    {
        assert!(
            updated.seconds > initial.seconds
                || (updated.seconds == initial.seconds && updated.nanos > initial.nanos),
            "Node1 should see Node2's heartbeat updates"
        );
    }

    // Shutdown
    node1
        .shutdown(Duration::from_secs(5))
        .await
        .expect("Node1 should shutdown");
    node2
        .shutdown(Duration::from_secs(5))
        .await
        .expect("Node2 should shutdown");
}

/// Test that nodes in different clusters cannot see each other
#[tokio::test]
async fn test_cluster_isolation() {
    let cluster1 = "cluster-1";
    let cluster2 = "cluster-2";

    // Create two nodes in different clusters
    let node1: Arc<Node> = Arc::new(
        NodeBuilder::new("node-cluster1")
            .with_in_memory_backends()
            .with_cluster_name(cluster1.to_string())
            .with_heartbeat_interval_ms(100) // Fast heartbeat for tests
            .build()
            .await,
    );

    let node2: Arc<Node> = Arc::new(
        NodeBuilder::new("node-cluster2")
            .with_in_memory_backends()
            .with_cluster_name(cluster2.to_string())
            .with_heartbeat_interval_ms(100) // Fast heartbeat for tests
            .build()
            .await,
    );

    // Start both nodes (clone Arc since start() takes ownership)
    node1.clone().start().await.expect("Node1 should start");
    node2.clone().start().await.expect("Node2 should start");

    // Wait for registration
    sleep(Duration::from_millis(500)).await;

    // Node1 should NOT be able to see Node2 (different cluster/namespace)
    let object_registry1 = node1
        .service_locator()
        .get_object_registry()
        .await
        .expect("ObjectRegistry should be available");

    // Use cluster1 namespace (node1's cluster)
    let ctx1 = node1
        .service_locator()
        .request_context_for_system_operations_with_namespace(cluster1.to_string())
        .await;

    let node2_lookup = object_registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-cluster2")
        .await
        .expect("Lookup should succeed");

    // Node2 should not be found (different namespace/cluster)
    assert!(
        node2_lookup.is_none(),
        "Node2 should not be visible from Node1's cluster"
    );

    // But Node1 should see itself
    let node1_lookup = object_registry1
        .lookup_full(&ctx1, ObjectType::ObjectTypeNode, "node-cluster1")
        .await
        .expect("Lookup should succeed")
        .expect("Node1 should see itself");

    assert_eq!(node1_lookup.object_id, "node-cluster1");
    assert_eq!(node1_lookup.namespace, cluster1);

    // Shutdown
    node1
        .shutdown(Duration::from_secs(5))
        .await
        .expect("Node1 should shutdown");
    node2
        .shutdown(Duration::from_secs(5))
        .await
        .expect("Node2 should shutdown");
}
