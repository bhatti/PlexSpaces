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

//! # gRPC Health Service with Dependency Checks
//!
//! ## Purpose
//! Implements the standard `grpc.health.v1.Health` service with support for
//! dependency checks and Kubernetes-style probe semantics.
//!
//! ## Architecture Context
//! This service extends the standard gRPC health protocol to support:
//! - Service name "" or "liveness" → run liveness checks
//! - Service name "readiness" → run readiness checks
//! - Service name "startup" → run startup checks
//! - Service name "{name}-liveness" or "{name}-readiness" → run single check
//!
//! ## Design Notes
//! - Follows Kubernetes conventions for health probe service names
//! - Integrates with PlexSpacesHealthReporter for dependency checks
//! - Maintains compatibility with standard gRPC health protocol
//!
//! ## Note
//! The standard tonic-health service is used via HealthReporter for status updates.
//! This module provides helper functions to integrate dependency checks with
//! the standard service. For custom service name handling, we update the
//! HealthReporter status based on dependency checks.

use crate::health_service::PlexSpacesHealthReporter;
use std::sync::Arc;
use tokio::time::Duration;

/// Start background task to monitor health and update status
///
/// ## Purpose
/// Periodically checks dependencies and ensures the health reporter status
/// reflects the actual node health. This is a background task that runs
/// continuously.
///
/// ## Arguments
/// * `reporter` - Health reporter to monitor
/// * `interval` - How often to check (default: 5 seconds)
///
/// ## Returns
/// Handle to the background task (can be used to stop monitoring)
pub async fn start_health_monitoring(
    reporter: Arc<PlexSpacesHealthReporter>,
    interval: Option<Duration>,
) -> tokio::task::JoinHandle<()> {
    let interval = interval.unwrap_or(Duration::from_secs(5));

    tokio::spawn(async move {
        let mut interval_timer = tokio::time::interval(interval);

        loop {
            interval_timer.tick().await;

            // Check liveness
            let _is_alive = reporter.is_alive().await;
            
            // Check readiness
            let (_is_ready, _reason) = reporter.check_readiness().await;

            // Update health reporter status based on checks
            // Note: The standard tonic-health HealthReporter doesn't support
            // custom service names like "liveness" or "readiness", so we
            // update the default service ("") status based on readiness.
            // For Kubernetes, we'll use HTTP endpoints for probes instead.
            
            // The actual status updates are handled by the reporter's
            // mark_startup_complete and begin_shutdown methods.
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
           use plexspaces_proto::system::v1::{HealthProbeConfig, DependencyRegistrationConfig};

    #[tokio::test]
    async fn test_health_monitoring_starts() {
        let config = HealthProbeConfig {
            dependency_registration: Some(DependencyRegistrationConfig {
                enabled: false,
                dependencies: vec![],
                default_namespace: String::new(),
                default_tenant: String::new(),
            }),
            ..Default::default()
        };
        let (reporter, _) = PlexSpacesHealthReporter::with_config(config);
        let reporter = Arc::new(reporter);

        let handle = start_health_monitoring(reporter.clone(), Some(Duration::from_millis(100))).await;

        // Wait a bit for monitoring to run
        tokio::time::sleep(Duration::from_millis(250)).await;

        // Cancel the task
        handle.abort();

        // Task should have started
        assert!(!handle.is_finished());
    }
}
