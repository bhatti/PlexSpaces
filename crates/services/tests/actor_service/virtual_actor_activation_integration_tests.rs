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

use async_trait::async_trait;
use plexspaces_behavior::GenServer;
use plexspaces_common::ActivationStrategy;
use plexspaces_core::behavior_factory::{BehaviorFactoryError, BehaviorRegistry};
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, Message, RequestContext,
    ServiceLocator,
};
use plexspaces_journaling::{ReminderFacet, TimerFacet, VirtualActorFacet};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, InvokeActorRequest,
};
use plexspaces_services::actor_service::ActorServiceImpl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
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

    fn new_with_count(count: i32) -> Self {
        Self { count }
    }
}

#[derive(Debug, Clone)]
struct WorkflowProbeActor {
    count: i32,
}

impl WorkflowProbeActor {
    fn new_with_count(count: i32) -> Self {
        Self { count }
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

#[async_trait]
impl ActorTrait for WorkflowProbeActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
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
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        msg.receiver_id.clone(),
                        Message {
                            id: ulid::Ulid::new().to_string(),
                            payload: serde_json::to_vec(&reply).unwrap(),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
                Ok(())
            }
        }
    }
}

#[async_trait]
impl GenServer for WorkflowProbeActor {
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
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        msg.receiver_id.clone(),
                        Message {
                            id: ulid::Ulid::new().to_string(),
                            payload: serde_json::to_vec(&reply).unwrap(),
                            ..Default::default()
                        },
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
                Ok(())
            }
        }
    }
}

async fn ask_for_count(
    actor_ref: &(dyn plexspaces_core::MessageSender + Send + Sync),
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let reply = actor_ref
        .ask(
            Message {
                id: ulid::Ulid::new().to_string(),
                payload: serde_json::to_vec(&CounterMessage::GetCount)?,
                message_type: "call".to_string(),
                ..Default::default()
            },
            Duration::from_secs(5),
        )
        .await?;
    let payload: serde_json::Value = serde_json::from_slice(&reply.payload)?;
    Ok(payload["count"].as_i64().unwrap_or_default() as i32)
}

async fn register_counter_behavior_with_initial_count(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    let registry = BehaviorRegistry::new();
    let module_name = actor_type.to_string();
    registry
        .register(actor_type.to_string(), move |args| {
            let args = args.to_vec();
            let module_name = module_name.clone();
            Box::pin(async move {
                let config = if args.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_slice(&args).map_err(|e| {
                        BehaviorFactoryError::InvalidArguments(
                            module_name.clone(),
                            format!("invalid JSON config: {}", e),
                        )
                    })?
                };
                let initial_count = config
                    .get("initial_count")
                    .and_then(|value| value.as_i64())
                    .unwrap_or_default() as i32;
                Ok(Box::new(CounterActor::new_with_count(initial_count)) as Box<dyn ActorTrait>)
            })
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(registry))
        .await;
}

