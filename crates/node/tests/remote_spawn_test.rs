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

//! Remote Actor Spawning Tests
//!
//! Tests for Erlang-style spawn/4 equivalent remote actor spawning.

use std::sync::Arc;
use tokio::time::{sleep, Duration};

use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_proto::{
    v1::actor::{ActorConfig as ProtoActorConfig, SpawnActorRequest},
    ActorService, ActorServiceServer,
};
use tonic::Request;

/// Helper to create a test node
fn create_test_node(id: &str, port: u16) -> Node {
    let mut config = NodeConfig::default();
    config.listen_addr = format!("127.0.0.1:{}", port);
    config.heartbeat_interval_ms = 100;

    Node::new(NodeId::new(id), config)
}

#[tokio::test]
async fn test_spawn_actor_basic() {
    // Create a node
    let node = Arc::new(create_test_node("test-node", 9501));

    // Create gRPC service
    let service = plexspaces_node::grpc_service::ActorServiceImpl::new(node.clone());

    // Create SpawnActorRequest (target node is implicit from gRPC endpoint)
    let request = Request::new(SpawnActorRequest {
        actor_id: String::new(), // Server will generate
        actor_type: "test_actor".to_string(),
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });

    // Spawn actor via gRPC
    let response = service.spawn_actor(request).await;

    // Should succeed
    assert!(response.is_ok(), "spawn_actor should succeed");

    let resp = response.unwrap().into_inner();

    // Should return actor_ref in format "actor_id@node_id"
    assert!(
        resp.actor_ref.contains("@test-node"),
        "actor_ref should contain @node_id"
    );

    // Should return actor details
    assert!(resp.actor.is_some(), "actor details should be present");
    let actor = resp.actor.unwrap();
    assert_eq!(actor.actor_type, "test_actor");
    assert_eq!(actor.node_id, "test-node");
}

#[tokio::test]
async fn test_spawn_remote_actor_missing_target_node() {
    let node = Arc::new(create_test_node("test-node", 9502));
    let service = plexspaces_node::grpc_service::ActorServiceImpl::new(node.clone());

    // Missing actor_type (should fail)
    let request = Request::new(SpawnActorRequest {
        actor_id: String::new(),
        actor_type: "".to_string(), // Empty actor_type should fail
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });

    let response = service.spawn_actor(request).await;

    // Should fail with invalid_argument
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Missing actor_type") || err.message().contains("actor_type"));
}

#[tokio::test]
async fn test_spawn_remote_actor_missing_actor_type() {
    let node = Arc::new(create_test_node("test-node", 9503));
    let service = plexspaces_node::grpc_service::ActorServiceImpl::new(node.clone());

    // Missing actor_type
    let request = Request::new(SpawnActorRequest {
        actor_id: String::new(),
        actor_type: "".to_string(),
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });

    let response = service.spawn_actor(request).await;

    // Should fail with invalid_argument
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("Missing actor_type"));
}

#[tokio::test]
async fn test_spawn_remote_actor_wrong_node() {
    let node = Arc::new(create_test_node("node1", 9504));
    let service = plexspaces_node::grpc_service::ActorServiceImpl::new(node.clone());

    // gRPC spawn_actor always spawns on the node receiving the request
    // The test for "wrong node" doesn't make sense anymore since target is implicit
    // This test is now redundant - spawn_actor always succeeds on the receiving node
    let request = Request::new(SpawnActorRequest {
        actor_id: String::new(),
        actor_type: "test_actor".to_string(),
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });

    let response = service.spawn_actor(request).await;

    // Should succeed - spawns on node1 (the node receiving the request)
    assert!(response.is_ok(), "spawn_actor should succeed on receiving node");
}

#[tokio::test]
async fn test_spawn_multiple_remote_actors() {
    let node = Arc::new(create_test_node("test-node", 9505));
    let service = plexspaces_node::grpc_service::ActorServiceImpl::new(node.clone());

    // Spawn 3 actors
    for i in 0..3 {
        let request = Request::new(SpawnActorRequest {
            actor_id: String::new(),
            actor_type: format!("test_actor_{}", i),
            initial_state: vec![],
            config: None,
            labels: std::collections::HashMap::new(),
        });

        let response = service.spawn_actor(request).await;
        assert!(response.is_ok(), "spawn {} should succeed", i);

        let resp = response.unwrap().into_inner();
        assert!(resp.actor_ref.contains("@test-node"));
    }

    // All 3 should be registered with node
    // (We'd need node.list_actors() to verify, but testing spawn succeeded is enough)
}

#[tokio::test]
#[ignore] // Ignored by default - requires running gRPC servers
async fn test_spawn_remote_actor_via_grpc() {
    // Start node with gRPC server
    let node = Arc::new(create_test_node("node1", 9601));

    // Start gRPC server in background
    let node_clone = node.clone();
    let server_handle = tokio::spawn(async move { node_clone.start().await });

    // Wait for server to start
    sleep(Duration::from_millis(500)).await;

    // Connect gRPC client
    use plexspaces_proto::ActorServiceClient;
    let mut client = ActorServiceClient::connect("http://127.0.0.1:9601")
        .await
        .expect("should connect");

    // Spawn actor via gRPC using RemoteActorClient
    use plexspaces_node::grpc_client::RemoteActorClient;
    let mut remote_client = RemoteActorClient::connect("http://127.0.0.1:9601").await.expect("should connect");
    let response = remote_client.spawn_actor("node1", "remote_test_actor", Some(vec![1, 2, 3, 4]), None, std::collections::HashMap::new()).await;
    assert!(response.is_ok(), "gRPC spawn should succeed");

    let actor_ref = response.unwrap();
    assert!(actor_ref.id().as_str().contains("@node1"));

    // Cleanup
    server_handle.abort();
}
