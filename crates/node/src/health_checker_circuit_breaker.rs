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

//! # Circuit Breaker Wrapper for Health Checkers
//!
//! ## Purpose
//! Wraps health checkers with circuit breakers to track dependency state transitions
//! and support degraded mode. When a non-critical dependency fails, the circuit breaker
//! allows the node to continue operating in degraded mode.
//!
//! ## Architecture Context
//! - Integrates circuit breaker pattern with health checking
//! - Supports degraded mode for non-critical dependencies
//! - Tracks dependency state transitions (healthy → degraded → unhealthy)
//! - Provides metrics for dashboard monitoring

use crate::health_checker::{HealthChecker, HealthCheckContext, HealthCheckError, HealthCheckResult};
use plexspaces_circuit_breaker::CircuitBreaker;
use plexspaces_proto::circuitbreaker::prv::{CircuitBreakerConfig, CircuitBreakerMetrics, CircuitState, FailureStrategy};
use plexspaces_proto::system::v1::{CircuitBreakerHealthMetrics, DependencyCircuitBreakerInfo};
use prost_types::Duration as ProstDuration;
use prost_types::Timestamp;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Circuit breaker wrapper for health checkers
///
/// ## Purpose
/// Wraps a health checker with a circuit breaker to track dependency state
/// and support degraded mode transitions.
///
/// ## Design Notes
/// - Critical dependencies: Circuit open = readiness false
/// - Non-critical dependencies: Circuit open = degraded mode (readiness can still be true)
/// - Circuit breaker tracks failure patterns and auto-recovers
pub struct CircuitBreakerHealthChecker {
    /// Underlying health checker
    checker: Arc<dyn HealthChecker>,
    
    /// Circuit breaker for tracking state
    circuit: Arc<RwLock<CircuitBreaker>>,
    
    /// Whether this dependency is critical
    is_critical: bool,
}

impl CircuitBreakerHealthChecker {
    /// Create a new circuit breaker health checker
    ///
    /// ## Arguments
    /// * `checker` - The underlying health checker
    /// * `config` - Optional circuit breaker configuration (uses defaults if None)
    ///
    /// ## Returns
    /// A new `CircuitBreakerHealthChecker` instance
    ///
    /// ## Design Notes
    /// Uses existing CircuitBreaker implementation from plexspaces-circuit-breaker crate.
    /// Follows proto-first design by using CircuitBreakerConfig from proto definitions.
    pub fn new(
        checker: Arc<dyn HealthChecker>,
        config: Option<CircuitBreakerConfig>,
    ) -> Self {
        let is_critical = checker.is_critical();
        
        let config = config.unwrap_or_else(|| {
            CircuitBreakerConfig {
                name: format!("health-checker-{}", checker.name()),
                failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
                failure_threshold: 5,
                success_threshold: 2,
                timeout: Some(ProstDuration {
                    seconds: 60,
                    nanos: 0,
                }),
                max_half_open_requests: 3,
                ..Default::default()
            }
        });
        
        let circuit = CircuitBreaker::new(config);
        
        Self {
            checker,
            circuit: Arc::new(RwLock::new(circuit)),
            is_critical,
        }
    }
    
    /// Create with default configuration (convenience method)
    pub fn with_defaults(checker: Arc<dyn HealthChecker>) -> Self {
        Self::new(checker, None)
    }
    
    /// Get the underlying health checker
    pub fn checker(&self) -> &Arc<dyn HealthChecker> {
        &self.checker
    }
    
    /// Get circuit breaker state
    pub async fn get_circuit_state(&self) -> CircuitState {
        let circuit = self.circuit.read().await;
        circuit.get_state().await
    }
    
    /// Get circuit breaker metrics
    pub async fn get_circuit_metrics(&self) -> CircuitBreakerMetrics {
        let circuit = self.circuit.read().await;
        circuit.get_metrics().await
    }
    
    /// Get circuit breaker info for dependency health check
    ///
    /// ## Returns
    /// `Some(DependencyCircuitBreakerInfo)` with circuit state and metrics, or `None` if not available
    ///
    /// ## Design Notes
    /// Uses existing CircuitBreaker implementation and converts metrics to proto format.
    /// Follows proto-first design by using proto-defined message types.
    pub async fn get_circuit_breaker_info(&self) -> Option<DependencyCircuitBreakerInfo> {
        Some(self.get_circuit_breaker_info_impl().await)
    }
    
