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

//! Exit reason and exit action types (proto-first design)
//!
//! ## Purpose
//! Defines why an actor terminated, enabling proper supervision decisions
//! and link/monitor propagation semantics (Erlang/OTP-style).
//!
//! ## Proto-First Design
//! These types are defined in `proto/plexspaces/v1/actors/actor_runtime.proto`
//! and generated via `buf generate`. This module provides Rust-friendly wrappers
//! and conversion methods.

use crate::ActorId;

/// Exit reason for actor termination (Erlang/OTP-style)
///
/// ## Erlang Equivalent
/// Maps to Erlang's exit reasons:
/// - `normal`: Normal termination (not an error)
/// - `shutdown`: Graceful shutdown requested
/// - `kill`: Forcefully killed
/// - `{error, Reason}`: Error with message
/// - `{linked, Pid, Reason}`: Linked actor died
///
/// ## Usage
/// - Used in `Actor::terminate()` and `Actor::handle_exit()` lifecycle hooks
/// - Propagated to linked actors (if trap_exit=false, causes cascading crash)
/// - Used by supervisors to decide restart policies
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    /// Normal termination (not an error)
    Normal,
    /// Shutdown requested (graceful)
    Shutdown,
    /// Killed forcefully
    Killed,
    /// Error with message
    Error(String),
    /// Linked actor died
    Linked {
        /// ID of the linked actor that died
        actor_id: ActorId,
        /// The exit reason from the linked actor
        reason: Box<ExitReason>,
    },
}

/// Exit action when handling EXIT from linked actor
///
/// ## Purpose
/// Determines how an actor responds to EXIT signal from a linked actor.
/// Only relevant when `ActorContext.trap_exit = true`.
///
/// ## Erlang Equivalent
/// Maps to Erlang's handle_exit behavior:
/// - `Propagate`: Actor will also terminate (default link behavior)
/// - `Handle`: Actor continues running (traps the exit)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitAction {
    /// Propagate exit - this actor will also terminate
    Propagate,
    /// Handle exit - this actor continues running
    Handle,
}

impl ExitReason {
    /// Convert to proto ExitReason enum
    pub fn to_proto(&self) -> plexspaces_proto::v1::actor::ExitReason {
        use plexspaces_proto::v1::actor::ExitReason as ProtoExitReason;
        match self {
            ExitReason::Normal => ProtoExitReason::ExitReasonNormal,
            ExitReason::Shutdown => ProtoExitReason::ExitReasonShutdown,
            ExitReason::Killed => ProtoExitReason::ExitReasonKilled,
            ExitReason::Error(_) => ProtoExitReason::ExitReasonError,
            ExitReason::Linked { .. } => ProtoExitReason::ExitReasonLinked,
        }
    }

    /// Convert to proto ExitReasonDetails (for error/linked reasons)
    pub fn to_proto_details(&self) -> Option<plexspaces_proto::v1::actor::ExitReasonDetails> {
        match self {
            ExitReason::Error(msg) => Some(plexspaces_proto::v1::actor::ExitReasonDetails {
                error_message: msg.clone(),
                linked_actor_id: String::new(),
                linked_reason: 0, // ExitReasonUnspecified
            }),
            ExitReason::Linked { actor_id, reason } => Some(plexspaces_proto::v1::actor::ExitReasonDetails {
                error_message: String::new(),
                linked_actor_id: actor_id.clone(),
                linked_reason: reason.to_proto() as i32,
            }),
            _ => None,
        }
    }

