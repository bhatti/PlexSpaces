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

//! Comprehensive tests for ExitReason and ExitAction (edge cases and proto conversion)

use plexspaces_core::{ExitAction, ExitReason};
use plexspaces_proto::v1::actor::{ExitReason as ProtoExitReason, ExitAction as ProtoExitAction, ExitReasonDetails};

#[test]
fn test_exit_reason_normal() {
    let reason = ExitReason::Normal;
    assert!(reason.is_normal());
    assert!(!reason.is_error());
    assert_eq!(reason.error_message(), None);
}

#[test]
fn test_exit_reason_shutdown() {
    let reason = ExitReason::Shutdown;
    assert!(!reason.is_normal());
    assert!(!reason.is_error());
    assert_eq!(reason.error_message(), None);
}

#[test]
fn test_exit_reason_killed() {
    let reason = ExitReason::Killed;
    assert!(!reason.is_normal());
    assert!(!reason.is_error());
    assert_eq!(reason.error_message(), None);
}

#[test]
fn test_exit_reason_error() {
    let reason = ExitReason::Error("database timeout".to_string());
    assert!(!reason.is_normal());
    assert!(reason.is_error());
    assert_eq!(reason.error_message(), Some("database timeout"));
}

#[test]
fn test_exit_reason_linked() {
    let linked_reason = ExitReason::Error("nested error".to_string());
    let reason = ExitReason::Linked {
        actor_id: "linked-actor".to_string(),
        reason: Box::new(linked_reason),
    };
    assert!(!reason.is_normal());
    assert!(reason.is_error());
    assert_eq!(reason.error_message(), Some("nested error"));
}

#[test]
fn test_exit_reason_linked_nested() {
    let nested_reason = ExitReason::Error("deep error".to_string());
    let linked_reason = ExitReason::Linked {
        actor_id: "nested-actor".to_string(),
        reason: Box::new(nested_reason),
    };
    let reason = ExitReason::Linked {
        actor_id: "linked-actor".to_string(),
        reason: Box::new(linked_reason),
    };
    assert!(!reason.is_normal());
    assert!(reason.is_error());
    assert_eq!(reason.error_message(), Some("deep error"));
}

#[test]
fn test_exit_reason_to_proto() {
    assert_eq!(ExitReason::Normal.to_proto(), ProtoExitReason::ExitReasonNormal);
    assert_eq!(ExitReason::Shutdown.to_proto(), ProtoExitReason::ExitReasonShutdown);
    assert_eq!(ExitReason::Killed.to_proto(), ProtoExitReason::ExitReasonKilled);
    assert_eq!(ExitReason::Error("test".to_string()).to_proto(), ProtoExitReason::ExitReasonError);
    
    let linked = ExitReason::Linked {
        actor_id: "actor1".to_string(),
        reason: Box::new(ExitReason::Normal),
    };
    assert_eq!(linked.to_proto(), ProtoExitReason::ExitReasonLinked);
}

#[test]
fn test_exit_reason_to_proto_details() {
    let error_reason = ExitReason::Error("test error".to_string());
    let details = error_reason.to_proto_details();
    assert!(details.is_some());
    let details = details.unwrap();
    assert_eq!(details.error_message, "test error");
    assert_eq!(details.linked_actor_id, "");
    
    let linked_reason = ExitReason::Linked {
        actor_id: "actor1".to_string(),
        reason: Box::new(ExitReason::Error("nested".to_string())),
    };
    let details = linked_reason.to_proto_details();
    assert!(details.is_some());
    let details = details.unwrap();
    assert_eq!(details.linked_actor_id, "actor1");
    assert!(details.linked_reason.is_some());
}

#[test]
fn test_exit_reason_from_proto() {
    let proto = ProtoExitReason::ExitReasonNormal;
    let reason = ExitReason::from_proto(proto, None);
    assert_eq!(reason, ExitReason::Normal);
    
    let proto = ProtoExitReason::ExitReasonShutdown;
    let reason = ExitReason::from_proto(proto, None);
    assert_eq!(reason, ExitReason::Shutdown);
    
    let proto = ProtoExitReason::ExitReasonKilled;
    let reason = ExitReason::from_proto(proto, None);
    assert_eq!(reason, ExitReason::Killed);
    
    let proto = ProtoExitReason::ExitReasonError;
    let details = Some(ExitReasonDetails {
        error_message: "test error".to_string(),
        linked_actor_id: String::new(),
        linked_reason: None,
    });
    let reason = ExitReason::from_proto(proto, details.as_ref());
    assert_eq!(reason, ExitReason::Error("test error".to_string()));
    
    let proto = ProtoExitReason::ExitReasonLinked;
    let details = Some(ExitReasonDetails {
        error_message: String::new(),
        linked_actor_id: "actor1".to_string(),
        linked_reason: Some(ProtoExitReason::ExitReasonError as i32),
    });
    let reason = ExitReason::from_proto(proto, details.as_ref());
    match reason {
        ExitReason::Linked { actor_id, .. } => {
            assert_eq!(actor_id, "actor1");
        }
        _ => panic!("Expected Linked reason"),
    }
}

#[test]
fn test_exit_action_to_proto() {
    assert_eq!(ExitAction::Propagate.to_proto(), ProtoExitAction::ExitActionPropagate);
    assert_eq!(ExitAction::Handle.to_proto(), ProtoExitAction::ExitActionHandle);
}

#[test]
fn test_exit_action_from_proto() {
    assert_eq!(ExitAction::from_proto(ProtoExitAction::ExitActionPropagate), ExitAction::Propagate);
    assert_eq!(ExitAction::from_proto(ProtoExitAction::ExitActionHandle), ExitAction::Handle);
    assert_eq!(ExitAction::from_proto(ProtoExitAction::ExitActionUnspecified), ExitAction::Propagate); // Default
}

#[test]
fn test_exit_reason_clone_and_debug() {
    let reason1 = ExitReason::Error("test".to_string());
    let reason2 = reason1.clone();
    assert_eq!(reason1, reason2);
    
    // Test debug formatting
    let debug_str = format!("{:?}", reason1);
    assert!(debug_str.contains("Error"));
    assert!(debug_str.contains("test"));
}

#[test]
fn test_exit_action_clone_and_debug() {
    let action1 = ExitAction::Propagate;
    let action2 = action1.clone();
    assert_eq!(action1, action2);
    
    // Test debug formatting
    let debug_str = format!("{:?}", action1);
    assert!(debug_str.contains("Propagate"));
}

#[test]
fn test_exit_reason_equality() {
    assert_eq!(ExitReason::Normal, ExitReason::Normal);
    assert_eq!(ExitReason::Shutdown, ExitReason::Shutdown);
    assert_eq!(ExitReason::Killed, ExitReason::Killed);
    assert_eq!(
        ExitReason::Error("test".to_string()),
        ExitReason::Error("test".to_string())
    );
    assert_ne!(ExitReason::Normal, ExitReason::Shutdown);
    assert_ne!(
        ExitReason::Error("error1".to_string()),
        ExitReason::Error("error2".to_string())
    );
}

#[test]
fn test_exit_action_equality() {
    assert_eq!(ExitAction::Propagate, ExitAction::Propagate);
    assert_eq!(ExitAction::Handle, ExitAction::Handle);
    assert_ne!(ExitAction::Propagate, ExitAction::Handle);
}


