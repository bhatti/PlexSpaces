// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for temporary sender pattern in ActorRef::ask()
// Tests all scenarios: outside sender, local actor, remote actor, chained asks

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{
    Actor, ActorContext, ActorId, ActorRegistry, ActorService, BehaviorError, BehaviorType,
    Message, MessageSender,
};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::NodeBuilder;
use plexspaces_proto::actor::v1::{actor_service_server::ActorServiceServer, ActorVisibility};
use plexspaces_services::actor_service::ActorServiceImpl;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};
use ulid::Ulid;

/// Helper to create a test message
fn create_test_message(payload: Vec<u8>) -> Message {
    Message {
        id: Ulid::new().to_string(),
        payload,
        ..Default::default()
    }
}

fn genserver_actor_id(name: &str, node_id: &str, namespace: &str) -> ActorId {
    ActorId::new(
        name.to_string(),
        "gen_server".to_string(),
        namespace.to_string(),
        node_id.to_string(),
    )
    .expect("test actor id should be valid")
}

fn actor_id_from_message_receiver(receiver_id: &str) -> ActorId {
    ActorId::from_canonical(receiver_id).expect("request receiver_id should be canonical")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum CounterMessage {
    Increment,
    Get,
    Value(i32),
}

/// Simple counter actor that responds to increment/get requests
struct CounterActor {
    value: i32,
}

impl CounterActor {
    fn new() -> Self {
        Self { value: 0 }
    }
}

#[async_trait::async_trait]
impl Actor for CounterActor {
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

#[async_trait::async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let request: CounterMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        match request {
            CounterMessage::Increment => {
                self.value += 1;
                info!("Counter incremented to: {}", self.value);

                // Send reply using ctx.send_reply()
                if !msg.sender_id.is_empty() {
                    let reply = CounterMessage::Value(self.value);
                    let reply_msg = create_test_message(serde_json::to_vec(&reply).unwrap());

                    ctx.send_reply(
                        if msg.correlation_id.is_empty() {
                            None
                        } else {
                            Some(msg.correlation_id.as_str())
                        },
                        &msg.sender_id,
                        actor_id_from_message_receiver(&msg.receiver_id),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
                Ok(())
            }
            CounterMessage::Get => {
                info!("Counter get request, value: {}", self.value);

                // Send reply using ctx.send_reply()
                if !msg.sender_id.is_empty() {
                    let reply = CounterMessage::Value(self.value);
                    let reply_msg = create_test_message(serde_json::to_vec(&reply).unwrap());

                    ctx.send_reply(
                        if msg.correlation_id.is_empty() {
                            None
                        } else {
                            Some(msg.correlation_id.as_str())
                        },
                        &msg.sender_id,
                        actor_id_from_message_receiver(&msg.receiver_id),
                        reply_msg,
                    )
                    .await
                    .map_err(|e| {
                        BehaviorError::ProcessingError(format!("Failed to send reply: {}", e))
                    })?;
                }
                Ok(())
            }
            _ => Err(BehaviorError::ProcessingError(
                "Invalid message".to_string(),
            )),
        }
    }
}

// ========================================================================
// Helper Functions for Multi-Node Testing
// ========================================================================

/// Helper to create a test ActorRegistry with a node registration
async fn create_test_registry_with_node(
    local_node_id: &str,
    node_id: &str,
    node_address: &str,
) -> Arc<ActorRegistry> {
    use async_trait::async_trait;
    use plexspaces_core::actor_context::ObjectRegistry as ObjectRegistryTrait;
    use plexspaces_object_registry::ObjectRegistry;
    use plexspaces_object_registry::SqliteObjectRegistryRepository;
    use plexspaces_proto::object_registry::v1::{
        ObjectRegistration as ProtoObjectRegistration, ObjectRegistration, ObjectType,
    };

    // Simple wrapper to adapt ObjectRegistry to ObjectRegistryTrait
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistry>,
    }

    #[async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_id: &str,
            object_type: Option<ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(ObjectType::ObjectTypeUnspecified);
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
            object_type: ObjectType,
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
            ctx: &plexspaces_core::RequestContext,
            object_type: Option<ObjectType>,
            object_category: Option<String>,
            capabilities: Option<Vec<String>>,
            labels: Option<Vec<String>>,
            health_status: Option<plexspaces_proto::object_registry::v1::HealthStatus>,
            limit: usize,
            offset: usize,
        ) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .discover(
                    ctx,
                    object_type,
                    object_category,
                    capabilities,
                    labels,
                    health_status,
                    limit,
                    offset,
                )
                .await
                .map_err(|e| {
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        e.to_string(),
                    )) as Box<dyn std::error::Error + Send + Sync>
                })
        }

        async fn unregister(
            &self,
            ctx: &plexspaces_core::RequestContext,
            object_type: ObjectType,
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
            object_type: ObjectType,
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

    let object_repo = Arc::new(
        SqliteObjectRegistryRepository::new(":memory:")
            .await
            .unwrap(),
    );
    let object_registry_impl = Arc::new(ObjectRegistry::new(object_repo));

    // Register node as a service object (nodes are registered as services)
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    // Nodes are registered with object_id = node_id (no "_node@" prefix)
    let node_object_id = node_id.to_string();
    let registration = ProtoObjectRegistration {
        object_id: node_object_id.clone(),
        object_type: ObjectType::ObjectTypeNode as i32,
        object_category: "node".to_string(),
        grpc_address: node_address.to_string(),
        ..Default::default()
    };

    object_registry_impl
        .register(&ctx, registration)
        .await
        .unwrap();

    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });
    Arc::new(ActorRegistry::new(
        object_registry,
        local_node_id.to_string(),
    ))
}