async fn register_workflow_probe_behavior(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    let registry = BehaviorRegistry::new();
    let module_name = actor_type.to_string();
    registry
        .register(actor_type.to_string(), move |args| {
            let args = args.to_vec();
            let module_name = module_name.clone();
            Box::pin(async move {
                let config = if args.is_empty() {
                    serde_json::json!({})
                } else {
                    serde_json::from_slice(&args).map_err(|e| {
                        BehaviorFactoryError::InvalidArguments(
                            module_name.clone(),
                            format!("invalid JSON config: {}", e),
                        )
                    })?
                };
                let initial_count = config
                    .get("initial_count")
                    .and_then(|value| value.as_i64())
                    .unwrap_or_default() as i32;
                Ok(Box::new(WorkflowProbeActor::new_with_count(initial_count))
                    as Box<dyn ActorTrait>)
            })
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(registry))
        .await;
}

async fn invoke_virtual_actor(
    actor_service: &ActorServiceImpl,
    tenant_id: &str,
    namespace: &str,
    actor_type: &str,
    http_method: &str,
    payload: Vec<u8>,
    query_params: HashMap<String, String>,
    ask: bool,
) -> Result<plexspaces_proto::actor::v1::InvokeActorResponse, tonic::Status> {
    let request = InvokeActorRequest {
        namespace: namespace.to_string(),
        actor_type: actor_type.to_string(),
        http_method: http_method.to_string(),
        payload,
        headers: HashMap::new(),
        query_params,
        path: String::new(),
        subpath: String::new(),
        ask,
        msg_type_override: String::new(),
        timeout: None,
    };

    let mut request = Request::new(request);
    request
        .metadata_mut()
        .insert("x-tenant-id", tenant_id.parse().unwrap());
    request
        .metadata_mut()
        .insert("x-namespace", namespace.parse().unwrap());

    ActorServiceTrait::invoke_actor(actor_service, request)
        .await
        .map(|response| response.into_inner())
}

async fn wait_for_actor_registration(
    actor_registry: &Arc<plexspaces_core::ActorRegistry>,
    actor_id: &String,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if actor_registry.lookup_actor(actor_id).await.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actor should be registered before continuing");
}

async fn wait_for_virtual_registration(
    virtual_actor_manager: &Arc<plexspaces_core::VirtualActorManager>,
    actor_id: &String,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if virtual_actor_manager.is_virtual(actor_id).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("virtual actor metadata should be registered before continuing");
}

async fn current_facet_types(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_id: &str,
) -> Vec<String> {
    let facet_manager = service_locator
        .get_facet_manager()
        .await
        .expect("FacetManager should be registered")
        .inner_clone();
    let facets = facet_manager
        .get_facets(actor_id)
        .await
        .expect("actor facets should be stored");
    let guard = facets.read().await;
    guard.list_facets()
}

async fn assert_has_concrete_timer_and_reminder_facets(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_id: &str,
) {
    let facet_manager = service_locator
        .get_facet_manager()
        .await
        .expect("FacetManager should be registered")
        .inner_clone();
    let facets = facet_manager
        .get_facets(actor_id)
        .await
        .expect("actor facets should be stored");
    let guard = facets.read().await;

    let timer = guard
        .get_facet("timer")
        .expect("timer facet should be attached to the running actor");
    let timer_guard = timer.read().await;
    assert!(
        timer_guard.as_any().downcast_ref::<TimerFacet>().is_some(),
        "timer facet should be recreated as a concrete TimerFacet"
    );
    drop(timer_guard);

    let reminder = guard
        .get_facet("reminder")
        .expect("reminder facet should be attached to the running actor");
    let reminder_guard = reminder.read().await;
    assert!(
        reminder_guard
            .as_any()
            .downcast_ref::<ReminderFacet>()
            .is_some(),
        "reminder facet should be recreated as a concrete ReminderFacet"
    );
}

/// Test: Virtual actor type registration and lazy activation
#[tokio::test]
async fn test_virtual_actor_type_registration_and_lazy_activation() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    // Register virtual actor type (simulating WASM application registration)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let namespace = "test-ns";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }
    });

    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None, // config
            namespace.to_string(),
            facet_config,
            None, // tenant_id
            None, // init_config_template
        )
        .await
        .unwrap();

    // Verify type is registered
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await
    );

    // Create actor ID using proper format
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");

    // Try to get or activate actor (should activate lazily)
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(response.is_ok(), "Should activate virtual actor lazily");

    // Verify actor is now active
    let actor_registry = service_locator.actor_registry().await.unwrap();
    assert!(actor_registry.lookup_actor(&actor_id).await.is_some());
}

