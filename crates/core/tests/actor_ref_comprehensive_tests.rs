// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive tests for ActorRef pure data structure

use plexspaces_core::{ActorRef, ActorError};

#[test]
fn test_actor_ref_getters() {
    let actor_ref = ActorRef::new("counter@node1".to_string()).unwrap();
    
    assert_eq!(actor_ref.id(), "counter@node1");
    assert_eq!(actor_ref.actor_name(), "counter");
    assert_eq!(actor_ref.node_id(), "node1");
}

#[test]
fn test_actor_ref_parse_actor_id_success() {
    let (name, node) = ActorRef::parse_actor_id("actor@node1").unwrap();
    assert_eq!(name, "actor");
    assert_eq!(node, "node1");
    
    let (name, node) = ActorRef::parse_actor_id("complex-actor-name@prod-node-5").unwrap();
    assert_eq!(name, "complex-actor-name");
    assert_eq!(node, "prod-node-5");
}

#[test]
fn test_actor_ref_parse_actor_id_failure() {
    let result = ActorRef::parse_actor_id("invalid");
    assert!(result.is_err());
    match result.unwrap_err() {
        ActorError::InvalidState(msg) => {
            assert!(msg.contains("Invalid actor ID format"));
        },
        _ => panic!("Expected InvalidState error"),
    }
}

#[test]
fn test_actor_ref_parse_actor_id_edge_cases() {
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

#[test]
fn test_actor_ref_is_remote() {
    let actor_ref = ActorRef::new("actor@node2".to_string()).unwrap();
    assert!(actor_ref.is_remote("node1"));
    assert!(!actor_ref.is_remote("node2"));
    
    let local_ref = ActorRef::new("actor@node1".to_string()).unwrap();
    assert!(!local_ref.is_remote("node1"));
    assert!(local_ref.is_remote("node2"));
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

#[test]
fn test_actor_ref_clone() {
    let ref1 = ActorRef::new("actor@node1".to_string()).unwrap();
    let ref2 = ref1.clone();
    
    assert_eq!(ref1, ref2);
    assert_eq!(ref1.id(), ref2.id());
    assert_eq!(ref1.actor_name(), ref2.actor_name());
    assert_eq!(ref1.node_id(), ref2.node_id());
}

#[test]
fn test_actor_ref_debug() {
    let actor_ref = ActorRef::new("test@node1".to_string()).unwrap();
    let debug_str = format!("{:?}", actor_ref);
    assert!(debug_str.contains("test@node1"));
    assert!(debug_str.contains("test"));
    assert!(debug_str.contains("node1"));
}

#[test]
fn test_actor_ref_serialize() {
    use serde_json;
    
    let actor_ref = ActorRef::new("actor@node1".to_string()).unwrap();
    let serialized = serde_json::to_string(&actor_ref).unwrap();
    let deserialized: ActorRef = serde_json::from_str(&serialized).unwrap();
    
    assert_eq!(actor_ref, deserialized);
}
