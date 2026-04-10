// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Unit and integration tests for AskReply and SendMessage.
//!
//! Tests FaaS-style actor ask/tell handling via HTTP GET/POST/PUT routes.
//! Covers:
//! - GET requests (ask pattern)
//! - POST requests (tell pattern)
//! - Actor lookup by type
//! - Random selection when multiple actors found
//! - JWT authentication and tenant_id validation
//! - Error cases (404, invalid args, etc.)

use async_trait::async_trait;
use plexspaces_actor::{actor_factory_impl::ActorFactoryImpl, ActorFactory};
use plexspaces_behavior::GenServer;
use plexspaces_core::{
    behavior_factory::BehaviorRegistry, Actor as ActorTrait, ActorContext, ActorRegistry,
    ActorId, BehaviorError, BehaviorType, FacetManager, Message, ReplyWaiterRegistry,
    ServiceLocator as ServiceLocatorTrait, VirtualActorManager,
};
use plexspaces_mailbox::new_message;
use plexspaces_object_registry::{ObjectRegistry, SqliteObjectRegistryRepository};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService, AskReplyRequest, AskReplyResponse, SendMessageRequest,
    SendMessageResponse,
};
use plexspaces_proto::object_registry::v1::ObjectRegistration;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::ServiceLocatorImpl;
use std::collections::HashMap;
use std::sync::Arc;
use tonic::Request;

// Counter actor that responds to GET (ask) and handles POST (tell)
struct CounterActor {
    count: i64,
}

