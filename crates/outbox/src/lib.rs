// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Flow-controlled messaging for PlexSpaces.
//!
//! ## Purpose
//! Provides sender-side (outbox relay + AIMD concurrency) and receiver-side
//! (bounded mailbox backpressure) flow control for high-fan-out messaging scenarios.
//!
//! ## Architecture Context
//! Sits at Layer 6 alongside journaling and elastic-pool. Depends on:
//! - `plexspaces-proto` (L0) — outbox proto types
//! - `plexspaces-common` (L1) — shared retry utility
//!
//! ## Components
//! - [`ConcurrencyLimiter`] — AIMD concurrency control (pure logic, atomic)
//! - [`OutboxRelay`] — Async drain loop bridging channel to actor delivery
//! - [`PartitionedOutboxRelay`] — N independent partitions with hash routing for scale
//! - [`HashRouter`] — FNV-1a hash routing (target_actor_id to partition)
//! - [`ConcurrencyLimiter`] — standalone AIMD utility for outbound throttling
//! - Dispatcher helpers — `parse_dispatch_request`, `build_dispatch_response`, etc.

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod concurrency_limiter;
pub mod dispatch_helpers;
pub mod hash_router;
pub mod outbox_relay;
pub mod partitioned_relay;

/// Sentinel matched in WASM outbox host and mapped to DeliveryError::Capacity.
pub const RESOURCE_EXHAUSTED: &str = "RESOURCE_EXHAUSTED";

pub use concurrency_limiter::{ConcurrencyLimiter, Permit};
pub use dispatch_helpers::{
    build_dispatch_response, build_dispatcher_status, parse_dispatch_request, validate_config,
};
pub use hash_router::HashRouter;
pub use outbox_relay::{DeliveryError, MessageDelivery, OutboxRelay, RelayStats};
pub use partitioned_relay::PartitionedOutboxRelay;
