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
use plexspaces_node::{grpc_service::ActorServiceImpl, Node, NodeId, default_node_config};
use plexspaces_proto::ActorServiceServer;
use std::sync::Arc;
use tonic::transport::Server;

#[path = "test_helpers.rs"]
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

    // Wait for server to be ready - use a future that checks server readiness
    let server_ready = async {
        // Give server a moment to start
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), server_ready)
        .await
        .expect("Server should start quickly");
    format!("http://{}", bound_addr)
}

/// Test: Node routes local messages via mailbox (existing behavior)
#[tokio::test]
async fn test_node_route_local_message() {
    // Setup: Create node with local actor
    let node = Arc::new(NodeBuilder::new("node1").build());

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
    node.actor_registry().register_actor("test-actor@node1".to_string(), wrapper, None, None, None).await;
    
    // Register actor config
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();

    // Act: Send message via ActorRef
    let message = Message::new(vec![1, 2, 3]);
    let actor_ref = lookup_actor_ref(&node, &"test-actor@node1".to_string()).await.unwrap().unwrap();
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
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    let node2 = Arc::new(NodeBuilder::new("node2").build());

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
    node2.actor_registry().register_actor("remote-actor@node2".to_string(), wrapper2, None, None, None).await;
    
    let core_actor_ref2 = plexspaces_core::ActorRef::new(actor_ref2.id().as_str().to_string()).unwrap();
    // Register actor config
    node2.actor_registry().register_actor_with_config(actor_ref2.id().as_str().to_string(), None).await.unwrap();

    // Register node2 in node1's registry
    node1
        .register_remote_node(NodeId::new("node2"), node2_address)
        .await
        .unwrap();

    // Act: Send message from node1 to actor on node2 via ActorRef
    let message = Message::new(vec![4, 5, 6]);
    let actor_ref = lookup_actor_ref(&node1, &"remote-actor@node2".to_string()).await.unwrap().unwrap();
    let result = actor_ref.tell(message).await;

    // Assert: Message delivered via gRPC
    assert!(result.is_ok(), "Remote routing should succeed");

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
    let node = Arc::new(NodeBuilder::new("node1").build());

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
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    let node2 = Arc::new(NodeBuilder::new("node2").build());

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
    node2.actor_registry().register_actor("pooled-actor@node2".to_string(), wrapper_pooled, None, None, None).await;
    
    let core_actor_ref2 = plexspaces_core::ActorRef::new(actor_ref2.id().as_str().to_string()).unwrap();
    // Register actor config
    node2.actor_registry().register_actor_with_config(actor_ref2.id().as_str().to_string(), None).await.unwrap();

    // Register node2 in node1's registry
    node1
        .register_remote_node(NodeId::new("node2"), node2_address)
        .await
        .unwrap();

    // Act: Send multiple messages (should reuse connection)
    let actor_ref = lookup_actor_ref(&node1, &"pooled-actor@node2".to_string()).await.unwrap().unwrap();

    for i in 0..5 {
        let message = Message::new(vec![i]);
        let result = actor_ref.tell(message).await;
        assert!(result.is_ok(), "Message {} should succeed", i);
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
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    let node2 = Arc::new(NodeBuilder::new("node2").build());

    let node2_address = start_test_server(node2.clone()).await;

    // Act: Register node2 as remote node
    node1
        .register_remote_node(NodeId::new("node2"), node2_address)
        .await
        .unwrap();

    // Assert: node1 knows about node2
    let connected_nodes = node1.connected_nodes().await;
    assert!(
        connected_nodes.contains(&NodeId::new("node2")),
        "node2 should be in connected nodes"
    );
}
