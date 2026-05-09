// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Health checker and reporter traits.

use async_trait::async_trait;
use plexspaces_proto::system::v1::{
    DependencyCheck, DetailedHealthCheck, HealthStatus, NodeHealthState, NodeReadinessStatus,
    ServingStatus,
};
use prost_types::{Duration, Timestamp};
use std::time::SystemTime;
use thiserror::Error;

/// Error type for health checks.
#[derive(Debug, Error)]
pub enum HealthCheckError {
    #[error("Health check failed: {0}")]
    CheckFailed(String),

    #[error("Health check timeout: {0}")]
    Timeout(String),

    #[error("Health check error: {0}")]
    Other(#[from] Box<dyn std::error::Error + Send + Sync>),
}

impl HealthCheckError {
    /// Return the proto error code for this error.
    pub fn code(&self) -> plexspaces_proto::node::v1::HealthCheckErrorCode {
        use plexspaces_proto::node::v1::HealthCheckErrorCode;
        match self {
            HealthCheckError::CheckFailed(_) => HealthCheckErrorCode::HealthCheckErrorCheckFailed,
            HealthCheckError::Timeout(_) => HealthCheckErrorCode::HealthCheckErrorTimeout,
            HealthCheckError::Other(_) => HealthCheckErrorCode::HealthCheckErrorOther,
        }
    }
}

/// Result type for health checks.
pub type HealthCheckResult = Result<(), HealthCheckError>;

/// Context for health checks.
#[derive(Debug, Clone)]
pub struct HealthCheckContext {
    /// Timeout for the check.
    pub timeout: Option<std::time::Duration>,
}

impl Default for HealthCheckContext {
    fn default() -> Self {
        Self {
            timeout: Some(std::time::Duration::from_secs(5)),
        }
    }
}

/// Health checker trait for dependencies.
#[async_trait]
pub trait HealthChecker: Send + Sync {
    /// Name of the dependency (e.g., "database", "redis", "external-api").
    fn name(&self) -> &str;

    /// Whether this dependency is critical for readiness/startup.
    fn is_critical(&self) -> bool;

    /// Perform the health check.
    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult;

    /// Get circuit breaker info if this checker is wrapped with a circuit breaker.
    async fn get_circuit_breaker_info(
        &self,
    ) -> Option<plexspaces_proto::system::v1::DependencyCircuitBreakerInfo> {
        None
    }
}

/// Trait for health reporting.
#[async_trait]
pub trait HealthReporter: Send + Sync {
    async fn is_alive(&self) -> bool;
    async fn check_readiness(&self) -> (bool, Option<String>);
    async fn check_startup(&self) -> (bool, Option<String>);
    async fn get_readiness(&self) -> NodeReadinessStatus;
    async fn get_state(&self) -> NodeHealthState;
    async fn get_detailed_health(&self, include_non_critical: bool) -> DetailedHealthCheck;
    async fn mark_startup_complete(&self, message: Option<String>) -> Duration;
    async fn begin_shutdown(&self, drain_timeout: Option<Duration>) -> (u64, Duration, bool);
    async fn set_service_status(&self, service_name: &str, status: ServingStatus);
    async fn get_service_status(&self, service_name: &str) -> ServingStatus;
    async fn get_all_service_statuses(&self) -> std::collections::HashMap<String, ServingStatus>;
    async fn is_shutting_down(&self) -> bool;
    async fn update_in_flight_requests(&self, count: u64);

    /// Check if node is ready (readiness probe).
    async fn is_ready(&self) -> bool;
}

/// Run a health check and return a DependencyCheck result.
pub async fn run_health_check(
    checker: &dyn HealthChecker,
    ctx: &HealthCheckContext,
) -> DependencyCheck {
    let start = SystemTime::now();
    let name = checker.name().to_string();
    let is_critical = checker.is_critical();

    let result = checker.check(ctx).await;
    let checked_at = SystemTime::now();
    let response_time = checked_at.duration_since(start).unwrap_or_default();
    let circuit_breaker_info = checker.get_circuit_breaker_info().await;

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
            circuit_breaker: circuit_breaker_info,
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
            circuit_breaker: circuit_breaker_info,
        },
    }
}
