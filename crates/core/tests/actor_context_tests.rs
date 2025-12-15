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

//! Unit tests for ActorContext and service wrappers

use plexspaces_core::{
    ActorContext, ActorService, NodeOperations, ObjectRegistry, TupleSpaceProvider,
};
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
use std::sync::Arc;

#[tokio::test]
async fn test_actor_context_minimal() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "node1".to_string(),
        "default".to_string(),
    );

    // actor_id is no longer stored in ActorContext - removed as part of envelope refactoring
    // Actors should get their ID from Envelope.target_id or Actor.id field
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.namespace, "default");
}

#[tokio::test]
#[ignore] // Integration test - should be in crates/node/tests/actor_context_integration.rs
async fn test_actor_context_new() {
    // This test should be an integration test in the node crate, not a unit test here.
    // Service wrappers are implemented and Node::create_actor_context() is available.
    // See crates/node/tests/actor_context_integration.rs for integration tests.
    // This test is kept here as a placeholder but should be removed or moved to node crate.
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper() {
    use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
    use plexspaces_tuplespace::TupleSpace;

    let tuplespace = Arc::new(TupleSpace::new());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Test write
    let tuple = Tuple::new(vec![
        TupleField::String("test".to_string()),
        TupleField::Integer(42),
    ]);
    wrapper.write(tuple.clone()).await.unwrap();

    // Test read
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("test".to_string())),
        PatternField::Wildcard,
    ]);
    let results = wrapper.read(&pattern).await.unwrap();
    assert_eq!(results.len(), 1);

    // Test count
    let count = wrapper.count(&pattern).await.unwrap();
    assert_eq!(count, 1);

    // Test take
    let taken = wrapper.take(&pattern).await.unwrap();
    assert!(taken.is_some());
}

#[tokio::test]
async fn test_stub_actor_service() {
    // StubActorService is private, test via minimal context
    use plexspaces_mailbox::Message;

    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "node1".to_string(),
        "default".to_string(),
    );
    let service = ctx.actor_service.clone();

    // All methods should return errors (stub implementation)
    let result = service
        .spawn_actor("test@node1", "GenServer", vec![])
        .await;
    assert!(result.is_err());

    let message = Message::new(vec![1, 2, 3]);
    let result = service.send("test@node1", message).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_object_registry() {
    // StubObjectRegistry is private, test via minimal context
    use plexspaces_proto::object_registry::v1::ObjectType;

    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "node1".to_string(),
        "default".to_string(),
    );
    let registry = ctx.object_registry.clone();

    // All methods should return errors (stub implementation)
    let result = registry
        .lookup("default", "test", "default", Some(ObjectType::ObjectTypeActor))
        .await;
    assert!(result.is_err());

    use plexspaces_proto::object_registry::v1::ObjectRegistration;
    let reg = ObjectRegistration::default();
    let result = registry.register(reg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_node_operations() {
    // StubNodeOperations is private, test via minimal context
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "node1".to_string(),
        "default".to_string(),
    );
    let node_ops = ctx.node.clone();

    assert_eq!(node_ops.node_id(), "node1");
    // Stub returns "stub://localhost:9001" - check it contains the port
    assert!(node_ops.node_address().contains("9001"));
}

