// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Distributed TupleSpace Integration Tests

use std::sync::Arc;
use std::time::Duration;

use plexspaces_actor::{ServiceLocator, ServiceLocatorBase, TupleSpaceProvider};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::{tuplespace::v1::*, TupleSpaceServiceClient};
use plexspaces_tuplespace::{Tuple as InternalTuple, TupleField as InternalTupleField};

async fn create_test_node(node_id: &str, port: u16) -> Arc<Node> {
    Arc::new(
        NodeBuilder::new(node_id)
            .with_listen_addr(&format!("127.0.0.1:{}", port))
            .with_auth_disabled()
            .build()
            .await,
    )
}

async fn wait_for_grpc(addr: &str) -> TupleSpaceServiceClient<tonic::transport::Channel> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        match TupleSpaceServiceClient::connect(addr.to_string()).await {
            Ok(client) => return client,
            Err(_) => {
                assert!(
                    tokio::time::Instant::now() < deadline,
                    "gRPC server at {} did not become ready within 5s",
                    addr
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

fn proto_tuple(fields: Vec<tuple_field::Value>) -> Tuple {
    Tuple {
        id: String::new(),
        fields: fields
            .into_iter()
            .map(|v| TupleField { value: Some(v) })
            .collect(),
        timestamp: None,
        lease: None,
        metadata: std::collections::HashMap::new(),
        location: None,
    }
}

fn read_req(pattern: Tuple, take: bool) -> ReadRequest {
    ReadRequest {
        request_id: ulid::Ulid::new().to_string(),
        template: Some(pattern),
        timeout: None,
        blocking: false,
        take,
        max_results: 10,
        transaction_id: String::new(),
        spatial_filter: None,
    }
}

#[tokio::test]
async fn test_distributed_tuplespace_write_read_across_nodes() {
    let node1 = create_test_node("node1", 8000).await;
    let node2 = create_test_node("node2", 8001).await;

    let h1 = tokio::spawn({
        let n = node1.clone();
        async move { n.start().await }
    });
    let h2 = tokio::spawn({
        let n = node2.clone();
        async move { n.start().await }
    });

    let mut client1 = wait_for_grpc("http://127.0.0.1:8000").await;
    let mut client2 = wait_for_grpc("http://127.0.0.1:8001").await;

    // Register node2 in node1's registry
    {
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
        let ctx = node1
            .service_locator()
            .request_context_for_system_operations()
            .await;
        let reg = ObjectRegistration {
            object_type: ObjectType::ObjectTypeNode as i32,
            object_id: "node2".to_string(),
            grpc_address: "http://127.0.0.1:8001".to_string(),
            object_category: "Node".to_string(),
            ..Default::default()
        };
        if let Some(registry) = node1.service_locator().get_object_registry().await {
            registry.register(&ctx, reg).await.expect("register node2");
        }
    }

    // Write to node1 locally, read back via gRPC
    {
        let ts = node1
            .service_locator()
            .get_tuplespace_provider()
            .await
            .expect("TupleSpaceProvider");
        let _: Result<(), _> = ts
            .write(InternalTuple::new(vec![
                InternalTupleField::String("test".to_string()),
                InternalTupleField::Integer(42),
            ]))
            .await;

        let pat = proto_tuple(vec![
            tuple_field::Value::String("test".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client1
            .read(tonic::Request::new(read_req(pat, false)))
            .await
            .expect("read");
        let tuples = resp.into_inner().tuples;
        assert_eq!(tuples.len(), 1);
        if let Some(tuple_field::Value::Integer(v)) = &tuples[0].fields[1].value {
            assert_eq!(*v, 42);
        } else {
            panic!("expected integer field");
        }
    }

    // Write to node2 via gRPC, read back locally
    {
        let write_tuple = proto_tuple(vec![
            tuple_field::Value::Integer(1),
            tuple_field::Value::Integer(2),
            tuple_field::Value::Integer(3),
        ]);
        let resp = client2
            .write(tonic::Request::new(WriteRequest {
                request_id: ulid::Ulid::new().to_string(),
                tuples: vec![write_tuple],
                transaction_id: String::new(),
            }))
            .await
            .expect("write");
        assert_eq!(resp.into_inner().tuple_ids.len(), 1);

        let ts = node2
            .service_locator()
            .get_tuplespace_provider()
            .await
            .expect("TupleSpaceProvider");
        let pat = plexspaces_tuplespace::Pattern::new(vec![
            plexspaces_tuplespace::PatternField::Wildcard,
            plexspaces_tuplespace::PatternField::Wildcard,
            plexspaces_tuplespace::PatternField::Wildcard,
        ]);
        let results: Vec<InternalTuple> = ts.read(&pat).await.expect("local read");
        assert!(!results.is_empty());
    }

    // Take via gRPC (destructive read)
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("test".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client1
            .take(tonic::Request::new(read_req(pat.clone(), true)))
            .await
            .expect("take");
        assert_eq!(resp.into_inner().tuples.len(), 1);

        // Verify gone
        let resp = client1
            .read(tonic::Request::new(read_req(pat, false)))
            .await
            .expect("read after take");
        assert_eq!(resp.into_inner().tuples.len(), 0);
    }

    h1.abort();
    h2.abort();
}

#[tokio::test]
async fn test_distributed_tuplespace_count_and_exists() {
    let node1 = create_test_node("node1", 8002).await;
    let node2 = create_test_node("node2", 8003).await;

    let h1 = tokio::spawn({
        let n = node1.clone();
        async move { n.start().await }
    });
    let h2 = tokio::spawn({
        let n = node2.clone();
        async move { n.start().await }
    });

    let mut client = wait_for_grpc("http://127.0.0.1:8002").await;

    let ts = node1
        .service_locator()
        .get_tuplespace_provider()
        .await
        .expect("TupleSpaceProvider");
    for i in 0..5_i64 {
        let _: Result<(), _> = ts
            .write(InternalTuple::new(vec![
                InternalTupleField::String("sensor".to_string()),
                InternalTupleField::Integer(i),
            ]))
            .await;
    }

    // Count
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("sensor".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client
            .count(tonic::Request::new(CountRequest {
                request_id: ulid::Ulid::new().to_string(),
                template: Some(pat),
                transaction_id: String::new(),
                spatial_filter: None,
            }))
            .await
            .expect("count");
        assert_eq!(resp.into_inner().count, 5);
    }

    // Exists (should find)
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("sensor".to_string()),
            tuple_field::Value::Integer(2),
        ]);
        let resp = client
            .exists(tonic::Request::new(ExistsRequest {
                request_id: ulid::Ulid::new().to_string(),
                template: Some(pat),
                transaction_id: String::new(),
            }))
            .await
            .expect("exists");
        assert!(resp.into_inner().exists);
    }

    // Exists (should not find)
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("nonexistent".to_string()),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client
            .exists(tonic::Request::new(ExistsRequest {
                request_id: ulid::Ulid::new().to_string(),
                template: Some(pat),
                transaction_id: String::new(),
            }))
            .await
            .expect("exists (absent)");
        assert!(!resp.into_inner().exists);
    }

    h1.abort();
    h2.abort();
}

#[tokio::test]
async fn test_distributed_tuplespace_pattern_matching() {
    let node = create_test_node("node1", 9005).await;

    let handle = tokio::spawn({
        let n = node.clone();
        async move { n.start().await }
    });

    let mut client = wait_for_grpc("http://127.0.0.1:9005").await;

    let ts = node
        .service_locator()
        .get_tuplespace_provider()
        .await
        .expect("TupleSpaceProvider");

    for (kind, id, action) in [
        ("user", 1i64, "login"),
        ("user", 2, "logout"),
        ("admin", 1, "login"),
    ] {
        let _: Result<(), _> = ts
            .write(InternalTuple::new(vec![
                InternalTupleField::String(kind.to_string()),
                InternalTupleField::Integer(id),
                InternalTupleField::String(action.to_string()),
            ]))
            .await;
    }

    // All "user" tuples → 2
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("user".to_string()),
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client
            .read(tonic::Request::new(read_req(pat, false)))
            .await
            .expect("read users");
        assert_eq!(resp.into_inner().tuples.len(), 2);
    }

    // Exact user id=1 → 1
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::String("user".to_string()),
            tuple_field::Value::Integer(1),
            tuple_field::Value::Wildcard(true),
        ]);
        let resp = client
            .read(tonic::Request::new(read_req(pat, false)))
            .await
            .expect("read user 1");
        assert_eq!(resp.into_inner().tuples.len(), 1);
    }

    // All "login" actions → 2
    {
        let pat = proto_tuple(vec![
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::Wildcard(true),
            tuple_field::Value::String("login".to_string()),
        ]);
        let resp = client
            .read(tonic::Request::new(read_req(pat, false)))
            .await
            .expect("read logins");
        assert_eq!(resp.into_inner().tuples.len(), 2);
    }

    handle.abort();
}