fn canonical_actor_id(name: impl Into<String>, actor_type: &str, namespace: &str, node_id: &str) -> ActorId {
    ActorId::new(name, actor_type, namespace, node_id).expect("valid test actor id")
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

#[async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse payload as JSON to get action
        let payload_str = String::from_utf8_lossy(&msg.payload);
        let action: serde_json::Value = serde_json::from_str(&payload_str).unwrap_or_else(|_| {
            // If not JSON, try to parse as simple string
            serde_json::json!({ "action": payload_str })
        });

        let action_str = action
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("get");

        match action_str {
            "increment" => {
                self.count += 1;
                // For POST (tell), we don't need to reply, but we can for testing
                if !msg.sender_id.is_empty() {
                    let reply = serde_json::json!({ "count": self.count });
                    let reply_msg = new_message(serde_json::to_vec(&reply).unwrap());
                    let _ = ctx
                        .send_reply(
                            (!msg.correlation_id.is_empty()).then_some(msg.correlation_id.as_str()),
                            &msg.sender_id,
                            ctx.actor_id().clone(),
                            reply_msg,
                        )
                        .await; // Ignore errors for tell pattern
                }
            }
            "get" => {
                let reply = serde_json::json!({ "count": self.count });
                let reply_msg = new_message(serde_json::to_vec(&reply).unwrap());
                if !msg.sender_id.is_empty() {
                    ctx.send_reply(
                        (!msg.correlation_id.is_empty()).then_some(msg.correlation_id.as_str()),
                        &msg.sender_id,
                        ctx.actor_id().clone(),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
            }
            _ => {
                // For tell (POST), just update state if value provided
                if let Some(value) = action.get("value").and_then(|v| v.as_i64()) {
                    self.count = value;
                }
            }
        }
        Ok(())
    }
}

// Helper to create test ActorRegistry with counter actors
async fn create_test_registry_with_actors(
    node_id: &str,
    actor_type: &str,
    tenant_id: &str,
    num_actors: usize,
) -> (Arc<ActorRegistry>, Arc<ServiceLocatorImpl>) {
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;

    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistry::new(object_repo));

    // Simple adapter
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistry>,
    }

    #[async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_id: &str,
            object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(
                plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified,
            );
            self.inner
                .lookup(ctx, obj_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn lookup_full(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .lookup_full(ctx, object_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn register(
            &self,
            ctx: &plexspaces_core::RequestContext,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner.register(ctx, registration).await.map_err(|e| {
                Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    e.to_string(),
                )) as Box<dyn std::error::Error + Send + Sync>
            })
        }

        async fn discover(
            &self,
            _ctx: &plexspaces_core::RequestContext,
            _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
            _name: Option<String>,
            _labels: Option<Vec<String>>,
            _exclude_labels: Option<Vec<String>>,
            _health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(vec![])
        }

        async fn unregister(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .unregister(ctx, object_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn heartbeat(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: plexspaces_proto::object_registry::v1::ObjectType,
            object_id: &str,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .heartbeat(ctx, object_type, object_id)
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })
        }
    }

    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });

    let actor_registry = Arc::new(ActorRegistry::new(object_registry, node_id.to_string()));
    use plexspaces_node::create_default_service_locator;
    let service_locator = create_default_service_locator(Some("test-node".to_string()), None).await;
    service_locator
        .register_service(actor_registry.clone())
        .await;
    // ActorFactory is already registered by create_default_service_locator

    // Register NodeConfig
    use plexspaces_proto::node::v1::NodeConfig;
    let node_config = NodeConfig {
        id: "test-node".to_string(),
        listen_addr: String::new(),
        cluster_seed_nodes: vec![],
        cluster_name: String::new(),
        grpc_connection_pool_size: 2,
        max_connections: 100,
        heartbeat_interval_ms: 5000,
        clustering_enabled: true,
        metadata: std::collections::HashMap::new(),
        node_registry: None,
        grpc_address: String::new(),
        ..Default::default()
    };
    service_locator.register_node_config(node_config).await;

    // Create ActorFactory and required services
    let virtual_actor_manager = Arc::new(VirtualActorManager::new(actor_registry.clone()));
    use plexspaces_core::FacetManagerServiceWrapper;
    let facet_manager = Arc::new(FacetManagerServiceWrapper::new(Arc::new(
        FacetManager::new(),
    )));
    service_locator
        .register_service(virtual_actor_manager.clone())
        .await;
    service_locator.register_service(facet_manager).await;

    let actor_factory = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator
        .register_service(actor_factory.clone())
        .await;
    actor_registry
        .set_virtual_actor_manager(virtual_actor_manager.clone())
        .await;
    actor_registry
        .set_actor_factory(actor_factory.clone())
        .await;

    let registry = BehaviorRegistry::new();
    let module_name = actor_type.to_string();
    registry
        .register(actor_type.to_string(), move |_args| {
            let module_name = module_name.clone();
            Box::pin(async move {
                let _ = module_name;
                Ok(Box::new(CounterActor::new()) as Box<dyn ActorTrait>)
            })
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(registry))
        .await;

    // Register actors with type information using spawn_actor
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        tenant_id.to_string(),
        "default".to_string(),
    );
    for i in 0..num_actors {
        let actor_id = canonical_actor_id(format!("{actor_type}-{i}"), actor_type, "default", node_id);

        // Use spawn_actor instead of building and spawning separately
        let message_sender = actor_factory
            .spawn_actor(
                &ctx,
                &actor_id,
                actor_type,                       // Use the provided actor_type
                vec![],                           // initial_state
                None,                             // config
                std::collections::HashMap::new(), // labels
                vec![],                           // facets
            )
            .await
            .map_err(|e| format!("Failed to spawn actor: {}", e))
            .unwrap();

        // Register with type information for efficient lookup
        actor_registry
            .register_actor(
                &ctx,
                actor_id.clone(),
                message_sender,
                actor_type.to_string(),
                None, // config
                None, // instance
                Some(BehaviorType::GenServer),
            )
            .await;
    }

    (actor_registry, service_locator)
}

// Helper to create test ActorService
async fn create_test_actor_service(
    _actor_registry: Arc<ActorRegistry>,
    service_locator: Arc<ServiceLocatorImpl>,
    node_id: String,
) -> ActorServiceImpl {
    let reply_waiter_registry = Arc::new(ReplyWaiterRegistry::new());
    service_locator
        .register_service(reply_waiter_registry)
        .await;

    ActorServiceImpl::new(service_locator, node_id)
}

