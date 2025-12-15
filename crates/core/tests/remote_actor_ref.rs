// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorRef pure data structure (parsing, getters, remote detection)

use plexspaces_core::{ActorRef, ActorError};

/// Test: Parse actor ID format
#[test]
fn test_parse_actor_id_success() {
    let (name, node) = ActorRef::parse_actor_id("actor@node1").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "node1");
    
    let (name, node) = ActorRef::parse_actor_id("complex-actor-name@prod-node-5").unwrap();
    assert_eq!(name, "complex-actor-name");
    assert_eq!(node, "prod-node-5");
}

/// Test: Parse actor ID failure
#[test]
fn test_parse_actor_id_failure() {
    let result = ActorRef::parse_actor_id("invalid");
    assert!(result.is_err());
    match result.unwrap_err() {
        ActorError::InvalidState(msg) => {
            assert!(msg.contains("Invalid actor ID format"));
        },
        _ => panic!("Expected InvalidState error"),
    }
}

/// Test: Parse actor ID edge cases
#[test]
fn test_parse_actor_id_edge_cases() {
    // Empty actor name
    let (name, node) = ActorRef::parse_actor_id("@node1").unwrap();
    assert_eq!(name, "");
    assert_eq!(node, "node1");
    
    // Empty node name
    let (name, node) = ActorRef::parse_actor_id("actor@").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "");
    
    // Multiple @ symbols (should split on first)
    let (name, node) = ActorRef::parse_actor_id("actor@node@extra").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "node@extra");
}

/// Test: Check if actor is remote (static helper)
#[test]
fn test_is_remote_actor() {
    // Same node - not remote
    assert!(!ActorRef::is_remote_actor("actor@node1", "node1"));
    
    // Different node - remote
    assert!(ActorRef::is_remote_actor("actor@node2", "node1"));
    
    // Invalid format - returns false
    assert!(!ActorRef::is_remote_actor("invalid", "node1"));
    
    // Complex node names
    assert!(ActorRef::is_remote_actor("actor@prod-5", "dev-1"));
    assert!(!ActorRef::is_remote_actor("actor@prod-5", "prod-5"));
}

/// Test: Create ActorRef and access getters
#[test]
fn test_actor_ref_getters() {
    let actor_ref = ActorRef::new("counter@node1".to_string()).unwrap();
    
    assert_eq!(actor_ref.id(), "counter@node1");
    assert_eq!(actor_ref.actor_name(), "counter");
    assert_eq!(actor_ref.node_id(), "node1");
}

/// Test: Check if actor is remote (instance method)
#[test]
fn test_actor_ref_is_remote() {
    let actor_ref = ActorRef::new("actor@node2".to_string()).unwrap();
    assert!(actor_ref.is_remote("node1"));
    assert!(!actor_ref.is_remote("node2"));
    
    let local_ref = ActorRef::new("actor@node1".to_string()).unwrap();
    assert!(!local_ref.is_remote("node1"));
    assert!(local_ref.is_remote("node2"));
}
