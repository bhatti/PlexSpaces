// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for Node integration with ServiceLocator (TDD)

use plexspaces_core::{ActorRegistry, ReplyTracker, ServiceLocator};
use plexspaces_node::{Node, NodeConfig, NodeId};
#[cfg(feature = "firecracker")]
use plexspaces_node::service_wrappers::FirecrackerVmServiceWrapper;
use std::sync::Arc;

#[tokio::test]
async fn test_node_creates_service_locator() {
    // Test: Node should create ServiceLocator in new()
    let node = Node::new(NodeId::new("test-node"), NodeConfig::default());
    
    // Verify ServiceLocator exists and can retrieve services
    let service_locator = node.service_locator();
    
    // Wait for services to be registered - poll until they're available
    let registration_future = async {
        loop {
            let actor_registry: Option<Arc<ActorRegistry>> = service_locator.get_service().await;
            let reply_tracker: Option<Arc<ReplyTracker>> = service_locator.get_service().await;
            if actor_registry.is_some() && reply_tracker.is_some() {
                return (actor_registry, reply_tracker);
            }
            tokio::task::yield_now().await;
        }
    };
    let (actor_registry, reply_tracker) = tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("Services should be registered within 5 seconds");
    
    // ActorRegistry should be registered
    assert!(actor_registry.is_some());
    
    // ReplyTracker should be registered
    assert!(reply_tracker.is_some());
}

#[tokio::test]
async fn test_node_registers_actor_registry() {
    // Test: Node should register ActorRegistry in ServiceLocator
    let node = Node::new(NodeId::new("test-node"), NodeConfig::default());
    let service_locator = node.service_locator();
    
    // Wait for async registration - poll until ActorRegistry is available
    let registration_future = async {
        loop {
            if let Some(registry) = service_locator.get_service::<ActorRegistry>().await {
                return registry;
            }
            tokio::task::yield_now().await;
        }
    };
    let actor_registry = tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("ActorRegistry should be registered within 5 seconds");
    
    // Verify it's the same instance as node.actor_registry
    let node_registry = node.actor_registry();
    assert_eq!(Arc::as_ptr(&actor_registry), Arc::as_ptr(&node_registry));
}

#[tokio::test]
async fn test_node_registers_reply_tracker() {
    // Test: Node should register ReplyTracker in ServiceLocator
    let node = Node::new(NodeId::new("test-node"), NodeConfig::default());
    let service_locator = node.service_locator();
    
    // Wait for async registration - poll until ReplyTracker is available
    let registration_future = async {
        loop {
            if let Some(tracker) = service_locator.get_service::<ReplyTracker>().await {
                return tracker;
            }
            tokio::task::yield_now().await;
        }
    };
    let reply_tracker = tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("ReplyTracker should be registered within 5 seconds");
    
    // Verify it's a valid ReplyTracker instance
    assert!(Arc::as_ptr(&reply_tracker) != std::ptr::null());
}

#[cfg(feature = "firecracker")]
#[tokio::test]
async fn test_node_registers_firecracker_service() {
    // Test: Node should register FirecrackerVmServiceWrapper in ServiceLocator when firecracker feature is enabled
    let node = Node::new(NodeId::new("test-node"), NodeConfig::default());
    let service_locator = node.service_locator();
    
    // Wait for async registration - poll until FirecrackerVmServiceWrapper is available
    let registration_future = async {
        loop {
            use plexspaces_node::service_wrappers::FirecrackerVmServiceWrapper;
            if let Some(service) = service_locator.get_service::<FirecrackerVmServiceWrapper>().await {
                return service;
            }
            tokio::task::yield_now().await;
        }
    };
    
    let firecracker_service = tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("FirecrackerVmServiceWrapper should be registered within 5 seconds");
    
    // Verify it's a valid service instance
    assert!(Arc::as_ptr(&firecracker_service) != std::ptr::null());
}

#[tokio::test]
async fn test_node_service_locator_shutdown() {
    // Test: Node shutdown should shutdown ServiceLocator (close gRPC clients)
    let node = Node::new(NodeId::new("test-node"), NodeConfig::default());
    let service_locator = node.service_locator();
    
    // Wait for async registration - poll until ActorRegistry is available
    let registration_future = async {
        loop {
            if service_locator.get_service::<ActorRegistry>().await.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    };
    tokio::time::timeout(tokio::time::Duration::from_secs(5), registration_future)
        .await
        .expect("Services should be registered within 5 seconds");
    
    // Shutdown should not panic
    service_locator.shutdown().await;
    
    // Verify services are still accessible after shutdown
    let actor_registry: Option<Arc<ActorRegistry>> = service_locator.get_service().await;
    assert!(actor_registry.is_some());
}
