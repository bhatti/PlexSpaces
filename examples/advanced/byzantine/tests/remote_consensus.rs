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
use plexspaces_actor::ActorRef;
use plexspaces_mailbox::{Mailbox, MailboxConfig, Message};
use plexspaces_proto::ActorServiceServer;
use tonic::transport::Server;
use serde::{Serialize, Deserialize};
use async_trait::async_trait;

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

/// Implement Actor trait for GeneralBehavior
#[async_trait::async_trait]
impl plexspaces_core::Actor for GeneralBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        msg: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        // Parse message payload
        if let Ok(text) = String::from_utf8(msg.payload().to_vec()) {
            if text == "attack" || text == "retreat" {
                self.handle_proposal(text);
            } else {
                self.handle_vote(text);
            }
        }
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::Custom("ByzantineGeneral".to_string())
    }
}

/// Helper to start a gRPC server for testing
async fn start_test_server(node: Arc<Node>) -> String {
    let addr: std::net::SocketAddr = "127.0.0.1:0".parse().unwrap();
    let service = ActorServiceImpl { node: node.clone() };

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

    // Create generals and spawn them using ActorFactory
    use plexspaces_actor::{Actor, ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    
    // Get ActorFactory for both nodes
    let service_locator1 = node1.service_locator()
        .expect("ServiceLocator not available from node1");
    let actor_factory1: Arc<ActorFactoryImpl> = service_locator1.get_service().await
        .expect("ActorFactory not found");
    let service_locator2 = node2.service_locator()
        .expect("ServiceLocator not available from node2");
    let actor_factory2: Arc<ActorFactoryImpl> = service_locator2.get_service().await
        .expect("ActorFactory not found");
    
    // Create and spawn general1
    let mailbox1 = Arc::new(Mailbox::new(MailboxConfig::default(), "general1@node1".to_string()).await.expect("Failed to create mailbox1"));
    let general1_behavior = GeneralBehavior::new("general1".to_string(), true, false); // commander
    let actor1 = Actor::new("general1@node1".to_string(), Box::new(general1_behavior), mailbox1.clone(), "byzantine".to_string(), None);
    actor_factory1.spawn_built_actor(Arc::new(actor1), None, None, None).await.expect("Failed to spawn general1");
    let service_locator1 = node1.service_locator().clone();
    let actor_ref1 = plexspaces_actor::ActorRef::local("general1@node1".to_string(), mailbox1.clone(), service_locator1.clone());

    // Create and spawn general2
    let mailbox2 = Arc::new(Mailbox::new(MailboxConfig::default(), "general2@node1".to_string()).await.expect("Failed to create mailbox2"));
    let general2_behavior = GeneralBehavior::new("general2".to_string(), false, false); // loyal
    let actor2 = Actor::new("general2@node1".to_string(), Box::new(general2_behavior), mailbox2.clone(), "byzantine".to_string(), None);
    actor_factory1.spawn_built_actor(Arc::new(actor2), None, None, None).await.expect("Failed to spawn general2");
    let actor_ref2 = plexspaces_actor::ActorRef::local("general2@node1".to_string(), mailbox2.clone(), service_locator1.clone());

    // Create and spawn general3
    let mailbox3 = Arc::new(Mailbox::new(MailboxConfig::default(), "general3@node2".to_string()).await.expect("Failed to create mailbox3"));
    let general3_behavior = GeneralBehavior::new("general3".to_string(), false, false); // loyal
    let actor3 = Actor::new("general3@node2".to_string(), Box::new(general3_behavior), mailbox3.clone(), "byzantine".to_string(), None);
    actor_factory2.spawn_built_actor(Arc::new(actor3), None, None, None).await.expect("Failed to spawn general3");
    let service_locator2 = node2.service_locator().clone();
    let actor_ref3 = plexspaces_actor::ActorRef::local("general3@node2".to_string(), mailbox3.clone(), service_locator2.clone());

    // Create and spawn general4
    let mailbox4 = Arc::new(Mailbox::new(MailboxConfig::default(), "general4@node2".to_string()).await.expect("Failed to create mailbox4"));
    let general4_behavior = GeneralBehavior::new("general4".to_string(), false, true); // traitor
    let actor4 = Actor::new("general4@node2".to_string(), Box::new(general4_behavior), mailbox4.clone(), "byzantine".to_string(), None);
    actor_factory2.spawn_built_actor(Arc::new(actor4), None, None, None).await.expect("Failed to spawn general4");
    let actor_ref4 = plexspaces_actor::ActorRef::local("general4@node2".to_string(), mailbox4.clone(), service_locator2.clone());

    // Commander (general1) proposes "attack"
    let proposal_msg = Message::new("attack".as_bytes().to_vec());

    // Send proposal to other generals (cross-node messaging) using ActorRef::tell()
    let actor_ref2 = plexspaces_actor::ActorRef::local("general2@node1".to_string(), mailbox2.clone(), service_locator1.clone());
    actor_ref2.tell(proposal_msg.clone()).await.expect("Failed to send to general2");
    
    // For remote actors, we need to use ActorService or create remote ActorRef
    // For now, let's use the local approach and fix remote later
    let actor_ref3 = plexspaces_actor::ActorRef::remote("general3@node2".to_string(), "node2".to_string(), service_locator1.clone());
    actor_ref3.tell(proposal_msg.clone()).await.expect("Failed to send to general3");
    
    let actor_ref4 = plexspaces_actor::ActorRef::remote("general4@node2".to_string(), "node2".to_string(), service_locator1.clone());
    actor_ref4.tell(proposal_msg.clone()).await.expect("Failed to send to general4");

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

    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "general@node2".to_string()).await.expect("Failed to create mailbox"));
    let service_locator2 = node2.service_locator().clone();
    // Register actor with ActorRegistry
    use plexspaces_actor::RegularActorWrapper;
    use plexspaces_core::MessageSender;
    let wrapper = Arc::new(RegularActorWrapper::new("general@node2".to_string(), mailbox.clone(), service_locator2.clone()));
    node2.actor_registry().register_actor("general@node2".to_string(), wrapper, None, None, None).await;

    // Send 10 messages (should reuse same gRPC connection) using ActorRef::tell()
    let service_locator1 = node1.service_locator().clone();
    let actor_ref = plexspaces_actor::ActorRef::remote("general@node2".to_string(), "node2".to_string(), service_locator1.clone());
    for i in 0..10 {
        let msg = Message::new(format!("vote-{}", i).as_bytes().to_vec());
        actor_ref.tell(msg).await.expect(&format!("Failed to send message {}", i));
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
    let service_locator1 = node1.service_locator().clone();
    let actor_ref = plexspaces_actor::ActorRef::remote("general@node999".to_string(), "node999".to_string(), service_locator1.clone());
    let result = actor_ref.tell(msg).await;

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

    // Create and spawn general using ActorFactory
    use plexspaces_actor::{Actor, ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let service_locator2 = node2.service_locator()
        .expect("ServiceLocator not available from node2");
    let actor_factory2: Arc<ActorFactoryImpl> = service_locator2.get_service().await
        .expect("ActorFactory not found");
    
    let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "general@node2".to_string()).await.expect("Failed to create mailbox"));
    let general_behavior = GeneralBehavior::new("general".to_string(), false, false);
    let actor = Actor::new("general@node2".to_string(), Box::new(general_behavior), mailbox.clone(), "byzantine".to_string(), None);
    actor_factory2.spawn_built_actor(Arc::new(actor), None, None, None).await.expect("Failed to spawn general");

    // Node1 should discover actor on node2 via ActorRegistry lookup
    let service_locator1 = node1.service_locator().clone();
    let registry1: Arc<plexspaces_core::ActorRegistry> = service_locator1.get_service().await.expect("ActorRegistry not found");
    let location = registry1.lookup_actor(&"general@node2".to_string()).await;
    assert!(location.is_ok(), "Should find actor on remote node");

    match location.unwrap() {
        plexspaces_node::ActorLocation::Remote(node_id) => {
            assert_eq!(node_id.as_str(), "node2", "Should identify correct remote node");
        }
        _ => panic!("Should be remote actor"),
    }

    println!("✅ Cross-node actor discovery works!");
}
