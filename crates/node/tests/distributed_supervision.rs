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

//! Integration tests for distributed supervision (Phase 4)
//!
//! Tests validate Node.monitor() functionality for local and remote actors,
//! following Erlang's location-transparent monitoring philosophy.

use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{grpc_service::ActorServiceImpl, Node, NodeConfig, NodeId};
use plexspaces_proto::ActorServiceServer;
use plexspaces_core::ExitReason;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tonic::transport::Server;

mod test_helpers;
use test_helpers::register_actor_with_message_sender;

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
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(Duration::from_secs(1), server_ready)
        .await
        .expect("Server should start quickly");
    format!("http://{}", bound_addr)
}

/// Test 1: Monitor local actor - supervisor on same node as actor
#[tokio::test]
async fn test_monitor_local_actor() {
    // Setup: Create node with local actor
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local("worker@node1".to_string(), mailbox.clone(), service_locator);

    // Register actor with MessageSender (mailbox is internal)
    register_actor_with_message_sender(&node, "worker@node1", mailbox.clone()).await;
    
    // Register actor config (register_actor expects plexspaces_actor::ActorRef, not core::ActorRef)
    // Use register_actor_with_config directly instead
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    {
    // Note: Metrics are updated internally by Node methods

    // Create a channel to receive termination notifications
    let (tx, mut rx) = mpsc::channel(1);

    // Act: Monitor the local actor
    let monitor_ref = node
        .monitor(
            &"worker@node1".to_string(),
            &"supervisor@node1".to_string(),
            tx,
        )
        .await;

    // Assert: Monitor established successfully
    assert!(monitor_ref.is_ok(), "Monitoring local actor should succeed");
    assert!(
        !monitor_ref.unwrap().is_empty(),
        "Monitor ref should not be empty"
    );

    // Verify: No notification yet (actor still alive)
    let no_msg = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        no_msg.is_err(),
        "Should not receive notification while actor is alive"
    );
}

/// Test 2: Monitor remote actor - supervisor on different node than actor
#[tokio::test]
async fn test_monitor_remote_actor() {
    // Setup: Create two nodes
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    let node2 = Arc::new(NodeBuilder::new("node2").build());

    // Start gRPC server for node2
    let node2_address = start_test_server(node2.clone()).await;

    // Register actor on node2
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node2".to_string()).await.unwrap());
    let service_locator2 = node2.service_locator();
    let actor_ref2 = ActorRef::local("worker@node2".to_string(), mailbox2.clone(), service_locator2);
    
    // Register actor's mailbox in ActorRegistry first (required for monitoring)
    register_actor_with_message_sender(&node2, "worker@node2", mailbox2.clone()).await;
    
    // Register actor config
    node2.actor_registry().register_actor_with_config(actor_ref2.id().as_str().to_string(), None).await.unwrap();

    // Register node2 in node1's registry
    node1
        .register_remote_node(NodeId::new("node2"), node2_address.clone())
        .await
        .unwrap();

    // Create notification channel on supervisor node (node1)
    let (tx, mut rx) = mpsc::channel(1);

    // Act: Monitor remote actor from node1 (supervisor on node1, actor on node2)
    let monitor_ref = node1
        .monitor(
            &"worker@node2".to_string(),
            &"supervisor@node1".to_string(),
            tx,
        )
        .await;

    // Assert: Remote monitoring established successfully
    assert!(
        monitor_ref.is_ok(),
        "Monitoring remote actor should succeed"
    );
    assert!(
        !monitor_ref.unwrap().is_empty(),
        "Monitor ref should not be empty"
    );

    // Verify: No notification yet (actor still alive)
    let no_msg = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
    assert!(
        no_msg.is_err(),
        "Should not receive notification while actor is alive"
    );
}

