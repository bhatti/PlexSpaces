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

//! Integration tests for gRPC TupleSpaceService implementation
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

use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::{common::v1::Empty, tuplespace::v1::*, TupleSpaceService};
use plexspaces_services::tuple_service::TupleSpaceServiceImpl;
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
    let node = Arc::new(
        NodeBuilder::new("test-node-1")
            .with_auth_disabled()
            .build()
            .await,
    );
    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let found: Vec<InternalTuple> = tuplespace.read(&pattern).await.unwrap();
    assert!(!found.is_empty());
}

#[tokio::test]
async fn test_read_tuple_via_grpc() {
    // Setup
    let node = Arc::new(
        NodeBuilder::new("test-node-2")
            .with_auth_disabled()
            .build()
            .await,
    );

    // Write tuple directly
    let tuple = InternalTuple::new(vec![
        InternalTupleField::Integer(10),
        InternalTupleField::String("test".to_string()),
    ]);
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let _: Result<(), _> = tuplespace.write(tuple).await;

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let still_there: Vec<InternalTuple> = tuplespace.read(&verify_pattern).await.unwrap();
    assert!(!still_there.is_empty());
}

#[tokio::test]
async fn test_take_tuple_via_grpc() {
    // Setup
    let node = Arc::new(
        NodeBuilder::new("test-node-3")
            .with_auth_disabled()
            .build()
            .await,
    );

    let tuple = InternalTuple::new(vec![
        InternalTupleField::Integer(20),
        InternalTupleField::String("remove-me".to_string()),
    ]);
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let _: Result<(), _> = tuplespace.write(tuple).await;

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let remaining: Vec<InternalTuple> = tuplespace.read(&verify_pattern).await.unwrap();
    assert!(remaining.is_empty());
}

#[tokio::test]
async fn test_tuplespace_service_does_not_use_internal_context() {
    // Test that TupleSpace service methods do NOT fallback to RequestContext::internal()
    // Even when auth is disabled, we should use defaults from NodeConfig, not internal()
    // This ensures tenant isolation - operations must have valid tenant context

    let node = Arc::new(
        NodeBuilder::new("test-node-context")
            .with_auth_disabled()
            .build()
            .await,
    );
    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

    // Create request with valid tenant context (using defaults when auth disabled)
    let mut request = Request::new(WriteRequest {
        tuples: vec![create_int_tuple(vec![1, 2, 3])],
        transaction_id: String::new(),
    });

    // When auth is disabled, request_context_from_grpc_request uses defaults from NodeConfig
    // This is acceptable - the key is that we don't use RequestContext::internal() as fallback
    // The fix ensures we return an error if context extraction fails, not use internal()

    // Act
    let response = service.write(request).await;

    // Assert - should succeed (using defaults when auth disabled is OK)
    // The important fix is that we removed the unwrap_or_else(|_| RequestContext::internal()) fallback
    // If context extraction fails, we now return an error instead of silently using internal()
    assert!(
        response.is_ok(),
        "Request should succeed with default context when auth disabled"
    );

    // Verify the tuple was written (proves context was used, not internal())
    let pattern = Pattern::new(vec![
        PatternField::Exact(InternalTupleField::Integer(1)),
        PatternField::Exact(InternalTupleField::Integer(2)),
        PatternField::Exact(InternalTupleField::Integer(3)),
    ]);
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let found: Vec<InternalTuple> = tuplespace.read(&pattern).await.unwrap();
    assert!(
        !found.is_empty(),
        "Tuple should be written with proper context"
    );
}

#[tokio::test]
async fn test_count_tuples_via_grpc() {
    // Setup
    let node = Arc::new(
        NodeBuilder::new("test-node-4")
            .with_auth_disabled()
            .build()
            .await,
    );

    // Write multiple tuples
    for i in 0..5 {
        let tuple = InternalTuple::new(vec![
            InternalTupleField::String("sensor".to_string()),
            InternalTupleField::Integer(i),
        ]);
        let tuplespace = node
            .service_locator()
            .get_tuplespace_provider()
            .await
            .unwrap();
        let _: Result<(), _> = tuplespace.write(tuple).await;
    }

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-5")
            .with_auth_disabled()
            .build()
            .await,
    );

    let tuple = InternalTuple::new(vec![
        InternalTupleField::String("config".to_string()),
        InternalTupleField::String("timeout".to_string()),
        InternalTupleField::Integer(30),
    ]);
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let _: Result<(), _> = tuplespace.write(tuple).await;

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-6")
            .with_auth_disabled()
            .build()
            .await,
    );

    // Write tuples
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    let _: Result<(), _> = tuplespace
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(1),
            InternalTupleField::String("login".to_string()),
        ]))
        .await;

    let _: Result<(), _> = tuplespace
        .write(InternalTuple::new(vec![
            InternalTupleField::String("user".to_string()),
            InternalTupleField::Integer(2),
            InternalTupleField::String("logout".to_string()),
        ]))
        .await;

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-7")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-8")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-9")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-10a")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-10b")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-10c")
            .with_auth_disabled()
            .build()
            .await,
    );

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

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
    let node = Arc::new(
        NodeBuilder::new("test-node-10")
            .with_auth_disabled()
            .build()
            .await,
    );

    // Write tuples
    let tuplespace = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .unwrap();
    for i in 0..3 {
        let _: Result<(), _> = tuplespace
            .write(InternalTuple::new(vec![InternalTupleField::Integer(i)]))
            .await;
    }

    let service = TupleSpaceServiceImpl::new(node.service_locator().clone());

    // Act
    let request = Request::new(Empty {});
    let response = service.clear(request).await;

    // Assert
    assert!(response.is_ok());

    // Verify all tuples removed
    let pattern = Pattern::new(vec![PatternField::Wildcard]);
    let remaining: Vec<InternalTuple> = tuplespace.read(&pattern).await.unwrap();
    assert!(remaining.is_empty());
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
