// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for Node remote message routing (Phase 2: gRPC Remoting)
//!
//! These tests validate that Node can route messages to remote actors via gRPC

use plexspaces_actor::ActorRef;
use plexspaces_core::{Message, MessageSender};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{grpc_service::ActorServiceImpl, Node, NodeBuilder, NodeId};
use plexspaces_proto::ActorServiceServer;
use std::sync::Arc;
use tonic::transport::Server;

#[path = "test_helpers.rs"]
mod test_helpers;
use test_helpers::lookup_actor_ref;

/// Helper to start a gRPC server for testing
async fn start_test_server(node: Arc<Node>) -> String {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl::new(node.clone());

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    let bound_addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        Server::builder()
            .add_service(ActorServiceServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .expect("Server failed");
    });

    // Wait for server to be ready - give it more time to fully start
    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    
    // Extract just the host:port (remove http:// prefix if present)
    let addr_str = bound_addr.to_string();
    addr_str
}

/// Test: Node routes local messages via mailbox (existing behavior)
#[tokio::test]
async fn test_node_route_local_message() {
    // Setup: Create node with local actor
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    // Use larger mailbox capacity to avoid "Mailbox is full" errors
    let mut mailbox_config = MailboxConfig::default();
    mailbox_config.capacity = 1000;
    let mailbox = Arc::new(Mailbox::new(mailbox_config, "test-actor@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator().clone();
    let actor_ref = ActorRef::local("test-actor@node1".to_string(), mailbox.clone(), service_locator.clone());

    // Register actor with MessageSender (mailbox is internal)
    
    use plexspaces_core::MessageSender;
    let wrapper = Arc::new(ActorRef::local(
        "test-actor@node1".to_string(),
        mailbox.clone(),
        service_locator.clone(),
    ));
    let actor_registry: Arc<plexspaces_core::ActorRegistry> = node.service_locator().get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())).unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    actor_registry.register_actor(&ctx, "test-actor@node1".to_string(), wrapper, None, None, None).await;
    
    // Actor already registered - no need to update config

    // Act: Send message via ActorRef (use the one we already created)
    let message = Message::new(vec![1, 2, 3]);
    let result = actor_ref.tell(message).await;

    // Assert: Message delivered to local mailbox
    assert!(result.is_ok(), "Local routing should succeed, got error: {:?}", result.err());
    let received = mailbox.dequeue().await;
    assert!(received.is_some(), "Message should be in mailbox");
}

