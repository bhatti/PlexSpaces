// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for ActorRef tell() and ask() with real gRPC ActorService

use plexspaces_core::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use plexspaces_actor_service::ActorServiceImpl;
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_object_registry::ObjectRegistry as ObjectRegistryImpl;
use plexspaces_proto::object_registry::v1::ObjectType;
use plexspaces_proto::actor::v1::actor_service_server::ActorServiceServer;
use std::sync::Arc;
use std::time::Duration;
use tonic::transport::Server;
use async_trait::async_trait;

/// Simple wrapper to adapt ObjectRegistryImpl to CoreObjectRegistry trait
struct ObjectRegistryAdapter {
    inner: Arc<ObjectRegistryImpl>,
}

#[async_trait]
impl CoreObjectRegistry for ObjectRegistryAdapter {
    async fn lookup(
        &self,
        tenant_id: &str,
        object_id: &str,
        namespace: &str,
        object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>,
    ) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
        let obj_type = object_type.unwrap_or(plexspaces_proto::object_registry::v1::ObjectType::ObjectTypeUnspecified);
        self.inner
            .lookup(tenant_id, namespace, obj_type, object_id)
            .await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
    }

    async fn lookup_full(
        &self,
        tenant_id: &str,
        namespace: &str,
        object_type: plexspaces_proto::object_registry::v1::ObjectType,
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

/// Wrapper that adapts ActorServiceImpl to ActorService trait for testing
struct TestActorServiceWrapper {
    inner: Arc<ActorServiceImpl>,
}

#[async_trait::async_trait]
impl ActorService for TestActorServiceWrapper {
    async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
        Err("Not implemented in test".into())
    }

    async fn send(&self, actor_id: &str, message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let (msg_id, _) = self.inner.send(actor_id, message, false, None).await
            .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e)) as Box<dyn std::error::Error + Send + Sync>)?;
        Ok(msg_id)
    }

}

/// Helper to create ActorServiceImpl with ObjectRegistryAdapter
/// Returns both the service and the actor_registry for registering actors
async fn create_actor_service(registry: Arc<ObjectRegistryImpl>, node_id: String) -> (ActorServiceImpl, Arc<ActorRegistry>) {
    let actor_registry = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        node_id.clone(),
    ));
    let service_locator = Arc::new(ServiceLocator::new());
    let reply_tracker = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator.register_service(actor_registry.clone()).await;
    service_locator.register_service(reply_tracker).await;
    service_locator.register_service(reply_waiter_registry).await;
    (ActorServiceImpl::new(service_locator, node_id), actor_registry)
}

/// Helper to create ActorService wrapper for unified ActorRef
fn create_actor_service_wrapper(
    actor_service_impl: Arc<ActorServiceImpl>,
) -> Arc<dyn ActorService> {
    Arc::new(TestActorServiceWrapper {
        inner: actor_service_impl,
    })
}

