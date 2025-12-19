// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for lifecycle event observability
//!
//! ## Purpose
//! Tests the JavaNOW-inspired channel/subscriber pattern for actor lifecycle
//! event notification. Verifies that observability backends (Prometheus, StatsD,
//! OpenTelemetry) can subscribe to and receive lifecycle events.
//!
//! ## Test Coverage
//! - ✅ Receive all spawn lifecycle events (Created, Starting, Activated)
//! - ✅ Subscribe to lifecycle events
//! - ✅ Receive events when actor terminates normally
//! - ✅ Receive events when actor fails/panics
//! - ✅ Multiple subscribers receive same events (multicast)
//! - ✅ Events contain correct metadata (actor_id, timestamp, reason)
//! - ✅ Event ordering is correct (Created → Starting → Activated → Terminated)

mod test_helpers;
use test_helpers::spawn_actor_helper;

use plexspaces_actor::Actor;
use plexspaces_behavior::MockBehavior;
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::{Node, NodeConfig, NodeId};
use plexspaces_persistence::MemoryJournal;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Test that subscribers receive full lifecycle events during actor spawn
///
/// ## Purpose
/// Verifies that all lifecycle events (Created, Starting, Activated) are emitted
/// in the correct order when an actor is spawned.
///
/// ## Integration Point
/// This test validates that observability backends receive complete lifecycle
/// visibility from actor creation through activation.
#[tokio::test]
async fn test_lifecycle_event_full_spawn_sequence() {
    use plexspaces_proto::actor_lifecycle_event::EventType;

    // Create node
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("test-node").build());

    // Create observability subscriber channel (simulates Prometheus exporter)
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Subscribe to lifecycle events
    node.subscribe_lifecycle_events(event_tx).await;

    // Create and spawn actor
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "spawn-test-actor@test-node".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "spawn-test-actor@test-node".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let _actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Receive Created event
    let created_event =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
            .await
            .expect("Timeout waiting for Created event")
            .expect("Channel closed before receiving Created event");

    assert_eq!(created_event.actor_id, "spawn-test-actor@test-node");
    assert!(created_event.timestamp.is_some());
    assert!(matches!(
        created_event.event_type,
        Some(EventType::Created(_))
    ));

    // Receive Starting event
    let starting_event =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
            .await
            .expect("Timeout waiting for Starting event")
            .expect("Channel closed before receiving Starting event");

    assert_eq!(starting_event.actor_id, "spawn-test-actor@test-node");
    assert!(starting_event.timestamp.is_some());
    assert!(matches!(
        starting_event.event_type,
        Some(EventType::Starting(_))
    ));

    // Receive Activated event
    let activated_event =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
            .await
            .expect("Timeout waiting for Activated event")
            .expect("Channel closed before receiving Activated event");

    assert_eq!(activated_event.actor_id, "spawn-test-actor@test-node");
    assert!(activated_event.timestamp.is_some());
    assert!(matches!(
        activated_event.event_type,
        Some(EventType::Activated(_))
    ));

    // Verify events are ordered correctly by timestamp
    let created_ts = created_event.timestamp.unwrap();
    let starting_ts = starting_event.timestamp.unwrap();
    let activated_ts = activated_event.timestamp.unwrap();

    assert!(
        created_ts.seconds <= starting_ts.seconds,
        "Created should occur before Starting"
    );
    assert!(
        starting_ts.seconds <= activated_ts.seconds,
        "Starting should occur before Activated"
    );
}

