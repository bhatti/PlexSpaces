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

//! Typed Messages for Actor Communication
//!
//! ## Purpose
//! Provides type-safe message handling using enums instead of HashMap metadata parsing.
//! Reduces boilerplate and improves developer experience.
//!
//! ## Design
//! - Enum-based messages (type-safe, pattern matching)
//! - Backward compatible with existing Message struct
//! - Conversion helpers for easy migration

use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Typed actor message (enum-based for type safety)
///
/// ## Usage
/// ```rust,no_run
/// use plexspaces_mailbox::Message;
/// # let msg = Message::new(b"data".to_vec());
/// match msg.as_typed() {
///     Ok(typed_msg) => {
///         // Handle typed message based on variant
///     }
///     Err(_) => {
///         // Handle conversion error
///     }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ActorMessage {
    /// Timer fired event (from TimerFacet)
    TimerFired {
        /// Timer name
        timer_name: String,
        /// When timer fired
        fired_at: Option<SystemTime>,
        /// Callback data (if any)
        callback_data: Vec<u8>,
    },
    
    /// Reminder fired event (from ReminderFacet)
    ReminderFired {
        /// Reminder name
        reminder_name: String,
        /// When reminder fired
        fired_at: Option<SystemTime>,
        /// Reminder data
        reminder_data: Vec<u8>,
    },
    
    /// User-defined message
    User {
        /// Message payload
        payload: Vec<u8>,
        /// Message type (for routing)
        message_type: String,
    },
    
    /// System message (internal framework messages)
    System {
        /// System message kind
        kind: SystemMessageKind,
        /// Message payload
        payload: Vec<u8>,
    },
}

/// System message kinds
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SystemMessageKind {
    /// Actor activation
    Activated,
    /// Actor deactivation
    Deactivated,
    /// Actor termination
    Terminated,
    /// Health check
    HealthCheck,
    /// Unknown system message
    Unknown(String),
}

impl ActorMessage {
    /// Create a TimerFired message
    pub fn timer_fired(timer_name: String, fired_at: Option<SystemTime>, callback_data: Vec<u8>) -> Self {
        Self::TimerFired {
            timer_name,
            fired_at,
            callback_data,
        }
    }
    
    /// Create a User message
    pub fn user(payload: Vec<u8>, message_type: String) -> Self {
        Self::User {
            payload,
            message_type,
        }
    }
    
    /// Create a System message
    pub fn system(kind: SystemMessageKind, payload: Vec<u8>) -> Self {
        Self::System {
            kind,
            payload,
        }
    }
}

// Conversion helpers will be added to Message impl in mod.rs

