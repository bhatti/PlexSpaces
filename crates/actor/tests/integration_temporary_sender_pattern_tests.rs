// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for temporary sender pattern in ActorRef::ask()
// Tests all scenarios: outside sender, local actor, remote actor, chained asks

use plexspaces_actor::ActorRef;
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorError, Actor, BehaviorType, ActorRegistry, application::ApplicationNode, ActorService};
use plexspaces_mailbox::{Message, Mailbox, mailbox_config_default};
use plexspaces_node::{NodeBuilder, service_wrappers::ActorServiceWrapper};
use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_proto::actor::v1::actor_service_server::ActorServiceServer;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, debug};

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
        let request: CounterMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match request {
            CounterMessage::Increment => {
                self.value += 1;
                info!("Counter incremented to: {}", self.value);
                
                // Send reply using ctx.send_reply()
                if let Some(sender_id) = &msg.sender {
                    let reply = CounterMessage::Value(self.value);
                    let reply_msg = Message::new(serde_json::to_vec(&reply).unwrap());
                    
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                Ok(())
            }
            CounterMessage::Get => {
                info!("Counter get request, value: {}", self.value);
                
                // Send reply using ctx.send_reply()
                if let Some(sender_id) = &msg.sender {
                    let reply = CounterMessage::Value(self.value);
                    let reply_msg = Message::new(serde_json::to_vec(&reply).unwrap());
                    
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
                Ok(())
            }
            _ => Err(BehaviorError::ProcessingError("Invalid message".to_string())),
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
    use plexspaces_keyvalue::InMemoryKVStore;
    use plexspaces_proto::object_registry::v1::{ObjectRegistration as ProtoObjectRegistration, ObjectType, ObjectRegistration};

    // Simple wrapper to adapt ObjectRegistry to ObjectRegistryTrait
    struct ObjectRegistryAdapter {
        inner: Arc<ObjectRegistry>,
    }

    #[async_trait]
    impl ObjectRegistryTrait for ObjectRegistryAdapter {
        async fn lookup(
            &self,
            tenant_id: &str,
            object_id: &str,
            namespace: &str,
            object_type: Option<ObjectType>,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            let obj_type = object_type.unwrap_or(ObjectType::ObjectTypeUnspecified);
            self.inner
                .lookup(tenant_id, namespace, obj_type, object_id)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn lookup_full(
            &self,
            tenant_id: &str,
            namespace: &str,
            object_type: ObjectType,
            object_id: &str,
        ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .lookup(tenant_id, namespace, object_type, object_id)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }

        async fn register(
            &self,
            registration: ObjectRegistration,
        ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            self.inner
                .register(registration)
                .await
                .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
        }
    }

    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry_impl = Arc::new(ObjectRegistry::new(kv));

    // Register node as a service object (nodes are registered as services)
    let node_object_id = format!("_node@{}", node_id);
    let registration = ProtoObjectRegistration {
        object_id: node_object_id.clone(),
        object_type: ObjectType::ObjectTypeService as i32,
        object_category: "node".to_string(),
        grpc_address: node_address.to_string(),
        tenant_id: "default".to_string(),
        namespace: "default".to_string(),
        ..Default::default()
    };

    object_registry_impl.register(registration).await.unwrap();

    let object_registry: Arc<dyn ObjectRegistryTrait> = Arc::new(ObjectRegistryAdapter {
        inner: object_registry_impl,
    });
    Arc::new(ActorRegistry::new(object_registry, local_node_id.to_string()))
}

/// Helper to create ActorServiceImpl with proper ServiceLocator setup
async fn create_test_actor_service(
    actor_registry: Arc<ActorRegistry>,
    node_id: String,
) -> ActorServiceImpl {
    let service_locator = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    ActorServiceImpl::new(service_locator, node_id)
}

/// Helper to register an actor with ActorRegistry
async fn register_test_actor(
    actor_registry: Arc<ActorRegistry>,
    actor_id: String,
    mailbox: Arc<Mailbox>,
    service_locator: Arc<plexspaces_core::ServiceLocator>,
) {
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
        actor_id.clone(),
        mailbox,
        service_locator,
    ));
    actor_registry.register_actor(actor_id, sender).await;
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
        let node_id = self.target_actor_id.split('@').nth(1).unwrap_or("unknown").to_string();
        let target_ref = ActorRef::remote(
            self.target_actor_id.clone(),
            node_id,
            ctx.service_locator.clone(),
        );

        let request: CounterMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        // Forward request to target actor using ask()
        let mut forward_msg = Message::new(serde_json::to_vec(&request).unwrap());
        forward_msg.receiver = self.target_actor_id.clone();
        forward_msg.sender = Some(msg.receiver.clone()); // Use our own ID as sender
        forward_msg.message_type = "call".to_string();

        let reply = target_ref.ask(forward_msg, Duration::from_secs(5)).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Forward ask failed: {}", e)))?;

        // Forward the reply back to original sender
        if let Some(sender_id) = &msg.sender {
            ctx.send_reply(
                msg.correlation_id.as_deref(),
                sender_id,
                msg.receiver.clone(),
                reply,
            ).await
                .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
        }

        Ok(())
    }
}

