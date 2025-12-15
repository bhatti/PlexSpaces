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

//! Actor Metrics - Track actor lifecycle and message routing/delivery metrics
//!
//! ## Purpose
//! Extracted from NodeMetrics to allow ActorRegistry to track actor and message metrics
//! without depending on Node. This enables better separation of concerns.
//!
//! ## Design
//! Uses proto-generated ActorMetrics for consistency with other metrics types.
//! Proto-first design ensures wire compatibility and language-agnostic observability.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Re-export proto-generated ActorMetrics
pub use plexspaces_proto::metrics::v1::ActorMetrics;

/// Extension trait for ActorMetrics to add helper methods
pub trait ActorMetricsExt {
    /// Create new ActorMetrics with all counters at zero
    fn new() -> Self;
    
    /// Increment spawn_total counter
    fn increment_spawn_total(&mut self);
    
    /// Increment active counter
    fn increment_active(&mut self);
    
    /// Decrement active counter
    fn decrement_active(&mut self);
    
    /// Increment messages_routed counter
    fn increment_messages_routed(&mut self);
    
    /// Increment local_deliveries counter
    fn increment_local_deliveries(&mut self);
    
    /// Increment remote_deliveries counter
    fn increment_remote_deliveries(&mut self);
    
    /// Increment failed_deliveries counter
    fn increment_failed_deliveries(&mut self);
    
    /// Increment error_total counter
    fn increment_error_total(&mut self);
    
    /// Get a snapshot of current metrics
    fn snapshot(&self) -> ActorMetrics;
}

impl ActorMetricsExt for ActorMetrics {
    fn new() -> Self {
        Self {
            spawn_total: 0,
            active: 0,
            messages_routed: 0,
            local_deliveries: 0,
            remote_deliveries: 0,
            failed_deliveries: 0,
            error_total: 0,
        }
    }

    fn increment_spawn_total(&mut self) {
        self.spawn_total = self.spawn_total.saturating_add(1);
    }

    fn increment_active(&mut self) {
        self.active = self.active.saturating_add(1);
    }

    fn decrement_active(&mut self) {
        self.active = self.active.saturating_sub(1);
    }

    fn increment_messages_routed(&mut self) {
        self.messages_routed = self.messages_routed.saturating_add(1);
    }

    fn increment_local_deliveries(&mut self) {
        self.local_deliveries = self.local_deliveries.saturating_add(1);
    }

    fn increment_remote_deliveries(&mut self) {
        self.remote_deliveries = self.remote_deliveries.saturating_add(1);
    }

    fn increment_failed_deliveries(&mut self) {
        self.failed_deliveries = self.failed_deliveries.saturating_add(1);
    }

    fn increment_error_total(&mut self) {
        self.error_total = self.error_total.saturating_add(1);
    }

    fn snapshot(&self) -> ActorMetrics {
        self.clone()
    }
}

/// Thread-safe wrapper for ActorMetrics
pub type ActorMetricsHandle = Arc<RwLock<ActorMetrics>>;

/// Create a new ActorMetricsHandle
pub fn new_actor_metrics() -> ActorMetricsHandle {
    Arc::new(RwLock::new(ActorMetricsExt::new()))
}