/// Helper to create ActorServiceImpl with proper ServiceLocator setup
async fn create_test_actor_service(
    _actor_registry: Arc<ActorRegistry>,
    node_id: String,
) -> ActorServiceImpl {
    use plexspaces_node::create_default_service_locator;
    // create_default_service_locator initializes all services including:
    // - ActorRegistry, VirtualActorManager, ReplyWaiterRegistry
    let service_locator = create_default_service_locator(Some(node_id.clone()), None).await;
    ActorServiceImpl::new(service_locator, node_id)
}

/// Helper to register an actor with ActorRegistry
async fn register_test_actor(
    actor_registry: Arc<ActorRegistry>,
    actor_id: ActorId,
    mailbox: Arc<Mailbox>,
    service_locator: Arc<dyn plexspaces_core::ServiceLocator>,
) {
    let sender: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
        actor_id.clone(),
        "test".to_string(),   // tenant_id
        "system".to_string(), // namespace
        mailbox,
        service_locator,
        ActorVisibility::ActorVisibilityPublic,
    ));
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    actor_registry
        .register_actor(
            &ctx,
            actor_id,
            sender,
            "test_actor".to_string(),
            None,
            None,
            None,
        )
        .await;
}

/// Helper to start a test gRPC server
async fn start_test_server(
    service: ActorServiceImpl,
    port: u16,
) -> tokio::task::JoinHandle<Result<(), tonic::transport::Error>> {
    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{}", port).parse().unwrap();
        tonic::transport::Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve(addr)
            .await
    })
}

/// Actor that forwards ask() calls to another actor (for chained ask tests)
struct ForwarderActor {
    target_actor_id: String,
}

impl ForwarderActor {
    fn new(target_actor_id: String) -> Self {
        Self { target_actor_id }
    }
}

#[async_trait::async_trait]
impl Actor for ForwarderActor {
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

#[async_trait::async_trait]
impl GenServer for ForwarderActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Forward the request to target actor
        let node_id = self
            .target_actor_id
            .split('@')
            .nth(1)
            .unwrap_or("unknown")
            .to_string();
        let target_ref = ActorRef::remote(
            self.target_actor_id.clone(),
            ctx.tenant_id.clone(), // tenant_id
            ctx.namespace.clone(), // namespace
            node_id,
            ctx.service_locator.clone(),
            ActorVisibility::ActorVisibilityPublic,
        );