#[tokio::test]
async fn test_outside_sender_calling_ask() {
    // Test: Outside sender (not an actor) calling ask() on a local actor
    // Expected: Temporary sender ID should be created and used
    
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    let node = Arc::new(NodeBuilder::new("test-node-outside-ask")
        .build());

    // Start node in background to initialize services
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await; // Wait for basic services
    
    // Manually register ActorFactory (normally done in create_actor_context_arc, but we need it for spawn_actor)
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(node.service_locator().clone()));
    node.service_locator().register_service(actor_factory_impl).await;
    
    // Manually register ActorService (normally done in create_actor_context_arc, but actors need it for send_reply)
    use plexspaces_actor_service::ActorServiceImpl;
    use plexspaces_node::service_wrappers::ActorServiceWrapper;
    use plexspaces_core::ActorService;
    
    // Wait for Node's ServiceLocator to have services registered (including ReplyWaiterRegistry)
    // Node::new() registers ReplyWaiterRegistry in a spawned task, so we need to wait
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create ActorServiceImpl using Node's ServiceLocator (ensures shared ReplyWaiterRegistry)
    // This is the proper way - ActorServiceImpl will use the same ServiceLocator as ActorRef
    let actor_service_impl = Arc::new(ActorServiceImpl::new(
        node.service_locator().clone(),
        node.id().as_str().to_string(),
    ));
    
    let actor_service_wrapper = Arc::new(ActorServiceWrapper::new(actor_service_impl.clone()));
    node.service_locator().register_service(actor_service_wrapper.clone()).await;
    let actor_service_trait: Arc<dyn ActorService> = actor_service_wrapper.clone() as Arc<dyn ActorService>;
    node.service_locator().register_actor_service(actor_service_trait).await;

    // Create and spawn counter actor
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    
    // Node implements ApplicationNode trait - spawn actor
    let actor_id = "counter-1@test-node-outside-ask".to_string();
    let _result = node
        .spawn_actor(actor_id.clone(), behavior, "default".to_string())
        .await
        .unwrap();
    
    // Get ActorRef - use remote pointing to local node (works for both local and remote)
    let counter_ref = ActorRef::remote(
        actor_id.clone(),
        "test-node-outside-ask".to_string(),
        node.service_locator().clone(),
    );

    // Wait for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Call ask() from outside - should use temporary sender
    let request = CounterMessage::Get;
    let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
    msg.receiver = actor_id.clone();
    msg.message_type = "call".to_string();
    // No sender set (outside caller) - temporary sender will be created
    
    let reply = counter_ref.ask(msg, Duration::from_secs(5)).await;
    
    assert!(reply.is_ok(), "ask() should succeed, got error: {:?}", reply.as_ref().err());
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(reply_msg.payload())
        .unwrap();
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
    // Expected: Should use actor's own ID as sender (no temporary sender needed)
    
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    let node = Arc::new(NodeBuilder::new("test-node-local-ask")
        .build());

    // Start node in background to initialize services
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await; // Wait for basic services
    
    // Manually register ActorFactory (normally done in create_actor_context_arc, but we need it for spawn_actor)
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(node.service_locator().clone()));
    node.service_locator().register_service(actor_factory_impl).await;
    
    // Manually register ActorService (normally done in create_actor_context_arc, but actors need it for send_reply)
    use plexspaces_actor_service::ActorServiceImpl;
    use plexspaces_node::service_wrappers::ActorServiceWrapper;
    use plexspaces_core::ActorService;
    
    // Wait for Node's ServiceLocator to have services registered (including ReplyWaiterRegistry)
    // Node::new() registers ReplyWaiterRegistry in a spawned task, so we need to wait
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create ActorServiceImpl using Node's ServiceLocator (ensures shared ReplyWaiterRegistry)
    // This is the proper way - ActorServiceImpl will use the same ServiceLocator as ActorRef
    let actor_service_impl = Arc::new(ActorServiceImpl::new(
        node.service_locator().clone(),
        node.id().as_str().to_string(),
    ));
    
    let actor_service_wrapper = Arc::new(ActorServiceWrapper::new(actor_service_impl.clone()));
    node.service_locator().register_service(actor_service_wrapper.clone()).await;
    let actor_service_trait: Arc<dyn ActorService> = actor_service_wrapper.clone() as Arc<dyn ActorService>;
    node.service_locator().register_actor_service(actor_service_trait).await;

    // Create two counter actors
    let behavior1: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    let behavior2: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    
    let actor1_id = "counter-1@test-node-local-ask".to_string();
    let actor2_id = "counter-2@test-node-local-ask".to_string();
    let _result1 = node.spawn_actor(actor1_id.clone(), behavior1, "default".to_string()).await.unwrap();
    let _result2 = node.spawn_actor(actor2_id.clone(), behavior2, "default".to_string()).await.unwrap();
    
    // Get ActorRef for counter2
    let counter2_ref = ActorRef::remote(
        actor2_id.clone(),
        "test-node-local-ask".to_string(),
        node.service_locator().clone(),
    );

    // Wait for actors to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Simulate counter1 calling ask() on counter2
    // This should use counter1_id as sender (not temporary sender)
    let request = CounterMessage::Get;
    let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
    msg.receiver = actor2_id.clone();
    msg.sender = Some(actor1_id.clone()); // Actor's own ID as sender
    msg.message_type = "call".to_string();
    
    let reply = counter2_ref.ask(msg, Duration::from_secs(5)).await;
    
    assert!(reply.is_ok(), "ask() should succeed");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(reply_msg.payload())
        .unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Initial counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }
    
    debug!("✅ Test: Local actor calling ask of local actor - PASSED");
}