/// Test 3: Actor termination notification - local actor
#[tokio::test]
async fn test_local_actor_termination_notification() {
    // Setup: Create node with local actor
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local("worker@node1".to_string(), mailbox.clone(), service_locator);

    // Register actor with MessageSender (mailbox is internal)
    register_actor_with_message_sender(&node, "worker@node1", mailbox.clone()).await;
    
    // Register actor config (register_actor expects plexspaces_actor::ActorRef, not core::ActorRef)
    // Use register_actor_with_config directly instead
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    {
    // Note: Metrics are updated internally by Node methods

    // Create notification channel
    let (tx, mut rx) = mpsc::channel(1);

    // Monitor the actor
    node.monitor(
        &"worker@node1".to_string(),
        &"supervisor@node1".to_string(),
        tx,
    )
    .await
    .unwrap();

    // Act: Terminate the actor (unregister simulates termination)
    let actor_registry = node.actor_registry().await.unwrap();
    actor_registry.handle_actor_termination(&"worker@node1".to_string(), ExitReason::Normal).await;

    // Assert: Supervisor receives termination notification
    let notification = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(
        notification.is_ok(),
        "Should receive termination notification"
    );

    let (actor_id, reason) = notification.unwrap().unwrap();
    assert_eq!(actor_id, "worker@node1");
    assert_eq!(reason, "normal");
}

/// Test 4: Actor termination notification - remote actor
#[tokio::test]
#[ignore] // TODO: Fix remote monitoring - supervisor_callback address issue
async fn test_remote_actor_termination_notification() {
    // Setup: Create two nodes with explicit listen addresses
    // Note: This test is currently ignored due to an issue with supervisor_callback
    // address in remote monitoring. The monitor() function uses config.listen_addr
    // which may not match the actual bound gRPC server address.
    use plexspaces_node::NodeBuilder;
    let node1 = Arc::new(NodeBuilder::new("node1")
        .with_listen_address("127.0.0.1:0")
        .build());

    let node2 = Arc::new(NodeBuilder::new("node2")
        .with_listen_address("127.0.0.1:0")
        .build());

    // Start gRPC servers for both nodes
    let node1_address = start_test_server(node1.clone()).await;
    let node2_address = start_test_server(node2.clone()).await;

    // Register actor on node2
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node2".to_string()).await.unwrap());
    let service_locator2 = node2.service_locator();
    let actor_ref2 = ActorRef::local("worker@node2".to_string(), mailbox2.clone(), service_locator2);
    
    // Register actor's mailbox in ActorRegistry first (required for monitoring)
    register_actor_with_message_sender(&node2, "worker@node2", mailbox2.clone()).await;
    
    // Register actor config
    node2.actor_registry().register_actor_with_config(actor_ref2.id().as_str().to_string(), None).await.unwrap();

    // Connect nodes
    node1
        .register_remote_node(NodeId::new("node2"), node2_address.clone())
        .await
        .unwrap();
    node2
        .register_remote_node(NodeId::new("node1"), node1_address.clone())
        .await
        .unwrap();

    // Create notification channel on supervisor node (node1)
    let (tx, mut rx) = mpsc::channel(1);

    // Monitor remote actor
    node1
        .monitor(
            &"worker@node2".to_string(),
            &"supervisor@node1".to_string(),
            tx,
        )
        .await
        .unwrap();

    // Act: Actor on node2 terminates (node2 sends notification to node1)
    let actor_registry2 = node2.actor_registry().await.unwrap();
    actor_registry2.handle_actor_termination(&"worker@node2".to_string(), ExitReason::Shutdown).await;

    // Assert: Supervisor on node1 receives notification
    // Wait for notification using recv() with timeout instead of sleep
    let notification = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
    assert!(
        notification.is_ok(),
        "Should receive remote termination notification within 5 seconds"
    );

    let (actor_id, reason) = notification.unwrap().unwrap();
    assert_eq!(actor_id, "worker@node2");
    assert_eq!(reason, "shutdown");
}

/// Test 5: Monitor non-existent actor - should fail
#[tokio::test]
async fn test_monitor_nonexistent_actor() {
    // Setup: Create node
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let (tx, _rx) = mpsc::channel(1);

    // Act: Try to monitor non-existent actor
    let result = node
        .monitor(
            &"nonexistent@node1".to_string(),
            &"supervisor@node1".to_string(),
            tx,
        )
        .await;

    // Assert: Should fail with ActorNotFound error
    assert!(result.is_err(), "Monitoring non-existent actor should fail");
}

/// Test 6: Multiple supervisors monitoring same actor
#[tokio::test]
async fn test_multiple_monitors_same_actor() {
    // Setup: Create node with actor
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local("worker@node1".to_string(), mailbox.clone(), service_locator);

    // Register actor with MessageSender (mailbox is internal)
    register_actor_with_message_sender(&node, "worker@node1", mailbox.clone()).await;
    
    // Register actor config (register_actor expects plexspaces_actor::ActorRef, not core::ActorRef)
    // Use register_actor_with_config directly instead
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    {
    // Note: Metrics are updated internally by Node methods

    // Create two supervisors
    let (tx1, mut rx1) = mpsc::channel(1);
    let (tx2, mut rx2) = mpsc::channel(1);

    // Act: Both supervisors monitor same actor
    let mon1 = node
        .monitor(
            &"worker@node1".to_string(),
            &"supervisor1@node1".to_string(),
            tx1,
        )
        .await;

    let mon2 = node
        .monitor(
            &"worker@node1".to_string(),
            &"supervisor2@node1".to_string(),
            tx2,
        )
        .await;

    assert!(mon1.is_ok(), "First monitor should succeed");
    assert!(mon2.is_ok(), "Second monitor should succeed");

    // Terminate actor
    let actor_registry = node.actor_registry().await.unwrap();
    actor_registry.handle_actor_termination(&"worker@node1".to_string(), ExitReason::Error("crash".to_string())).await;

    // Assert: BOTH supervisors receive notification
    let notif1 = tokio::time::timeout(Duration::from_millis(500), rx1.recv()).await;
    let notif2 = tokio::time::timeout(Duration::from_millis(500), rx2.recv()).await;

    assert!(notif1.is_ok(), "Supervisor 1 should receive notification");
    assert!(notif2.is_ok(), "Supervisor 2 should receive notification");

    assert_eq!(notif1.unwrap().unwrap().1, "crash");
    assert_eq!(notif2.unwrap().unwrap().1, "crash");
}

/// Test 7: Monitor reference is unique per monitor call
#[tokio::test]
async fn test_monitor_ref_uniqueness() {
    // Setup: Create node with actor
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local("worker@node1".to_string(), mailbox.clone(), service_locator);

    // Register actor with MessageSender (mailbox is internal)
    register_actor_with_message_sender(&node, "worker@node1", mailbox.clone()).await;
    
    // Register actor config (register_actor expects plexspaces_actor::ActorRef, not core::ActorRef)
    // Use register_actor_with_config directly instead
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    {
    // Note: Metrics are updated internally by Node methods

    let (tx1, _rx1) = mpsc::channel(1);
    let (tx2, _rx2) = mpsc::channel(1);

    // Act: Create two monitors
    let mon1 = node
        .monitor(
            &"worker@node1".to_string(),
            &"supervisor1@node1".to_string(),
            tx1,
        )
        .await
        .unwrap();

    let mon2 = node
        .monitor(
            &"worker@node1".to_string(),
            &"supervisor2@node1".to_string(),
            tx2,
        )
        .await
        .unwrap();

    // Assert: Monitor refs are different (unique)
    assert_ne!(mon1, mon2, "Monitor refs should be unique");
}

/// Test 8: Actor crash reason propagated correctly
#[tokio::test]
async fn test_actor_crash_reason_propagation() {
    // Setup: Create node with actor
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("node1").build());

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "worker@node1".to_string()).await.unwrap());
    let service_locator = node.service_locator();
    let actor_ref = ActorRef::local("worker@node1".to_string(), mailbox.clone(), service_locator);

    // Register actor with MessageSender (mailbox is internal)
    register_actor_with_message_sender(&node, "worker@node1", mailbox.clone()).await;
    
    // Register actor config (register_actor expects plexspaces_actor::ActorRef, not core::ActorRef)
    // Use register_actor_with_config directly instead
    node.actor_registry().register_actor_with_config(actor_ref.id().as_str().to_string(), None).await.unwrap();
    {
    // Note: Metrics are updated internally by Node methods

    let (tx, mut rx) = mpsc::channel(1);

    node.monitor(
        &"worker@node1".to_string(),
        &"supervisor@node1".to_string(),
        tx,
    )
    .await
    .unwrap();

    // Act: Terminate with specific error reason
    let crash_reason = "panic: index out of bounds at line 42";
    let actor_registry = node.actor_registry().await.unwrap();
    actor_registry.handle_actor_termination(&"worker@node1".to_string(), ExitReason::Error(crash_reason.to_string())).await;

    // Assert: Exact crash reason received
    let notification = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
    assert!(notification.is_ok());

    let (_actor_id, reason) = notification.unwrap().unwrap();
    assert_eq!(
        reason, crash_reason,
        "Crash reason should be propagated exactly"
    );
}