    async fn get_circuit_breaker_info_impl(&self) -> DependencyCircuitBreakerInfo {
        let circuit = self.circuit.read().await;
        let state = circuit.get_state().await;
        let metrics = circuit.get_metrics().await;
        
        DependencyCircuitBreakerInfo {
            circuit_name: metrics.name.clone(),
            state: state as i32,
            metrics: Some(CircuitBreakerHealthMetrics {
                total_requests: metrics.total_requests,
                failed_requests: metrics.failed_requests,
                rejected_requests: metrics.rejected_requests,
                error_rate: metrics.error_rate,
                consecutive_failures: metrics.consecutive_failures,
                trip_count: metrics.trip_count,
                last_opened: metrics.last_opened.clone(),
                time_in_state: metrics.time_in_state.clone(),
            }),
        }
    }
    
    /// Check if circuit is open (dependency unavailable)
    pub async fn is_circuit_open(&self) -> bool {
        let circuit = self.circuit.read().await;
        !circuit.is_request_allowed().await
    }
    
    /// Check if dependency is in degraded mode
    ///
    /// ## Returns
    /// `true` if circuit is open but dependency is non-critical (degraded mode)
    pub async fn is_degraded(&self) -> bool {
        !self.is_critical && self.is_circuit_open().await
    }
}

#[async_trait::async_trait]
impl HealthChecker for CircuitBreakerHealthChecker {
    fn name(&self) -> &str {
        self.checker.name()
    }
    
    fn is_critical(&self) -> bool {
        self.is_critical
    }
    
    
    async fn check(&self, ctx: &HealthCheckContext) -> HealthCheckResult {
        // Check if circuit allows the request
        let circuit_allowed = {
            let circuit = self.circuit.read().await;
            circuit.is_request_allowed().await
        };
        
        if !circuit_allowed {
            // Circuit is open - fail fast for critical dependencies
            if self.is_critical {
                return Err(HealthCheckError::CheckFailed(format!(
                    "Circuit breaker open for critical dependency '{}'",
                    self.checker.name()
                )));
            }
            // For non-critical dependencies, return error but allow degraded mode
            return Err(HealthCheckError::CheckFailed(format!(
                "Circuit breaker open for dependency '{}' (degraded mode)",
                self.checker.name()
            )));
        }
        
        // Circuit allows request - perform actual health check
        let result = self.checker.check(ctx).await;
        
        // Update circuit breaker based on result
        {
            let circuit = self.circuit.read().await;
            match &result {
                Ok(_) => {
                    circuit.record_success().await;
                }
                Err(_) => {
                    circuit.record_failure().await;
                }
            }
        }
        
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::health_checker::{PingChecker, HealthCheckContext};
    
    #[tokio::test]
    async fn test_circuit_breaker_health_checker_success() {
        let checker = Arc::new(PingChecker);
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        
        let ctx = HealthCheckContext::default();
        let result = cb_checker.check(&ctx).await;
        
        assert!(result.is_ok());
        assert_eq!(cb_checker.name(), "ping");
        assert!(!cb_checker.is_critical());
    }
    
    #[tokio::test]
    async fn test_circuit_breaker_health_checker_metrics() {
        let checker = Arc::new(PingChecker);
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        
        let metrics = cb_checker.get_circuit_metrics().await;
        assert_eq!(metrics.state, CircuitState::CircuitStateClosed as i32);
    }
    
    #[tokio::test]
    async fn test_circuit_breaker_health_checker_degraded_mode() {
        let checker = Arc::new(PingChecker);
        // Create with low failure threshold for testing
        let config = CircuitBreakerConfig {
            name: "test-ping".to_string(),
            failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
            failure_threshold: 1,
            success_threshold: 1,
            timeout: Some(ProstDuration { seconds: 1, nanos: 0 }),
            max_half_open_requests: 1,
            ..Default::default()
        };
        let cb_checker = CircuitBreakerHealthChecker::new(Arc::new(PingChecker), Some(config));
        
        // Initially not degraded
        assert!(!cb_checker.is_degraded().await);
        
        // Since PingChecker always succeeds, we can't test degraded mode easily
        // This test verifies the API works
        let is_degraded = cb_checker.is_degraded().await;
        assert!(!is_degraded); // PingChecker is non-critical but should succeed
    }
    
    #[tokio::test]
    async fn test_circuit_breaker_info() {
        let checker = Arc::new(PingChecker);
        let cb_checker = CircuitBreakerHealthChecker::with_defaults(checker);
        
        let info = cb_checker.get_circuit_breaker_info().await;
        assert!(info.is_some());
        let info = info.unwrap();
        assert!(!info.circuit_name.is_empty());
        assert_eq!(info.state, CircuitState::CircuitStateClosed as i32);
        assert!(info.metrics.is_some());
    }
}