/// Test: Local actor calling ask() on remote actor (requires gRPC server setup)
///
/// Scenario:
/// 1. Spawn node1 and node2 with in-process gRPC servers
/// 2. Register counter actor on node2
/// 3. Actor on node1 calls ask() on counter@node2
/// 4. Verify reply received correctly
#[tokio::test]
async fn test_local_actor_calling_ask_of_remote_actor() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ARRANGE: Set up node2 with counter actor
    let node2_port = 19997;
    let node2_address = format!("127.0.0.1:{}", node2_port);
    let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

    let mailbox2 = Arc::new(
        Mailbox::new(
            mailbox_config_default(),
            "counter@node2".to_string(),
        )
        .await
        .expect("Failed to create mailbox"),
    );

    // Create ServiceLocator for node2 and register services
    let service_locator2 = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    
    let service2 = ActorServiceImpl::new(service_locator2.clone(), "node2".to_string());
    
    // Register actor directly with ActorRegistry
    register_test_actor(registry2.clone(), "counter@node2".to_string(), mailbox2.clone(), service_locator2).await;

    // Start node2's gRPC server
    let _server2_handle = start_test_server(service2, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await; // Wait for server to be ready

    // ARRANGE: Set up node1 with forwarder actor
    // Create node1 - ActorRef::remote will handle routing to node2
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    // Start node1 in background to initialize services
    let node1_for_start = node1.clone();
    tokio::spawn(async move {
        let _ = node1_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await; // Wait for services to initialize

    // Create forwarder actor that will call ask() on remote counter
    let forwarder_behavior: Box<dyn plexspaces_core::Actor> = Box::new(ForwarderActor::new("counter@node2".to_string()));

    let forwarder_id = "forwarder@node1".to_string();
    let _result = node1
        .spawn_actor(forwarder_id.clone(), forwarder_behavior, "default".to_string())
        .await
        .unwrap();

    // Get ActorRef for forwarder
    let forwarder_ref = ActorRef::remote(
        forwarder_id.clone(),
        "node1".to_string(),
        node1.service_locator().clone(),
    );

    // Wait for actors to initialize
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ACT: Call ask() on forwarder, which will forward to remote counter
    let request = CounterMessage::Get;
    let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
    msg.receiver = forwarder_id.clone();
    msg.message_type = "call".to_string();
    // No sender set (outside caller) - temporary sender will be created

    let reply = forwarder_ref.ask(msg, Duration::from_secs(10)).await;

    // ASSERT: Should receive reply from remote counter via forwarder
    assert!(reply.is_ok(), "ask() should succeed across nodes");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(reply_msg.payload()).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Local actor calling ask of remote actor - PASSED");
}

/// Test: Chained asks (outside -> actor1@node1 -> actor2@node2)
///
/// Scenario:
/// 1. Spawn node1 and node2 with in-process gRPC servers
/// 2. Register actor1 (forwarder) on node1, actor2 (counter) on node2
/// 3. Outside caller calls ask() on actor1@node1
/// 4. actor1 calls ask() on actor2@node2
/// 5. actor2 replies to actor1, actor1 replies to outside caller
/// 6. Verify both replies received correctly
#[tokio::test]
async fn test_chained_asks_multi_node() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    // ARRANGE: Set up node2 with counter actor
    let node2_port = 19998;
    let node2_address = format!("127.0.0.1:{}", node2_port);
    let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

    let mailbox2 = Arc::new(
        Mailbox::new(
            mailbox_config_default(),
            "counter@node2".to_string(),
        )
        .await
        .expect("Failed to create mailbox"),
    );

    // Create ServiceLocator for node2 and register services
    let service_locator2 = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    
    let service2 = ActorServiceImpl::new(service_locator2.clone(), "node2".to_string());
    
    // Register actor directly with ActorRegistry
    register_test_actor(registry2.clone(), "counter@node2".to_string(), mailbox2.clone(), service_locator2).await;

    // Start node2's gRPC server
    let _server2_handle = start_test_server(service2, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ARRANGE: Set up node1 with forwarder actor
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    // Start node1 in background to initialize services
    let node1_for_start = node1.clone();
    tokio::spawn(async move {
        let _ = node1_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await; // Wait for services to initialize
    
    // Create forwarder actor on node1
    let forwarder_behavior: Box<dyn plexspaces_core::Actor> = Box::new(ForwarderActor::new("counter@node2".to_string()));

    let forwarder_id = "forwarder@node1".to_string();
    let _result = node1
        .spawn_actor(forwarder_id.clone(), forwarder_behavior, "default".to_string())
        .await
        .unwrap();

    // Get ActorRef for forwarder
    let forwarder_ref = ActorRef::remote(
        forwarder_id.clone(),
        "node1".to_string(),
        node1.service_locator().clone(),
    );

    // Wait for actors to initialize
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ACT: Outside caller calls ask() on forwarder@node1, which forwards to counter@node2
    let request = CounterMessage::Increment;
    let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
    msg.receiver = forwarder_id.clone();
    msg.message_type = "call".to_string();
    // No sender set (outside caller) - temporary sender will be created

    let reply = forwarder_ref.ask(msg, Duration::from_secs(10)).await;

    // ASSERT: Should receive reply from remote counter via forwarder
    assert!(reply.is_ok(), "Chained ask() should succeed");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(reply_msg.payload()).unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 1, "Counter should be incremented to 1");
        }
        _ => panic!("Unexpected reply type"),
    }

    debug!("✅ Test: Chained asks multi-node - PASSED");
}

/// Test: Concurrent ask() calls across nodes
///
/// Scenario:
/// 1. Spawn node1 and node2 with in-process gRPC servers
/// 2. Register counter actor on node2
/// 3. Spawn 10 concurrent ask() calls from node1 to counter@node2
/// 4. Verify all replies received correctly
#[tokio::test]
async fn test_concurrent_asks_multi_node() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // ARRANGE: Set up node2 with counter actor
    let node2_port = 19999;
    let node2_address = format!("127.0.0.1:{}", node2_port);
    let registry2 = create_test_registry_with_node("node2", "node2", &node2_address).await;

    let mailbox2 = Arc::new(
        Mailbox::new(
            mailbox_config_default(),
            "counter@node2".to_string(),
        )
        .await
        .expect("Failed to create mailbox"),
    );

    // Create ServiceLocator for node2 and register services
    let service_locator2 = Arc::new(plexspaces_core::ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    
    let service2 = ActorServiceImpl::new(service_locator2.clone(), "node2".to_string());
    
    // Register actor directly with ActorRegistry
    register_test_actor(registry2.clone(), "counter@node2".to_string(), mailbox2.clone(), service_locator2).await;

    // Start node2's gRPC server
    let _server2_handle = start_test_server(service2, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ARRANGE: Set up node1
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    // Start node1 in background to initialize services
    let node1_for_start = node1.clone();
    tokio::spawn(async move {
        let _ = node1_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await; // Wait for services to initialize

    // Create ActorRef for remote counter
    let counter_ref = ActorRef::remote(
        "counter@node2".to_string(),
        "node2".to_string(),
        node1.service_locator().clone(),
    );

    // Wait for setup
    tokio::time::sleep(Duration::from_millis(200)).await;

    // ACT: Spawn 10 concurrent ask() calls
    let mut handles = vec![];
    for i in 0..10 {
        let counter_ref_clone = counter_ref.clone();
        let handle = tokio::spawn(async move {
            let request = CounterMessage::Increment;
            let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
            msg.receiver = "counter@node2".to_string();
            msg.message_type = "call".to_string();

            let reply = counter_ref_clone.ask(msg, Duration::from_secs(10)).await;
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
        let value: CounterMessage = serde_json::from_slice(reply_msg.payload()).unwrap();
        match value {
            CounterMessage::Value(v) => {
                // Each increment should result in a value >= 1 (order may vary due to concurrency)
                assert!(v >= 1, "Counter value should be at least 1 for request {}", i);
            }
            _ => panic!("Unexpected reply type for request {}", i),
        }
    }

    debug!("✅ Test: Concurrent asks multi-node - PASSED (10 concurrent requests)");
}

/// Test: Verify unified send_reply API works correctly
///
/// Scenario:
/// 1. Create actor that receives message
/// 2. Actor calls ctx.send_reply() (which delegates to ActorService::send_reply())
/// 3. Verify reply is sent correctly (both local and remote paths)
#[tokio::test]
async fn test_unified_send_reply_api() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("debug")
        .try_init();

    let node = Arc::new(NodeBuilder::new("test-node-send-reply")
        .build());

    // Start node in background to initialize services
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(500)).await; // Wait for basic services
    
    // Manually register ActorFactory (normally done in create_actor_context_arc, but we need it for spawn_actor)
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(node.service_locator().clone()));
    node.service_locator().register_service(actor_factory_impl).await;
    
    // Manually register ActorService (normally done in create_actor_context_arc, but actors need it for send_reply)
    use plexspaces_actor_service::ActorServiceImpl;
    use plexspaces_node::service_wrappers::ActorServiceWrapper;
    use plexspaces_core::ActorService;
    
    // Wait for Node's ServiceLocator to have services registered (including ReplyWaiterRegistry)
    // Node::new() registers ReplyWaiterRegistry in a spawned task, so we need to wait
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // Create ActorServiceImpl using Node's ServiceLocator (ensures shared ReplyWaiterRegistry)
    // This is the proper way - ActorServiceImpl will use the same ServiceLocator as ActorRef
    let actor_service_impl = Arc::new(ActorServiceImpl::new(
        node.service_locator().clone(),
        node.id().as_str().to_string(),
    ));
    
    let actor_service_wrapper = Arc::new(ActorServiceWrapper::new(actor_service_impl.clone()));
    node.service_locator().register_service(actor_service_wrapper.clone()).await;
    let actor_service_trait: Arc<dyn ActorService> = actor_service_wrapper.clone() as Arc<dyn ActorService>;
    node.service_locator().register_actor_service(actor_service_trait).await;

    // Create and spawn counter actor
    let behavior: Box<dyn plexspaces_core::Actor> = Box::new(CounterActor::new());
    
    let actor_id = "counter-1@test-node-send-reply".to_string();
    let _result = node
        .spawn_actor(actor_id.clone(), behavior, "default".to_string())
        .await
        .unwrap();
    
    // Get ActorRef
    let counter_ref = ActorRef::remote(
        actor_id.clone(),
        "test-node-send-reply".to_string(),
        node.service_locator().clone(),
    );

    // Wait for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Call ask() - actor will use ctx.send_reply() which delegates to ActorService::send_reply()
    let request = CounterMessage::Get;
    let mut msg = Message::new(serde_json::to_vec(&request).unwrap());
    msg.receiver = actor_id.clone();
    msg.message_type = "call".to_string();
    
    let reply = counter_ref.ask(msg, Duration::from_secs(5)).await;
    
    assert!(reply.is_ok(), "ask() should succeed with unified send_reply API");
    let reply_msg = reply.unwrap();
    let value: CounterMessage = serde_json::from_slice(reply_msg.payload())
        .unwrap();
    match value {
        CounterMessage::Value(v) => {
            assert_eq!(v, 0, "Initial counter value should be 0");
        }
        _ => panic!("Unexpected reply type"),
    }
    
    debug!("✅ Test: Unified send_reply API - PASSED");
}
