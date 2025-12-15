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

//! # Health Checker Trait
//!
//! ## Purpose
//! Defines the health checker interface for dependency checks, following industry
//! standards (similar to Go's health.Checker pattern).
//!
//! ## Architecture Context
//! This module provides the foundation for dependency health checks:
//! - Critical dependencies block readiness/startup if unhealthy
//! - Non-critical dependencies allow partial failure (circuit breaker fallback)
//! - Supports both liveness and readiness checks
//!
//! ## Design Notes
//! - Inspired by the sample_health Go implementation
//! - Each dependency implements the HealthChecker trait
//! - Checks are async to support network calls
//! - Errors indicate unhealthy dependencies

use plexspaces_proto::system::v1::{DependencyCheck, HealthStatus};
use prost_types::{Duration, Timestamp};
use std::time::SystemTime;
use thiserror::Error;

/// Error type for health checks
#[derive(Debug, Error)]
pub enum HealthCheckError {
    #[error("Health check failed: {0}")]
    CheckFailed(String),
    
    #[error("Health check timeout: {0}")]
    Timeout(String),
    
    #[error("Health check error: {0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

/// Result type for health checks
pub type HealthCheckResult = Result<(), HealthCheckError>;

/// Health checker trait for dependencies
///
/// ## Purpose
/// Defines the interface for checking the health of a dependency.
/// Each dependency (database, external service, etc.) implements this trait.
///
/// ## Design Notes
/// - Similar to Go's `health.Checker` interface
/// - Async to support network calls
/// - Returns error if dependency is unhealthy
///
/// ## Examples
/// ```rust,ignore
/// struct DatabaseChecker {
///     connection: DatabaseConnection,
/// }
///
/// #[async_trait]
/// impl HealthChecker for DatabaseChecker {
///     fn name(&self) -> &str {
///         "database"
///     }
///
///     fn is_critical(&self) -> bool {
///         true  // Database is critical for readiness
///     }
///
///     async fn check(&self, _ctx: &HealthCheckContext) -> HealthCheckResult {
///         self.connection.ping().await
///             .map_err(|e| HealthCheckError::CheckFailed(e.to_string()))
///     }
/// }
/// ```
#[async_trait::async_trait]
pub trait HealthChecker: Send + Sync {
    /// Name of the dependency (e.g., "database", "redis", "external-api")
    fn name(&self) -> &str;

    /// Whether this dependency is critical for readiness/startup
    /// - `true`: Node not ready if dependency unhealthy
    /// - `false`: Node can be ready even if dependency unhealthy (fallback)
    fn is_critical(&self) -> bool;

    /// Perform the health check
    ///
    /// ## Arguments
    /// * `ctx` - Health check context (timeout, etc.)
    ///
    /// ## Returns
    /// * `Ok(())` - Dependency is healthy
    /// * `Err(HealthCheckError)` - Dependency is unhealthy
    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult;
}

/// Context for health checks
#[derive(Debug, Clone)]
pub struct HealthCheckContext {
    /// Timeout for the check
    pub timeout: Option<std::time::Duration>,
}

impl Default for HealthCheckContext {
    fn default() -> Self {
        Self {
            timeout: Some(std::time::Duration::from_secs(5)),
        }
    }
}

/// Run a health check and return a DependencyCheck result
///
/// ## Purpose
/// Executes a health check and converts the result to a proto DependencyCheck.
///
/// ## Arguments
/// * `checker` - The health checker to run
/// * `ctx` - Health check context
///
/// ## Returns
/// `DependencyCheck` with status, error message, and timing
pub async fn run_health_check(
    checker: &dyn HealthChecker,
    ctx: &HealthCheckContext,
) -> DependencyCheck {
    let start = SystemTime::now();
    let name = checker.name().to_string();
    let is_critical = checker.is_critical();

    let result = checker.check(ctx).await;
    let checked_at = SystemTime::now();
    let response_time = checked_at
        .duration_since(start)
        .unwrap_or_default();

    match result {
        Ok(()) => DependencyCheck {
            name,
            is_critical,
            status: HealthStatus::HealthStatusHealthy as i32,
            error_message: String::new(),
            checked_at: Some(Timestamp::from(checked_at)),
            response_time: Some(Duration {
                seconds: response_time.as_secs() as i64,
                nanos: response_time.subsec_nanos() as i32,
            }),
            details: std::collections::HashMap::new(),
        },
        Err(e) => DependencyCheck {
            name,
            is_critical,
            status: HealthStatus::HealthStatusUnhealthy as i32,
            error_message: e.to_string(),
            checked_at: Some(Timestamp::from(checked_at)),
            response_time: Some(Duration {
                seconds: response_time.as_secs() as i64,
                nanos: response_time.subsec_nanos() as i32,
            }),
            details: std::collections::HashMap::new(),
        },
    }
}

/// Ping check (always passes)
///
/// ## Purpose
/// Basic liveness check that always succeeds. Used to verify the health
/// check system is working.
pub struct PingChecker;

#[async_trait::async_trait]
impl HealthChecker for PingChecker {
    fn name(&self) -> &str {
        "ping"
    }