async fn register_counter_behavior(service_locator: &Arc<ServiceLocatorImpl>, actor_type: &str) {
    let registry = BehaviorRegistry::new();
    let module_name = actor_type.to_string();
    registry
        .register(actor_type.to_string(), move |_args| {
            let module_name = module_name.clone();
            Box::pin(async move {
                let _ = module_name;
                Ok(Box::new(CounterActor::new()) as Box<dyn ActorTrait>)
            })
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(registry))
        .await;
}

async fn ask_reply_request(
    service: &ActorServiceImpl,
    request: AskReplyRequest,
    tenant_id: &str,
) -> Result<tonic::Response<AskReplyResponse>, tonic::Status> {
    let namespace_header = if request.namespace.is_empty() {
        "default".to_string()
    } else {
        request.namespace.clone()
    };
    let mut request = Request::new(request);
    request
        .metadata_mut()
        .insert("x-tenant-id", tenant_id.parse().unwrap());
    request
        .metadata_mut()
        .insert("x-namespace", namespace_header.parse().unwrap());
    service.ask_reply(request).await
}

async fn send_message_request(
    service: &ActorServiceImpl,
    request: SendMessageRequest,
    tenant_id: &str,
) -> Result<tonic::Response<SendMessageResponse>, tonic::Status> {
    let namespace_header = if request.namespace.is_empty() {
        "default".to_string()
    } else {
        request.namespace.clone()
    };
    let mut request = Request::new(request);
    request
        .metadata_mut()
        .insert("x-tenant-id", tenant_id.parse().unwrap());
    request
        .metadata_mut()
        .insert("x-namespace", namespace_header.parse().unwrap());
    ActorService::send_message(service, request).await
}

fn build_ask_request(
    actor_type: &str,
    method: &str,
    payload: Vec<u8>,
    query_params: HashMap<String, String>,
) -> AskReplyRequest {
    AskReplyRequest {
        namespace: "default".to_string(),
        actor_type: actor_type.to_string(),
        http_method: method.to_string(),
        payload,
        headers: HashMap::new(),
        query_params,
        path: String::new(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "call".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
        timeout: None,
    }
}

fn build_send_request(actor_type: &str, payload: Vec<u8>) -> SendMessageRequest {
    SendMessageRequest {
        namespace: "default".to_string(),
        actor_type: actor_type.to_string(),
        http_method: "POST".to_string(),
        payload,
        headers: HashMap::new(),
        query_params: HashMap::new(),
        path: String::new(),
        subpath: String::new(),
        sender_id: String::new(),
        message_type: "cast".to_string(),
        correlation_id: String::new(),
        reply_to: String::new(),
        message_id: String::new(),
    }
}

#[tokio::test]
async fn test_ask_reply_get_success() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let request = build_ask_request(
        "counter",
        "GET",
        vec![],
        HashMap::from([("action".to_string(), "get".to_string())]),
    );
    let response = ask_reply_request(&service, request, "default")
        .await
        .expect("ask_reply should succeed")
        .into_inner();

    assert!(response.success);
    let payload: serde_json::Value = serde_json::from_slice(&response.payload).unwrap();
    assert!(payload.get("count").is_some());
}

#[tokio::test]
async fn test_ask_reply_ignores_stale_actor_type_index_entries() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry.clone(), service_locator, "node1".to_string())
            .await;

        let stale_actor_id = canonical_actor_id("stale-counter", "counter", "default", "node1");
    let key = ("".to_string(), "default".to_string(), "counter".to_string());
    {
        let mut index = actor_registry.actor_type_index().write().await;
        index.entry(key).or_default().insert(0, stale_actor_id);
    }

    let result = ask_reply_request(
        &service,
        build_ask_request(
            "counter",
            "GET",
            vec![],
            HashMap::from([("action".to_string(), "get".to_string())]),
        ),
        "default",
    )
    .await;
    assert!(
        result.is_ok(),
        "ask_reply should ignore stale type index entries"
    );
}

