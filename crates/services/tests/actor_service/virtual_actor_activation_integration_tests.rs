// SPDX-License-Identifier: AGPL-3.0-or-later
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
use plexspaces_actor::behavior_factory::{BehaviorFactoryError, BehaviorRegistry};
use plexspaces_actor::{
    Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType,
    InitializableServiceLocator, Message, RequestContext, ServiceLocator, RequestContextExt,
};
use plexspaces_journaling::{ReminderFacet, TimerFacet, VirtualActorFacet};
use plexspaces_node::{Node, NodeBuilder, ReleaseSpec};
use plexspaces_proto::actor::v1::{
    actor_service_server::ActorService as ActorServiceTrait, AskReplyRequest, SendMessageRequest,
};
use plexspaces_proto::node::v1::RuntimeConfig;
use plexspaces_proto::storage::v1::SharedDbConfig;
use plexspaces_services::actor_service::ActorServiceImpl;
use plexspaces_services::process_group_service::ProcessGroupServiceImpl;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tonic::Request;

/// Test actor for virtual actor tests
#[derive(Debug, Clone)]
struct CounterActor {
    count: i32,
}

fn canonical_actor_id(
    name: impl Into<String>,
    actor_type: &str,
    namespace: &str,
    node_id: &str,
) -> ActorId {
    ActorId::new(name, actor_type, namespace, node_id).expect("valid test actor id")
}

fn writable_test_db_url(temp_dir: &TempDir) -> String {
    let db_path: PathBuf = temp_dir.path().join("db").join("plexspaces.db");
    std::fs::create_dir_all(
        db_path
            .parent()
            .expect("sqlite test db path should always have a parent directory"),
    )
    .expect("failed to create writable sqlite test directory");
    format!("sqlite://{}?mode=rwc", db_path.display())
}

async fn build_test_node(node_id: &str) -> (Node, TempDir) {
    let temp_dir = TempDir::new().expect("failed to create temp dir for sqlite-backed test node");
    let node = NodeBuilder::new(node_id)
        .with_sqlite_journaling()
        .with_release_spec(ReleaseSpec {
            name: format!("virtual-actor-activation-{node_id}"),
            version: "0.0.0-test".to_string(),
            runtime: Some(RuntimeConfig {
                db: Some(SharedDbConfig {
                    connection_string: writable_test_db_url(&temp_dir),
                    auto_migrate: true,
                    ..Default::default()
                }),
                ..Default::default()
            }),
            ..Default::default()
        })
        .build()
        .await;
    let process_group_service: Arc<dyn plexspaces_actor::ProcessGroupService> = Arc::new(
        ProcessGroupServiceImpl::new(node.service_locator(), node_id.to_string()),
    );
    node.service_locator()
        .register_process_group_service(process_group_service)
        .await;
    (node, temp_dir)
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

#[derive(Debug, Clone)]
struct DurableWorkflowProbeActor {
    count: i32,
}

impl DurableWorkflowProbeActor {
    fn new_with_count(count: i32) -> Self {
        Self { count }
    }
}

#[derive(Debug, Clone)]
struct DurableCounterActor {
    count: i32,
}

impl DurableCounterActor {
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

#[async_trait]
impl ActorTrait for DurableWorkflowProbeActor {
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

    async fn capture_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
    ) -> Result<Option<Vec<u8>>, plexspaces_actor::ActorError> {
        serde_json::to_vec(&serde_json::json!({ "count": self.count }))
            .map(Some)
            .map_err(|e| plexspaces_actor::ActorError::BehaviorError(e.to_string()))
    }

    async fn restore_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
        state_data: &[u8],
    ) -> Result<bool, plexspaces_actor::ActorError> {
        let payload: serde_json::Value = serde_json::from_slice(state_data)
            .map_err(|e| plexspaces_actor::ActorError::BehaviorError(e.to_string()))?;
        self.count = payload["count"].as_i64().unwrap_or_default() as i32;
        Ok(true)
    }
}

#[async_trait]
impl ActorTrait for DurableCounterActor {
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