/// Test: Type-level metadata preserves eager strategy and full facet config
#[tokio::test]
async fn test_virtual_actor_type_registration_preserves_eager_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "counter-eager";
    let namespace = "test-ns";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "7m",
            "activation_strategy": "eager"
        },
        "durability": {
            "enabled": true
        }
    });

    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None,
            namespace.to_string(),
            facet_config.clone(),
            Some("tenant-a".to_string()),
            Some(br#"{"initial_count":41}"#.to_vec()),
        )
        .await
        .unwrap();

    let metadata = virtual_actor_manager
        .get_virtual_actor_type(actor_type)
        .await
        .unwrap();
    assert_eq!(metadata.namespace, namespace);
    assert_eq!(metadata.tenant_id, "tenant-a");
    assert_eq!(
        metadata.activation_strategy,
        ActivationStrategy::ActivationStrategyEager
    );
    assert_eq!(metadata.facet_config, Some(facet_config));
    assert_eq!(
        metadata.init_config_template,
        Some(br#"{"initial_count":41}"#.to_vec())
    );
}

/// Test: Virtual actor suspension and reactivation with instance-level metadata
#[tokio::test]
async fn test_virtual_actor_suspension_and_reactivation_instance_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    use plexspaces_core::actor_id::build_actor_id;
    let namespace = "test-ns";
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");

    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(
        serde_json::json!({
            "idle_timeout": "1s",
            "activation_strategy": "lazy"
        }),
        100,
    );

    let actor_ref = node
        .spawn(
            &ctx,
            &actor_id,
            actor_type,
            vec![],
            None,
            HashMap::new(),
            vec![Box::new(virtual_facet)],
        )
        .await
        .unwrap();

    // Verify actor is registered as virtual (instance-level)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    wait_for_virtual_registration(&virtual_actor_manager, &actor_id).await;

    // Send a message to activate actor
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();

    // Wait for activation
    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Suspend actor (simulate deactivation)
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    // Verify actor is suspended (not active but still registered)
    assert!(!virtual_actor_manager.is_active(&actor_id).await);
    assert!(virtual_actor_manager.is_virtual(&actor_id).await); // Still virtual

    // Reactivate actor
    use plexspaces_services::actor_service::ActorServiceImpl;
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "Should reactivate suspended virtual actor"
    );

    // Verify actor is active again
    assert!(virtual_actor_manager.is_active(&actor_id).await);
    assert!(actor_registry.lookup_actor(&actor_id).await.is_some());
}

/// Test: Virtual actor reactivation with type-level metadata fallback
#[tokio::test]
async fn test_virtual_actor_reactivation_type_level_fallback() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    // Register virtual actor type (type-level registration, no instance-level)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let namespace = "test-ns";
    let facet_config = serde_json::json!({
        "virtual_actor": {
            "idle_timeout": "5m",
            "activation_strategy": "lazy"
        }
    });

    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None, // config
            namespace.to_string(),
            facet_config,
            None, // tenant_id
            None, // init_config_template
        )
        .await
        .unwrap();

    // Create actor ID
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");

    // Spawn actor (this will register it instance-level)
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(
        serde_json::json!({
            "idle_timeout": "1s",
            "activation_strategy": "lazy"
        }),
        100,
    );

    let actor_ref = node
        .spawn(
            &ctx,
            &actor_id,
            actor_type,
            vec![],
            None,
            HashMap::new(),
            vec![Box::new(virtual_facet)],
        )
        .await
        .unwrap();

    // Send message to activate
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();
    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Suspend actor
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    // CRITICAL: Remove instance-level metadata (simulating the error case)
    // In real scenario, this shouldn't happen, but we test the fallback
    // Note: We can't directly remove from VirtualActorManager, but we can test
    // by creating a new actor ID that was never instance-registered

    // Create a new actor ID that matches the type but was never instance-registered
    let new_actor_id = build_actor_id("user-2", actor_type, Some(namespace), "test-node");

    // Verify type-level registration exists
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await
    );

    // Try to activate new actor (should use type-level metadata)
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-2", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "Should activate using type-level metadata fallback"
    );

    // Verify actor is active
    assert!(actor_registry.lookup_actor(&new_actor_id).await.is_some());
}

