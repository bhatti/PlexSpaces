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

//! Distributed TupleSpace Integration Tests
//!
//! ## Purpose
//! Tests TupleSpace operations across multiple nodes via gRPC. Verifies that:
//! - Nodes can discover each other via gRPC
//! - TupleSpace write/read/take operations work across nodes
//! - Pattern matching works in distributed scenarios
//! - Distributed coordination primitives (barriers, etc.) function correctly
//!
//! ## Test Strategy
//! - Spawn 2+ nodes with different listen addresses
//! - Register nodes with each other (node discovery)
//! - Test distributed TupleSpace operations via gRPC
//! - Verify location transparency (same API for local and remote)

use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use plexspaces_node::{Node, NodeId, default_node_config};
use plexspaces_proto::{tuplespace::v1::*, TuplePlexSpaceServiceClient};
use plexspaces_tuplespace::{
    Pattern, PatternField, Tuple as InternalTuple, TupleField as InternalTupleField,
};

// Type aliases for clarity
type ProtoTuple = plexspaces_proto::tuplespace::v1::Tuple;
type ProtoTupleField = plexspaces_proto::tuplespace::v1::TupleField;

/// Helper to create a test node with a specific port
fn create_test_node(node_id: &str, port: u16) -> Node {
    let mut config = default_node_config();
    config.listen_addr = format!("127.0.0.1:{}", port);

    Node::new(NodeId::new(node_id), config)
}

/// Helper to create proto tuple from integer values
fn create_proto_int_tuple(values: Vec<i64>) -> ProtoTuple {
    ProtoTuple {
        id: String::new(),
        fields: values
            .into_iter()
            .map(|v| ProtoTupleField {
                value: Some(tuple_field::Value::Integer(v)),
            })
            .collect(),
        timestamp: None,
        lease: None,
        metadata: std::collections::HashMap::new(),
        location: None,
    }
}

/// Helper to create proto pattern from values and wildcards
fn create_proto_pattern(fields: Vec<tuple_field::Value>) -> ProtoTuple {
    ProtoTuple {
        id: String::new(),
        fields: fields
            .into_iter()
            .map(|v| ProtoTupleField { value: Some(v) })
            .collect(),
        timestamp: None,
        lease: None,
        metadata: std::collections::HashMap::new(),
        location: None,
    }
}

#[tokio::test]
#[ignore] // Ignored by default - requires running nodes
async fn test_distributed_tuplespace_write_read_across_nodes() {
    // Setup: Create 2 nodes
    let node1 = Arc::new(create_test_node("node1", 9001));
    let node2 = Arc::new(create_test_node("node2", 9002));

    // Start node1 in background
    let node1_clone = node1.clone();
    let node1_handle = tokio::spawn(async move { node1_clone.start().await });

    // Start node2 in background
    let node2_clone = node2.clone();
    let node2_handle = tokio::spawn(async move { node2_clone.start().await });

    // Wait for nodes to start
    sleep(Duration::from_millis(500)).await;

    // Register node2 as remote in node1's registry
    node1
        .register_remote_node(NodeId::new("node2"), "http://127.0.0.1:9002".to_string())
        .await
        .expect("Failed to register node2");

    // Test 1: Write tuple on node1, read from node2 via gRPC
    {
        // Write tuple to node1's local TupleSpace
        let tuple = InternalTuple::new(vec![
            InternalTupleField::String("test".to_string()),
            InternalTupleField::Integer(42),
        ]);
        node1
            .tuplespace()
            .write(tuple)
            .await
            .expect("Failed to write tuple");

        // Connect to node1 via gRPC from external client
        let mut client = TuplePlexSpaceServiceClient::connect("http://127.0.0.1:9001")
            .await
            .expect("Failed to connect to node1");

        // Read tuple via gRPC
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("test".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.read(request).await.expect("Failed to read tuple");
        let tuples = response.into_inner().tuples;

        assert_eq!(tuples.len(), 1);
        assert_eq!(tuples[0].fields.len(), 2);
        if let Some(tuple_field::Value::Integer(val)) = &tuples[0].fields[1].value {
            assert_eq!(*val, 42);
        } else {
            panic!("Expected integer field");
        }
    }

    // Test 2: Write via gRPC to node2, read from node2's local TupleSpace
    {
        // Connect to node2 via gRPC
        let mut client = TuplePlexSpaceServiceClient::connect("http://127.0.0.1:9002")
            .await
            .expect("Failed to connect to node2");

        // Write tuple via gRPC
        let tuple = create_proto_int_tuple(vec![1, 2, 3]);
        let request = tonic::Request::new(WriteRequest {
            tuples: vec![tuple],
            transaction_id: String::new(),
        });

        let response = client.write(request).await.expect("Failed to write tuple");
        assert_eq!(response.into_inner().tuple_ids.len(), 1);

        // Read from node2's local TupleSpace
        let pattern = Pattern::new(vec![
            PatternField::Wildcard,
            PatternField::Wildcard,
            PatternField::Wildcard,
        ]);

        let result = node2
            .tuplespace()
            .read(pattern)
            .await
            .expect("Failed to read tuple");
        assert!(result.is_some());
    }

    // Test 3: Take operation via gRPC
    {
        // Connect to node1 via gRPC
        let mut client = TuplePlexSpaceServiceClient::connect("http://127.0.0.1:9001")
            .await
            .expect("Failed to connect to node1");

        // Take tuple via gRPC (destructive read)
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("test".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern.clone()),
            timeout: None,
            blocking: false,
            take: true,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.take(request).await.expect("Failed to take tuple");
        assert_eq!(response.into_inner().tuples.len(), 1);

        // Verify tuple removed (read should return empty)
        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 1,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.read(request).await.expect("Failed to read tuple");
        assert_eq!(response.into_inner().tuples.len(), 0);
    }

    // Cleanup: Abort background tasks
    node1_handle.abort();
    node2_handle.abort();
}

#[tokio::test]
#[ignore] // Ignored by default - requires running nodes
async fn test_distributed_tuplespace_count_and_exists() {
    // Setup: Create 2 nodes
    let node1 = Arc::new(create_test_node("node1", 9003));
    let node2 = Arc::new(create_test_node("node2", 9004));

    // Start nodes in background
    let node1_clone = node1.clone();
    let node1_handle = tokio::spawn(async move { node1_clone.start().await });

    let node2_clone = node2.clone();
    let node2_handle = tokio::spawn(async move { node2_clone.start().await });

    // Wait for nodes to start
    sleep(Duration::from_millis(500)).await;

    // Write multiple tuples to node1
    for i in 0..5 {
        let tuple = InternalTuple::new(vec![
            InternalTupleField::String("sensor".to_string()),
            InternalTupleField::Integer(i),
        ]);
        node1
            .tuplespace()
            .write(tuple)
            .await
            .expect("Failed to write tuple");
    }

    // Connect to node1 via gRPC
    let mut client = TuplePlexSpaceServiceClient::connect("http://127.0.0.1:9003")
        .await
        .expect("Failed to connect to node1");

    // Test count operation
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("sensor".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(CountRequest {
            template: Some(pattern),
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.count(request).await.expect("Failed to count tuples");
        assert_eq!(response.into_inner().count, 5);
    }

    // Test exists operation (should exist)
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("sensor".to_string()),
            tuple_field::Value::Integer(2),
        ]);

        let request = tonic::Request::new(ExistsRequest {
            template: Some(pattern),
            transaction_id: String::new(),
        });

        let response = client
            .exists(request)
            .await
            .expect("Failed to check exists");
        assert!(response.into_inner().exists);
    }

    // Test exists operation (should not exist)
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("nonexistent".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(ExistsRequest {
            template: Some(pattern),
            transaction_id: String::new(),
        });

        let response = client
            .exists(request)
            .await
            .expect("Failed to check exists");
        assert!(!response.into_inner().exists);
    }

    // Cleanup
    node1_handle.abort();
    node2_handle.abort();
}