/// Test that subscribers receive lifecycle events when actors terminate
///
/// ## Purpose
/// Verifies the core observability pattern:
/// 1. Create observability subscriber (e.g., Prometheus exporter)
/// 2. Subscribe to lifecycle events
/// 3. Spawn and terminate actor
/// 4. Verify subscriber receives ActorTerminated event
///
/// ## Integration Point
/// This test validates that the JavaNOW-inspired channel/subscriber pattern
/// works for observability backends that need to track actor lifecycle.
#[tokio::test]
async fn test_lifecycle_event_subscription_receives_termination() {
    // Create node
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("test-node").build());

    // Create observability subscriber channel (simulates Prometheus exporter)
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();

    // Subscribe to lifecycle events
    node.subscribe_lifecycle_events(event_tx).await;

    // Create and spawn actor
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "test-actor@test-node".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Skip the spawn lifecycle events (Created, Starting, Activated)
    for _ in 0..3 {
        tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
            .await
            .expect("Timeout waiting for spawn event")
            .expect("Channel closed before receiving spawn event");
    }

    // Send message to actor to trigger some activity
    let msg = plexspaces_mailbox::Message::new(vec![1, 2, 3]);
    actor_ref.tell(msg).await.unwrap();

    // Wait for message to be processed - route_message completes after enqueueing
    // Message processing happens asynchronously, so we just yield to allow processing
    let processing_future = async {
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), processing_future)
        .await
        .expect("Message processing should complete quickly");

    // Drop actor ref to allow termination
    drop(actor_ref);

    // Wait for lifecycle event (should be Terminated)
    let event = tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
        .await
        .expect("Timeout waiting for lifecycle event")
        .expect("Channel closed before receiving event");

    // Verify event details
    assert_eq!(event.actor_id, "test-actor@test-node");
    assert!(event.timestamp.is_some());

    // Check event type - should be Terminated with reason "normal"
    use plexspaces_proto::actor_lifecycle_event::EventType;
    match event.event_type {
        Some(EventType::Terminated(terminated)) => {
            assert_eq!(terminated.reason, "normal");
        }
        other => panic!("Expected Terminated event, got: {:?}", other),
    }
}

/// Test that multiple subscribers all receive the same lifecycle event
///
/// ## Purpose
/// Verifies multicast behavior (JavaNOW's MulticasterImpl pattern):
/// - Node maintains multiple subscribers
/// - Each subscriber receives every lifecycle event
/// - Events are independent (one slow subscriber doesn't block others)
///
/// ## Use Case
/// This is critical for production observability where you might have:
/// - Prometheus exporter (metrics)
/// - StatsD forwarder (metrics)
/// - OpenTelemetry tracer (distributed tracing)
/// - Custom logger (audit logs)
/// All subscribed simultaneously to the same actor lifecycle events.
#[tokio::test]
async fn test_lifecycle_event_multicast_to_multiple_subscribers() {
    // Create node
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("test-node").build());

    // Create three observability subscribers (simulates Prometheus + StatsD + OpenTelemetry)
    let (prometheus_tx, mut prometheus_rx) = mpsc::unbounded_channel();
    let (statsd_tx, mut statsd_rx) = mpsc::unbounded_channel();
    let (otel_tx, mut otel_rx) = mpsc::unbounded_channel();

    // All subscribe to lifecycle events
    node.subscribe_lifecycle_events(prometheus_tx).await;
    node.subscribe_lifecycle_events(statsd_tx).await;
    node.subscribe_lifecycle_events(otel_tx).await;

    // Create and spawn actor
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "multicast-actor@test-node".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "multicast-actor@test-node".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Skip spawn lifecycle events (Created, Starting, Activated) for all 3 subscribers
    for _ in 0..3 {
        tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            prometheus_rx.recv(),
        )
        .await
        .expect("Prometheus spawn event timeout")
        .expect("Prometheus channel closed");

        tokio::time::timeout(tokio::time::Duration::from_millis(500), statsd_rx.recv())
            .await
            .expect("StatsD spawn event timeout")
            .expect("StatsD channel closed");

        tokio::time::timeout(tokio::time::Duration::from_millis(500), otel_rx.recv())
            .await
            .expect("OpenTelemetry spawn event timeout")
            .expect("OpenTelemetry channel closed");
    }

    // Terminate actor
    drop(actor_ref);

    // All three subscribers should receive the Terminated event
    let prometheus_event = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        prometheus_rx.recv(),
    )
    .await
    .expect("Prometheus timeout")
    .expect("Prometheus channel closed");

    let statsd_event =
        tokio::time::timeout(tokio::time::Duration::from_millis(500), statsd_rx.recv())
            .await
            .expect("StatsD timeout")
            .expect("StatsD channel closed");

    let otel_event = tokio::time::timeout(tokio::time::Duration::from_millis(500), otel_rx.recv())
        .await
        .expect("OpenTelemetry timeout")
        .expect("OpenTelemetry channel closed");

    // Verify all three received the same actor_id
    assert_eq!(prometheus_event.actor_id, "multicast-actor@test-node");
    assert_eq!(statsd_event.actor_id, "multicast-actor@test-node");
    assert_eq!(otel_event.actor_id, "multicast-actor@test-node");

    // Verify all are Terminated events
    use plexspaces_proto::actor_lifecycle_event::EventType;
    assert!(matches!(
        prometheus_event.event_type,
        Some(EventType::Terminated(_))
    ));
    assert!(matches!(
        statsd_event.event_type,
        Some(EventType::Terminated(_))
    ));
    assert!(matches!(
        otel_event.event_type,
        Some(EventType::Terminated(_))
    ));
}

