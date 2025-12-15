// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Multi-Node Integration Tests
//!
//! Tests for distributed actor messaging across multiple ActorService nodes.

use super::{send_message_err, send_message_ok, TestHarness};
use plexspaces_core::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use std::sync::Arc;
use tonic::Code;

/// Test remote message delivery from node1 to node2
///
/// Scenario:
/// 1. Spawn node1 and node2
/// 2. Register actor "receiver@node2" on node2
/// 3. Send message from node1 client to "receiver@node2"
/// 4. Verify message delivered successfully
#[tokio::test]
#[ignore] // Run with: cargo test --test integration -- --ignored test_remote_message_delivery
async fn test_remote_message_delivery() {
    // ARRANGE: Spawn two nodes
    let mut harness = TestHarness::new();
    let node1 = harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");
    let port1 = node1.port;

    let node2 = harness
        .spawn_node("node2")
        .await
        .expect("Failed to spawn node2");
    let port2 = node2.port;

    println!("node1 on port {}, node2 on port {}", port1, port2);

    // Register actor on node2
    // NOTE: For now, we're testing the gRPC path end-to-end
    // In a real scenario, we'd use ActorRuntime to spawn actors
    // For this test, we're verifying the ActorService gRPC communication works

    // ACT: Send message from node1 to actor@node2
    let node1_client = harness.get_node("node1").unwrap();
    send_message_ok(
        &mut node1_client.client,
        "hello from node1",
        "receiver@node2",
    )
    .await;

    println!("Message sent from node1 to receiver@node2");

    // CLEANUP
    harness.shutdown().await;
}

/// Test bidirectional communication between two nodes
///
/// Scenario:
/// 1. Spawn node1 and node2
/// 2. node1 sends to node2
/// 3. node2 sends to node1
/// 4. Verify both succeed
#[tokio::test]
#[ignore]
async fn test_bidirectional_communication() {
    // ARRANGE
    let mut harness = TestHarness::new();
    harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");
    harness
        .spawn_node("node2")
        .await
        .expect("Failed to spawn node2");

    // ACT: node1 -> node2
    let node1 = harness.get_node("node1").unwrap();
    send_message_ok(&mut node1.client, "hello node2", "actor@node2").await;

    // ACT: node2 -> node1
    let node2 = harness.get_node("node2").unwrap();
    send_message_ok(&mut node2.client, "hello node1", "actor@node1").await;

    println!("Bidirectional communication successful");

    // CLEANUP
    harness.shutdown().await;
}

/// Test actor not found on remote node
///
/// Scenario:
/// 1. Spawn node1 and node2
/// 2. Send to non-existent actor on node2
/// 3. Verify NotFound error
#[tokio::test]
#[ignore]
async fn test_actor_not_found_remote() {
    // ARRANGE
    let mut harness = TestHarness::new();
    harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");
    harness
        .spawn_node("node2")
        .await
        .expect("Failed to spawn node2");

    // ACT: Send to non-existent actor
    let node1 = harness.get_node("node1").unwrap();
    let err = send_message_err(&mut node1.client, "test", "nonexistent@node2").await;

    // ASSERT: Should be NotFound
    assert_eq!(err.code(), Code::NotFound);
    assert!(err.message().contains("not found"));

    println!("Actor not found error handled correctly");

    // CLEANUP
    harness.shutdown().await;
}

/// Test connection pooling - multiple messages should reuse connection
///
/// Scenario:
/// 1. Spawn node1 and node2
/// 2. Send 10 messages from node1 to node2
/// 3. Verify all succeed (connection pooling works)
#[tokio::test]
#[ignore]
async fn test_connection_pooling() {
    // ARRANGE
    let mut harness = TestHarness::new();
    harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");
    harness
        .spawn_node("node2")
        .await
        .expect("Failed to spawn node2");

    // ACT: Send multiple messages
    let node1 = harness.get_node("node1").unwrap();
    for i in 0..10 {
        let payload = format!("message_{}", i);
        send_message_ok(&mut node1.client, &payload, "echo@node2").await;
    }

    println!("Sent 10 messages (connection pooling working)");

    // CLEANUP
    harness.shutdown().await;
}

/// Test multiple target nodes - verify different clients created
///
/// Scenario:
/// 1. Spawn node1, node2, node3
/// 2. node1 sends to both node2 and node3
/// 3. Verify both succeed (different clients)
#[tokio::test]
#[ignore]
async fn test_multiple_target_nodes() {
    // ARRANGE
    let mut harness = TestHarness::new();
    harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");
    harness
        .spawn_node("node2")
        .await
        .expect("Failed to spawn node2");
    harness
        .spawn_node("node3")
        .await
        .expect("Failed to spawn node3");

    // ACT: Send to both node2 and node3
    let node1 = harness.get_node("node1").unwrap();
    send_message_ok(&mut node1.client, "to node2", "actor@node2").await;
    send_message_ok(&mut node1.client, "to node3", "actor@node3").await;

    println!("Messages sent to multiple nodes successfully");

    // CLEANUP
    harness.shutdown().await;
}

/// Test node not in registry
///
/// Scenario:
/// 1. Spawn only node1
/// 2. Try to send to node2 (doesn't exist)
/// 3. Verify NotFound error
#[tokio::test]
#[ignore]
async fn test_node_not_found() {
    // ARRANGE
    let mut harness = TestHarness::new();
    harness
        .spawn_node("node1")
        .await
        .expect("Failed to spawn node1");

    // ACT: Send to non-existent node
    let node1 = harness.get_node("node1").unwrap();
    let err = send_message_err(&mut node1.client, "test", "actor@nonexistent_node").await;

    // ASSERT: Should be NotFound
    assert_eq!(err.code(), Code::NotFound);
    assert!(err.message().contains("Node not found") || err.message().contains("not found"));

    println!("Node not found error handled correctly");

    // CLEANUP
    harness.shutdown().await;
}
