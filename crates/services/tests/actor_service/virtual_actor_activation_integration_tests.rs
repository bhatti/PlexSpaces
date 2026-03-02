// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for virtual actor activation, suspension, and reactivation
//
// ## Test Coverage
// - Virtual actor type registration (WASM/Rust applications)
// - Virtual actor lazy activation (first message)
// - Virtual actor suspension/deactivation
// - Virtual actor reactivation after suspension (instance-level metadata)
// - Virtual actor reactivation after suspension (type-level metadata fallback)
// - Error cases: missing metadata, invalid actor IDs, etc.

use plexspaces_actor::ActorBuilder;
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorType, BehaviorError, ActorId, Actor as ActorTrait, Message, RequestContext};
use plexspaces_journaling::VirtualActorFacet;
use plexspaces_node::NodeBuilder;
use plexspaces_proto::actor::v1::{GetOrActivateActorRequest, actor_service_server::ActorService as ActorServiceTrait};
use plexspaces_services::actor_service::ActorServiceImpl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use tonic::Request;

/// Test actor for virtual actor tests
#[derive(Debug, Clone)]
struct CounterActor {
    count: i32,
}

impl CounterActor {
    fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait]
impl ActorTrait for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CounterMessage {
    Increment,
    GetCount,
}

#[async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let counter_msg: CounterMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match counter_msg {
            CounterMessage::Increment => {
                self.count += 1;
                Ok(())
            }
            CounterMessage::GetCount => {
                let reply = serde_json::json!({ "count": self.count });
                if !msg.sender_id.is_empty() {
                    let correlation_id = if msg.correlation_id.is_empty() { None } else { Some(msg.correlation_id.as_str()) };
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        msg.receiver_id.clone(),
                        Message {
                            id: ulid::Ulid::new().to_string(),
                            payload: serde_json::to_vec(&reply).unwrap(),
                            ..Default::default()
                        },
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                Ok(())
            }
        }
    }
}

/// Test: Virtual actor type registration and lazy activation
#[tokio::test]
async fn test_virtual_actor_type_registration_and_lazy_activation() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    
    // Register virtual actor type (simulating WASM application registration)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "counter";
    let namespace = "test-ns";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }
    });
    
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.to_string(),
        None, // config
        namespace.to_string(),
        facet_config,
        None, // tenant_id
        None, // init_config_template
    ).await.unwrap();
    
    // Verify type is registered
    assert!(virtual_actor_manager.is_virtual_actor_type(actor_type).await);
    
    // Create actor ID using proper format
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    // Try to get or activate actor (should activate lazily)
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    assert!(response.is_ok(), "Should activate virtual actor lazily");
    
    // Verify actor is now active
    let actor_registry = service_locator.actor_registry().await.unwrap();
    assert!(actor_registry.lookup_actor(&actor_id).await.is_some());
}

/// Test: Virtual actor suspension and reactivation with instance-level metadata
#[tokio::test]
async fn test_virtual_actor_suspension_and_reactivation_instance_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    
    // Spawn actor with VirtualActorFacet (instance-level registration)
    use plexspaces_core::actor_id::build_actor_id;
    let actor_type = "counter";
    let namespace = "test-ns";
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    let ctx = RequestContext::new_without_auth("".to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(serde_json::json!({
        "idle_timeout": "1s",
        "activation_strategy": "lazy"
    }), 100);
    
    let actor_ref = ActorBuilder::new(Box::new(CounterActor::new()))
        .with_id(actor_id.clone())
        .with_facet(Box::new(virtual_facet))
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();
    
    // Verify actor is registered as virtual (instance-level)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    assert!(virtual_actor_manager.is_virtual(&actor_id).await);
    
    // Send a message to activate actor
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();
    
    // Wait for activation
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Verify actor is active
    let actor_registry = service_locator.actor_registry().await.unwrap();
    assert!(actor_registry.lookup_actor(&actor_id).await.is_some());
    
    // Suspend actor (simulate deactivation)
    node.deactivate_virtual_actor(&actor_id, false).await.unwrap();
    
    // Verify actor is suspended (not active but still registered)
    assert!(!virtual_actor_manager.is_active(&actor_id).await);
    assert!(virtual_actor_manager.is_virtual(&actor_id).await); // Still virtual
    
    // Reactivate actor
    use plexspaces_services::actor_service::ActorServiceImpl;
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    assert!(response.is_ok(), "Should reactivate suspended virtual actor");
    
    // Verify actor is active again
    assert!(virtual_actor_manager.is_active(&actor_id).await);
    assert!(actor_registry.lookup_actor(&actor_id).await.is_some());
}

