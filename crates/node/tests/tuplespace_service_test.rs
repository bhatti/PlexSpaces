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

//! Integration tests for gRPC TuplePlexSpaceService implementation
//!
//! ## Purpose
//! Tests the distributed TupleSpace service (Phase 3 - Distributed Coordination).
//! Verifies that TupleSpace operations work correctly via gRPC.
//!
//! ## Test Coverage
//! - Write tuples via gRPC
//! - Read tuples via gRPC (non-destructive)
//! - Take tuples via gRPC (destructive)
//! - Count matching tuples
//! - Check existence
//! - Error handling (invalid patterns, not found, etc.)

use plexspaces_node::tuplespace_service::TuplePlexSpaceServiceImpl;
use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_proto::{common::v1::Empty, tuplespace::v1::*, TuplePlexSpaceService};
use plexspaces_tuplespace::{
    Pattern, PatternField, Tuple as InternalTuple, TupleField as InternalTupleField,
};
use std::sync::Arc;
use tonic::Request;

// Type aliases for proto types (avoid conflict with internal types)
type ProtoTuple = plexspaces_proto::tuplespace::v1::Tuple;
type ProtoTupleField = plexspaces_proto::tuplespace::v1::TupleField;

/// Helper to create a tuple with integer fields
fn create_int_tuple(values: Vec<i64>) -> ProtoTuple {
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

/// Helper to create a tuple with mixed fields
fn create_mixed_tuple(int_val: i64, str_val: &str) -> ProtoTuple {
    ProtoTuple {
        id: String::new(),
        fields: vec![
            ProtoTupleField {
                value: Some(tuple_field::Value::Integer(int_val)),
            },
            ProtoTupleField {
                value: Some(tuple_field::Value::String(str_val.to_string())),
            },
        ],
        timestamp: None,
        lease: None,
        metadata: std::collections::HashMap::new(),
        location: None,
    }
}

/// Helper to create an exact pattern template
fn create_exact_pattern(values: Vec<tuple_field::Value>) -> ProtoTuple {
    ProtoTuple {
        id: String::new(),
        fields: values
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
async fn test_write_tuple_via_grpc() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-1"), NodeConfig::default()));
    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Create write request
    let tuple = create_int_tuple(vec![1, 2, 3]);
    let request = Request::new(WriteRequest {
        tuples: vec![tuple],
        transaction_id: String::new(),
    });

    // Act
    let response = service.write(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuple_ids.len(), 1);
    assert!(!resp.tuple_ids[0].is_empty());

    // Verify tuple written to TupleSpace
    let pattern = Pattern::new(vec![
        PatternField::Exact(InternalTupleField::Integer(1)),
        PatternField::Exact(InternalTupleField::Integer(2)),
        PatternField::Exact(InternalTupleField::Integer(3)),
    ]);
    let found = node.tuplespace().read(pattern).await.unwrap();
    assert!(found.is_some());
}

#[tokio::test]
async fn test_read_tuple_via_grpc() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-2"), NodeConfig::default()));

    // Write tuple directly
    let tuple = InternalTuple::new(vec![
        InternalTupleField::Integer(10),
        InternalTupleField::String("test".to_string()),
    ]);
    node.tuplespace().write(tuple).await.unwrap();

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Create read request
    let pattern = create_exact_pattern(vec![
        tuple_field::Value::Integer(10),
        tuple_field::Value::String("test".to_string()),
    ]);

    let request = Request::new(ReadRequest {
        template: Some(pattern),
        timeout: None,
        blocking: false,
        take: false,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.read(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuples.len(), 1);
    assert_eq!(resp.tuples[0].fields.len(), 2);

    // Verify non-destructive
    let verify_pattern = Pattern::new(vec![
        PatternField::Exact(InternalTupleField::Integer(10)),
        PatternField::Wildcard,
    ]);
    let still_there = node.tuplespace().read(verify_pattern).await.unwrap();
    assert!(still_there.is_some());
}

#[tokio::test]
async fn test_take_tuple_via_grpc() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-3"), NodeConfig::default()));

    let tuple = InternalTuple::new(vec![
        InternalTupleField::Integer(20),
        InternalTupleField::String("remove-me".to_string()),
    ]);
    node.tuplespace().write(tuple).await.unwrap();

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Create take request
    let pattern = create_exact_pattern(vec![
        tuple_field::Value::Integer(20),
        tuple_field::Value::Wildcard(true),
    ]);

    let request = Request::new(ReadRequest {
        template: Some(pattern),
        timeout: None,
        blocking: false,
        take: true,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.take(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuples.len(), 1);

    // Verify tuple removed
    let verify_pattern = Pattern::new(vec![
        PatternField::Exact(InternalTupleField::Integer(20)),
        PatternField::Wildcard,
    ]);
    let remaining = node.tuplespace().read(verify_pattern).await.unwrap();
    assert!(remaining.is_none());
}

#[tokio::test]
async fn test_count_tuples_via_grpc() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-4"), NodeConfig::default()));

    // Write multiple tuples
    for i in 0..5 {
        let tuple = InternalTuple::new(vec![
            InternalTupleField::String("sensor".to_string()),
            InternalTupleField::Integer(i),
        ]);
        node.tuplespace().write(tuple).await.unwrap();
    }

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Create count request
    let pattern = create_exact_pattern(vec![
        tuple_field::Value::String("sensor".to_string()),
        tuple_field::Value::Wildcard(true),
    ]);

    let request = Request::new(CountRequest {
        template: Some(pattern),
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.count(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.count, 5);
}

#[tokio::test]
async fn test_exists_tuples_via_grpc() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-5"), NodeConfig::default()));

    let tuple = InternalTuple::new(vec![
        InternalTupleField::String("config".to_string()),
        InternalTupleField::String("timeout".to_string()),
        InternalTupleField::Integer(30),
    ]);
    node.tuplespace().write(tuple).await.unwrap();

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Test 1: Exists (should be true)
    let pattern_exists = create_exact_pattern(vec![
        tuple_field::Value::String("config".to_string()),
        tuple_field::Value::String("timeout".to_string()),
        tuple_field::Value::Wildcard(true),
    ]);

    let request = Request::new(ExistsRequest {
        template: Some(pattern_exists),
        transaction_id: String::new(),
    });

    let response = service.exists(request).await;
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert!(resp.exists);

    // Test 2: Not exists (should be false)
    let pattern_not_exists = create_exact_pattern(vec![
        tuple_field::Value::String("config".to_string()),
        tuple_field::Value::String("nonexistent".to_string()),
        tuple_field::Value::Wildcard(true),
    ]);

    let request = Request::new(ExistsRequest {
        template: Some(pattern_not_exists),
        transaction_id: String::new(),
    });

    let response = service.exists(request).await;
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert!(!resp.exists);
}

#[tokio::test]
async fn test_read_with_wildcard_pattern() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-6"), NodeConfig::default()));

    // Write tuples
    node.tuplespace()
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(1),
            InternalTupleField::String("login".to_string()),
        ]))
        .await
        .unwrap();

    node.tuplespace()
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(2),
            InternalTupleField::String("logout".to_string()),
        ]))
        .await
        .unwrap();

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Pattern: ("user", ?, ?)
    let mut pattern = ProtoTuple::default();
    pattern.fields = vec![
        ProtoTupleField {
            value: Some(tuple_field::Value::String("user".to_string())),
        },
        ProtoTupleField {
            value: Some(tuple_field::Value::Wildcard(true)),
        },
        ProtoTupleField {
            value: Some(tuple_field::Value::Wildcard(true)),
        },
    ];

    let request = Request::new(ReadRequest {
        template: Some(pattern),
        timeout: None,
        blocking: false,
        take: false,
        max_results: 10,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.read(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuples.len(), 2);
}

#[tokio::test]
async fn test_read_no_match() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-7"), NodeConfig::default()));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Pattern that won't match
    let pattern = create_exact_pattern(vec![tuple_field::Value::String("nonexistent".to_string())]);

    let request = Request::new(ReadRequest {
        template: Some(pattern),
        timeout: None,
        blocking: false,
        take: false,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.read(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuples.len(), 0);
    assert!(!resp.has_more);
}

#[tokio::test]
async fn test_write_multiple_tuples() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-8"), NodeConfig::default()));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Create batch
    let tuples = vec![
        create_int_tuple(vec![1, 2, 3]),
        create_int_tuple(vec![4, 5, 6]),
        create_mixed_tuple(10, "test"),
    ];

    let request = Request::new(WriteRequest {
        tuples,
        transaction_id: String::new(),
    });

    // Act
    let response = service.write(request).await;

    // Assert
    assert!(response.is_ok());
    let resp = response.unwrap().into_inner();
    assert_eq!(resp.tuple_ids.len(), 3);
    for id in &resp.tuple_ids {
        assert!(!id.is_empty());
    }
}

#[tokio::test]
async fn test_write_with_missing_template() {
    // Setup
    let node = Arc::new(Node::new(NodeId::new("test-node-9"), NodeConfig::default()));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Request with no template
    let request = Request::new(ReadRequest {
        template: None,
        timeout: None,
        blocking: false,
        take: false,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.read(request).await;

    // Assert
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("template") || err.message().contains("required"));
}

#[tokio::test]
async fn test_take_with_missing_template() {
    // Setup
    let node = Arc::new(Node::new(
        NodeId::new("test-node-10a"),
        NodeConfig::default(),
    ));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Request with no template
    let request = Request::new(ReadRequest {
        template: None,
        timeout: None,
        blocking: false,
        take: true,
        max_results: 1,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.take(request).await;

    // Assert
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("template") || err.message().contains("required"));
}

#[tokio::test]
async fn test_count_with_missing_template() {
    // Setup
    let node = Arc::new(Node::new(
        NodeId::new("test-node-10b"),
        NodeConfig::default(),
    ));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Request with no template
    let request = Request::new(CountRequest {
        template: None,
        transaction_id: String::new(),
        spatial_filter: None,
    });

    // Act
    let response = service.count(request).await;

    // Assert
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("template") || err.message().contains("required"));
}

#[tokio::test]
async fn test_exists_with_missing_template() {
    // Setup
    let node = Arc::new(Node::new(
        NodeId::new("test-node-10c"),
        NodeConfig::default(),
    ));

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Request with no template
    let request = Request::new(ExistsRequest {
        template: None,
        transaction_id: String::new(),
    });

    // Act
    let response = service.exists(request).await;

    // Assert
    assert!(response.is_err());
    let err = response.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("template") || err.message().contains("required"));
}

#[tokio::test]
async fn test_clear_tuplespace() {
    // Setup
    let node = Arc::new(Node::new(
        NodeId::new("test-node-10"),
        NodeConfig::default(),
    ));

    // Write tuples
    for i in 0..3 {
        node.tuplespace()
            .write(InternalTuple::new(vec![InternalTupleField::Integer(i)]))
            .await
            .unwrap();
    }

    let service = TuplePlexSpaceServiceImpl::new(node.clone());

    // Act
    let request = Request::new(Empty {});
    let response = service.clear(request).await;

    // Assert
    assert!(response.is_ok());

    // Verify all tuples removed
    let pattern = Pattern::new(vec![PatternField::Wildcard]);
    let remaining = node.tuplespace().read(pattern).await.unwrap();
    assert!(remaining.is_none());
}


/*
 * TODO: Barrier synchronization tests removed - barrier() gRPC RPC was intentionally removed
 *
 * These tests need to be reimplemented using TupleSpace primitives (write + count + watch)
 * instead of the removed barrier() gRPC RPC.
 *
 * See docs/BARRIER_RPC_REMOVAL.md for migration guide and rationale.
 *
 * The LOCAL TupleSpace already has barrier() helper at crates/tuplespace/src/mod.rs:812
 * that uses primitives internally.
 *
 * Tests that were commented out:
 * - test_barrier_wait_all_arrive()
 * - test_barrier_timeout()
 * - test_multi_barrier()
 * - test_barrier_validation()
 * - test_concurrent_barriers_same_name()
 */
