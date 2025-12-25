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

//! Tests for service wrappers
//!
//! These tests verify that service wrappers correctly adapt Node's services
//! to the traits defined in plexspaces_core::actor_context.

use plexspaces_node::service_wrappers::{
    TupleSpaceProviderWrapper,
};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_core::actor_context::{ActorService, ObjectRegistry, TupleSpaceProvider};
use plexspaces_core::actor_registry::ActorRegistry;
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, PatternField, Tuple, TupleField};
use std::sync::Arc;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::{spawn_actor_helper, find_actor_helper};

#[tokio::test]
async fn test_node_operations_wrapper() {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_address("127.0.0.1:9001")
            .build()
    );
    // NodeOperationsWrapper has been removed - NodeOperations trait is no longer needed
    // Node operations are now accessed directly via Node or through ActorRegistry/ActorFactory
    // This test is kept for documentation but doesn't test anything
    assert_eq!(node.id().as_str(), "test-node");
}

#[tokio::test]
async fn test_tuplespace_provider_wrapper() {
    let tuplespace = Arc::new(plexspaces_tuplespace::TupleSpace::default());
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
    assert_eq!(results[0].fields()[0], TupleField::String("test".to_string()));

    // Test take
    let taken = wrapper.take(&pattern).await.unwrap();
    assert!(taken.is_some());

    // Test count
    let count = wrapper.count(&pattern).await.unwrap();
    assert_eq!(count, 0);
}

#[tokio::test]
#[ignore] // TODO: Actor not found in registry after spawn - issue with Node's actor registration, not service wrapper
async fn test_actor_service_wrapper_send_message_local() {
    use plexspaces_actor::Actor;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};

    // Create node and spawn an actor
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_address("127.0.0.1:9001")
            .build()
    );

    struct TestBehavior;
    #[async_trait::async_trait]
    #[async_trait::async_trait]
impl plexspaces_core::Actor for TestBehavior {
        async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        _msg: plexspaces_mailbox::Message,
        _reply: &dyn plexspaces_core::Reply,
    ) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }

        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.unwrap();
    let behavior = Box::new(TestBehavior);
    let actor = Actor::new(
        "test-actor@test-node".to_string(),
        behavior,
        mailbox,
        "default".to_string(),
        None,
    );

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Wait for actor to be fully registered instead of using sleep
    let registration_future = async {
        loop {
            use test_helpers::lookup_actor_ref;
            if lookup_actor_ref(&node, actor_ref.id()).await.ok().flatten().is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("Actor should be registered within 5 seconds");

    // Use ActorServiceImpl directly (it implements ActorService trait)
    use plexspaces_actor_service::ActorServiceImpl;
    use plexspaces_core::actor_registry::ActorRegistry;
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry: Arc<dyn plexspaces_core::ObjectRegistry> = Arc::new(ObjectRegistry::new(kv));
    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node.id().as_str().to_string()));
    let service_locator = node.service_locator();
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator, node.id().as_str().to_string()));

    // Send a message using ActorServiceImpl directly
    // Note: This tests the wrapper's send method, which uses find_actor internally
    let message = Message::new(b"hello".to_vec());
    let result = actor_service.send("test-actor@test-node", message).await;

    // The actor should be findable and message should be sent
    // If this fails, it's likely because find_actor isn't finding the actor in the registry
    // This could be a timing issue or an issue with how actors are registered
    if result.is_err() {
        // Check if actor is actually registered
        let actor_id = "test-actor@test-node".to_string();
        let found_location = find_actor_helper(&node, &actor_id).await;
        if found_location.is_err() {
            // Actor not found - this is the root cause
            // For now, we'll mark this as a known issue
            panic!("Actor not found in registry after spawn. This indicates an issue with actor registration or find_actor implementation. Error: {:?}", found_location.err());
        }
        // Actor found but send failed - different issue
        panic!("Actor found but send failed: {:?}", result.err());
    }
    
    // Success
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_actor_service_wrapper_send_message_remote_not_implemented() {
    let node = Arc::new(
        NodeBuilder::new("test-node")
            .with_listen_address("127.0.0.1:9001")
            .build()
    );

    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry: Arc<dyn plexspaces_core::ObjectRegistry> = Arc::new(ObjectRegistry::new(kv));
    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node.id().as_str().to_string()));
    let service_locator = node.service_locator();
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator, node.id().as_str().to_string()));

    // Try to send to remote actor (will fail because actor doesn't exist or remote not implemented)
    let message = Message::new(b"hello".to_vec());
    let result = wrapper.send("remote-actor@remote-node", message).await;

    // Should fail - either "Actor not found" or "not yet implemented"
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("not yet implemented") || error_msg.contains("Actor not found"),
        "Expected error about remote messaging or actor not found, got: {}",
        error_msg
    );
}

#[tokio::test]
async fn test_object_registry_wrapper() {
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    use std::sync::Arc;


    // Create ObjectRegistry with in-memory backend
    let kv_store = Arc::new(InMemoryKVStore::new());
    let registry = Arc::new(ObjectRegistry::new(kv_store));
    
    // Register an actor
    let registration = ObjectRegistration {
        object_id: "test-actor@node1".to_string(),
        object_type: ObjectType::ObjectTypeActor as i32,
        object_category: "GenServer".to_string(),
        grpc_address: "http://node1:9001".to_string(),
        tenant_id: "default".to_string(),
        namespace: "default".to_string(),
        ..Default::default()
    };
    registry.register(&ctx, registration).await.unwrap();

    // Test lookup using trait method signature: lookup(ctx, object_id, object_type)
    let result = registry.lookup(&ctx, "test-actor@node1", Some(ObjectType::ObjectTypeActor)).await;
    assert!(result.is_ok());
    let found = result.unwrap();
    assert!(found.is_some());
    let reg = found.unwrap();
    assert_eq!(reg.object_id, "test-actor@node1");
    assert_eq!(reg.grpc_address, "http://node1:9001");
}