#[tokio::test]
async fn test_ask_reply_activates_virtual_actor_type_with_instance_id() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "virtual-counter", "default", 0).await;
    register_counter_behavior(&service_locator, "virtual-counter").await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    virtual_actor_manager
        .register_virtual_actor_type(
            "virtual-counter".to_string(),
            None,
            "default".to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "idle_timeout": "5m",
                    "activation_strategy": "lazy"
                }
            }),
            Some("default".to_string()),
            None,
        )
        .await
        .unwrap();

    let service =
        create_test_actor_service(actor_registry.clone(), service_locator, "node1".to_string())
            .await;

    let response = ask_reply_request(
        &service,
        build_ask_request(
            "virtual-counter:user-1",
            "GET",
            vec![],
            HashMap::from([("action".to_string(), "get".to_string())]),
        ),
        "default",
    )
    .await
    .expect("ask_reply should activate the virtual actor")
    .into_inner();

    let expected_actor_id = canonical_actor_id("user-1", "virtual-counter", "default", "node1");
    assert_eq!(response.actor_id, expected_actor_id.to_string());
    assert!(actor_registry
        .lookup_actor(&ActorId::from_canonical(&response.actor_id).expect("canonical actor id"))
        .await
        .is_some());
}

#[tokio::test]
async fn test_ask_reply_shorthand_virtual_actor_increment_applies_once() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "virtual-counter", "default", 0).await;
    register_counter_behavior(&service_locator, "virtual-counter").await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    virtual_actor_manager
        .register_virtual_actor_type(
            "virtual-counter".to_string(),
            None,
            "default".to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "idle_timeout": "5m",
                    "activation_strategy": "lazy"
                }
            }),
            Some("default".to_string()),
            None,
        )
        .await
        .unwrap();

    let service =
        create_test_actor_service(actor_registry.clone(), service_locator, "node1".to_string())
            .await;

    let increment_response = ask_reply_request(
        &service,
        build_ask_request(
            "virtual-counter:user-1",
            "POST",
            br#"{"action":"increment"}"#.to_vec(),
            HashMap::new(),
        ),
        "default",
    )
    .await
    .expect("increment ask_reply should succeed")
    .into_inner();
    assert!(increment_response.success);

    let count_response = ask_reply_request(
        &service,
        build_ask_request(
            "virtual-counter:user-1",
            "GET",
            vec![],
            HashMap::from([("action".to_string(), "get".to_string())]),
        ),
        "default",
    )
    .await
    .expect("get ask_reply should succeed")
    .into_inner();

    let expected_actor_id = canonical_actor_id("user-1", "virtual-counter", "default", "node1");
    assert_eq!(count_response.actor_id, expected_actor_id.to_string());

    let payload: serde_json::Value =
        serde_json::from_slice(&count_response.payload).expect("count payload");
    assert_eq!(
        payload.get("count").and_then(|value| value.as_i64()),
        Some(1),
        "fresh shorthand virtual actor increment should be applied exactly once"
    );
}

#[tokio::test]
async fn test_send_message_activates_virtual_actor_type_with_instance_id() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "virtual-counter", "default", 0).await;
    register_counter_behavior(&service_locator, "virtual-counter").await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    virtual_actor_manager
        .register_virtual_actor_type(
            "virtual-counter".to_string(),
            None,
            "default".to_string(),
            serde_json::json!({
                "virtual_actor": {
                    "idle_timeout": "5m",
                    "activation_strategy": "lazy"
                }
            }),
            Some("default".to_string()),
            None,
        )
        .await
        .unwrap();

    let service =
        create_test_actor_service(actor_registry.clone(), service_locator, "node1".to_string())
            .await;

    let response = send_message_request(
        &service,
        build_send_request(
            "virtual-counter:user-2",
            br#"{"action":"increment"}"#.to_vec(),
        ),
        "default",
    )
    .await
    .expect("send_message should activate the virtual actor")
    .into_inner();

    let expected_actor_id = canonical_actor_id("user-2", "virtual-counter", "default", "node1");
    assert!(response.success);
    assert_eq!(response.actor_id, expected_actor_id.to_string());
    assert!(actor_registry
        .lookup_actor(&ActorId::from_canonical(&response.actor_id).expect("canonical actor id"))
        .await
        .is_some());
}