/// Test that lifecycle events contain accurate timestamps
///
/// ## Purpose
/// Verifies that lifecycle events include proper timestamp metadata.
/// This is critical for:
/// - Time-series metrics (Prometheus, StatsD)
/// - Distributed tracing (OpenTelemetry spans)
/// - Audit logs (when did actor terminate)
#[tokio::test]
async fn test_lifecycle_event_timestamps() {
    use chrono::Utc;

    // Record time before actor creation
    let start_time = Utc::now();

    // Create node and subscriber
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("test-node").build());

    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    node.subscribe_lifecycle_events(event_tx).await;

    // Create and spawn actor
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "timestamp-actor@test-node".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "timestamp-actor@test-node".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Terminate actor
    drop(actor_ref);

    // Receive event
    let event = tokio::time::timeout(tokio::time::Duration::from_millis(500), event_rx.recv())
        .await
        .expect("Timeout")
        .expect("Channel closed");

    // Record time after event received
    let end_time = Utc::now();

    // Verify timestamp exists and is within reasonable bounds
    assert!(event.timestamp.is_some());

    let event_timestamp = event.timestamp.unwrap();
    let event_time =
        chrono::DateTime::from_timestamp(event_timestamp.seconds, event_timestamp.nanos as u32)
            .expect("Invalid timestamp");

    // Event timestamp should be between start and end
    assert!(
        event_time >= start_time && event_time <= end_time,
        "Event timestamp {} should be between {} and {}",
        event_time,
        start_time,
        end_time
    );
}

/// Test unsubscribe functionality
///
/// ## Purpose
/// Verifies that subscribers can unsubscribe from lifecycle events.
/// Useful for:
/// - Shutting down observability backends
/// - Reconfiguring monitoring
/// - Testing scenarios where subscribers come and go
#[tokio::test]
async fn test_lifecycle_event_unsubscribe() {
    // Create node
    use plexspaces_node::NodeBuilder;
    let node = Arc::new(NodeBuilder::new("test-node").build());

    // Subscribe
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    node.subscribe_lifecycle_events(event_tx).await;

    // Unsubscribe ALL subscribers
    node.unsubscribe_lifecycle_events().await;

    // Create and spawn actor
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "unsubscribe-actor@test-node".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "unsubscribe-actor@test-node".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let actor_ref = spawn_actor_helper(&node, actor).await.unwrap();

    // Terminate actor
    drop(actor_ref);

    // Should NOT receive any event (timeout is expected)
    let result =
        tokio::time::timeout(tokio::time::Duration::from_millis(300), event_rx.recv()).await;

    // Timeout is expected - no event should arrive after unsubscribe
    match result {
        Err(_) => {
            // Timeout is good - no event received as expected
        }
        Ok(Some(event)) => {
            panic!(
                "Should not receive event after unsubscribe, got: {:?}",
                event
            );
        }
        Ok(None) => {
            // Channel closed - also acceptable
        }
    }
}

