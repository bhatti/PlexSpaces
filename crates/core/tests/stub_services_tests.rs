// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for stub service implementations to improve coverage

use plexspaces_core::ActorContext;
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{Pattern, Tuple, TupleField, TupleSpaceError};
use std::sync::Arc;

// Test stub services via minimal context
#[tokio::test]
async fn test_stub_channel_service() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    // All stub methods should return errors
    let msg = Message::new(vec![1, 2, 3]);
    let result = ctx.channel_service.send_to_queue("queue", msg.clone()).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("StubChannelService"));
    
    let result = ctx.channel_service.publish_to_topic("topic", msg.clone()).await;
    assert!(result.is_err());
    
    let result = ctx.channel_service.subscribe_to_topic("topic").await;
    assert!(result.is_ok()); // Returns empty stream
    
    let result = ctx.channel_service.receive_from_queue("queue", None).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_actor_service() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    let result = ctx.actor_service.spawn_actor("test@node1", "GenServer", vec![]).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("StubActorService"));
    
    let msg = Message::new(vec![1, 2, 3]);
    let result = ctx.actor_service.send("test@node1", msg.clone()).await;
    assert!(result.is_err());
    
    let result = ctx.actor_service.send("test@node1", msg.clone()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_object_registry() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    let result = ctx.object_registry.lookup("default", "test", "default", None).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("StubObjectRegistry"));
    
    use plexspaces_proto::object_registry::v1::ObjectType;
    let result = ctx.object_registry.lookup_full("tenant", "ns", ObjectType::ObjectTypeActor, "test").await;
    assert!(result.is_err());
    
    use plexspaces_core::ObjectRegistration;
    let reg = ObjectRegistration::default();
    let result = ctx.object_registry.register(reg).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_tuplespace_provider() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    let tuple = Tuple::new(vec![TupleField::String("test".to_string())]);
    let result = ctx.tuplespace.write(tuple).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        TupleSpaceError::BackendError(msg) => {
            assert!(msg.contains("StubTupleSpaceProvider"));
        },
        _ => panic!("Expected BackendError"),
    }
    
    let pattern = Pattern::new(vec![]);
    let result = ctx.tuplespace.read(&pattern).await;
    assert!(result.is_err());
    
    let result = ctx.tuplespace.take(&pattern).await;
    assert!(result.is_err());
    
    let result = ctx.tuplespace.count(&pattern).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_process_group_service() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    let result = ctx.process_group_service.join_group("group", "tenant", "ns", "actor").await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("StubProcessGroupService"));
    
    let result = ctx.process_group_service.leave_group("group", "tenant", "ns", "actor").await;
    assert!(result.is_err());
    
    let msg = Message::new(vec![1, 2, 3]);
    let result = ctx.process_group_service.publish_to_group("group", "tenant", "ns", msg).await;
    assert!(result.is_err());
    
    let result = ctx.process_group_service.get_members("group", "tenant", "ns").await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_stub_node_operations() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    assert_eq!(ctx.node.node_id(), "test-node");
    assert!(ctx.node.node_address().contains("9001"));
}

#[tokio::test]
async fn test_stub_facet_service() {
    let ctx = ActorContext::minimal(
        "test-actor".to_string(),
        "test-node".to_string(),
        "default".to_string(),
    );
    
    let result = ctx.facet_service.get_facet(&"test-actor".to_string(), "timer").await;
    assert!(result.is_err());
    // Just verify it returns an error (can't easily format the error due to trait object)
    // The error path is covered by the assertion above
}

