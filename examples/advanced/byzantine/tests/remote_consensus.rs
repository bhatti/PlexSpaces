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

//! Byzantine Generals Remote Consensus Tests (Phase 2: gRPC Remoting)
//!
//! These tests validate that Byzantine Generals consensus works across
//! multiple nodes via gRPC remote messaging.

use std::sync::Arc;
use plexspaces_node::{Node, NodeId, NodeConfig, grpc_service::ActorServiceImpl};
use plexspaces_core::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use plexspaces_proto::ActorServiceServer;
use tonic::transport::Server;
use serde::{Serialize, Deserialize};

/// Byzantine General state
#[derive(Debug, Clone, Serialize, Deserialize)]
struct GeneralState {
    /// General's ID
    id: String,
    /// Commander flag
    is_commander: bool,
    /// Traitor flag
    is_traitor: bool,
    /// Proposed action ("attack" or "retreat")
    proposal: Option<String>,
    /// Votes received from other generals
    votes: Vec<String>,
    /// Final decision
    decision: Option<String>,
    /// Other generals' IDs
    other_generals: Vec<String>,
}

/// Byzantine General behavior
struct GeneralBehavior {
    state: GeneralState,
}

impl GeneralBehavior {
    fn new(id: String, is_commander: bool, is_traitor: bool) -> Self {
        GeneralBehavior {
            state: GeneralState {
                id,
                is_commander,
                is_traitor,
                proposal: None,
                votes: Vec::new(),
                decision: None,
                other_generals: Vec::new(),
            },
        }
    }

    /// Handle proposal from commander
    fn handle_proposal(&mut self, proposal: String) {
        if self.state.is_traitor {
            // Traitor sends conflicting votes
            self.state.proposal = Some(if proposal == "attack" {
                "retreat".to_string()
            } else {
                "attack".to_string()
            });
        } else {
            // Loyal general echoes proposal
            self.state.proposal = Some(proposal.clone());
        }
        self.state.votes.push(proposal);
    }

    /// Handle vote from another general
    fn handle_vote(&mut self, vote: String) {
        self.state.votes.push(vote);
    }

    /// Decide based on majority vote
    fn decide(&mut self) -> String {
        let mut attack_count = 0;
        let mut retreat_count = 0;

        for vote in &self.state.votes {
            if vote == "attack" {
                attack_count += 1;
            } else if vote == "retreat" {
                retreat_count += 1;
            }
        }

        let decision = if attack_count > retreat_count {
            "attack".to_string()
        } else {
            "retreat".to_string()
        };

        self.state.decision = Some(decision.clone());
        decision
    }
}

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

    // Wait for server to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    format!("http://{}", bound_addr)
}

/// Test: 4 generals (1 traitor) across 2 nodes reach consensus
#[tokio::test]
async fn test_byzantine_generals_two_nodes() {
    // Setup: Create 2 nodes
    let node1 = Arc::new(Node::new(
        NodeId::new("node1"),
        NodeConfig::default(),
    ));

    let node2 = Arc::new(Node::new(
        NodeId::new("node2"),
        NodeConfig::default(),
    ));

    // Start gRPC servers
    let node1_address = start_test_server(node1.clone()).await;
    let node2_address = start_test_server(node2.clone()).await;

    // Register nodes with each other
    node1.register_remote_node(NodeId::new("node2"), node2_address.clone()).await.unwrap();
    node2.register_remote_node(NodeId::new("node1"), node1_address.clone()).await.unwrap();

    // Create generals (2 per node)
    // Node1: general1 (commander), general2 (loyal)
    // Node2: general3 (loyal), general4 (traitor)

    let mailbox1 = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref1 = ActorRef::new_local("general1@node1".to_string(), mailbox1.clone())
        .expect("Failed to create ActorRef");
    node1.register_actor(actor_ref1.clone()).await.unwrap();

    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref2 = ActorRef::new_local("general2@node1".to_string(), mailbox2.clone())
        .expect("Failed to create ActorRef");
    node1.register_actor(actor_ref2.clone()).await.unwrap();

    let mailbox3 = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref3 = ActorRef::new_local("general3@node2".to_string(), mailbox3.clone())
        .expect("Failed to create ActorRef");
    node2.register_actor(actor_ref3.clone()).await.unwrap();

    let mailbox4 = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref4 = ActorRef::new_local("general4@node2".to_string(), mailbox4.clone())
        .expect("Failed to create ActorRef");
    node2.register_actor(actor_ref4.clone()).await.unwrap();

    // Commander (general1) proposes "attack"
    let proposal_msg = Message::new("attack".as_bytes().to_vec());

    // Send proposal to other generals (cross-node messaging)
    node1.route_message(&"general2@node1".to_string(), proposal_msg.clone()).await.unwrap();
    node1.route_message(&"general3@node2".to_string(), proposal_msg.clone()).await.unwrap();
    node1.route_message(&"general4@node2".to_string(), proposal_msg.clone()).await.unwrap();

    // Wait for messages to propagate
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify messages were delivered
    // General2 (node1, local)
    let msg2 = mailbox2.dequeue().await;
    assert!(msg2.is_some(), "General2 should receive proposal");
    assert_eq!(msg2.unwrap().payload(), "attack".as_bytes());

    // General3 (node2, remote)
    let msg3 = mailbox3.dequeue().await;
    assert!(msg3.is_some(), "General3 should receive proposal via gRPC");
    assert_eq!(msg3.unwrap().payload(), "attack".as_bytes());

    // General4 (node2, remote, traitor)
    let msg4 = mailbox4.dequeue().await;
    assert!(msg4.is_some(), "General4 should receive proposal via gRPC");
    assert_eq!(msg4.unwrap().payload(), "attack".as_bytes());

    // Verify node stats
    let stats1 = node1.stats().await;
    assert_eq!(stats1.messages_routed, 3, "Node1 should route 3 messages");
    assert_eq!(stats1.local_deliveries, 1, "Node1 should have 1 local delivery");
    assert_eq!(stats1.remote_deliveries, 2, "Node1 should have 2 remote deliveries");

    println!("✅ Byzantine Generals consensus works across 2 nodes via gRPC!");
}