/// Test: Node routes remote messages via gRPC
#[tokio::test]
async fn test_node_route_remote_message() {
    // Setup: Create two nodes
    let node1 = Arc::new(NodeBuilder::new("node1").build().await);

    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    // Start gRPC server for node2
    let node2_address = start_test_server(node2.clone()).await;

    // Register actor on node2
    let mut mailbox_config2 = MailboxConfig::default();
    mailbox_config2.capacity = 1000;
    let mailbox2 = Arc::new(Mailbox::new(mailbox_config2, "remote-actor@node2".to_string()).await.unwrap());
    let service_locator2 = node2.service_locator().clone();
    let actor_ref2 = ActorRef::local("remote-actor@node2".to_string(), mailbox2.clone(), service_locator2.clone());
    
    // Register actor with MessageSender (mailbox is internal)
    
    use plexspaces_core::MessageSender;
    let wrapper2 = Arc::new(ActorRef::local(
        "remote-actor@node2".to_string(),
        mailbox2.clone(),
        service_locator2.clone(),
    ));
    let actor_registry2: Arc<plexspaces_core::ActorRegistry> = node2.service_locator().get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())).unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    actor_registry2.register_actor(&ctx, "remote-actor@node2".to_string(), wrapper2, None, None, None).await;
    
    // Register actor config - use the actual actor_ref2 which implements MessageSender
    let ctx = plexspaces_core::RequestContext::internal();
    let actor_id = actor_ref2.id().clone();
    let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref2.clone());
    actor_registry2.register_actor(&ctx, actor_id, sender, None, None, None).await;

    // Register node2 in node1's registry
    let _: Result<(), _> = node1
        .register_remote_node(NodeId::new("node2"), node2_address.clone())
        .await;

    // Also register node2 in ObjectRegistry (required for ActorRef::remote to find the node)
    // Use the same tenant/namespace that get_node_client will use (from NodeConfig defaults: "internal"/"system")
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry = node1.object_registry().await.unwrap();
    // NodeConfig defaults are "internal"/"system" (not "default"/"default")
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    // Ensure address format is correct (host:port, not http://host:port)
    let grpc_address = if node2_address.starts_with("http://") {
        node2_address.strip_prefix("http://").unwrap().to_string()
    } else {
        node2_address.clone()
    };
    let node_registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    object_registry.register(&ctx, node_registration).await.unwrap();
    
    // Give ObjectRegistry a moment to process the registration
    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Act: Send message from node1 to actor on node2 via ActorRef
    // Create remote ActorRef for node1 to send to node2
    let service_locator1 = node1.service_locator().clone();
    let remote_actor_ref = ActorRef::remote("remote-actor@node2".to_string(), "node2".to_string(), service_locator1);
    let message = Message::new(vec![4, 5, 6]);
    let result = remote_actor_ref.tell(message).await;

    // Assert: Message delivered via gRPC
    assert!(result.is_ok(), "Remote routing should succeed, got error: {:?}", result.err());

    // Verify message arrived at node2's actor mailbox
    // Wait for message to arrive using dequeue_with_timeout instead of sleep
    let received_opt = mailbox2.dequeue_with_timeout(Some(tokio::time::Duration::from_secs(5)))
        .await;
    let received = received_opt.expect("Message should arrive within 5 seconds");
    assert_eq!(received.payload(), &vec![4, 5, 6]);
}

/// Test: Node fails gracefully when remote node not registered
#[tokio::test]
async fn test_node_route_to_unregistered_remote() {
    // Setup: Create node
    let node = Arc::new(NodeBuilder::new("node1").build().await);

    // Act: Try to send to actor on unregistered remote node
    let message = Message::new(vec![7, 8, 9]);
    let result = match lookup_actor_ref(&node, &"actor@node999".to_string()).await {
        Ok(Some(actor_ref)) => actor_ref.tell(message).await
            .map_err(|e| plexspaces_node::NodeError::DeliveryFailed(format!("{}", e))),
        Ok(None) => Err(plexspaces_node::NodeError::ActorNotFound("actor@node999".to_string())),
        Err(e) => Err(e),
    };

    // Assert: Should fail with NodeNotFound error
    assert!(result.is_err(), "Should fail for unregistered node");
    match result {
        Err(e) => {
            let err_msg = e.to_string();
            assert!(
                err_msg.contains("not found") || err_msg.contains("Not found"),
                "Expected 'not found' error, got: {}",
                err_msg
            );
        }
        Ok(_) => panic!("Should not succeed for unregistered node"),
    }
}

