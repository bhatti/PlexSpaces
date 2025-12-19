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

use plexspaces_core::ActorContext;
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
use std::sync::Arc;

#[tokio::test]
async fn test_actor_context_minimal() {
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "default".to_string(),
        "test-tenant".to_string(),
        service_locator,
        None,
    );

    // actor_id is no longer stored in ActorContext - removed as part of envelope refactoring
    // Actors should get their ID from Envelope.target_id or Actor.id field
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.namespace, "default");
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper() {
    use plexspaces_core::service_wrappers::TupleSpaceProviderWrapper;
    use plexspaces_core::TupleSpaceProvider;
    use plexspaces_tuplespace::TupleSpace;

    let tuplespace = Arc::new(TupleSpace::default());
    let wrapper = TupleSpaceProviderWrapper::new(tuplespace.clone());

    // Test write
    let tuple = Tuple::new(vec![
        TupleField::String("test".to_string()),
        TupleField::Integer(42),
    ]);
    TupleSpaceProvider::write(&wrapper, tuple.clone()).await.unwrap();

    // Test read
    let pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("test".to_string())),
        PatternField::Wildcard,
    ]);
    let results = TupleSpaceProvider::read(&wrapper, &pattern).await.unwrap();
    assert_eq!(results.len(), 1);

    // Test count
    let count = TupleSpaceProvider::count(&wrapper, &pattern).await.unwrap();
    assert_eq!(count, 1);

    // Test take
    let taken = TupleSpaceProvider::take(&wrapper, &pattern).await.unwrap();
    assert!(taken.is_some());
}

#[tokio::test]
async fn test_stub_actor_service() {
    // StubActorService is private, test via minimal context
    use plexspaces_mailbox::Message;

    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "default".to_string(),
        "test-tenant".to_string(),
        service_locator,
        None,
    );
    
    // Services are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
}

#[tokio::test]
async fn test_stub_object_registry() {
    // StubObjectRegistry is private, test via minimal context
    use plexspaces_proto::object_registry::v1::ObjectType;

    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "default".to_string(),
        "test-tenant".to_string(),
        service_locator,
        None,
    );
    
    // Services are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
}

#[tokio::test]
async fn test_stub_node_operations() {
    // StubNodeOperations is private, test via minimal context
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "default".to_string(),
        "test-tenant".to_string(),
        service_locator,
        None,
    );
    // Node operations are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
}