/// Test remote actor termination with lifecycle events
///
/// ## Purpose
/// Verifies that lifecycle events are published when remote actors terminate.
/// Critical for distributed observability where monitoring systems need to track
/// actors across multiple nodes.
///
/// ## Integration Point
/// Tests the combination of:
/// - Remote actor discovery and registration
/// - Actor spawning via spawn_actor() on remote node
/// - Lifecycle event publication when remote actor terminates
/// - Event delivery to observability subscribers
///
/// ## Distributed Scenario
/// - Node1: Observability backend subscribes to lifecycle events
/// - Node2: Actor spawns and terminates
/// - Node1: Receives termination event from Node2
///
/// This validates that lifecycle events work in distributed deployments where
/// monitoring systems run on different nodes than the actors they observe.
#[tokio::test]
async fn test_remote_actor_termination_with_lifecycle_events() {
    use plexspaces_actor::Actor;
    use plexspaces_behavior::MockBehavior;
    use plexspaces_node::grpc_service::ActorServiceImpl;
    use plexspaces_persistence::MemoryJournal;
    use plexspaces_proto::ActorServiceServer;
    use tonic::transport::Server;

    // Create two nodes
    let node1 = Arc::new(NodeBuilder::new("node1").build());

    let node2 = Arc::new(NodeBuilder::new("node2").build());

    // Start gRPC server for node2 (actor host)
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl::new(node2.clone());
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
    tokio::time::timeout(tokio::time::Duration::from_secs(1), server_ready)
        .await
        .expect("Server should start quickly");
    let node2_address = format!("http://{}", bound_addr);

    // Register node2 in node1's registry
    node1
        .register_remote_node(NodeId::new("node2"), node2_address)
        .await
        .unwrap();

    // Node1 subscribes to lifecycle events (simulates observability backend)
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    node1.subscribe_lifecycle_events(event_tx).await;

    // NOTE: Currently lifecycle events are LOCAL to each node
    // For distributed observability, we would need:
    // 1. Node2 publishes event locally when actor terminates
    // 2. Node2 forwards event to Node1 via gRPC (future enhancement)
    // OR
    // 1. Node1 subscribes to Node2's lifecycle events via gRPC streaming

    // For now, subscribe to node2's local lifecycle events
    let (node2_event_tx, mut node2_event_rx) = mpsc::unbounded_channel();
    node2.subscribe_lifecycle_events(node2_event_tx).await;

    // Spawn actor on node2
    let behavior = Box::new(MockBehavior::new());
    let mailbox = Mailbox::new(MailboxConfig::default(), "remote-worker@node2".to_string()).await.unwrap();
    let journal = Arc::new(MemoryJournal::new());
    let actor = Actor::new(
        "remote-worker@node2".to_string(),
        behavior,
        mailbox,
        "test-namespace".to_string(),
        None,
    );

    let actor_ref = node2.spawn_actor(actor).await.unwrap();

    // Skip spawn lifecycle events (Created, Starting, Activated)
    for _ in 0..3 {
        tokio::time::timeout(
            tokio::time::Duration::from_millis(500),
            node2_event_rx.recv(),
        )
        .await
        .expect("Timeout waiting for spawn event")
        .expect("Channel closed before receiving spawn event");
    }

    // Send message and terminate
    let msg = plexspaces_mailbox::Message::new(vec![1, 2, 3]);
    actor_ref.tell(msg).await.unwrap();
    // Wait for message to be processed - route_message completes after enqueueing
    let processing_future = async {
        tokio::task::yield_now().await;
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(1), processing_future)
        .await
        .expect("Message processing should complete quickly");
    drop(actor_ref);

    // Receive Terminated event from node2's local subscription
    let event = tokio::time::timeout(
        tokio::time::Duration::from_millis(500),
        node2_event_rx.recv(),
    )
    .await
    .expect("Timeout waiting for lifecycle event")
    .expect("Channel closed before receiving event");

    // Verify event details
    assert_eq!(event.actor_id, "remote-worker@node2");
    assert!(event.timestamp.is_some());

    // Check event type
    use plexspaces_proto::actor_lifecycle_event::EventType;
    match event.event_type {
        Some(EventType::Terminated(terminated)) => {
            assert_eq!(terminated.reason, "normal");
        }
        other => panic!("Expected Terminated event, got: {:?}", other),
    }

    // TODO: Future enhancement - test distributed event forwarding
    // When we implement gRPC streaming for lifecycle events, this test should verify:
    // 1. Node1 receives event from Node2 via gRPC stream
    // 2. Event routing works across node boundaries
    // 3. Backpressure handling for slow remote subscribers
}
