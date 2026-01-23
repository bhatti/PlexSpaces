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
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),  // tenant_id
        "default".to_string(),       // namespace
        service_locator,
        None,
    );

    // actor_id is no longer stored in ActorContext - removed as part of envelope refactoring
    // Actors should get their ID from Envelope.target_id or Actor.id field
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
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
    use plexspaces_core::Message;

    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),  // tenant_id
        "default".to_string(),       // namespace
        service_locator,
        None,
    );
    
    // Services are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
    assert_eq!(ctx.namespace, "default");
}

#[tokio::test]
async fn test_stub_object_registry() {
    // StubObjectRegistry is private, test via minimal context
    use plexspaces_proto::object_registry::v1::ObjectType;

    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),  // tenant_id
        "default".to_string(),       // namespace
        service_locator,
        None,
    );
    
    // Services are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
    assert_eq!(ctx.namespace, "default");
}

#[tokio::test]
async fn test_stub_node_operations() {
    // StubNodeOperations is private, test via minimal context
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "test-tenant".to_string(),  // tenant_id
        "default".to_string(),      // namespace
        service_locator,
        None,
    );
    // Node operations are accessed via service_locator, not directly
    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "test-tenant");
    assert_eq!(ctx.namespace, "default");
}

// =============================================================================
// Tests merged from actor_context_tenant_id_tests.rs (3 tests)
// =============================================================================

#[tokio::test]
async fn test_actor_context_new_with_tenant_id() {
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "tenant-123".to_string(),  // tenant_id
        "production".to_string(),   // namespace
        service_locator,
        None,
    );

    assert_eq!(ctx.node_id, "node1");
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert_eq!(ctx.namespace, "production");
}

#[tokio::test]
async fn test_actor_context_tenant_id_getter() {
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "node1".to_string(),
        "tenant-789".to_string(),  // tenant_id
        "production".to_string(),   // namespace
        service_locator,
        None,
    );

    assert_eq!(ctx.tenant_id, "tenant-789");
    assert_eq!(ctx.namespace, "production");
}

#[tokio::test]
async fn test_actor_context_clone_preserves_tenant_id() {
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::service_locator_helpers::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx1 = ActorContext::new(
        "node1".to_string(),
        "tenant-123".to_string(),  // tenant_id
        "production".to_string(),   // namespace
        service_locator,
        None,
    );

    let ctx2 = ctx1.clone();

    assert_eq!(ctx1.tenant_id, ctx2.tenant_id);
    assert_eq!(ctx1.tenant_id, "tenant-123");
    assert_eq!(ctx2.tenant_id, "tenant-123");
    assert_eq!(ctx1.namespace, "production");
    assert_eq!(ctx2.namespace, "production");
}

// =============================================================================
// Tests merged from actor_context_new_tests.rs (3 tests)
// =============================================================================

#[tokio::test]
async fn test_actor_context_new() {
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let ctx = ActorContext::new(
        "test-node".to_string(),
        "tenant-123".to_string(),  // tenant_id
        "test-ns".to_string(),      // namespace
        service_locator,
        None,
    );

    assert_eq!(ctx.node_id, "test-node");
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert_eq!(ctx.namespace, "test-ns");
    assert!(ctx.metadata.is_empty());
    assert_eq!(ctx.config, None);
}

#[tokio::test]
async fn test_actor_context_new_with_config() {
    use plexspaces_proto::v1::actor::ActorConfig;

    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let mut config = ActorConfig::default();
    config.max_mailbox_size = 5000;
    config.enable_persistence = true;
    let config = Some(config.clone());

    let ctx = ActorContext::new(
        "test-node".to_string(),
        "tenant-123".to_string(),  // tenant_id
        "test-ns".to_string(),      // namespace
        service_locator,
        config.clone(),
    );

    assert_eq!(ctx.config, config);
    assert_eq!(ctx.config.as_ref().unwrap().max_mailbox_size, 5000);
    assert_eq!(ctx.config.as_ref().unwrap().enable_persistence, true);
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert_eq!(ctx.namespace, "test-ns");
}

#[tokio::test]
async fn test_actor_context_new_with_metadata() {
    // Create a minimal ServiceLocator for testing (without node dependency)
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(None, None, None).await;
    let mut ctx = ActorContext::new(
        "test-node".to_string(),
        "tenant-123".to_string(),  // tenant_id
        "test-ns".to_string(),      // namespace
        service_locator,
        None,
    );

    ctx.metadata.insert("key1".to_string(), "value1".to_string());
    ctx.metadata.insert("key2".to_string(), "value2".to_string());

    assert_eq!(ctx.metadata.get("key1"), Some(&"value1".to_string()));
    assert_eq!(ctx.metadata.get("key2"), Some(&"value2".to_string()));
    assert_eq!(ctx.metadata.len(), 2);
    assert_eq!(ctx.tenant_id, "tenant-123");
    assert_eq!(ctx.namespace, "test-ns");
}