/// Test: Error case - virtual actor activation fails when metadata is missing
#[tokio::test]
async fn test_virtual_actor_activation_error_missing_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    register_counter_behavior_with_initial_count(&service_locator, "read-state-tracker").await;

    // Don't register virtual actor type or instance
    let actor_type = "nonexistent";
    let namespace = "test-ns";

    // Try to activate actor that doesn't exist
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        false,
    )
    .await;

    // Should fail because actor type is not registered as virtual
    assert!(response.is_err());
    let err = response.unwrap_err();
    let error_msg = err.message();
    assert!(error_msg.contains("not found") || error_msg.contains("No actors found"));
}

/// Test: Virtual actor with proper actor ID format
#[tokio::test]
async fn test_virtual_actor_actor_id_format() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_core::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_core::ServiceLocator>;

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

    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None,
            namespace.to_string(),
            facet_config,
            None,
            None,
        )
        .await
        .unwrap();

    // Test proper actor ID format: {id}//{actor_type}::{namespace}@{node_id}
    use plexspaces_core::actor_id::{build_actor_id, parse_actor_id};
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");

    // Verify format
    assert_eq!(
        actor_id,
        "user-1//read-state-tracker::orbit-read-state-ts@test-node"
    );

    // Parse and verify components
    let parsed = parse_actor_id(&actor_id).unwrap();
    assert_eq!(parsed.id, "user-1");
    assert_eq!(parsed.actor_type, "read-state-tracker");
    assert_eq!(parsed.namespace, Some("orbit-read-state-ts".to_string()));
    assert_eq!(parsed.node_id, "test-node");

    // Verify type-level registration works with this format
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await
    );
}

/// Test: Virtual actor activation with HTTP-style actor_type format (read-state-tracker:user-1)
/// This test validates the fix for migrating_orbit example where actor_type comes from HTTP path
#[tokio::test]
async fn test_virtual_actor_activation_with_http_format() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    register_counter_behavior_with_initial_count(&service_locator, "read-state-tracker").await;

    // Register virtual actor type (matches migrating_orbit example)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "read-state-tracker";
    let namespace = "orbit-read-state-ts";

    virtual_actor_manager
        .register_virtual_actor_type(
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
            None,
        )
        .await
        .unwrap();

    // Simulate HTTP request format: "read-state-tracker:user-1"
    let http_actor_type = "read-state-tracker:user-1";
    let base_actor_type = "read-state-tracker";

    // Extract instance ID from HTTP format (same logic as invoke_actor)
    let instance_id = if http_actor_type.contains(':') {
        http_actor_type
            .split_once(':')
            .map(|(_actor_type_part, instance_id)| instance_id.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string())
    } else {
        ulid::Ulid::new().to_string()
    };

    // Build proper actor ID format
    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id(&instance_id, base_actor_type, Some(namespace), "test-node");

    // Verify format is correct (not //read-state-tracker::...)
    assert!(
        !actor_id.starts_with("//"),
        "Actor ID should not start with // - missing instance ID"
    );
    assert!(
        actor_id.contains(&instance_id),
        "Actor ID should contain instance ID"
    );
    assert_eq!(
        actor_id,
        format!(
            "{}//{}::{}@test-node",
            instance_id, base_actor_type, namespace
        )
    );

    // Test activation via ActorService
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        http_actor_type,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "Should activate virtual actor with proper actor ID format"
    );

    // Verify actor is registered
    wait_for_virtual_registration(&virtual_actor_manager, &actor_id).await;
}