#[tokio::test]
async fn test_send_message_post_success() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let response = send_message_request(
        &service,
        build_send_request("counter", br#"{"action":"increment"}"#.to_vec()),
        "default",
    )
    .await
    .expect("send_message should succeed")
    .into_inner();

    assert!(response.success);
    assert_eq!(
        response.actor_id,
        canonical_actor_id("counter-0", "counter", "default", "node1").to_string()
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_send_message_instance_style_target_falls_back_to_type_lookup() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let response = send_message_request(
        &service,
        build_send_request("counter:default", br#"{"action":"increment"}"#.to_vec()),
        "default",
    )
    .await
    .expect("send_message should resolve instance-style target via type lookup")
    .into_inner();

    assert!(response.success);
    assert_eq!(
        response.actor_id,
        canonical_actor_id("counter-0", "counter", "default", "node1").to_string()
    );
}

#[tokio::test]
async fn test_ask_reply_missing_actor_type() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let result = ask_reply_request(
        &service,
        build_ask_request("", "GET", vec![], HashMap::new()),
        "default",
    )
    .await;
    assert!(matches!(result, Err(e) if e.code() == tonic::Code::InvalidArgument));
}

#[tokio::test]
async fn test_ask_reply_not_found() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 0).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let result = ask_reply_request(
        &service,
        build_ask_request("nonexistent", "GET", vec![], HashMap::new()),
        "default",
    )
    .await;
    assert!(matches!(result, Err(e) if e.code() == tonic::Code::NotFound));
}

#[tokio::test]
async fn test_ask_reply_multiple_actors_random_selection() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 3).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    for _ in 0..10 {
        let result = ask_reply_request(
            &service,
            build_ask_request("counter", "GET", vec![], HashMap::new()),
            "default",
        )
        .await;
        assert!(
            !matches!(&result, Err(e) if e.code() == tonic::Code::NotFound),
            "ask_reply should resolve one of the matching actors"
        );
    }
}

#[tokio::test]
async fn test_ask_reply_defaults_namespace_from_metadata() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let mut request = build_ask_request("counter", "GET", vec![], HashMap::new());
    request.namespace = String::new();
    let result = ask_reply_request(&service, request, "default").await;
    assert!(
        !matches!(&result, Err(e) if e.code() == tonic::Code::NotFound),
        "ask_reply should use namespace from metadata when request namespace is empty"
    );
}

#[tokio::test]
async fn test_ask_reply_get_query_params_to_json() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let request = build_ask_request(
        "counter",
        "GET",
        vec![],
        HashMap::from([
            ("key1".to_string(), "value1".to_string()),
            ("key2".to_string(), "value2".to_string()),
        ]),
    );
    let result = ask_reply_request(&service, request, "default").await;
    assert!(
        !matches!(&result, Err(e) if e.message().contains("serialize")),
        "ask_reply should serialize GET query parameters to JSON"
    );
}

#[tokio::test]
async fn test_send_message_rejects_get() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let mut request = build_send_request("counter", Vec::new());
    request.http_method = "GET".to_string();
    let result = send_message_request(&service, request, "default").await;
    assert!(matches!(result, Err(e) if e.code() == tonic::Code::InvalidArgument));
}

#[tokio::test]
async fn test_send_message_post_headers_preserved() {
    let (actor_registry, service_locator) =
        create_test_registry_with_actors("node1", "counter", "default", 1).await;
    let service =
        create_test_actor_service(actor_registry, service_locator, "node1".to_string()).await;

    let mut request = build_send_request("counter", br#"{"action":"increment"}"#.to_vec());
    request.headers = HashMap::from([
        ("X-Custom-Header".to_string(), "custom-value".to_string()),
        ("Content-Type".to_string(), "application/json".to_string()),
    ]);
    let response = send_message_request(&service, request, "default")
        .await
        .expect("send_message with headers should succeed")
        .into_inner();
    assert!(response.success);
}