        let request: CounterMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        // Forward request to target actor using ask()
        // Get forwarder's own actor ID from context
        let forwarder_id = ctx
            .self_ref()
            .map(|r| r.id().to_string())
            .unwrap_or_else(|| msg.receiver_id.clone());
        let mut forward_msg = create_test_message(serde_json::to_vec(&request).unwrap());
        forward_msg.receiver_id = self.target_actor_id.clone();
        forward_msg.sender_id = forwarder_id; // Use forwarder's own ID as sender
        forward_msg.message_type = "call".to_string();

        let routing_ctx = plexspaces_core::RequestContext::new_without_auth(
            ctx.tenant_id.clone(),
            ctx.namespace.clone(),
        );
        let reply = target_ref
            .ask(&routing_ctx, forward_msg, Duration::from_secs(5))
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("Forward ask failed: {}", e)))?;

        // Forward the reply back to original sender
        if !msg.sender_id.is_empty() {
            ctx.send_reply(
                if msg.correlation_id.is_empty() {
                    None
                } else {
                    Some(msg.correlation_id.as_str())
                },
                &msg.sender_id,
                actor_id_from_message_receiver(&msg.receiver_id),
                reply,
            )
            .await
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }

        Ok(())
    }
}

#[tokio::test]
async fn test_outside_sender_calling_ask() {
    // Test: Outside sender (not an actor) calling ask() on a local actor
    // Expected: Temporary sender ID should be created and used

    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // Create node
    let node = NodeBuilder::new("test-node-outside-ask")
        .with_in_memory_backends()
        .build()
        .await;

    // Create and spawn counter actor using ActorBuilder (simpler setup)
    let actor_id = genserver_actor_id("counter-1", "test-node-outside-ask", "default");
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let counter_ref = ActorBuilder::new(behavior)
        .with_id(actor_id.to_string())
        .with_namespace("default".to_string())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .unwrap();

    // Call ask() from outside - should use temporary sender
    let request = CounterMessage::Get;
    let mut msg = create_test_message(serde_json::to_vec(&request).unwrap());
    msg.message_type = "call".to_string();
    msg.receiver_id = actor_id.to_string();
    // No sender set (outside caller) - temporary sender will be created

    let reply = counter_ref
        .ask(&ctx, msg, Duration::from_secs(5))
        .await;

    assert!(
        reply.is_ok(),
        "ask() should succeed, got error: {:?}",
        reply.as_ref().err()
    );
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(&reply_msg.payload).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Initial counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Outside sender calling ask - PASSED");
}

#[tokio::test]
async fn test_local_actor_calling_ask_of_local_actor() {
    // Test: Local actor calling ask() on another local actor
    // Expected: Temporary sender is always created for ask(), but sender ID is set to actor's own ID

    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // Create node
    let node = NodeBuilder::new("test-node-local-ask")
        .with_in_memory_backends()
        .build()
        .await;

    // Create and spawn two counter actors using ActorBuilder
    let actor1_id = genserver_actor_id("counter-1", "test-node-local-ask", "default");
    let actor2_id = genserver_actor_id("counter-2", "test-node-local-ask", "default");

    let behavior1: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    let behavior2: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());

    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "internal".to_string(),
        "system".to_string(),
    );
    let _counter1_ref = ActorBuilder::new(behavior1)
        .with_id(actor1_id.to_string())
        .with_namespace("default".to_string())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .unwrap();

    let counter2_ref = ActorBuilder::new(behavior2)
        .with_id(actor2_id.to_string())
        .with_namespace("default".to_string())
        .spawn(&ctx, node.service_locator().clone())
        .await
        .unwrap();

    // Simulate counter1 calling ask() on counter2
    // Set sender to counter1's ID
    let request = CounterMessage::Get;
    let mut msg = create_test_message(serde_json::to_vec(&request).unwrap());
    msg.message_type = "call".to_string();
    msg.receiver_id = actor2_id.to_string();
    msg.sender_id = actor1_id.to_string(); // Actor's own ID as sender

    let reply = counter2_ref
        .ask(&ctx, msg, Duration::from_secs(5))
        .await;

    assert!(reply.is_ok(), "ask() should succeed");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(&reply_msg.payload).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Initial counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Local actor calling ask of local actor - PASSED");
}