    async fn capture_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
    ) -> Result<Option<Vec<u8>>, plexspaces_actor::ActorError> {
        serde_json::to_vec(&serde_json::json!({ "count": self.count }))
            .map(Some)
            .map_err(|e| plexspaces_actor::ActorError::BehaviorError(e.to_string()))
    }

    async fn restore_checkpoint_state(
        &mut self,
        _ctx: &ActorContext,
        state_data: &[u8],
    ) -> Result<bool, plexspaces_actor::ActorError> {
        let payload: serde_json::Value = serde_json::from_slice(state_data)
            .map_err(|e| plexspaces_actor::ActorError::BehaviorError(e.to_string()))?;
        self.count = payload["count"].as_i64().unwrap_or_default() as i32;
        Ok(true)
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
                if !msg.sender_id.is_empty() {
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    let reply = serde_json::json!({ "count": self.count });
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        ctx.actor_id().clone(),
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
                        ctx.actor_id().clone(),
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
                if !msg.sender_id.is_empty() {
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    let reply = serde_json::json!({ "count": self.count });
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        ctx.actor_id().clone(),
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
                        ctx.actor_id().clone(),
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
impl GenServer for DurableCounterActor {
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
                if !msg.sender_id.is_empty() {
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    let reply = serde_json::json!({ "count": self.count });
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        ctx.actor_id().clone(),
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
                        ctx.actor_id().clone(),
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
impl GenServer for DurableWorkflowProbeActor {
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
                if !msg.sender_id.is_empty() {
                    let correlation_id = if msg.correlation_id.is_empty() {
                        None
                    } else {
                        Some(msg.correlation_id.as_str())
                    };
                    let reply = serde_json::json!({ "count": self.count });
                    ctx.send_reply(
                        correlation_id,
                        &msg.sender_id,
                        ctx.actor_id().clone(),
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
                        ctx.actor_id().clone(),
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
    ctx: &RequestContext,
    actor_ref: &(dyn plexspaces_actor::MessageSender + Send + Sync),
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let reply = actor_ref
        .ask(
            ctx,
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

async fn increment_count(
    ctx: &RequestContext,
    actor_ref: &(dyn plexspaces_actor::MessageSender + Send + Sync),
) -> Result<i32, Box<dyn std::error::Error + Send + Sync>> {
    let reply = actor_ref
        .ask(
            ctx,
            Message {
                id: ulid::Ulid::new().to_string(),
                payload: serde_json::to_vec(&CounterMessage::Increment)?,
                message_type: "call".to_string(),
                ..Default::default()
            },
            Duration::from_secs(5),
        )
        .await?;
    let payload: serde_json::Value = serde_json::from_slice(&reply.payload)?;
    Ok(payload["count"].as_i64().unwrap_or_default() as i32)
}

fn build_counter_actor(initial_count: i32) -> Box<dyn ActorTrait> {
    Box::new(CounterActor::new_with_count(initial_count))
}

fn build_durable_counter_actor(initial_count: i32) -> Box<dyn ActorTrait> {
    Box::new(DurableCounterActor::new_with_count(initial_count))
}

fn build_workflow_probe_actor(initial_count: i32) -> Box<dyn ActorTrait> {
    Box::new(WorkflowProbeActor::new_with_count(initial_count))
}

fn build_durable_workflow_probe_actor(initial_count: i32) -> Box<dyn ActorTrait> {
    Box::new(DurableWorkflowProbeActor::new_with_count(initial_count))
}

async fn register_behavior_with_initial_count(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
    build_actor: fn(i32) -> Box<dyn ActorTrait>,
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
                Ok(build_actor(initial_count))
            })
        })
        .await;
    service_locator
        .register_behavior_registry(Arc::new(registry))
        .await;
}

async fn register_counter_behavior_with_initial_count(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    register_behavior_with_initial_count(service_locator, actor_type, build_counter_actor).await;
}

async fn register_durable_counter_behavior_with_initial_count(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    register_behavior_with_initial_count(service_locator, actor_type, build_durable_counter_actor)
        .await;
}

async fn register_workflow_probe_behavior(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    register_behavior_with_initial_count(service_locator, actor_type, build_workflow_probe_actor)
        .await;
}

async fn register_durable_workflow_probe_behavior(
    service_locator: &plexspaces_services::ServiceLocatorImpl,
    actor_type: &str,
) {
    register_behavior_with_initial_count(
        service_locator,
        actor_type,
        build_durable_workflow_probe_actor,
    )
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
) -> Result<LocalInvokeResponse, tonic::Status> {
    if ask {
        let mut request = Request::new(AskReplyRequest {
            namespace: namespace.to_string(),
            actor_type: actor_type.to_string(),
            actor_name: String::new(),
            http_method: http_method.to_string(),
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
        });
        request
            .metadata_mut()
            .insert("x-tenant-id", tenant_id.parse().unwrap());
        request
            .metadata_mut()
            .insert("x-namespace", namespace.parse().unwrap());
        ActorServiceTrait::ask_reply(actor_service, request)
            .await
            .map(|response| {
                let response = response.into_inner();
                LocalInvokeResponse {
                    success: response.success,
                    payload: response.payload,
                    actor_id: response.actor_id,
                    error_message: response.error_message,
                }
            })
    } else {
        let mut request = Request::new(SendMessageRequest {
            namespace: namespace.to_string(),
            actor_type: actor_type.to_string(),
            actor_name: String::new(),
            http_method: http_method.to_string(),
            payload,
            headers: HashMap::new(),
            query_params,
            path: String::new(),
            subpath: String::new(),
            sender_id: String::new(),
            message_type: "cast".to_string(),
            correlation_id: String::new(),
            reply_to: String::new(),
            message_id: String::new(),
        });
        request
            .metadata_mut()
            .insert("x-tenant-id", tenant_id.parse().unwrap());
        request
            .metadata_mut()
            .insert("x-namespace", namespace.parse().unwrap());
        ActorServiceTrait::send_message(actor_service, request)
            .await
            .map(|response| {
                let response = response.into_inner();
                LocalInvokeResponse {
                    success: response.success,
                    payload: Vec::new(),
                    actor_id: response.actor_id,
                    error_message: response.error_message,
                }
            })
    }
}

#[derive(Debug)]
struct LocalInvokeResponse {
    success: bool,
    payload: Vec<u8>,
    actor_id: String,
    error_message: String,
}

async fn wait_for_actor_registration(
    actor_registry: &Arc<plexspaces_actor::ActorRegistry>,
    actor_id: &ActorId,
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

async fn wait_for_actor_unregistration(
    actor_registry: &Arc<plexspaces_actor::ActorRegistry>,
    actor_id: &ActorId,
) {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if actor_registry.lookup_actor(actor_id).await.is_none() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("actor should be unregistered before continuing");
}

async fn wait_for_virtual_registration(
    virtual_actor_manager: &Arc<plexspaces_actor::VirtualActorManager>,
    actor_id: &ActorId,
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

fn count_from_invoke_response(response: &LocalInvokeResponse) -> i32 {
    let payload: serde_json::Value =
        serde_json::from_slice(&response.payload).expect("ask reply payload should be JSON");
    payload["count"].as_i64().unwrap_or_default() as i32
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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_type_registration_and_lazy_activation() {
    let (node, _db_dir) = build_test_node("test-node").await;
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
    let actor_id = canonical_actor_id("user-1", actor_type, namespace, "test-node");

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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_type_registration_preserves_eager_metadata() {
    let (node, _db_dir) = build_test_node("test-node").await;
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
    assert_eq!(metadata.spec.namespace, namespace);
    assert_eq!(metadata.spec.tenant_id, "tenant-a");
    assert_eq!(
        metadata.activation_strategy(),
        ActivationStrategy::ActivationStrategyEager
    );
    // facet_config is derived from spec.facets; init_config_template args are in spec.args
    assert!(metadata.facet_config().is_some());
    // args should contain initial_count from init_config_template
    assert_eq!(
        metadata.spec.args.get("initial_count").map(|s| s.as_str()),
        Some("41")
    );
}

/// Test: Virtual actor suspension and reactivation with instance-level metadata
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_suspension_and_reactivation_instance_metadata() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let namespace = "test-ns";
    let actor_id = canonical_actor_id("user-1", actor_type, namespace, "test-node");

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

    // Activate the lazy virtual actor via invoke (direct tell() on lazy actor sends to dead mailbox)
    use plexspaces_services::actor_service::ActorServiceImpl;
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let activate_response = invoke_virtual_actor(
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
    assert!(activate_response.is_ok(), "initial activation must succeed");

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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_reactivation_type_level_fallback() {
    let (node, _db_dir) = build_test_node("test-node").await;
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
            Some(tenant_id.to_string()),
            None, // init_config_template
        )
        .await
        .unwrap();

    // Create actor ID
    let actor_id = canonical_actor_id("user-1", actor_type, namespace, "test-node");

    // Spawn actor (this registers it at the instance level in VirtualActorManager)
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(
        serde_json::json!({
            "idle_timeout": "1s",
            "activation_strategy": "lazy"
        }),
        100,
    );

    node.spawn(
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

    // Activate user-1 via invoke (not direct tell on lazy actor)
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let activate_response = invoke_virtual_actor(
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
        activate_response.is_ok(),
        "initial activation of user-1 must succeed"
    );

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

    // Create a new actor ID that matches the type but was never instance-registered
    let new_actor_id = canonical_actor_id("user-2", actor_type, namespace, "test-node");

    // Verify type-level registration exists
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await
    );

    // Try to activate new actor (should use type-level metadata)
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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_activation_error_missing_metadata() {
    let (node, _db_dir) = build_test_node("test-node").await;
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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_actor_id_format() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;

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
    let actor_id = canonical_actor_id("user-1", actor_type, namespace, "test-node");

    // Verify format
    assert_eq!(
        actor_id.to_string(),
        "user-1//read-state-tracker::orbit-read-state-ts@test-node"
    );

    // Parse and verify components
    let parsed = ActorId::from_canonical(actor_id.as_str()).unwrap();
    assert_eq!(parsed.name(), "user-1");
    assert_eq!(parsed.actor_type(), "read-state-tracker");
    assert_eq!(parsed.namespace(), "orbit-read-state-ts");
    assert_eq!(parsed.node_id(), "test-node");

    // Verify type-level registration works with this format
    assert!(
        virtual_actor_manager
            .is_virtual_actor_type(actor_type)
            .await
    );
}

/// Test: Virtual actor activation with HTTP-style actor_type format (read-state-tracker:user-1)
/// This test validates the fix for migrating_orbit example where actor_type comes from HTTP path
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_activation_with_http_format() {
    // Use unique actor_type to avoid interference when tests run in parallel
    let (node, _db_dir) = build_test_node("test-node-http-fmt").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    register_counter_behavior_with_initial_count(&service_locator, "orbit-tracker").await;

    // Register virtual actor type (matches migrating_orbit example)
    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "orbit-tracker";
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
                }
            }),
            Some(tenant_id.to_string()),
            None,
        )
        .await
        .unwrap();

    // Simulate HTTP request format: "orbit-tracker:user-1"
    let http_actor_type = "orbit-tracker:user-1";
    let base_actor_type = "orbit-tracker";

    // Extract instance ID from HTTP actor-type format used by AskReply and SendMessage.
    let instance_id = if http_actor_type.contains(':') {
        http_actor_type
            .split_once(':')
            .map(|(_actor_type_part, instance_id)| instance_id.to_string())
            .unwrap_or_else(|| ulid::Ulid::new().to_string())
    } else {
        ulid::Ulid::new().to_string()
    };

    // Build proper actor ID format
    let actor_id = canonical_actor_id(
        &instance_id,
        base_actor_type,
        namespace,
        "test-node-http-fmt",
    );

    // Verify format is correct (not //orbit-tracker::...)
    assert!(
        !actor_id.starts_with("//"),
        "Actor ID should not start with // - missing instance ID"
    );
    assert!(
        actor_id.contains(&instance_id),
        "Actor ID should contain instance ID"
    );
    assert_eq!(
        actor_id.to_string(),
        canonical_actor_id(
            &instance_id,
            base_actor_type,
            namespace,
            "test-node-http-fmt",
        )
        .to_string()
    );

    // Test activation via ActorService
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node-http-fmt".to_string(),
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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_reactivation_type_registered_only() {
    let (node, _db_dir) = build_test_node("test-node").await;
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
    let instance_id = "user-1";
    let actor_id = canonical_actor_id(instance_id, actor_type, namespace, "test-node");

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
    assert!(
        response.is_ok(),
        "Should activate virtual actor even if only type-registered: {:?}",
        response.as_ref().err().map(|s| s.message())
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
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_type_registration_reinstantiates_behavior_from_template() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";

    register_counter_behavior_with_initial_count(&service_locator, "counter-from-template").await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let actor_type = "counter-from-template";
    let namespace = "test-ns";
    let actor_id = canonical_actor_id("user-template", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

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
    let initial_count = ask_for_count(&ctx, actor_ref.as_ref()).await.unwrap();
    assert_eq!(
        initial_count, 7,
        "behavior should be rebuilt from init_config_template on first activation"
    );

    let incremented_count = increment_count(&ctx, actor_ref.as_ref()).await.unwrap();
    assert_eq!(
        incremented_count, 8,
        "live actor state should reflect mutations before explicit stop"
    );

    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();
    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_unregistration(&actor_registry, &actor_id).await;
    let stopped_metadata = virtual_actor_manager
        .get_metadata(&actor_id)
        .await
        .expect("instance metadata should persist across stop");
    // initial_state is now derived from spec.args via wasm_init_payload; verify args are preserved
    assert_eq!(
        stopped_metadata.spec.args.get("initial_count").map(|s| s.as_str()),
        Some("7"),
        "reactivation metadata should preserve the original init template args for non-durable actors"
    );
    let pending_messages = virtual_actor_manager
        .registry()
        .pending_activations()
        .read()
        .await;
    assert!(
        pending_messages.get(&actor_id).is_none(),
        "reactivation should not inherit stale pending messages from the previous live instance"
    );
    drop(pending_messages);

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
    let reactivated_count = ask_for_count(&ctx, reactivated_ref.as_ref()).await.unwrap();
    assert_eq!(
        reactivated_count, 7,
        "non-durable reactivation should rebuild from init_config_template instead of leaked in-memory state"
    );

    let instance_metadata = virtual_actor_manager.get_metadata(&actor_id).await.unwrap();
    assert_eq!(instance_metadata.actor_type(), actor_type);
    assert_eq!(instance_metadata.spec.namespace, namespace);
    assert_eq!(
        instance_metadata.activation_strategy(),
        ActivationStrategy::ActivationStrategyLazy
    );
}

/// Test: Virtual actor activation preserves state after suspension/reactivation
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_state_preservation() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let namespace = "test-ns";
    let actor_id = canonical_actor_id("user-1", actor_type, namespace, "test-node");

    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let virtual_facet = VirtualActorFacet::new(
        serde_json::json!({
            "idle_timeout": "1s",
            "activation_strategy": "lazy"
        }),
        100,
    );

    node.spawn(
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

    // Activate via invoke (direct tell on lazy actor sends to dead mailbox buffer)
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let activate_response = invoke_virtual_actor(
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
    assert!(activate_response.is_ok(), "initial activation must succeed");

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

#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_reactivation_recreates_timer_and_reminder_facets() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "counter-with-facets";
    let namespace = "test-ns";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let actor_id = canonical_actor_id("user-facets", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Type-level registration stores facet_config so activate_virtual_actor can recreate
    // timer and reminder facets on reactivation. init_config_template supplies initial_count.
    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,
        Some(&[
            plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                config: std::collections::HashMap::from([
                    ("idle_timeout".to_string(), "1s".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "timer".to_string(),
                config: std::collections::HashMap::from([(
                    "interval_ms".to_string(),
                    "250".to_string(),
                )]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "reminder".to_string(),
                config: std::collections::HashMap::from([(
                    "default_due_time".to_string(),
                    "100ms".to_string(),
                )]),
                ..Default::default()
            },
        ]),
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"initial_count":3}"#.to_vec()),
    )
    .await
    .expect("type registration must succeed");

    let actor_registry = service_locator.actor_registry().await.unwrap();
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));

    // Initial activation via invoke (correct path for virtual actors)
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
        "initial activation must succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;

    let facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        facet_types.contains(&"virtual_actor".to_string()),
        "virtual_actor facet missing before stop"
    );
    assert!(
        facet_types.contains(&"timer".to_string()),
        "timer facet missing before stop"
    );
    assert!(
        facet_types.contains(&"reminder".to_string()),
        "reminder facet missing before stop"
    );
    assert_has_concrete_timer_and_reminder_facets(&service_locator, &actor_id).await;

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
        &format!("{}:user-facets", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "reactivation with timer/reminder facets should succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    let reactivated_facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        reactivated_facet_types.contains(&"virtual_actor".to_string()),
        "virtual_actor missing after respawn"
    );
    assert!(
        reactivated_facet_types.contains(&"timer".to_string()),
        "timer missing after respawn"
    );
    assert!(
        reactivated_facet_types.contains(&"reminder".to_string()),
        "reminder missing after respawn"
    );
    assert_has_concrete_timer_and_reminder_facets(&service_locator, &actor_id).await;

    let actor_ref = actor_registry.lookup_actor(&actor_id).await.unwrap();
    let count = ask_for_count(&ctx, actor_ref.as_ref()).await.unwrap();
    assert_eq!(
        count, 3,
        "reactivation should rebuild from init_config_template"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_reactivation_restores_durable_state() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "durable-counter";
    let namespace = "test-ns";
    register_durable_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let actor_id = canonical_actor_id("cart-1", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,
        Some(&[
            plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                config: std::collections::HashMap::from([
                    ("idle_timeout".to_string(), "1s".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "durability".to_string(),
                config: std::collections::HashMap::from([
                    ("checkpoint_interval".to_string(), "1".to_string()),
                    ("replay_on_activation".to_string(), "true".to_string()),
                    ("state_schema_version".to_string(), "1".to_string()),
                ]),
                ..Default::default()
            },
        ]),
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"initial_count":0}"#.to_vec()),
    )
    .await
    .expect("type registration must succeed");

    let actor_registry = service_locator.actor_registry().await.unwrap();
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:cart-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "initial activation must succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    assert!(
        actor_registry.get_actor_instance(&actor_id).await.is_some(),
        "reactivated durable virtual actor should store a live actor instance for explicit stop"
    );
    let increment_one = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:cart-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("first increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_one), 1);
    let increment_two = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:cart-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("second increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_two), 2);

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
        &format!("{}:cart-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "reactivation must succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    assert!(
        actor_registry.get_actor_instance(&actor_id).await.is_some(),
        "reactivated durable virtual actor should store a live actor instance after reactivation"
    );
    let reactivated_facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        reactivated_facet_types.contains(&"virtual_actor".to_string()),
        "virtual_actor missing after durable reactivation"
    );
    assert!(
        reactivated_facet_types.contains(&"durability".to_string()),
        "durability missing after durable reactivation"
    );
    let reactivated = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:cart-1", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("reactivated count query should succeed");
    assert_eq!(
        count_from_invoke_response(&reactivated),
        2,
        "durable virtual actor should restore last checkpoint"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_reactivation_recreates_process_group_facet() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let actor_type = "channel-with-group";
    let namespace = "test-ns";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let actor_id = canonical_actor_id("alerts", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,
        Some(&[
            plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                config: std::collections::HashMap::from([
                    ("idle_timeout".to_string(), "1s".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "process_group".to_string(),
                config: std::collections::HashMap::from([(
                    "group".to_string(),
                    "abstractions-group".to_string(),
                )]),
                ..Default::default()
            },
        ]),
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"role":"channel","group":"abstractions-group"}"#.to_vec()),
    )
    .await
    .expect("type registration must succeed");

    let actor_registry = service_locator.actor_registry().await.unwrap();
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:alerts", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "initial activation must succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    let facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        facet_types.contains(&"virtual_actor".to_string()),
        "virtual_actor facet missing before stop"
    );
    assert!(
        facet_types.contains(&"process_group".to_string()),
        "process_group facet missing before stop"
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
        &format!("{}:alerts", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "reactivation with process_group facet should succeed: {:?}",
        response.err()
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;
    let reactivated_facet_types = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        reactivated_facet_types.contains(&"virtual_actor".to_string()),
        "virtual_actor facet missing after stop/reactivate"
    );
    assert!(
        reactivated_facet_types.contains(&"process_group".to_string()),
        "process_group facet missing after stop/reactivate"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_abstractions_example_runtime_send_primes_channel_definition_metadata() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-ns";
    let actor_type = "abstractions_wasm";

    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;

    for (instance_name, initial_count) in [("abstractions", 2u32), ("channel", 9u32)] {
        plexspaces_actor::register_virtual_actor_definition(
            &sl_dyn,
            plexspaces_proto::actor::v1::ActorSpawnSpec {
                identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: instance_name.to_string(),
                    actor_type: actor_type.to_string(),
                }),
                role: instance_name.to_string(),
                namespace: namespace.to_string(),
                tenant_id: tenant_id.to_string(),
                visibility: 0,
                behavior_kind: "GenServer".to_string(),
                args: std::collections::HashMap::from([(
                    "initial_count".to_string(),
                    initial_count.to_string(),
                )]),
                facets: vec![plexspaces_proto::common::v1::Facet {
                    r#type: "virtual_actor".to_string(),
                    config: std::collections::HashMap::from([
                        ("idle_timeout".to_string(), "5m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    ..Default::default()
                }],
                labels: std::collections::HashMap::new(),
                config: None,
            },
        )
        .await
        .expect("named virtual actor definition must register");
    }

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());
    let target_actor_id = canonical_actor_id("alerts", actor_type, namespace, "test-node");

    let send_result = actor_service
        .send_message(
            "channel:alerts",
            Message {
                id: ulid::Ulid::new().to_string(),
                message_type: "cast".to_string(),
                sender_id: canonical_actor_id("sender", "sender", namespace, "test-node")
                    .to_string(),
                receiver_id: "channel:alerts".to_string(),
                payload: serde_json::to_vec(&CounterMessage::Increment).unwrap(),
                ..Default::default()
            },
            Some(&ctx),
        )
        .await;

    assert!(
        send_result.is_ok(),
        "runtime send to named virtual actor should succeed: {:?}",
        send_result.err()
    );

    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &target_actor_id).await;

    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        "channel:alerts",
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("querying activated named virtual actor should succeed");

    assert_eq!(
        count_from_invoke_response(&response),
        10,
        "runtime send must preserve the channel declaration's init template from the abstractions example"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_abstractions_example_ephemeral_named_actor_reactivates_from_init_template() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-ns";
    let actor_type = "abstractions_wasm";

    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;

    for (instance_name, initial_count) in [("abstractions", 2u32), ("ephemeral", 5u32)] {
        plexspaces_actor::register_virtual_actor_definition(
            &sl_dyn,
            plexspaces_proto::actor::v1::ActorSpawnSpec {
                identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                    name: instance_name.to_string(),
                    actor_type: actor_type.to_string(),
                }),
                role: instance_name.to_string(),
                namespace: namespace.to_string(),
                tenant_id: tenant_id.to_string(),
                visibility: 0,
                behavior_kind: "GenServer".to_string(),
                args: std::collections::HashMap::from([(
                    "initial_count".to_string(),
                    initial_count.to_string(),
                )]),
                facets: vec![plexspaces_proto::common::v1::Facet {
                    r#type: "virtual_actor".to_string(),
                    config: std::collections::HashMap::from([
                        ("idle_timeout".to_string(), "5m".to_string()),
                        ("activation_strategy".to_string(), "lazy".to_string()),
                    ]),
                    ..Default::default()
                }],
                labels: std::collections::HashMap::new(),
                config: None,
            },
        )
        .await
        .expect("named virtual actor definition must register");
    }

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let actor_registry = service_locator.actor_registry().await.unwrap();
    let actor_id = canonical_actor_id("session-1", actor_type, namespace, "test-node");
    let actor_name = "ephemeral:session-1";
    let stop_ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    let initial = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("ephemeral named actor should activate");
    wait_for_actor_registration(&actor_registry, &actor_id).await;
    assert_eq!(
        count_from_invoke_response(&initial),
        5,
        "named ephemeral definition should seed first activation from its init template"
    );

    let increment_one = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("first increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_one), 6);

    let increment_two = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("second increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_two), 7);

    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&stop_ctx, &actor_id)
        .await
        .unwrap();
    wait_for_actor_unregistration(&actor_registry, &actor_id).await;

    let reactivated = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("ephemeral named actor should reactivate");
    assert_eq!(
        count_from_invoke_response(&reactivated),
        5,
        "named non-durable actors that share a behavior class must reactivate from their declaration template"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_workflow_behavior_reactivates_from_type_metadata() {
    let (node, _db_dir) = build_test_node("test-node").await;
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

    let actor_id = canonical_actor_id("execution-1", actor_type, namespace, "test-node");
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
    let count = ask_for_count(&stop_ctx, actor_ref.as_ref()).await.unwrap();
    assert_eq!(count, 11);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_durable_workflow_behavior_restores_checkpoint() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-workflows";
    let actor_type = "durable-workflow-probe";
    register_durable_workflow_probe_behavior(&service_locator, actor_type).await;

    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,
        Some(&[
            plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                config: std::collections::HashMap::from([
                    ("idle_timeout".to_string(), "1s".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "durability".to_string(),
                config: std::collections::HashMap::from([
                    ("checkpoint_interval".to_string(), "1".to_string()),
                    ("replay_on_activation".to_string(), "true".to_string()),
                    ("state_schema_version".to_string(), "1".to_string()),
                ]),
                ..Default::default()
            },
        ]),
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"initial_count":0}"#.to_vec()),
    )
    .await
    .expect("type registration must succeed");

    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let actor_registry = service_locator.actor_registry().await.unwrap();
    let actor_name = format!("{}:execution-1", actor_type);
    let actor_id = canonical_actor_id("execution-1", actor_type, namespace, "test-node");
    let stop_ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

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
    assert!(response.is_ok(), "workflow actor should activate");
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    let increment_one = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("first increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_one), 1);

    let increment_two = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::Increment).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("second increment should succeed");
    assert_eq!(count_from_invoke_response(&increment_two), 2);

    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&stop_ctx, &actor_id)
        .await
        .unwrap();

    let reactivated = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &actor_name,
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await
    .expect("reactivated workflow count query should succeed");
    assert_eq!(
        count_from_invoke_response(&reactivated),
        2,
        "durable workflow virtual actor should restore last checkpoint"
    );
}

// ============================================================================
// Test: type-level virtual actor registration is NOT evicted on actor vacation
// ============================================================================

/// Asserts that `virtual_actor_types` (type-level registry) is never cleared when an individual
/// actor instance is deactivated (vacationed / LRU evicted). Only application undeploy should
/// remove a type registration.
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_type_not_evicted_on_vacation() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-ns-vac";
    let actor_type = "counter-vacation";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();

    // Register type with both virtual_actor and timer facet configs
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
                "timer": {
                    "interval_ms": 1000
                }
            }),
            Some(tenant_id.to_string()),
            Some(br#"{"initial_count":0}"#.to_vec()),
        )
        .await
        .unwrap();

    let actor_id = canonical_actor_id("vacation-test", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Activate the first instance via HTTP invoke (correct way to start a lazy virtual actor).
    // invoke_virtual_actor triggers activate_virtual_actor which rebuilds from type-level metadata.
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:vacation-test", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "initial activation should succeed: {:?}",
        response.err()
    );

    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Stop the actor (simulates vacation / deactivation)
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    // CRITICAL INVARIANT: type registration must persist after actor vacation
    let meta = virtual_actor_manager
        .get_virtual_actor_type(actor_type)
        .await;
    assert!(
        meta.is_some(),
        "virtual actor type '{}' must NOT be evicted when instance is deactivated",
        actor_type
    );
    let meta = meta.unwrap();
    let fc = meta.facet_config().expect("facet_config must be present");
    assert!(
        fc.get("virtual_actor").is_some(),
        "virtual_actor facet config must be preserved in type metadata"
    );
    assert!(
        fc.get("timer").is_some(),
        "timer facet config must be preserved in type metadata"
    );
    assert_eq!(
        fc["virtual_actor"]["activation_strategy"].as_str().unwrap(),
        "lazy",
        "activation_strategy must be preserved"
    );

    // Verify actor can be resurrected using the persisted type registration
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:vacation-test", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "actor should be resurrectable from persisted type registration"
    );

    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Type registration still present after resurrection
    assert!(
        virtual_actor_manager
            .get_virtual_actor_type(actor_type)
            .await
            .is_some(),
        "type registration must still be present after resurrection"
    );
}

// ============================================================================
// Test: spawn → stop → respawn preserves all facets (virtual + timer + reminder)
// ============================================================================

/// Verifies that when a virtual actor is stopped and then reactivated via HTTP invoke,
/// all declared facets (virtual_actor, timer, reminder) are recreated on the new instance.
#[tokio::test(flavor = "multi_thread")]
async fn test_virtual_actor_stop_respawn_all_facets_preserved() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let tenant_id = "test-tenant";
    let namespace = "test-ns-respawn";
    let actor_type = "counter-respawn";
    register_counter_behavior_with_initial_count(&service_locator, actor_type).await;

    let actor_id = canonical_actor_id("respawn-test", actor_type, namespace, "test-node");
    let ctx = RequestContext::new_without_auth(tenant_id.to_string(), namespace.to_string());

    // Register the actor type with ALL three facet configs at the type level.
    // This is required for activate_virtual_actor to recreate timer+reminder from stored config
    // (the else-if facet_config branch in activate_virtual_actor). instance-level spawn()
    // does NOT store facet_config, so we must use the type-level registration path.
    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,
        Some(&[
            plexspaces_proto::common::v1::Facet {
                r#type: "virtual_actor".to_string(),
                config: std::collections::HashMap::from([
                    ("idle_timeout".to_string(), "5m".to_string()),
                    ("activation_strategy".to_string(), "lazy".to_string()),
                ]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "timer".to_string(),
                config: std::collections::HashMap::from([(
                    "interval_ms".to_string(),
                    "250".to_string(),
                )]),
                ..Default::default()
            },
            plexspaces_proto::common::v1::Facet {
                r#type: "reminder".to_string(),
                config: std::collections::HashMap::from([(
                    "default_due_time".to_string(),
                    "100ms".to_string(),
                )]),
                ..Default::default()
            },
        ]),
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"initial_count":5}"#.to_vec()),
    )
    .await
    .expect("type registration must succeed");

    // Activate first instance via HTTP invoke — triggers activate_virtual_actor which reads
    // the type-level facet_config and recreates all three facets using the FacetRegistry.
    let actor_service = Arc::new(ActorServiceImpl::new(
        service_locator.clone(),
        "test-node".to_string(),
    ));
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:respawn-test", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(
        response.is_ok(),
        "initial activation must succeed: {:?}",
        response.err()
    );

    let actor_registry = service_locator.actor_registry().await.unwrap();
    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Assert all 3 facets present before stop
    let pre_stop_facets = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        pre_stop_facets.contains(&"virtual_actor".to_string()),
        "must have virtual_actor before stop"
    );
    assert!(
        pre_stop_facets.contains(&"timer".to_string()),
        "must have timer before stop"
    );
    assert!(
        pre_stop_facets.contains(&"reminder".to_string()),
        "must have reminder before stop"
    );

    // Stop the actor (vacation)
    service_locator
        .get_actor_factory()
        .await
        .unwrap()
        .stop_actor(&ctx, &actor_id)
        .await
        .unwrap();

    // Resurrect via HTTP invoke (same path as WASM/production resurrection)
    let response = invoke_virtual_actor(
        &actor_service,
        tenant_id,
        namespace,
        &format!("{}:respawn-test", actor_type),
        "POST",
        serde_json::to_vec(&CounterMessage::GetCount).unwrap(),
        HashMap::new(),
        true,
    )
    .await;
    assert!(response.is_ok(), "actor must be resurrectable after stop");

    wait_for_actor_registration(&actor_registry, &actor_id).await;

    // Assert all 3 facets present after resurrection — critical invariant
    let post_respawn_facets = current_facet_types(&service_locator, &actor_id).await;
    assert!(
        post_respawn_facets.contains(&"virtual_actor".to_string()),
        "virtual_actor facet must be recreated after resurrection"
    );
    assert!(
        post_respawn_facets.contains(&"timer".to_string()),
        "timer facet must be recreated after resurrection"
    );
    assert!(
        post_respawn_facets.contains(&"reminder".to_string()),
        "reminder facet must be recreated after resurrection"
    );

    assert_has_concrete_timer_and_reminder_facets(&service_locator, &actor_id).await;
}

// ============================================================================
// Test: WASM app-deployment simulation — all facet configs propagated to type metadata
// ============================================================================

/// Simulates the wasm_application.rs registration path by constructing proto facets
/// matching what app-config.toml produces, then verifying all configs are stored in
/// VirtualActorMetadata.facet_config for correct resurrection.
#[tokio::test(flavor = "multi_thread")]
async fn test_wasm_deployment_virtual_timer_facet_config_propagation() {
    let (node, _db_dir) = build_test_node("test-node").await;
    let service_locator = node.service_locator();
    let actor_type = "wasm-sim-actor";
    let namespace = "test-ns-wasm";
    let tenant_id = "test-tenant";

    // Simulate the proto facets that wasm_application.rs would extract from app-config.toml
    use plexspaces_proto::common::v1::Facet as ProtoFacet;
    let proto_facets = vec![
        ProtoFacet {
            r#type: "virtual_actor".to_string(),
            config: std::collections::HashMap::from([
                ("idle_timeout".to_string(), "8m".to_string()),
                ("activation_strategy".to_string(), "lazy".to_string()),
            ]),
            ..Default::default()
        },
        ProtoFacet {
            r#type: "timer".to_string(),
            config: std::collections::HashMap::from([(
                "interval_ms".to_string(),
                "500".to_string(),
            )]),
            ..Default::default()
        },
        ProtoFacet {
            r#type: "reminder".to_string(),
            config: std::collections::HashMap::from([(
                "default_due_time".to_string(),
                "200ms".to_string(),
            )]),
            ..Default::default()
        },
    ];

    // Call the same helper used by wasm_application.rs.
    // Coerce ServiceLocatorImpl → Arc<dyn ServiceLocator> as the function expects.
    let sl_dyn: Arc<dyn plexspaces_actor::ServiceLocator> =
        service_locator.clone() as Arc<dyn plexspaces_actor::ServiceLocator>;
    let result = plexspaces_actor::register_virtual_actor_type_consistent(
        &sl_dyn,
        actor_type.to_string(),
        String::new(), // instance_name — empty when name == type (standalone virtual actors)
        namespace.to_string(),
        None,                // No trait-object facets (WASM path)
        Some(&proto_facets), // Proto facets from app-config.toml
        None,
        Some(tenant_id.to_string()),
        Some(br#"{"initial_count":0}"#.to_vec()),
    )
    .await;
    assert!(result.is_ok(), "registration must succeed: {:?}", result);

    let virtual_actor_manager = service_locator.virtual_actor_manager().await.unwrap();
    let meta = virtual_actor_manager
        .get_virtual_actor_type(actor_type)
        .await
        .expect("actor type must be registered");

    let fc = meta
        .facet_config()
        .expect("facet_config must be present after registration");

    // virtual_actor facet config
    assert!(
        fc.get("virtual_actor").is_some(),
        "virtual_actor config must be stored"
    );
    assert_eq!(
        fc["virtual_actor"]["idle_timeout"].as_str().unwrap(),
        "8m",
        "idle_timeout must be propagated from proto facet config"
    );
    assert_eq!(
        fc["virtual_actor"]["activation_strategy"].as_str().unwrap(),
        "lazy",
        "activation_strategy must be propagated"
    );

    // timer facet config — proto_config_to_value parses "500" as JSON number, not string
    assert!(fc.get("timer").is_some(), "timer config must be stored");
    let timer_interval_val = &fc["timer"]["interval_ms"];
    // Accept either JSON number 500 or string "500" (both valid serializations of the config)
    let timer_interval_ok =
        timer_interval_val.as_u64() == Some(500) || timer_interval_val.as_str() == Some("500");
    assert!(
        timer_interval_ok,
        "timer interval_ms must be 500 (got {:?})",
        timer_interval_val
    );

    // reminder facet config
    assert!(
        fc.get("reminder").is_some(),
        "reminder config must be stored"
    );
    assert_eq!(
        fc["reminder"]["default_due_time"].as_str().unwrap_or(""),
        "200ms",
        "reminder default_due_time must be propagated"
    );

    // Verify activation_strategy is parsed correctly from metadata
    assert_eq!(
        meta.activation_strategy(),
        ActivationStrategy::ActivationStrategyLazy,
        "activation_strategy in VirtualActorMetadata must be Lazy"
    );
}