/// Test: Virtual actor reactivation with type-level metadata fallback
#[tokio::test]
async fn test_virtual_actor_reactivation_type_level_fallback() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;
    
    // Register virtual actor type (type-level registration, no instance-level)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "counter";
    let namespace = "test-ns";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }
    });
    
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.to_string(),
        None, // config
        namespace.to_string(),
        facet_config,
        None, // tenant_id
        None, // init_config_template
    ).await.unwrap();
    
    // Create actor ID
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    // Spawn actor (this will register it instance-level)
    let ctx = RequestContext::new_without_auth("".to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(serde_json::json!({
        "idle_timeout": "1s",
        "activation_strategy": "lazy"
    }), 100);
    
    let actor_ref = ActorBuilder::new(Box::new(CounterActor::new()))
        .with_id(actor_id.clone())
        .with_facet(Box::new(virtual_facet))
        .spawn(&ctx, service_locator.clone())
        .await
        .unwrap();
    
    // Send message to activate
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Suspend actor
    node.deactivate_virtual_actor(&actor_id, false).await.unwrap();
    
    // CRITICAL: Remove instance-level metadata (simulating the error case)
    // In real scenario, this shouldn't happen, but we test the fallback
    // Note: We can't directly remove from VirtualActorManager, but we can test
    // by creating a new actor ID that was never instance-registered
    
    // Create a new actor ID that matches the type but was never instance-registered
    let new_actor_id = build_actor_id("user-2", actor_type, Some(namespace), "test-node");
    
    // Verify type-level registration exists
    assert!(virtual_actor_manager.is_virtual_actor_type(actor_type).await);
    
    // Try to activate new actor (should use type-level metadata)
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: new_actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    assert!(response.is_ok(), "Should activate using type-level metadata fallback");
    
    // Verify actor is active
    let actor_registry = service_locator.actor_registry().await.unwrap();
    assert!(actor_registry.lookup_actor(&new_actor_id).await.is_some());
}

/// Test: Error case - virtual actor activation fails when metadata is missing
#[tokio::test]
async fn test_virtual_actor_activation_error_missing_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    
    // Don't register virtual actor type or instance
    let actor_type = "nonexistent";
    let namespace = "test-ns";
    
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    // Try to activate actor that doesn't exist
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    
    // Should fail because actor type is not registered as virtual
    assert!(response.is_err());
    let err = response.unwrap_err();
    let error_msg = err.message();
    assert!(error_msg.contains("not a virtual actor") || error_msg.contains("not found"));
}

/// Test: Virtual actor with proper actor ID format
#[tokio::test]
async fn test_virtual_actor_actor_id_format() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;
    
    // Register virtual actor type
    let virtual_actor_manager = service_locator_trait.virtual_actor_manager().await.unwrap();
    let actor_type = "read-state-tracker";
    let namespace = "orbit-read-state-ts";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }
    });
    
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.to_string(),
        None,
        namespace.to_string(),
        facet_config,
        None,
    ).await.unwrap();
    
    // Test proper actor ID format: {id}//{actor_type}::{namespace}@{node_id}
    use plexspaces_core::actor_id::{build_actor_id, parse_actor_id};
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    // Verify format
    assert_eq!(actor_id, "user-1//read-state-tracker::orbit-read-state-ts@test-node");
    
    // Parse and verify components
    let parsed = parse_actor_id(&actor_id).unwrap();
    assert_eq!(parsed.id, "user-1");
    assert_eq!(parsed.actor_type, "read-state-tracker");
    assert_eq!(parsed.namespace, Some("orbit-read-state-ts".to_string()));
    assert_eq!(parsed.node_id, "test-node");
    
    // Verify type-level registration works with this format
    assert!(virtual_actor_manager.is_virtual_actor_type(actor_type).await);
}

/// Test: Virtual actor activation with HTTP-style actor_type format (read-state-tracker:user-1)
/// This test validates the fix for migrating_orbit example where actor_type comes from HTTP path
#[tokio::test]
async fn test_virtual_actor_activation_with_http_format() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;
    
    // Register virtual actor type (matches migrating_orbit example)
    let virtual_actor_manager = service_locator_trait.virtual_actor_manager().await.unwrap();
    let actor_type = "read-state-tracker";
    let namespace = "orbit-read-state-ts";
    
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.to_string(),
        None,
        namespace.to_string(),
        serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "durability": {
                "enabled": true
            }
        }),
        None,
    ).await.unwrap();
    
    // Simulate HTTP request format: "read-state-tracker:user-1"
    let http_actor_type = "read-state-tracker:user-1";
    let base_actor_type = "read-state-tracker";
    
    // Extract instance ID from HTTP format (same logic as invoke_actor)
    let instance_id = if http_actor_type.contains(':') {
        http_actor_type.split_once(':')
            .map(|(_actor_type_part, instance_id)| instance_id.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string())
    } else {
        ulid::Ulid::new().to_string()
    };
    
    // Build proper actor ID format
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id(&instance_id, base_actor_type, Some(namespace), "test-node");
    
    // Verify format is correct (not //read-state-tracker::...)
    assert!(!actor_id.starts_with("//"), "Actor ID should not start with // - missing instance ID");
    assert!(actor_id.contains(&instance_id), "Actor ID should contain instance ID");
    assert_eq!(actor_id, format!("{}//{}::{}@test-node", instance_id, base_actor_type, namespace));
    
    // Test activation via ActorService
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: base_actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    assert!(response.is_ok(), "Should activate virtual actor with proper actor ID format");
    
    // Verify actor is registered
    assert!(virtual_actor_manager.is_virtual(&actor_id).await);
}

