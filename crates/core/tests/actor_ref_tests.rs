// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorRef pure data structure (parsing, getters, remote detection)

use plexspaces_core::{ActorRef, ActorError};

#[test]
fn test_actor_ref_new() {
    let actor_ref = ActorRef::new("counter@node1".to_string()).unwrap();
    
    assert_eq!(actor_ref.id(), "counter@node1");
    assert_eq!(actor_ref.actor_name(), "counter");
    assert_eq!(actor_ref.node_id(), "node1");
}

#[test]
fn test_actor_ref_new_invalid_id() {
    let result = ActorRef::new("invalid".to_string());
    assert!(result.is_err());
    match result.unwrap_err() {
        ActorError::InvalidState(msg) => {
            assert!(msg.contains("Invalid actor ID format"));
        },
        _ => panic!("Expected InvalidState error"),
    }
}

#[test]
fn test_actor_ref_parse_actor_id_success() {
    let (name, node) = ActorRef::parse_actor_id("actor@node1").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "node1");
}

#[test]
fn test_actor_ref_parse_actor_id_failure() {
    let result = ActorRef::parse_actor_id("invalid");
    assert!(result.is_err());
}

#[test]
fn test_actor_ref_parse_actor_id_edge_cases() {
    // Multiple @ symbols (should split on first)
    let (name, node) = ActorRef::parse_actor_id("actor@node@extra").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "node@extra");
    
    // Empty actor name
    let (name, node) = ActorRef::parse_actor_id("@node").unwrap();
    assert_eq!(name, "");
    assert_eq!(node, "node");
    
    // Empty node name
    let (name, node) = ActorRef::parse_actor_id("actor@").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "");
}

#[test]
fn test_is_remote_actor() {
    assert!(ActorRef::is_remote_actor("actor@node2", "node1"));
    assert!(!ActorRef::is_remote_actor("actor@node1", "node1"));
    assert!(!ActorRef::is_remote_actor("invalid", "node1")); // Invalid format returns false
}

#[test]
fn test_actor_ref_is_remote() {
    let actor_ref = ActorRef::new("actor@node2".to_string()).unwrap();
    assert!(actor_ref.is_remote("node1"));
    assert!(!actor_ref.is_remote("node2"));
}

#[test]
fn test_actor_ref_getters() {
    let actor_ref = ActorRef::new("test-actor@prod-node-5".to_string()).unwrap();
    
    assert_eq!(actor_ref.id(), "test-actor@prod-node-5");
    assert_eq!(actor_ref.actor_name(), "test-actor");
    assert_eq!(actor_ref.node_id(), "prod-node-5");
}

#[test]
fn test_actor_ref_equality() {
    let ref1 = ActorRef::new("actor@node1".to_string()).unwrap();
    let ref2 = ActorRef::new("actor@node1".to_string()).unwrap();
    let ref3 = ActorRef::new("actor@node2".to_string()).unwrap();
    
    assert_eq!(ref1, ref2);
    assert_ne!(ref1, ref3);
}

#[test]
fn test_actor_ref_hash() {
    use std::collections::HashMap;
    
    let mut map = HashMap::new();
    let ref1 = ActorRef::new("actor@node1".to_string()).unwrap();
    let ref2 = ActorRef::new("actor@node2".to_string()).unwrap();
    
    map.insert(ref1.clone(), "value1");
    map.insert(ref2.clone(), "value2");
    
    assert_eq!(map.get(&ref1), Some(&"value1"));
    assert_eq!(map.get(&ref2), Some(&"value2"));
}