    /// Convert from proto ExitReason enum
    ///
    /// ## Arguments
    /// * `proto` - Proto exit reason enum value
    /// * `details` - Optional ExitReasonDetails for error/linked reasons
    pub fn from_proto(
        proto: plexspaces_proto::v1::actor::ExitReason,
        details: Option<&plexspaces_proto::v1::actor::ExitReasonDetails>,
    ) -> Self {
        use plexspaces_proto::v1::actor::ExitReason as ProtoExitReason;
        
        match proto {
            ProtoExitReason::ExitReasonNormal => ExitReason::Normal,
            ProtoExitReason::ExitReasonShutdown => ExitReason::Shutdown,
            ProtoExitReason::ExitReasonKilled => ExitReason::Killed,
            ProtoExitReason::ExitReasonError => {
                let error_msg = details
                    .and_then(|d| {
                        if !d.error_message.is_empty() {
                            Some(d.error_message.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "unknown error".to_string());
                ExitReason::Error(error_msg)
            }
            ProtoExitReason::ExitReasonLinked => {
                let actor_id = details
                    .and_then(|d| {
                        if !d.linked_actor_id.is_empty() {
                            Some(d.linked_actor_id.clone())
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "unknown".to_string());
                let linked_reason = details
                    .and_then(|d| {
                        if d.linked_reason != 0 {
                            ProtoExitReason::try_from(d.linked_reason).ok()
                                .map(|proto_reason| Box::new(ExitReason::from_proto(proto_reason, None)))
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| Box::new(ExitReason::Normal));
                ExitReason::Linked { actor_id, reason: linked_reason }
            }
            ProtoExitReason::ExitReasonUnspecified => ExitReason::Normal, // Default to Normal
        }
    }

    /// Check if this is a normal exit (not an error)
    pub fn is_normal(&self) -> bool {
        matches!(self, ExitReason::Normal)
    }

    /// Check if this is an error exit
    pub fn is_error(&self) -> bool {
        matches!(self, ExitReason::Error(_) | ExitReason::Linked { .. })
    }

    /// Get error message if this is an error exit
    pub fn error_message(&self) -> Option<&str> {
        match self {
            ExitReason::Error(msg) => Some(msg.as_str()),
            ExitReason::Linked { reason, .. } => reason.error_message(),
            _ => None,
        }
    }

    /// Convert from string representation (for backward compatibility)
    ///
    /// ## Purpose
    /// Converts string exit reasons (from old notify_actor_down API) to ExitReason enum.
    ///
    /// ## Arguments
    /// * `reason_str` - String representation of exit reason
    ///
    /// ## Returns
    /// ExitReason enum value
    ///
    /// ## Examples
    /// - "normal" -> ExitReason::Normal
    /// - "shutdown" -> ExitReason::Shutdown
    /// - "killed" -> ExitReason::Killed
    /// - "error message" -> ExitReason::Error("error message")
    /// - "linked:actor-id:reason" -> ExitReason::Linked { ... }
    pub fn from_str(reason_str: &str) -> Self {
        match reason_str {
            "normal" => ExitReason::Normal,
            "shutdown" => ExitReason::Shutdown,
            "killed" => ExitReason::Killed,
            s if s.starts_with("linked:") => {
                // Parse "linked:actor-id:reason" format
                let parts: Vec<&str> = s.splitn(3, ':').collect();
                if parts.len() >= 3 {
                    let linked_actor_id = parts[1].to_string();
                    let linked_reason_str = parts[2];
                    let linked_reason = ExitReason::from_str(linked_reason_str);
                    ExitReason::Linked {
                        actor_id: linked_actor_id,
                        reason: Box::new(linked_reason),
                    }
                } else {
                    ExitReason::Error(s.to_string())
                }
            }
            s => ExitReason::Error(s.to_string()),
        }
    }
}

impl ExitAction {
    /// Convert to proto ExitAction enum
    pub fn to_proto(&self) -> plexspaces_proto::v1::actor::ExitAction {
        use plexspaces_proto::v1::actor::ExitAction as ProtoExitAction;
        match self {
            ExitAction::Propagate => ProtoExitAction::ExitActionPropagate,
            ExitAction::Handle => ProtoExitAction::ExitActionHandle,
        }
    }

    /// Convert from proto ExitAction enum
    pub fn from_proto(proto: plexspaces_proto::v1::actor::ExitAction) -> Self {
        use plexspaces_proto::v1::actor::ExitAction as ProtoExitAction;
        
        match proto {
            ProtoExitAction::ExitActionPropagate => ExitAction::Propagate,
            ProtoExitAction::ExitActionHandle => ExitAction::Handle,
            ProtoExitAction::ExitActionUnspecified => ExitAction::Propagate, // Default to Propagate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_reason_normal() {
        let reason = ExitReason::Normal;
        assert!(reason.is_normal());
        assert!(!reason.is_error());
    }

    #[test]
    fn test_exit_reason_error() {
        let reason = ExitReason::Error("test error".to_string());
        assert!(!reason.is_normal());
        assert!(reason.is_error());
        assert_eq!(reason.error_message(), Some("test error"));
    }

    #[test]
    fn test_exit_reason_linked() {
        let linked_reason = ExitReason::Error("nested error".to_string());
        let reason = ExitReason::Linked {
            actor_id: "actor-1".to_string(),
            reason: Box::new(linked_reason),
        };
        assert!(!reason.is_normal());
        assert!(reason.is_error());
        assert_eq!(reason.error_message(), Some("nested error"));
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
}