/// Test: Local actor calling ask() on remote actor (simulated)
///
/// Scenario:
/// 1. Create node1 (local)
/// 2. Register counter actor on node1 with local_node_id (simulates node2)
/// 3. Actor on node1 calls ask() on counter using Remote ActorRef
/// 4. Verify reply received correctly via "local via remote" path
#[tokio::test]
async fn test_local_actor_calling_ask_of_remote_actor() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // ARRANGE: Create node1 (local) - reuse same pattern as test_ask_with_simulated_remote
    let node1 = Arc::new(
        NodeBuilder::new("node1")
            .with_in_memory_backends()
            .build()
            .await,
    );
    let node1_service_locator = node1.service_locator().clone();

    // Get node1's ActorRegistry
    use plexspaces_core::service_names;
    use plexspaces_core::MessageSender;
    let actor_registry1: Arc<ActorRegistry> = node1_service_locator
        .actor_registry()
        .await
        .expect("ActorRegistry should be registered");

    // Create counter actor on node1 with node1's local_node_id (so "local via remote" path is used)
    // When we use ActorRef::remote with node_id matching local_node_id, tell_impl will
    // detect it's actually local and use the "local via remote" path, routing to the local actor.
    // This simulates remote behavior (using Remote ActorRef) without needing real gRPC.
    let local_node_id = actor_registry1.local_node_id();
    let counter_id = genserver_actor_id("counter", &local_node_id, "default");
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox_counter = Arc::new(
        Mailbox::new(mailbox_config, counter_id.to_string())
            .await
            .unwrap(),
    );
    let sender_counter: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
        counter_id.clone(),
        "test".to_string(),    // tenant_id
        "default".to_string(), // namespace
        Arc::clone(&mailbox_counter),
        node1_service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry1
        .register_actor(
            &ctx,
            counter_id.clone(),
            sender_counter,
            "test_actor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Spawn task to handle messages and reply via ActorRegistry (simpler and more robust)
    let mailbox_counter_clone = mailbox_counter.clone();
    let actor_registry1_clone = actor_registry1.clone();
    let counter_id_for_spawn = counter_id.to_string();
    let reply_ctx = ctx.clone();
    let mut counter_actor = CounterActor::new();
    tokio::spawn(async move {
        while let Some(msg) = mailbox_counter_clone.dequeue().await {
            if let Ok(request) = serde_json::from_slice::<CounterMessage>(&msg.payload) {
                let reply = match request {
                    CounterMessage::Get => CounterMessage::Value(counter_actor.value),
                    CounterMessage::Increment => {
                        counter_actor.value += 1;
                        CounterMessage::Value(counter_actor.value)
                    }
                    _ => continue,
                };
                // Send reply using ActorRegistry to get temporary sender's ActorRef and call tell()
                // This routes correctly to ReplyWaiter for ask() pattern
                // Note: msg is plexspaces_mailbox::Message from dequeue(), use sender_id() method
                if !msg.sender_id.is_empty() {
                    let sender = &msg.sender_id;
                    if !sender.is_empty() {
                        let mut reply_msg =
                            create_test_message(serde_json::to_vec(&reply).unwrap());
                        reply_msg.receiver_id = sender.to_string();
                        reply_msg.sender_id = counter_id_for_spawn.clone();
                        if !msg.correlation_id.is_empty() {
                            reply_msg.correlation_id = msg.correlation_id.clone();
                        }
                        // Use ActorRegistry to get temporary sender's ActorRef and call tell() directly
                        // This ensures proper routing to ReplyWaiter
                        if let Ok(sender_id) = ActorId::from_canonical(sender) {
                            if let Some(sender_ref) =
                                actor_registry1_clone.lookup_actor(&sender_id).await
                            {
                                let _ = sender_ref.tell(&reply_ctx, reply_msg).await;
                            }
                        }
                    }
                }
            }
        }
    });

    // Create forwarder actor on node1 using ActorBuilder::spawn()
    // Forwarder will use Remote ActorRef pointing to counter@local_node_id
    let forwarder_behavior: Box<dyn plexspaces_core::Actor> =
        Box::new(ForwarderActor::new(counter_id.to_string()));
    let forwarder_id = genserver_actor_id("forwarder", "node1", "default");
    let ctx_spawn = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );

    let forwarder_ref = ActorBuilder::new(forwarder_behavior)
        .with_id(forwarder_id.to_string())
        .with_namespace("default".to_string())
        .spawn(&ctx_spawn, node1_service_locator.clone())
        .await
        .unwrap();

    // ACT: Call ask() on forwarder, which will forward to counter using Remote ActorRef
    let request = CounterMessage::Get;
    let mut msg = create_test_message(serde_json::to_vec(&request).unwrap());
    msg.message_type = "call".to_string();
    msg.receiver_id = forwarder_id.to_string();
    // No sender set (outside caller) - temporary sender will be created

    let reply = forwarder_ref
        .ask(&ctx_spawn, msg, Duration::from_secs(10))
        .await;

    // ASSERT: Should receive reply from counter via forwarder
    assert!(reply.is_ok(), "ask() should succeed");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(&reply_msg.payload).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Local actor calling ask of remote actor - PASSED");
}