/// Helper to create a test ActorContext with real ActorService
fn create_context_with_real_actor_service(
    actor_id: &str,
    node_id: &str,
    actor_service_impl: Arc<ActorServiceImpl>,
) -> ActorContext {
    let actor_service: Arc<dyn ActorService> = create_actor_service_wrapper(actor_service_impl);
    struct MockChannelService;
    #[async_trait::async_trait]
    impl ChannelService for MockChannelService {
        async fn send_to_queue(&self, _queue_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
        }
        async fn publish_to_topic(&self, _topic_name: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
        }
        async fn subscribe_to_topic(&self, _topic_name: &str) -> Result<futures::stream::BoxStream<'static, Message>, Box<dyn std::error::Error + Send + Sync>> {
            use futures::stream;
            Ok(Box::pin(stream::empty()))
        }
        async fn receive_from_queue(&self, _queue_name: &str, _timeout: Option<std::time::Duration>) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
    }

    struct MockObjectRegistry;
    #[async_trait::async_trait]
    impl CoreObjectRegistry for MockObjectRegistry {
        async fn lookup(&self, _tenant_id: &str, _object_id: &str, _namespace: &str, _object_type: Option<plexspaces_proto::object_registry::v1::ObjectType>) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn lookup_full(&self, _tenant_id: &str, _namespace: &str, _object_type: plexspaces_proto::object_registry::v1::ObjectType, _object_id: &str) -> Result<Option<plexspaces_core::ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(None)
        }
        async fn register(&self, _registration: plexspaces_core::ObjectRegistration) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }

    struct MockTupleSpaceProvider;
    #[async_trait::async_trait]
    impl TupleSpaceProvider for MockTupleSpaceProvider {
        async fn write(&self, _tuple: plexspaces_tuplespace::Tuple) -> Result<(), plexspaces_tuplespace::TupleSpaceError> {
            Ok(())
        }
        async fn read(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Vec<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
            Ok(Vec::new())
        }
        async fn take(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<Option<plexspaces_tuplespace::Tuple>, plexspaces_tuplespace::TupleSpaceError> {
            Ok(None)
        }
        async fn count(&self, _pattern: &plexspaces_tuplespace::Pattern) -> Result<usize, plexspaces_tuplespace::TupleSpaceError> {
            Ok(0)
        }
    }

    struct MockProcessGroupService;
    #[async_trait::async_trait]
    impl ProcessGroupService for MockProcessGroupService {
        async fn join_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn leave_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _actor_id: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
        async fn publish_to_group(&self, _group_name: &str, _tenant_id: &str, _namespace: &str, _message: Message) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }
        async fn get_members(&self, _group_name: &str, _tenant_id: &str, _namespace: &str) -> Result<Vec<String>, Box<dyn std::error::Error + Send + Sync>> {
            Ok(Vec::new())
        }
    }


/// Test tell() with real gRPC ActorService - remote actor
#[tokio::test]
#[ignore] // Requires real gRPC server setup
async fn test_tell_with_grpc_remote() {
    // ARRANGE: Create node2 with gRPC server
    let node2_port = 19001;
    let node2_address = format!("http://127.0.0.1:{}", node2_port);
    
    // Create registry and register node2
    let kv_store = Arc::new(InMemoryKVStore::new());
    let registry2 = Arc::new(ObjectRegistryImpl::new(kv_store));
    let node_object_id = format!("_node@{}", "node2");
    registry2
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // Create actor on node2
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "actor2@node2".to_string()).await.unwrap());
    // Create ActorRegistry for service2
    use plexspaces_core::ActorRegistry;
    let actor_registry2 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry2.clone(),
        }) as Arc<dyn plexspaces_core::ObjectRegistry>,
        "node2".to_string(),
    ));
    let service_locator2 = Arc::new(ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(actor_registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    let service2 = ActorServiceImpl::new(service_locator2.clone(), "node2".to_string());
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
        "target-actor@node2".to_string(),
        Arc::clone(&mailbox2),
        service_locator2,
    ));
    actor_registry2.register_actor("target-actor@node2".to_string(), sender).await;

    // Start node2's gRPC server (service2 is moved here)
    let _server_handle = start_test_server(service2, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await; // Wait for server

    // Create node1 with registry that knows about node2
    let kv_store1 = Arc::new(InMemoryKVStore::new());
    let registry1 = Arc::new(ObjectRegistryImpl::new(kv_store1));
    let node_object_id = format!("_node@{}", "node2");
    registry1
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let (service1, _) = create_actor_service(registry1.clone(), "node1".to_string()).await;
    let service1 = Arc::new(service1);

    // Create ServiceLocator and register ActorRegistry
    let actor_registry1 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry1.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node1".to_string(),
    ));
    let service_locator = Arc::new(ServiceLocator::new());
    service_locator.register_service(actor_registry1).await;

    // ACT: Send message from node1 to actor on node2 using unified ActorRef
    // Create unified ActorRef with ServiceLocator for remote messaging
    let actor_service = create_actor_service_wrapper(service1.clone());
    let actor_ref = plexspaces_actor::ActorRef::remote(
        "target-actor@node2".to_string(),
        "node2".to_string(),
        service_locator,
    );
    let message = Message::new(b"hello from node1".to_vec());
    actor_ref.tell(message.clone()).await.unwrap();

    // Wait a bit for message to be delivered
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ASSERT: Verify message was received on node2
    let received = tokio::time::timeout(Duration::from_secs(2), mailbox2.dequeue()).await.unwrap().unwrap();
    assert_eq!(received.payload(), message.payload());
}