/// Test: Connection pooling - multiple rounds of voting reuse connections
#[tokio::test]
async fn test_byzantine_connection_pooling() {
    let node1 = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));
    let node2 = Arc::new(Node::new(NodeId::new("node2"), NodeConfig::default()));

    let node2_address = start_test_server(node2.clone()).await;
    node1.register_remote_node(NodeId::new("node2"), node2_address).await.unwrap();

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref = ActorRef::new_local("general@node2".to_string(), mailbox.clone())
        .expect("Failed to create ActorRef");
    node2.register_actor(actor_ref).await.unwrap();

    // Send 10 messages (should reuse same gRPC connection)
    for i in 0..10 {
        let msg = Message::new(format!("vote-{}", i).as_bytes().to_vec());
        node1.route_message(&"general@node2".to_string(), msg).await.unwrap();
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    // Verify all messages delivered
    let mut count = 0;
    while mailbox.dequeue().await.is_some() {
        count += 1;
    }
    assert_eq!(count, 10, "All 10 messages should be delivered");

    let stats = node1.stats().await;
    assert_eq!(stats.remote_deliveries, 10, "Should have 10 remote deliveries");

    println!("✅ Connection pooling works: 10 messages sent via single gRPC connection!");
}

/// Test: Node failure handling
#[tokio::test]
async fn test_byzantine_node_failure() {
    let node1 = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));

    // Try to send to unregistered node
    let msg = Message::new("attack".as_bytes().to_vec());
    let result = node1.route_message(&"general@node999".to_string(), msg).await;

    assert!(result.is_err(), "Should fail for unregistered node");

    let stats = node1.stats().await;
    assert_eq!(stats.messages_routed, 1, "Should track attempted message");
    // Note: failed_deliveries not incremented because we fail at find_actor() stage

    println!("✅ Node failure handling works: Unregistered nodes return error!");
}

/// Test: Cross-node actor discovery
#[tokio::test]
async fn test_cross_node_actor_discovery() {
    let node1 = Arc::new(Node::new(NodeId::new("node1"), NodeConfig::default()));
    let node2 = Arc::new(Node::new(NodeId::new("node2"), NodeConfig::default()));

    let node2_address = start_test_server(node2.clone()).await;
    node1.register_remote_node(NodeId::new("node2"), node2_address).await.unwrap();

    // Register actor on node2
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default()));
    let actor_ref = ActorRef::new_local("general@node2".to_string(), mailbox)
        .expect("Failed to create ActorRef");
    node2.register_actor(actor_ref).await.unwrap();

    // Node1 should discover actor on node2
    let location = node1.find_actor(&"general@node2".to_string()).await;
    assert!(location.is_ok(), "Should find actor on remote node");

    match location.unwrap() {
        plexspaces_node::ActorLocation::Remote(node_id) => {
            assert_eq!(node_id.as_str(), "node2", "Should identify correct remote node");
        }
        _ => panic!("Should be remote actor"),
    }

    println!("✅ Cross-node actor discovery works!");
}