/// Test: Chained asks (outside -> actor1@node1 -> actor2@node2) (simulated)
///
/// Scenario:
/// 1. Create node1 (local)
/// 2. Register actor1 (forwarder) on node1, actor2 (counter) on node1 with local_node_id (simulates node2)
/// 3. Outside caller calls ask() on actor1@node1
/// 4. actor1 calls ask() on actor2 using Remote ActorRef
/// 5. actor2 replies to actor1, actor1 replies to outside caller
/// 6. Verify both replies received correctly via "local via remote" path
#[tokio::test]
async fn test_chained_asks_multi_node() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // ARRANGE: Create node1 (local) - reuse same pattern as test_ask_with_simulated_remote
    let node1 = Arc::new(
        NodeBuilder::new("node1")
            .with_in_memory_backends()
            .build()
            .await,
    );
    let node1_service_locator = node1.service_locator().clone();

    // Get node1's ActorRegistry
    use plexspaces_core::service_names;
    use plexspaces_core::MessageSender;
    let actor_registry1: Arc<ActorRegistry> = node1_service_locator
        .actor_registry()
        .await
        .expect("ActorRegistry should be registered");

    // Create counter actor on node1 with node1's local_node_id (so "local via remote" path is used)
    let local_node_id = actor_registry1.local_node_id();
    let counter_id = genserver_actor_id("counter", &local_node_id, "default");
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox_counter = Arc::new(
        Mailbox::new(mailbox_config, counter_id.to_string())
            .await
            .unwrap(),
    );
    let sender_counter: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
        counter_id.clone(),
        "test".to_string(),    // tenant_id
        "default".to_string(), // namespace
        Arc::clone(&mailbox_counter),
        node1_service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry1
        .register_actor(
            &ctx,
            counter_id.clone(),
            sender_counter,
            "test_actor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Spawn task to handle messages and reply via ActorRegistry (simpler and more robust)
    let mailbox_counter_clone = mailbox_counter.clone();
    let actor_registry1_clone = actor_registry1.clone();
    let counter_id_for_spawn = counter_id.to_string();
    let reply_ctx = ctx.clone();
    let mut counter_actor = CounterActor::new();
    tokio::spawn(async move {
        while let Some(msg) = mailbox_counter_clone.dequeue().await {
            if let Ok(request) = serde_json::from_slice::<CounterMessage>(&msg.payload) {
                let reply = match request {
                    CounterMessage::Get => CounterMessage::Value(counter_actor.value),
                    CounterMessage::Increment => {
                        counter_actor.value += 1;
                        CounterMessage::Value(counter_actor.value)
                    }
                    _ => continue,
                };
                // Send reply using ActorRegistry to get temporary sender's ActorRef and call tell()
                // This routes correctly to ReplyWaiter for ask() pattern
                // Note: msg is plexspaces_mailbox::Message from dequeue(), use sender_id() method
                if !msg.sender_id.is_empty() {
                    let sender = &msg.sender_id;
                    if !sender.is_empty() {
                        let mut reply_msg =
                            create_test_message(serde_json::to_vec(&reply).unwrap());
                        reply_msg.receiver_id = sender.to_string();
                        reply_msg.sender_id = counter_id_for_spawn.clone();
                        if !msg.correlation_id.is_empty() {
                            reply_msg.correlation_id = msg.correlation_id.clone();
                        }
                        // Use ActorRegistry to get temporary sender's ActorRef and call tell() directly
                        // This ensures proper routing to ReplyWaiter
                        if let Ok(sender_id) = ActorId::from_canonical(sender) {
                            if let Some(sender_ref) =
                                actor_registry1_clone.lookup_actor(&sender_id).await
                            {
                                let _ = sender_ref.tell(&reply_ctx, reply_msg).await;
                            }
                        }
                    }
                }
            }
        }
    });

    // Create forwarder actor on node1 using ActorBuilder::spawn()
    // Forwarder will use Remote ActorRef pointing to counter@local_node_id
    let forwarder_behavior: Box<dyn plexspaces_core::Actor> =
        Box::new(ForwarderActor::new(counter_id.to_string()));
    let forwarder_id = genserver_actor_id("forwarder", "node1", "default");
    let ctx_spawn = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );

    let forwarder_ref = ActorBuilder::new(forwarder_behavior)
        .with_id(forwarder_id.to_string())
        .with_namespace("default".to_string())
        .spawn(&ctx_spawn, node1_service_locator.clone())
        .await
        .unwrap();

    // ACT: Outside caller calls ask() on forwarder@node1, which forwards to counter using Remote ActorRef
    let request = CounterMessage::Increment;
    let mut msg = create_test_message(serde_json::to_vec(&request).unwrap());
    msg.message_type = "call".to_string();
    msg.receiver_id = forwarder_id.to_string();
    // No sender set (outside caller) - temporary sender will be created

    let reply = forwarder_ref
        .ask(&ctx_spawn, msg, Duration::from_secs(10))
        .await;

    // ASSERT: Should receive reply from counter via forwarder
    assert!(reply.is_ok(), "Chained ask() should succeed");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(&reply_msg.payload).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 1, "Counter should be incremented to 1");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Chained asks multi-node - PASSED");
}

