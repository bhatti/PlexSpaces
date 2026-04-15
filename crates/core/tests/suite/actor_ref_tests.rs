// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

use plexspaces_core::{ActorId, ActorRef};

fn actor_id(name: &str, node_id: &str) -> ActorId {
    ActorId::new(name, "worker", "default", node_id).expect("valid actor id")
}

#[test]
fn actor_ref_exposes_structured_identity_fields() {
    let actor_ref = ActorRef::new(actor_id("counter", "node1")).unwrap();

    assert_eq!(actor_ref.id().as_str(), "counter//worker::default@node1");
    assert_eq!(actor_ref.actor_name(), "counter");
    assert_eq!(actor_ref.actor_type(), "worker");
    assert_eq!(actor_ref.namespace(), "default");
    assert_eq!(actor_ref.node_id(), "node1");
}

#[test]
fn actor_ref_is_remote_when_node_differs() {
    let actor_ref = ActorRef::new(actor_id("actor", "node2")).unwrap();

    assert!(actor_ref.is_remote("node1"));
    assert!(!actor_ref.is_remote("node2"));
}

#[test]
fn actor_ref_equality_and_hash_use_canonical_identity() {
    use std::collections::HashMap;

    let ref1 = ActorRef::new(actor_id("actor", "node1")).unwrap();
    let ref2 = ActorRef::new(actor_id("actor", "node1")).unwrap();
    let ref3 = ActorRef::new(actor_id("actor", "node2")).unwrap();

    assert_eq!(ref1, ref2);
    assert_ne!(ref1, ref3);

    let mut map = HashMap::new();
    map.insert(ref1.clone(), "value1");
    map.insert(ref3.clone(), "value2");

    assert_eq!(map.get(&ref1), Some(&"value1"));
    assert_eq!(map.get(&ref2), Some(&"value1"));
    assert_eq!(map.get(&ref3), Some(&"value2"));
}

#[test]
fn actor_ref_debug_and_serde_use_canonical_form() {
    let actor_ref = ActorRef::new(actor_id("test", "node1")).unwrap();

    let debug_str = format!("{:?}", actor_ref);
    assert!(debug_str.contains("test//worker::default@node1"));

    let serialized = serde_json::to_string(&actor_ref).unwrap();
    let deserialized: ActorRef = serde_json::from_str(&serialized).unwrap();

    assert_eq!(actor_ref, deserialized);
    assert_eq!(deserialized.id().as_str(), "test//worker::default@node1");
}
