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

//! Tests for ExitReason and ExitAction (proto-first, TDD)

use plexspaces_core::{ActorId, ExitAction, ExitReason};

#[test]
fn test_exit_reason_normal() {
    let reason = ExitReason::Normal;
    assert_eq!(reason, ExitReason::Normal);
}

#[test]
fn test_exit_reason_shutdown() {
    let reason = ExitReason::Shutdown;
    assert_eq!(reason, ExitReason::Shutdown);
}

#[test]
fn test_exit_reason_killed() {
    let reason = ExitReason::Killed;
    assert_eq!(reason, ExitReason::Killed);
}

#[test]
fn test_exit_reason_error() {
    let reason = ExitReason::Error("test error".to_string());
    match reason {
        ExitReason::Error(msg) => assert_eq!(msg, "test error"),
        _ => panic!("Expected Error variant"),
    }
}

#[test]
fn test_exit_reason_linked() {
    let linked_reason = ExitReason::Error("linked actor error".to_string());
    let reason = ExitReason::Linked {
        actor_id: ActorId::from("actor-1"),
        reason: Box::new(linked_reason.clone()),
    };
    
    match reason {
        ExitReason::Linked { actor_id, reason } => {
            assert_eq!(actor_id, ActorId::from("actor-1"));
            assert_eq!(*reason, linked_reason);
        }
        _ => panic!("Expected Linked variant"),
    }
}

#[test]
fn test_exit_reason_clone() {
    let reason1 = ExitReason::Error("test".to_string());
    let reason2 = reason1.clone();
    assert_eq!(reason1, reason2);
}

#[test]
fn test_exit_reason_debug() {
    let reason = ExitReason::Normal;
    let debug_str = format!("{:?}", reason);
    assert!(debug_str.contains("Normal"));
}

#[test]
fn test_exit_action_propagate() {
    let action = ExitAction::Propagate;
    assert_eq!(action, ExitAction::Propagate);
}

#[test]
fn test_exit_action_handle() {
    let action = ExitAction::Handle;
    assert_eq!(action, ExitAction::Handle);
}

#[test]
fn test_exit_action_clone() {
    let action1 = ExitAction::Propagate;
    let action2 = action1.clone();
    assert_eq!(action1, action2);
}

// Proto conversion tests will be added after buf generate
// For now, we test the core functionality




