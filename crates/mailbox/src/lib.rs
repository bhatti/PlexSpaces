// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Composable mailbox implementations for PlexSpaces
//!
//! This crate provides message queue abstractions for actors:
//! - Priority-based message delivery
//! - Backpressure support
//! - Async message handling
//! - Metrics and monitoring hooks

#![warn(missing_docs)]
#![warn(clippy::all)]

// Export the mailbox module

mod builder;
mod messages;
mod r#mod;

// Re-export all public items
pub use builder::MailboxBuilder;
pub use messages::*;
pub use r#mod::*;
// Re-export Message for convenience (mailbox tests use plexspaces_mailbox::Message)
pub use plexspaces_proto::common::v1::Message;
// message_helpers are re-exported from r#mod::* above, no need to re-export again

// TTL tests
#[cfg(test)]
mod ttl_tests;