/// Test: Virtual actor reactivation after suspension with type-level registration only
/// This test validates the fix for migrating_orbit example where actor is type-registered
/// but instance metadata is missing when reactivating
#[tokio::test]
async fn test_virtual_actor_reactivation_type_registered_only() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    register_counter_behavior_with_initial_count(&service_locator, "read-state-tracker").await;

    // Register virtual actor type (type-level registration only, no instance-level)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "read-state-tracker";
    let namespace = "orbit-read-state-ts";

    virtual_actor_manager
        .register_virtual_actor_type(
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
            None,
        )
        .await
        .unwrap();

    // Build actor ID (simulating HTTP request format: "read-state-tracker:user-1")
    use plexspaces_core::actor_id::build_actor_id;
    let instance_id = "user-1";
    let actor_id = build_actor_id(instance_id, actor_type, Some(namespace), "test-node");

    // Verify actor is NOT instance-registered (only type-registered)
    let instance_metadata = virtual_actor_manager.get_metadata(&actor_id).await;
    assert!(
        instance_metadata.is_none(),
        "Actor should not be instance-registered yet"
    );
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await,
        "Actor type should be registered"
    );

    // Try to activate actor (should work even though instance is not registered)
    // This simulates the migrating_orbit scenario where actor is activated via HTTP
    use plexspaces_services::actor_service::ActorServiceImpl;

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:{}", actor_type, instance_id),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    if let Err(e) = &response {
        eprintln!("Activation failed: {}", e.message());
    }
    assert!(
        response.is_ok(),
        "Should activate virtual actor even if only type-registered"
    );

    // Verify actor is now instance-registered
    let instance_metadata_after = virtual_actor_manager.get_metadata(&actor_id).await;
    assert!(
        instance_metadata_after.is_some(),
        "Actor should be instance-registered after activation"
    );
    assert!(
        virtual_actor_manager.is_virtual(&actor_id).await,
        "Actor should be virtual"
    );
}

/// Test: type-level registration can rebuild a behavior from stored init config
#[tokio::test]
async fn test_virtual_actor_type_registration_reinstantiates_behavior_from_template() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";

    register_counter_behavior_with_initial_count(&service_locator, "counter-from-template").await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "counter-from-template";
    let namespace = "test-ns";
    let actor_id = plexspaces_core::actor_id::build_actor_id(
        "user-template",
        actor_type,
        Some(namespace),
        "test-node",
    );

    virtual_actor_manager
        .register_virtual_actor_type(
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
            Some(br#"{"initial_count":7}"#.to_vec()),
        )
        .await
        .unwrap();

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-template", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "type-registered virtual actor should activate"
    );

    let actor_ref = service_locator
        .actor_registry()
        .await
        .unwrap()
        .lookup_actor(&actor_id)
        .await
        .unwrap();
    let initial_count = ask_for_count(actor_ref.as_ref()).await.unwrap();
    assert_eq!(
        initial_count, 7,
        "behavior should be rebuilt from init_config_template on first activation"
    );

    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-template", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "reactivation should succeed after deactivation"
    );

    let reactivated_ref = service_locator
        .actor_registry()
        .await
        .unwrap()
        .lookup_actor(&actor_id)
        .await
        .unwrap();
    let reactivated_count = ask_for_count(reactivated_ref.as_ref()).await.unwrap();
    assert_eq!(
        reactivated_count, 7,
        "reactivation should rebuild from stored type metadata rather than a retained instance"
    );

    let instance_metadata = virtual_actor_manager.get_metadata(&actor_id).await.unwrap();
    assert_eq!(instance_metadata.actor_type, actor_type);
    assert_eq!(instance_metadata.namespace, namespace);
    assert_eq!(
        instance_metadata.activation_strategy,
        ActivationStrategy::ActivationStrategyLazy
    );
}

/// Test: Virtual actor activation preserves state after suspension/reactivation
#[tokio::test]
async fn test_virtual_actor_state_preservation() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    use plexspaces_core::actor_id::build_actor_id;
    let namespace = "test-ns";
    let actor_id = build_actor_id("user-1", actor_type, Some(namespace), "test-node");

    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(
        serde_json::json!({
            "idle_timeout": "1s",
            "activation_strategy": "lazy"
        }),
        100,
    );

    let actor_ref = node
        .spawn(
            &ctx,
            &actor_id,
            actor_type,
            vec![],
            None,
            HashMap::new(),
            vec![Box::new(virtual_facet)],
        )
        .await
        .unwrap();

    // Increment counter
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        ..Default::default()
    };
    actor_ref.tell(msg).await.unwrap();
    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Note: This test would need durability facet to preserve state across suspension
    // For now, we just verify the activation/reactivation flow works
    // State preservation requires DurabilityFacet which is a separate concern

    // Suspend and reactivate
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(response.is_ok(), "Should reactivate successfully");
}

