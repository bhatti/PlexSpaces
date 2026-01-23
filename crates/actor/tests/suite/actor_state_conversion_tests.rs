// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for ActorState enum conversion between Rust and proto

use plexspaces_actor::ActorState;
use plexspaces_proto::v1::actor::ActorState as ProtoActorState;

#[test]
fn test_actor_state_to_proto_unspecified() {
    let state = ActorState::Unspecified;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateUnspecified);
}

#[test]
fn test_actor_state_to_proto_creating() {
    let state = ActorState::Creating;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateCreating);
}

#[test]
fn test_actor_state_to_proto_active() {
    let state = ActorState::Active;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateActive);
}

#[test]
fn test_actor_state_to_proto_inactive() {
    let state = ActorState::Inactive;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateInactive);
}

#[test]
fn test_actor_state_to_proto_activating() {
    let state = ActorState::Activating;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateActivating);
}

#[test]
fn test_actor_state_to_proto_deactivating() {
    let state = ActorState::Deactivating;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateDeactivating);
}

#[test]
fn test_actor_state_to_proto_migrating() {
    let state = ActorState::Migrating;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateMigrating);
}

#[test]
fn test_actor_state_to_proto_failed() {
    let state = ActorState::Failed("test error".to_string());
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateFailed);
}

#[test]
fn test_actor_state_to_proto_terminated() {
    let state = ActorState::Terminated;
    assert_eq!(state.to_proto(), ProtoActorState::ActorStateTerminated);
}

#[test]
fn test_actor_state_from_proto_unspecified() {
    let proto = ProtoActorState::ActorStateUnspecified;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Unspecified);
}

#[test]
fn test_actor_state_from_proto_creating() {
    let proto = ProtoActorState::ActorStateCreating;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Creating);
}

#[test]
fn test_actor_state_from_proto_active() {
    let proto = ProtoActorState::ActorStateActive;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Active);
}

#[test]
fn test_actor_state_from_proto_inactive() {
    let proto = ProtoActorState::ActorStateInactive;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Inactive);
}

#[test]
fn test_actor_state_from_proto_activating() {
    let proto = ProtoActorState::ActorStateActivating;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Activating);
}

#[test]
fn test_actor_state_from_proto_deactivating() {
    let proto = ProtoActorState::ActorStateDeactivating;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Deactivating);
}

#[test]
fn test_actor_state_from_proto_migrating() {
    let proto = ProtoActorState::ActorStateMigrating;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Migrating);
}

#[test]
fn test_actor_state_from_proto_failed_with_message() {
    let proto = ProtoActorState::ActorStateFailed;
    let state = ActorState::from_proto(proto, Some("test error".to_string()));
    assert_eq!(state, ActorState::Failed("test error".to_string()));
}

#[test]
fn test_actor_state_from_proto_failed_without_message() {
    let proto = ProtoActorState::ActorStateFailed;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Failed(String::new()));
}

#[test]
fn test_actor_state_from_proto_terminated() {
    let proto = ProtoActorState::ActorStateTerminated;
    let state = ActorState::from_proto(proto, None);
    assert_eq!(state, ActorState::Terminated);
}

#[test]
fn test_actor_state_round_trip() {
    let states = vec![
        ActorState::Unspecified,
        ActorState::Creating,
        ActorState::Active,
        ActorState::Inactive,
        ActorState::Activating,
        ActorState::Deactivating,
        ActorState::Migrating,
        ActorState::Failed("error".to_string()),
        ActorState::Terminated,
    ];

    for state in states {
        let proto = state.to_proto();
        let error_msg = match &state {
            ActorState::Failed(msg) => Some(msg.clone()),
            _ => None,
        };
        let round_trip = ActorState::from_proto(proto, error_msg);
        assert_eq!(state, round_trip, "Round trip failed for {:?}", state);
    }
}