    fn is_critical(&self) -> bool {
        false  // Ping is not critical
    }

    async fn check(&self, _ctx: &HealthCheckContext) -> HealthCheckResult {
        Ok(())  // Always succeeds
    }
}

/// Shutdown check (fails if shutdown in progress)
///
/// ## Purpose
/// Checks if the node is shutting down. Used in readiness checks to
/// prevent new traffic during graceful shutdown.
pub struct ShutdownChecker {
    shutdown_tx: tokio::sync::watch::Receiver<bool>,
}

impl ShutdownChecker {
    /// Create a new `ShutdownChecker`
    pub fn new(shutdown_rx: tokio::sync::watch::Receiver<bool>) -> Self {
        Self {
            shutdown_tx: shutdown_rx,
        }
    }
}

#[async_trait::async_trait]
impl HealthChecker for ShutdownChecker {
    fn name(&self) -> &str {
        "shutdown"
    }

    fn is_critical(&self) -> bool {
        true  // Shutdown blocks readiness
    }

    async fn check(&self, _ctx: &HealthCheckContext) -> HealthCheckResult {
        if *self.shutdown_tx.borrow() {
            Err(HealthCheckError::CheckFailed(
                "Process is shutting down".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_ping_checker() {
        let checker = PingChecker;
        let ctx = HealthCheckContext::default();

        let result = checker.check(&ctx).await;
        assert!(result.is_ok());
        assert_eq!(checker.name(), "ping");
        assert!(!checker.is_critical());
    }

    #[tokio::test]
    async fn test_shutdown_checker_not_shutting_down() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let checker = ShutdownChecker::new(rx);
        let ctx = HealthCheckContext::default();

        let result = checker.check(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_shutdown_checker_shutting_down() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let checker = ShutdownChecker::new(rx);
        let ctx = HealthCheckContext::default();

        // Signal shutdown
        tx.send(true).unwrap();

        let result = checker.check(&ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("shutting down"));
    }

    #[tokio::test]
    async fn test_run_health_check_success() {
        let checker = PingChecker;
        let ctx = HealthCheckContext::default();

        let result = run_health_check(&checker, &ctx).await;
        assert_eq!(result.status, HealthStatus::HealthStatusHealthy as i32);
        assert!(result.error_message.is_empty());
        assert!(result.checked_at.is_some());
        assert!(result.response_time.is_some());
    }

    #[tokio::test]
    async fn test_run_health_check_failure() {
        let (tx, rx) = tokio::sync::watch::channel(false);
        let checker = ShutdownChecker::new(rx);
        let ctx = HealthCheckContext::default();

        // Signal shutdown
        tx.send(true).unwrap();

        let result = run_health_check(&checker, &ctx).await;
        assert_eq!(result.status, HealthStatus::HealthStatusUnhealthy as i32);
        assert!(!result.error_message.is_empty());
        assert!(result.checked_at.is_some());
    }
}