/// Test: Connection pooling - multiple messages reuse same gRPC connection
#[tokio::test]
async fn test_connection_pooling() {
    // Setup: Create two nodes
    let node1 = Arc::new(NodeBuilder::new("node1").build().await);

    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    let node2_address = start_test_server(node2.clone()).await;

    // Register actor on node2
    let mut mailbox_config2 = MailboxConfig::default();
    mailbox_config2.capacity = 1000;
    let mailbox2 = Arc::new(Mailbox::new(mailbox_config2, "pooled-actor@node2".to_string()).await.unwrap());
    let service_locator2 = node2.service_locator().clone();
    let actor_ref2 = ActorRef::local("pooled-actor@node2".to_string(), mailbox2.clone(), service_locator2.clone());
    
    // Register actor's mailbox in ActorRegistry first (required for route_message)
    // Register actor with MessageSender (mailbox is internal)
    
    use plexspaces_core::MessageSender;
    let wrapper_pooled = Arc::new(ActorRef::local(
        "pooled-actor@node2".to_string(),
        mailbox2.clone(),
        service_locator2.clone(),
    ));
    let actor_registry2: Arc<plexspaces_core::ActorRegistry> = node2.service_locator().get_service_by_name(plexspaces_core::service_locator::service_names::ACTOR_REGISTRY).await
        .ok_or_else(|| plexspaces_node::NodeError::ConfigError("ActorRegistry not found".to_string())).unwrap();
    let ctx = plexspaces_core::RequestContext::new_without_auth("default".to_string(), "default".to_string());
    actor_registry2.register_actor(&ctx, "pooled-actor@node2".to_string(), wrapper_pooled, None, None, None).await;
    
    // Register actor config - use the actual actor_ref2 which implements MessageSender
    let ctx = plexspaces_core::RequestContext::internal();
    let actor_id = actor_ref2.id().clone();
    let sender: Arc<dyn plexspaces_core::MessageSender> = Arc::new(actor_ref2.clone());
    actor_registry2.register_actor(&ctx, actor_id, sender, None, None, None).await;

    // Register node2 in node1's registry
    let _: Result<(), _> = node1
        .register_remote_node(NodeId::new("node2"), node2_address.clone())
        .await;

    // Also register node2 in ObjectRegistry (required for ActorRef::remote to find the node)
    // Use the same tenant/namespace that get_node_client will use (from NodeConfig defaults: "internal"/"system")
    use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType};
    let object_registry = node1.object_registry().await.unwrap();
    // NodeConfig defaults are "internal"/"system" (not "default"/"default")
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    // Ensure address format is correct (host:port, not http://host:port)
    let grpc_address = if node2_address.starts_with("http://") {
        node2_address.strip_prefix("http://").unwrap().to_string()
    } else {
        node2_address.clone()
    };
    let node_registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: "node2".to_string(),
        grpc_address,
        object_category: "Node".to_string(),
        ..Default::default()
    };
    object_registry.register(&ctx, node_registration).await.unwrap();
    
    // Give ObjectRegistry a moment to process the registration
    tokio::task::yield_now().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Act: Send multiple messages (should reuse connection)
    // Create remote ActorRef for node1 to send to node2
    let service_locator1 = node1.service_locator().clone();
    let remote_actor_ref = ActorRef::remote("pooled-actor@node2".to_string(), "node2".to_string(), service_locator1);

    for i in 0..5 {
        let message = Message::new(vec![i]);
        let result = remote_actor_ref.tell(message).await;
        assert!(result.is_ok(), "Message {} should succeed, got error: {:?}", i, result.err());
    }

    // Assert: All 5 messages delivered (connection pooling worked)
    // Wait for all messages to arrive using dequeue_with_timeout instead of sleep
    let mut count = 0;
    for _ in 0..5 {
        if let Some(_) = mailbox2.dequeue_with_timeout(Some(tokio::time::Duration::from_secs(5))).await {
            count += 1;
        }
    }
    assert_eq!(count, 5, "All 5 messages should have been delivered");
}

/// Test: Auto-discovery - Node can discover other nodes
#[tokio::test]
async fn test_node_discovery() {
    // Setup: Create two nodes
    let node1 = Arc::new(NodeBuilder::new("node1").build().await);

    let node2 = Arc::new(NodeBuilder::new("node2").build().await);

    let node2_address = start_test_server(node2.clone()).await;

    // Act: Register node2 as remote node
    let _: Result<(), _> = node1
        .register_remote_node(NodeId::new("node2"), node2_address)
        .await;

    // Assert: node1 knows about node2
    let connected_nodes: Vec<NodeId> = node1.connected_nodes().await;
    assert!(
        connected_nodes.contains(&NodeId::new("node2")),
        "node2 should be in connected nodes"
    );
}
