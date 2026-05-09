// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Health checker implementations (trait lives in `plexspaces-service-traits`).

pub use plexspaces_service_traits::health::{
    run_health_check, HealthCheckContext, HealthCheckError, HealthCheckResult, HealthChecker,
};

/// Ping check (always passes).
pub struct PingChecker;

#[async_trait::async_trait]
impl HealthChecker for PingChecker {
    fn name(&self) -> &str {
        "ping"
    }

    fn is_critical(&self) -> bool {
        false
    }

    async fn check(&self, _ctx: &HealthCheckContext) -> HealthCheckResult {
        Ok(())
    }
}

/// Shutdown check (fails if shutdown in progress).
pub struct ShutdownChecker {
    shutdown_tx: tokio::sync::watch::Receiver<bool>,
}

impl ShutdownChecker {
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
        true
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
    use plexspaces_proto::system::v1::HealthStatus;

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
        let (_tx, rx) = tokio::sync::watch::channel(false);
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

        tx.send(true).unwrap();

        let result = run_health_check(&checker, &ctx).await;
        assert_eq!(result.status, HealthStatus::HealthStatusUnhealthy as i32);
        assert!(!result.error_message.is_empty());
        assert!(result.checked_at.is_some());
    }
}