#[tokio::test]
async fn test_virtual_actor_reactivation_recreates_timer_and_reminder_facets() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter-with-facets";
    let namespace = "test-ns";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    use plexspaces_core::actor_id::build_actor_id;
    let actor_id = build_actor_id("user-facets", actor_type, Some(namespace), "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    let actor_ref = node
        .spawn(
            &ctx,
            &actor_id,
            actor_type,
            br#"{"initial_count":3}"#.to_vec(),
            None,
            HashMap::new(),
            vec![
                Box::new(VirtualActorFacet::new(
                    serde_json::json!({
                        "idle_timeout": "1s",
                        "activation_strategy": "lazy"
                    }),
                    100,
                )),
                Box::new(plexspaces_journaling::TimerFacet::new(
                    serde_json::json!({
                        "interval_ms": 250
                    }),
                    60,
                    service_locator.clone(),
                )),
                Box::new(plexspaces_journaling::ReminderFacet::new(
                    service_locator.get_journal_storage().await.unwrap(),
                    serde_json::json!({
                        "default_due_time": "100ms"
                    }),
                    70,
                    service_locator.clone(),
                )),
            ],
        )
        .await
        .unwrap();

    actor_ref
        .tell(Message {
            id: ulid::Ulid::new().to_string(),
            payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
            ..Default::default()
        })
        .await
        .unwrap();

    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    let facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(facet_types.contains(&"virtual_actor".to_string()));
    assert!(facet_types.contains(&"timer".to_string()));
    assert!(facet_types.contains(&"reminder".to_string()));
    assert_has_concrete_timer_and_reminder_facets(&service_locator, &actor_id).await;

    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:user-facets", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "reactivation with timer/reminder facets should succeed"
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    let reactivated_facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(reactivated_facet_types.contains(&"virtual_actor".to_string()));
    assert!(reactivated_facet_types.contains(&"timer".to_string()));
    assert!(reactivated_facet_types.contains(&"reminder".to_string()));
    assert_has_concrete_timer_and_reminder_facets(&service_locator, &actor_id).await;

    let actor_ref = actor_registry.lookup_actor(&actor_id).await.unwrap();
    let count = ask_for_count(actor_ref.as_ref()).await.unwrap();
    assert_eq!(
        count, 3,
        "reactivation should rebuild from preserved virtual metadata"
    );
}

#[tokio::test]
async fn test_virtual_workflow_behavior_reactivates_from_type_metadata() {
    let node = NodeBuilder::new("test-node").build().await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-workflows";
    let actor_type = "workflow-probe";
    register_workflow_probe_behavior(&service_locator, actor_type).await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    virtual_actor_manager
        .register_virtual_actor_type(
            actor_type.to_string(),
            None,
            namespace.to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "idle_timeout": "5m",
                    "activation_strategy": "lazy"
                }
            }),
            Some(tenant_id.to_string()),
            Some(br#"{"initial_count":11}"#.to_vec()),
        )
        .await
        .unwrap();

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let actor_name = format!("{}:execution-1", actor_type);
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "workflow-style virtual actor should activate"
    );

    let actor_id = plexspaces_core::actor_id::build_actor_id(
        "execution-1",
        actor_type,
        Some(namespace),
        "test-node",
    );
    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    let stop_ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&stop_ctx, &actor_id)
        .await
        .unwrap();

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "workflow-style virtual actor should reactivate from stored type metadata"
    );

    let actor_ref = actor_registry.lookup_actor(&actor_id).await.unwrap();
    let count = ask_for_count(actor_ref.as_ref()).await.unwrap();
    assert_eq!(count, 11);
}
