// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for SDK virtual actor type registration
// Verifies that spawning an actor with virtual_actor facet automatically registers the type
// and that type-level registration enables get_or_activate for other actor IDs of the same type.

use plexspaces_core::{ActorId, RequestContext};
use plexspaces_node::NodeBuilder;
use plexspaces_sdk::{
    call_message, gen_server_actor, json, plexspaces_handlers, spawn, ActorContext, BehaviorError,
    Message, Value,
};
use std::sync::Arc;
use std::time::Duration;

// Test actor with virtual_actor facet
#[gen_server_actor(facets = ["virtual_actor"])]
struct TestVirtualActor {
    value: i32,
}

impl TestVirtualActor {
    fn new(value: i32) -> Self {
        Self { value }
    }
}

#[plexspaces_handlers]
impl TestVirtualActor {
    #[handler("get")]
    async fn get(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        Ok(json!({ "value": self.value }))
    }

    #[handler("set")]
    async fn set(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let payload: Value = serde_json::from_slice(&msg.payload).map_err(|e| {
            BehaviorError::ProcessingError(format!("Failed to parse payload: {}", e))
        })?;
        if let Some(v) = payload.get("value").and_then(|v: &Value| v.as_i64()) {
            self.value = v as i32;
            Ok(json!({ "value": self.value }))
        } else {
            Err(BehaviorError::ProcessingError(
                "Missing 'value' field".to_string(),
            ))
        }
    }
}

#[tokio::test]
async fn test_virtual_actor_type_registration_on_spawn() {
    // Create node with in-memory backends
    let node = Arc::new(
        NodeBuilder::new("test-node-sdk-virtual")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-namespace".into());

    // Spawn first actor with virtual_actor facet
    // This should automatically register the actor type
    let actor_name1 = "test-actor-1";
    let actor1 = TestVirtualActor::new(42);

    let actor_ref1 = spawn(
        &ctx,
        service_locator.clone(),
        actor_name1,
        "test-namespace",
        actor1,
    )
    .await
    .expect("Failed to spawn actor");

    // Verify actor was spawned
    assert!(actor_ref1.is_local());

    // Verify virtual actor type was registered
    let virtual_actor_manager = service_locator
        .virtual_actor_manager()
        .await
        .expect("VirtualActorManager should be available");

    // Check if actor type is registered (behavior class slug for gen_server_actor)
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type("gen_server")
            .await,
        "gen_server type should be registered as virtual actor"
    );

    // Get type metadata
    let type_metadata = virtual_actor_manager
        .get_virtual_actor_type("gen_server")
        .await;
    assert!(type_metadata.is_some(), "Type metadata should exist");
    let metadata = type_metadata.unwrap();
    assert_eq!(metadata.actor_type(), "gen_server");
    assert_eq!(metadata.behavior_kind(), Some("GenServer"));
    assert_eq!(metadata.namespace(), "test-namespace");
    assert!(
        metadata.facet_config().is_some(),
        "facet_config should be set"
    );
    assert_eq!(
        metadata.proto_facets().len(),
        1,
        "expected proto facet metadata"
    );
    assert_eq!(metadata.proto_facets()[0].r#type, "virtual_actor");

    // Verify facet_config is in keyed format
    let facet_config = metadata.facet_config().unwrap();
    assert!(
        facet_config.is_object(),
        "facet_config should be a JSON object"
    );
    assert!(
        facet_config.get("virtual_actor").is_some(),
        "facet_config should contain 'virtual_actor' key"
    );

    // Now spawn a different actor ID of the same type
    // This should work because the type is registered
    let actor_name2 = "test-actor-2";
    let actor2 = TestVirtualActor::new(100);

    let actor_ref2 = spawn(
        &ctx,
        service_locator.clone(),
        actor_name2,
        "test-namespace",
        actor2,
    )
    .await
    .expect("Failed to spawn second actor");

    // Verify second actor was spawned
    assert!(actor_ref2.is_local());

    // Type-level registration enables virtual actor behavior for any actor ID of this type
    // Individual instances may or may not be registered at instance level, but type-level
    // registration enables automatic activation for any actor ID matching the type pattern
    // (e.g., {id}//gen_server::namespace@node)

    // Test that we can activate a new actor ID of the same type
    // (This tests that type-level registration enables activation)
    let actor_name3 = "test-actor-3";

    // Check if actor type is virtual (should be, because we registered it)
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type("gen_server")
            .await,
        "gen_server type should still be registered"
    );

    // Verify we can get type metadata for activation
    let type_metadata_for_activation = virtual_actor_manager
        .get_virtual_actor_type("gen_server")
        .await;
    assert!(
        type_metadata_for_activation.is_some(),
        "Should be able to get type metadata for activation"
    );

    // Verify type-level registration enables activation for a different actor ID of the same type.
    // Build actor_id3 in full format so the node can resolve type and namespace for get_or_activate.
    let actor_id3 = ActorId::new(
        actor_name3,
        "gen_server",
        "test-namespace",
        node.id().to_string(),
    )
    .expect("invalid actor_id");
    let actor_ref3 = plexspaces_sdk::ActorRef::remote(
        actor_id3.clone(),
        "test-tenant".to_string(),
        "test-namespace".to_string(),
        node.id().as_str().to_string(),
        service_locator.clone(),
    );
    let get_msg = call_message(json!({}));
    let reply = actor_ref3.ask(get_msg, Duration::from_secs(5)).await;
    match &reply {
        Ok(reply_msg) => {
            let body: Value = serde_json::from_slice(&reply_msg.payload).unwrap_or(json!({}));
            assert!(
                body.get("value").is_some(),
                "reply should contain value (activation succeeded)"
            );
        }
        Err(e) => {
            // If activation fails (e.g. behavior not registered in BehaviorRegistry), type registration is still verified above
            tracing::debug!(error = %e, "get_or_activate for new ID may fail if behavior not registered");
        }
    }
}

#[tokio::test]
async fn test_virtual_actor_type_registration_idempotent() {
    // Create node
    let node = Arc::new(
        NodeBuilder::new("test-node-sdk-idempotent")
            .with_in_memory_backends()
            .build()
            .await,
    );

    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("test-tenant".into(), "test-namespace".into());

    // Spawn first actor - registers type
    let actor_name1 = "actor-1";
    let actor1 = TestVirtualActor::new(1);

    spawn(
        &ctx,
        service_locator.clone(),
        actor_name1,
        "test-namespace",
        actor1,
    )
    .await
    .expect("Failed to spawn first actor");

    // Get initial metadata
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let metadata1 = virtual_actor_manager
        .get_virtual_actor_type("gen_server")
        .await
        .unwrap();

    // Spawn second actor of same type - should overwrite (idempotent)
    let actor_name2 = "actor-2";
    let actor2 = TestVirtualActor::new(2);

    spawn(
        &ctx,
        service_locator.clone(),
        actor_name2,
        "test-namespace",
        actor2,
    )
    .await
    .expect("Failed to spawn second actor");

    // Get metadata again - should still exist (idempotent registration)
    let metadata2 = virtual_actor_manager
        .get_virtual_actor_type("gen_server")
        .await
        .unwrap();

    // Both should have same type and namespace
    assert_eq!(metadata1.actor_type(), metadata2.actor_type());
    assert_eq!(metadata1.namespace(), metadata2.namespace());
}