/// Test: Concurrent ask() calls across nodes (simulated)
///
/// Scenario:
/// 1. Create node1 (local)
/// 2. Register counter actor on node1 with local_node_id (simulates node2)
/// 3. Spawn 10 concurrent ask() calls from node1 to counter using Remote ActorRef
/// 4. Verify all replies received correctly via "local via remote" path
#[tokio::test]
async fn test_concurrent_asks_multi_node() {
    let _ = tracing_subscriber::fmt().with_env_filter("warn").try_init();

    // ARRANGE: Create node1 (local) - reuse same pattern as test_ask_with_simulated_remote
    let node1 = Arc::new(
        NodeBuilder::new("node1")
            .with_in_memory_backends()
            .build()
            .await,
    );
    let node1_service_locator = node1.service_locator().clone();

    // Get node1's ActorRegistry
    use plexspaces_core::service_names;
    use plexspaces_core::MessageSender;
    let actor_registry1: Arc<ActorRegistry> = node1_service_locator
        .actor_registry()
        .await
        .expect("ActorRegistry should be registered");

    // Create counter actor on node1 with node1's local_node_id (so "local via remote" path is used)
    let local_node_id = actor_registry1.local_node_id();
    let counter_id = genserver_actor_id("counter", &local_node_id, "default");
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox_counter = Arc::new(
        Mailbox::new(mailbox_config, counter_id.to_string())
            .await
            .unwrap(),
    );
    let sender_counter: Arc<dyn MessageSender> = Arc::new(ActorRef::local(
        counter_id.clone(),
        "test".to_string(),    // tenant_id
        "default".to_string(), // namespace
        Arc::clone(&mailbox_counter),
        node1_service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    ));
    let ctx = plexspaces_core::RequestContext::new_without_auth(
        "default".to_string(),
        "default".to_string(),
    );
    actor_registry1
        .register_actor(
            &ctx,
            counter_id.clone(),
            sender_counter,
            "test_actor".to_string(),
            None,
            None,
            None,
        )
        .await;

    // Spawn task to handle messages and reply via ActorRegistry (simpler and more robust)
    let mailbox_counter_clone = mailbox_counter.clone();
    let actor_registry1_clone = actor_registry1.clone();
    let counter_id_for_spawn = counter_id.to_string();
    let reply_ctx = ctx.clone();
    let mut counter_actor = CounterActor::new();
    tokio::spawn(async move {
        while let Some(msg) = mailbox_counter_clone.dequeue().await {
            if let Ok(request) = serde_json::from_slice::<CounterMessage>(&msg.payload) {
                let reply = match request {
                    CounterMessage::Get => CounterMessage::Value(counter_actor.value),
                    CounterMessage::Increment => {
                        counter_actor.value += 1;
                        CounterMessage::Value(counter_actor.value)
                    }
                    _ => continue,
                };
                // Send reply using ActorRegistry to get temporary sender's ActorRef and call tell()
                // This routes correctly to ReplyWaiter for ask() pattern
                // Note: msg is plexspaces_mailbox::Message from dequeue(), use sender_id() method
                if !msg.sender_id.is_empty() {
                    let sender = &msg.sender_id;
                    if !sender.is_empty() {
                        let mut reply_msg =
                            create_test_message(serde_json::to_vec(&reply).unwrap());
                        reply_msg.receiver_id = sender.to_string();
                        reply_msg.sender_id = counter_id_for_spawn.clone();
                        if !msg.correlation_id.is_empty() {
                            reply_msg.correlation_id = msg.correlation_id.clone();
                        }
                        // Use ActorRegistry to get temporary sender's ActorRef and call tell() directly
                        // This ensures proper routing to ReplyWaiter
                        if let Ok(sender_id) = ActorId::from_canonical(sender) {
                            if let Some(sender_ref) =
                                actor_registry1_clone.lookup_actor(&sender_id).await
                            {
                                let _ = sender_ref.tell(&reply_ctx, reply_msg).await;
                            }
                        }
                    }
                }
            }
        }
    });

    // Create ActorRef for remote counter using local_node_id (triggers "local via remote" path)
    let counter_ref = ActorRef::remote(
        counter_id.clone(),
        "test".to_string(),        // tenant_id
        "default".to_string(),     // namespace
        local_node_id.to_string(), // Matches local_node_id, so "local via remote" path is used
        node1_service_locator.clone(),
        ActorVisibility::ActorVisibilityPublic,
    );

    // ACT: Spawn 10 concurrent ask() calls
    let mut handles = vec![];
    for i in 0..10 {
        let counter_ref_clone = counter_ref.clone();
        let counter_id_clone = counter_id.clone();
        let ask_ctx = plexspaces_core::RequestContext::new_without_auth(
            "test".to_string(),
            "default".to_string(),
        );
        let handle = tokio::spawn(async move {
            let request = CounterMessage::Increment;
            let mut msg = create_test_message(serde_json::to_vec(&request).unwrap());
            msg.message_type = "call".to_string();
            msg.receiver_id = counter_id_clone.to_string();

            let reply = counter_ref_clone
                .ask(&ask_ctx, msg, Duration::from_secs(10))
                .await;
            (i, reply)
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    let mut results = vec![];
    for handle in handles {
        let result = handle.await.unwrap();
        results.push(result);
    }

    // ASSERT: All requests should succeed
    results.sort_by_key(|(i, _)| *i);
    for (i, reply_result) in results {
        assert!(reply_result.is_ok(), "Request {} should succeed", i);
        let reply_msg = reply_result.unwrap();
        let value: CounterMessage = serde_json::from_slice(&reply_msg.payload).unwrap();
        match value {
            CounterMessage::Value(v) => {
                // Each increment should result in a value >= 1 (order may vary due to concurrency)
                assert!(
                    v >= 1,
                    "Counter value should be at least 1 for request {}",
                    i
                );
            }
            _ => panic!("Unexpected reply type for request {}", i),
        }
    }

    debug!("✅ Test: Concurrent asks multi-node - PASSED (10 concurrent requests)");
}