#[tokio::test]
#[ignore] // Ignored by default - requires running nodes
async fn test_distributed_tuplespace_pattern_matching() {
    // Setup: Create node
    let node = Arc::new(create_test_node("node1", 9005));

    // Start node in background
    let node_clone = node.clone();
    let node_handle = tokio::spawn(async move { node_clone.start().await });

    // Wait for node to start
    sleep(Duration::from_millis(500)).await;

    // Write tuples with different patterns
    node.tuplespace()
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(1),
            InternalTupleField::String("login".to_string()),
        ]))
        .await
        .expect("Failed to write tuple 1");

    node.tuplespace()
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(2),
            InternalTupleField::String("logout".to_string()),
        ]))
        .await
        .expect("Failed to write tuple 2");

    node.tuplespace()
        .write(InternalTuple::new(vec![
            InternalTupleField::String("admin".to_string()),
            InternalTupleField::Integer(1),
            InternalTupleField::String("login".to_string()),
        ]))
        .await
        .expect("Failed to write tuple 3");

    // Connect via gRPC
    let mut client = TuplePlexSpaceServiceClient::connect("http://127.0.0.1:9005")
        .await
        .expect("Failed to connect to node");

    // Pattern 1: Match all "user" tuples (should find 2)
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("user".to_string()),
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 10,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.read(request).await.expect("Failed to read tuples");
        assert_eq!(response.into_inner().tuples.len(), 2);
    }

    // Pattern 2: Match exact user ID 1 (should find 1)
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::String("user".to_string()),
            tuple_field::Value::Integer(1),
            tuple_field::Value::Wildcard(true),
        ]);

        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 10,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.read(request).await.expect("Failed to read tuples");
        assert_eq!(response.into_inner().tuples.len(), 1);
    }

    // Pattern 3: Match all "login" actions (should find 2)
    {
        let pattern = create_proto_pattern(vec![
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::String("login".to_string()),
        ]);

        let request = tonic::Request::new(ReadRequest {
            template: Some(pattern),
            timeout: None,
            blocking: false,
            take: false,
            max_results: 10,
            transaction_id: String::new(),
            spatial_filter: None,
        });

        let response = client.read(request).await.expect("Failed to read tuples");
        assert_eq!(response.into_inner().tuples.len(), 2);
    }

    // Cleanup
    node_handle.abort();
}
