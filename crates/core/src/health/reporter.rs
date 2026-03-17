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

//! Health Reporter trait for health monitoring
//!
//! ## Purpose
//! Defines the interface for health reporting used by SystemService and other components.
//! This trait allows components to depend on health functionality without depending on
//! the concrete implementation.

use async_trait::async_trait;
use plexspaces_proto::system::v1::{
    DetailedHealthCheck, NodeHealthState, NodeReadinessStatus, ServingStatus,
};
use prost_types::Duration;

/// Trait for health reporting
///
/// ## Purpose
/// Provides health status information for system components.
/// Implementations track liveness, readiness, and startup status.
#[async_trait]
pub trait HealthReporter: Send + Sync {
    /// Check if the system is alive
    ///
    /// ## Returns
    /// `true` if alive, `false` if dead/crashed
    async fn is_alive(&self) -> bool;

    /// Check if the system is ready to serve traffic
    ///
    /// ## Returns
    /// Tuple of `(is_ready, not_ready_reason)`
    /// - `is_ready`: `true` if ready, `false` otherwise
    /// - `not_ready_reason`: Reason if not ready, `None` if ready
    async fn check_readiness(&self) -> (bool, Option<String>);

    /// Check if startup is complete
    ///
    /// ## Returns
    /// Tuple of `(startup_complete, not_complete_reason)`
    /// - `startup_complete`: `true` if startup complete, `false` otherwise
    /// - `not_complete_reason`: Reason if not complete, `None` if complete
    async fn check_startup(&self) -> (bool, Option<String>);

    /// Get current readiness status
    ///
    /// ## Returns
    /// Current `NodeReadinessStatus`
    async fn get_readiness(&self) -> NodeReadinessStatus;

    /// Get current health state
    ///
    /// ## Returns
    /// Current `NodeHealthState`
    async fn get_state(&self) -> NodeHealthState;

    /// Get detailed health check results
    ///
    /// ## Arguments
    /// * `include_non_critical` - Whether to include non-critical dependency checks
    ///
    /// ## Returns
    /// `DetailedHealthCheck` with all health information
    async fn get_detailed_health(&self, include_non_critical: bool) -> DetailedHealthCheck;

    /// Mark startup complete (NOT_SERVING → SERVING transition)
    ///
    /// ## Arguments
    /// * `message` - Optional message explaining what was initialized
    ///
    /// ## Returns
    /// Startup duration
    async fn mark_startup_complete(&self, message: Option<String>) -> Duration;

    /// Begin graceful shutdown sequence
    ///
    /// ## Arguments
    /// * `drain_timeout` - Override default drain timeout (default: 30s from config)
    ///
    /// ## Returns
    /// Tuple of:
    /// - `requests_drained`: Number of requests drained
    /// - `drain_duration`: Time taken to drain
    /// - `drain_completed`: Whether drain completed or timed out
    async fn begin_shutdown(&self, drain_timeout: Option<Duration>) -> (u64, Duration, bool);

    /// Set service-specific health status
    ///
    /// ## Arguments
    /// * `service_name` - Service name (e.g., "plexspaces.actor.v1.ActorService")
    /// * `status` - Serving status for the service
    async fn set_service_status(&self, service_name: &str, status: ServingStatus);

    /// Get service-specific health status
    ///
    /// ## Arguments
    /// * `service_name` - Service name to check
    ///
    /// ## Returns
    /// `ServingStatus` for the service, or `ServingStatus::ServingStatusUnknown` if not tracked
    async fn get_service_status(&self, service_name: &str) -> ServingStatus;

    /// Get all service health statuses
    ///
    /// ## Returns
    /// HashMap of service names to their health statuses
    async fn get_all_service_statuses(&self) -> std::collections::HashMap<String, ServingStatus>;

    /// Check if shutdown is in progress
    ///
    /// ## Returns
    /// `true` if shutdown is in progress, `false` otherwise
    async fn is_shutting_down(&self) -> bool;

    /// Update in-flight request count (called by Node)
    ///
    /// ## Purpose
    /// Node calls this to update request count for accurate draining
    async fn update_in_flight_requests(&self, count: u64);

    /// Check if node is ready (readiness probe)
    ///
    /// ## Purpose
    /// Readiness check for Kubernetes. Returns false if node should not receive requests.
    async fn is_ready(&self) -> bool;
}