/// Test ask() with real gRPC ActorService - remote actor
#[tokio::test]
async fn test_ask_with_grpc_remote() {
    // ARRANGE: Create node2 with gRPC server
    let node2_port = 19002;
    let node2_address = format!("http://127.0.0.1:{}", node2_port);
    
    let kv_store2 = Arc::new(InMemoryKVStore::new());
    let registry2 = Arc::new(ObjectRegistryImpl::new(kv_store2));
    let node_object_id = format!("_node@{}", "node2");
    registry2
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // Create actor on node2 that replies to messages
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "responder@node2".to_string()).await.unwrap());
    
    // Create service2 and register actor
    let actor_registry2 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry2.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node2".to_string(),
    ));
    let service_locator2 = Arc::new(ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(actor_registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    let service2 = Arc::new(ActorServiceImpl::new(service_locator2.clone(), "node2".to_string()));
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
        "responder@node2".to_string(),
        Arc::clone(&mailbox2),
        service_locator2,
    ));
    actor_registry2.register_actor("responder@node2".to_string(), sender).await;
    
    // Spawn task to handle messages and reply via ActorService
    // This simulates an actor that processes messages and sends replies
    let mailbox2_clone = mailbox2.clone();
    let service2_for_reply = service2.clone();
    tokio::spawn(async move {
        while let Some(request) = mailbox2_clone.dequeue().await {
            // Create reply with same correlation_id
            let mut reply = Message::new(b"reply from node2".to_vec());
            reply.correlation_id = request.correlation_id.clone();
            reply.sender = Some("responder@node2".to_string());
            // Send reply back to sender using send() method which routes via gRPC if needed
            if let Some(sender_id) = request.sender {
                // Use send() method which routes via gRPC if needed
                let _ = service2_for_reply.send_message(&sender_id, reply).await;
            }
        }
    });

    // Start node2's gRPC server - need to clone Arc to get the inner value
    // Since we can't clone ActorServiceImpl, we'll need to restructure
    // For now, we'll create a new service for the server
    let actor_registry2_for_server = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry2.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node2".to_string(),
    ));
    let service_locator2_for_server = Arc::new(ServiceLocator::new());
    let reply_tracker2_for_server = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2_for_server = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2_for_server.register_service(actor_registry2_for_server.clone()).await;
    service_locator2_for_server.register_service(reply_tracker2_for_server).await;
    service_locator2_for_server.register_service(reply_waiter_registry2_for_server).await;
    let service2_for_server = ActorServiceImpl::new(service_locator2_for_server.clone(), "node2".to_string());
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
        "responder@node2".to_string(),
        Arc::clone(&mailbox2),
        service_locator2_for_server,
    ));
    actor_registry2_for_server.register_actor("responder@node2".to_string(), sender).await;
    let _server_handle = start_test_server(service2_for_server, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Create node1 with registry that knows about node2
    let kv_store1 = Arc::new(InMemoryKVStore::new());
    let registry1 = Arc::new(ObjectRegistryImpl::new(kv_store1));
    let node_object_id = format!("_node@{}", "node2");
    registry1
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let (service1, _) = create_actor_service(registry1.clone(), "node1".to_string()).await;
    let service1 = Arc::new(service1);

    // Create ServiceLocator and register ActorRegistry
    let actor_registry1 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry1.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node1".to_string(),
    ));
    let service_locator = Arc::new(ServiceLocator::new());
    service_locator.register_service(actor_registry1).await;

    // ACT: Send ask() from node1 to actor on node2 using unified ActorRef
    let actor_service = create_actor_service_wrapper(service1.clone());
    let actor_ref = plexspaces_actor::ActorRef::remote(
        "responder@node2".to_string(),
        "node2".to_string(),
        service_locator,
    );
    let request = Message::new(b"request from node1".to_vec());
    let reply = actor_ref
        .ask(request, Duration::from_secs(5))
        .await
        .unwrap();

    // ASSERT: Verify reply was received
    assert_eq!(reply.payload(), b"reply from node2");
    assert!(reply.correlation_id.is_some());
}