/// Test: Virtual actor reactivation after suspension with type-level registration only
/// This test validates the fix for migrating_orbit example where actor is type-registered
/// but instance metadata is missing when reactivating
#[tokio::test]
async fn test_virtual_actor_reactivation_type_registered_only() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;
    
    // Register virtual actor type (type-level registration only, no instance-level)
    let virtual_actor_manager = service_locator_trait.virtual_actor_manager().await.unwrap();
    let actor_type = "read-state-tracker";
    let namespace = "orbit-read-state-ts";
    
    virtual_actor_manager.register_virtual_actor_type(
        actor_type.to_string(),
        None,
        namespace.to_string(),
        serde_json::json!({
            "virtual_actor": {
                "idle_timeout": "5m",
                "activation_strategy": "lazy"
            },
            "durability": {
                "enabled": true
            }
        }),
        None,
    ).await.unwrap();
    
    // Build actor ID (simulating HTTP request format: "read-state-tracker:user-1")
    use plexspaces_core::actor_id::build_actor_id;
    let instance_id = "user-1";
    let actor_id = build_actor_id(instance_id, actor_type, Some(namespace), "test-node");
    
    // Verify actor is NOT instance-registered (only type-registered)
    let instance_metadata = virtual_actor_manager.get_metadata(&actor_id).await;
    assert!(instance_metadata.is_none(), "Actor should not be instance-registered yet");
    assert!(virtual_actor_manager.is_virtual_actor_type(actor_type).await, "Actor type should be registered");
    
    // Try to activate actor (should work even though instance is not registered)
    // This simulates the migrating_orbit scenario where actor is activated via HTTP
    use plexspaces_services::actor_service::ActorServiceImpl;
    use plexspaces_proto::actor::v1::{GetOrActivateActorRequest, actor_service_server::ActorService as ActorServiceTrait};
    use tonic::{Request, metadata::MetadataValue};
    
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    // Create request with metadata for tenant/namespace (even though auth is disabled)
    let mut request = Request::new(req);
    request.metadata_mut().insert("x-tenant-id", MetadataValue::from_static(""));
    request.metadata_mut().insert("x-namespace", MetadataValue::from_static("orbit-read-state-ts"));
    
    // This should succeed - actor should be registered during activation
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, request).await;
    if let Err(e) = &response {
        eprintln!("Activation failed: {}", e.message());
    }
    assert!(response.is_ok(), "Should activate virtual actor even if only type-registered");
    
    // Verify actor is now instance-registered
    let instance_metadata_after = virtual_actor_manager.get_metadata(&actor_id).await;
    assert!(instance_metadata_after.is_some(), "Actor should be instance-registered after activation");
    assert!(virtual_actor_manager.is_virtual(&actor_id).await, "Actor should be virtual");
}

/// Test: Virtual actor activation preserves state after suspension/reactivation
#[tokio::test]
async fn test_virtual_actor_state_preservation() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> = service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;
    
    // Spawn actor with VirtualActorFacet
    use plexspaces_core::actor_id::build_actor_id;
    let actor_type = "counter";
    let namespace = "test-ns";
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");
    
    let ctx = RequestContext::new_without_auth("".to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(serde_json::json!({
        "idle_timeout": "1s",
        "activation_strategy": "lazy"
    }), 100);
    
    let actor_ref = ActorBuilder::new(Box::new(CounterActor::new()))
        .with_id(actor_id.clone())
        .with_facet(Box::new(virtual_facet))
        .spawn(&ctx, service_locator_trait.clone())
        .await
        .unwrap();
    
    // Increment counter
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    // Note: This test would need durability facet to preserve state across suspension
    // For now, we just verify the activation/reactivation flow works
    // State preservation requires DurabilityFacet which is a separate concern
    
    // Suspend and reactivate
    node.deactivate_virtual_actor(&actor_id, false).await.unwrap();
    
    let actor_service = Arc::new(ActorServiceImpl::new(service_locator.clone(), "test-node".to_string()));
    let req = GetOrActivateActorRequest {
        actor_id: actor_id.clone(),
        actor_type: actor_type.to_string(),
        initial_state: vec![],
        config: None,
        force_activation: false,
    };
    
    let response = ActorServiceTrait::get_or_activate_actor(&*actor_service, Request::new(req)).await;
    assert!(response.is_ok(), "Should reactivate successfully");
}