/// Test ask() timeout with real gRPC ActorService
#[tokio::test]
async fn test_ask_with_grpc_timeout() {
    // ARRANGE: Create node2 with gRPC server (but actor doesn't reply)
    let node2_port = 19003;
    let node2_address = format!("http://127.0.0.1:{}", node2_port);
    
    let kv_store2 = Arc::new(InMemoryKVStore::new());
    let registry2 = Arc::new(ObjectRegistryImpl::new(kv_store2));
    let node_object_id = format!("_node@{}", "node2");
    registry2
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    // Create actor on node2 that doesn't reply
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "non-responder@node2".to_string()).await.unwrap());
    let actor_registry2 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry2.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node2".to_string(),
    ));
    let service_locator2 = Arc::new(ServiceLocator::new());
    let reply_tracker2 = Arc::new(plexspaces_core::ReplyTracker::new());
    let reply_waiter_registry2 = Arc::new(plexspaces_core::ReplyWaiterRegistry::new());
    service_locator2.register_service(actor_registry2.clone()).await;
    service_locator2.register_service(reply_tracker2).await;
    service_locator2.register_service(reply_waiter_registry2).await;
    let service2 = ActorServiceImpl::new(service_locator2.clone(), "node2".to_string());
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let sender: Arc<dyn MessageSender> = Arc::new(RegularActorWrapper::new(
        "silent@node2".to_string(),
        Arc::clone(&mailbox2),
        service_locator2,
    ));
    actor_registry2.register_actor("silent@node2".to_string(), sender).await;

    // Start node2's gRPC server
    let _server_handle = start_test_server(service2, node2_port).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Create node1
    let kv_store1 = Arc::new(InMemoryKVStore::new());
    let registry1 = Arc::new(ObjectRegistryImpl::new(kv_store1));
    let node_object_id = format!("_node@{}", "node2");
    registry1
        .register(plexspaces_proto::object_registry::v1::ObjectRegistration {
            object_id: node_object_id.clone(),
            object_type: ObjectType::ObjectTypeService as i32,
            object_category: "node".to_string(),
            grpc_address: node2_address.clone(),
            tenant_id: "default".to_string(),
            namespace: "default".to_string(),
            ..Default::default()
        })
        .await
        .unwrap();

    let (service1, _) = create_actor_service(registry1.clone(), "node1".to_string()).await;
    let service1 = Arc::new(service1);

    // Create ServiceLocator and register ActorRegistry
    let actor_registry1 = Arc::new(ActorRegistry::new(
        Arc::new(ObjectRegistryAdapter {
            inner: registry1.clone(),
        }) as Arc<dyn CoreObjectRegistry>,
        "node1".to_string(),
    ));
    let service_locator = Arc::new(ServiceLocator::new());
    service_locator.register_service(actor_registry1).await;

    // ACT: Send ask() with short timeout using unified ActorRef
    let actor_service = create_actor_service_wrapper(service1.clone());
    let actor_ref = plexspaces_actor::ActorRef::remote(
        "silent@node2".to_string(),
        "node2".to_string(),
        service_locator,
    );
    let request = Message::new(b"request".to_vec());
    let result = actor_ref
        .ask(request, Duration::from_millis(100))
        .await;

    // ASSERT: Should timeout
    assert!(result.is_err());
    use plexspaces_actor::ActorRefError;
    assert!(matches!(result.unwrap_err(), ActorRefError::Timeout));
}

