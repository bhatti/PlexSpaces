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

//! CircuitBreaker - Fault tolerance and resilience pattern
//!
//! Core implementation of circuit breaker for preventing cascading failures.

use plexspaces_proto::circuitbreaker::prv::*;
use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{Duration, Instant};

/// Error types for CircuitBreaker operations
#[derive(Debug, thiserror::Error)]
pub enum CircuitBreakerError {
    /// Circuit is open, request rejected
    #[error("Circuit open: {0}")]
    CircuitOpen(String),

    /// Configuration error
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Circuit breaker not found
    #[error("Circuit breaker not found: {0}")]
    NotFound(String),
}

/// Request tracking for sliding window
#[derive(Debug, Clone)]
struct RequestRecord {
    success: bool,
    timestamp: Instant,
}

/// Circuit breaker state data
struct CircuitBreakerState {
    config: CircuitBreakerConfig,
    state: CircuitState,
    state_since: Instant,

    // Counters
    total_requests: u64,
    successful_requests: u64,
    failed_requests: u64,
    rejected_requests: u64,
    consecutive_failures: u32,
    consecutive_successes: u32,
    trip_count: u64,

    // Sliding window
    request_window: VecDeque<RequestRecord>,

    // Timestamps
    last_opened: Option<Instant>,
    last_closed: Option<Instant>,

    // Half-open tracking
    half_open_requests: u32,
}

impl CircuitBreakerState {
    fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            state: CircuitState::CircuitStateClosed,
            state_since: Instant::now(),
            total_requests: 0,
            successful_requests: 0,
            failed_requests: 0,
            rejected_requests: 0,
            consecutive_failures: 0,
            consecutive_successes: 0,
            trip_count: 0,
            request_window: VecDeque::new(),
            last_opened: None,
            last_closed: None,
            half_open_requests: 0,
        }
    }

    /// Check if request should be allowed
    fn is_request_allowed(&mut self) -> bool {
        match self.state {
            CircuitState::CircuitStateClosed => true,
            CircuitState::CircuitStateOpen => {
                // Check if timeout expired
                if let Some(timeout) = self.config.timeout.as_ref() {
                    if let Some(opened_at) = self.last_opened {
                        let timeout_duration = Duration::from_secs(timeout.seconds as u64);
                        if opened_at.elapsed() >= timeout_duration {
                            // Transition to half-open
                            self.transition_to_half_open();
                            return true;
                        }
                    }
                }
                false
            }
            CircuitState::CircuitStateHalfOpen => {
                // Allow limited requests in half-open state
                let max_requests = self.config.max_half_open_requests;
                if self.half_open_requests < max_requests {
                    self.half_open_requests += 1;
                    true
                } else {
                    false
                }
            }
            CircuitState::CircuitStateDisabled => true,
        }
    }

    /// Record successful request
    fn record_success(&mut self) {
        self.total_requests += 1;
        self.successful_requests += 1;
        self.consecutive_failures = 0;
        self.consecutive_successes += 1;

        // Add to sliding window
        self.add_to_window(true);

        if self.state == CircuitState::CircuitStateHalfOpen {
            // Check if enough successes to close circuit
            if self.consecutive_successes >= self.config.success_threshold {
                self.transition_to_closed();
            }
        }
    }

    /// Record failed request
    fn record_failure(&mut self) {
        self.total_requests += 1;
        self.failed_requests += 1;
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;

        // Add to sliding window
        self.add_to_window(false);

        match self.state {
            CircuitState::CircuitStateClosed => {
                // Check if should trip
                if self.should_trip() {
                    self.transition_to_open();
                }
            }
            CircuitState::CircuitStateHalfOpen => {
                // Any failure in half-open → back to open
                self.transition_to_open();
            }
            _ => {}
        }
    }

    /// Check if circuit should trip based on strategy
    fn should_trip(&self) -> bool {
        let strategy = FailureStrategy::try_from(self.config.failure_strategy)
            .unwrap_or(FailureStrategy::FailureStrategyConsecutive);

        match strategy {
            FailureStrategy::FailureStrategyConsecutive => {
                self.consecutive_failures >= self.config.failure_threshold
            }
            FailureStrategy::FailureStrategySlidingWindow => {
                if let Some(window_config) = &self.config.sliding_window {
                    let failures_in_window = self.count_failures_in_window();
                    let total_in_window = self.request_window.len() as u32;

                    // Need minimum requests before tripping
                    if total_in_window < window_config.minimum_requests {
                        return false;
                    }

                    failures_in_window >= self.config.failure_threshold
                } else {
                    false
                }
            }
            FailureStrategy::FailureStrategyErrorRate => {
                if let Some(window_config) = &self.config.sliding_window {
                    let total_in_window = self.request_window.len() as u32;

                    // Need minimum requests before calculating error rate
                    if total_in_window < window_config.minimum_requests {
                        return false;
                    }

                    let failures_in_window = self.count_failures_in_window();
                    let error_rate = (failures_in_window as f64) / (total_in_window as f64);
                    let threshold_rate = (self.config.failure_threshold as f64) / 100.0;

                    error_rate >= threshold_rate
                } else {
                    false
                }
            }
            FailureStrategy::FailureStrategyCustom => {
                // TODO: Support custom strategies
                false
            }
        }
    }

    /// Add request to sliding window
    fn add_to_window(&mut self, success: bool) {
        if let Some(window_config) = &self.config.sliding_window {
            let record = RequestRecord {
                success,
                timestamp: Instant::now(),
            };

            self.request_window.push_back(record);

            // Remove old records outside window
            if let Some(window_duration) = window_config.window_duration.as_ref() {
                let cutoff = Instant::now() - Duration::from_secs(window_duration.seconds as u64);
                while let Some(oldest) = self.request_window.front() {
                    if oldest.timestamp < cutoff {
                        self.request_window.pop_front();
                    } else {
                        break;
                    }
                }
            }

            // Limit by size
            while self.request_window.len() > window_config.window_size as usize {
                self.request_window.pop_front();
            }
        }
    }

    /// Count failures in sliding window
    fn count_failures_in_window(&self) -> u32 {
        self.request_window.iter().filter(|r| !r.success).count() as u32
    }

    /// Transition to open state
    fn transition_to_open(&mut self) {
        self.state = CircuitState::CircuitStateOpen;
        self.state_since = Instant::now();
        self.last_opened = Some(Instant::now());
        self.trip_count += 1;
        self.half_open_requests = 0;
    }

    /// Transition to half-open state
    fn transition_to_half_open(&mut self) {
        self.state = CircuitState::CircuitStateHalfOpen;
        self.state_since = Instant::now();
        self.consecutive_successes = 0;
        self.consecutive_failures = 0;
        self.half_open_requests = 0;
    }

    /// Transition to closed state
    fn transition_to_closed(&mut self) {
        self.state = CircuitState::CircuitStateClosed;
        self.state_since = Instant::now();
        self.last_closed = Some(Instant::now());
        self.consecutive_failures = 0;
        self.consecutive_successes = 0;
        self.half_open_requests = 0;
    }

    /// Calculate current error rate
    fn calculate_error_rate(&self) -> f64 {
        if self.total_requests == 0 {
            return 0.0;
        }
        (self.failed_requests as f64) / (self.total_requests as f64)
    }

    /// Get metrics
    fn get_metrics(&self) -> CircuitBreakerMetrics {
        CircuitBreakerMetrics {
            name: self.config.name.clone(),
            state: self.state.clone() as i32,
            total_requests: self.total_requests,
            successful_requests: self.successful_requests,
            failed_requests: self.failed_requests,
            rejected_requests: self.rejected_requests,
            error_rate: self.calculate_error_rate(),
            consecutive_failures: self.consecutive_failures,
            consecutive_successes: self.consecutive_successes,
            last_opened: self.last_opened.map(|t| prost_types::Timestamp {
                seconds: t.elapsed().as_secs() as i64,
                nanos: t.elapsed().subsec_nanos() as i32,
            }),
            last_closed: self.last_closed.map(|t| prost_types::Timestamp {
                seconds: t.elapsed().as_secs() as i64,
                nanos: t.elapsed().subsec_nanos() as i32,
            }),
            time_in_state: Some(prost_types::Duration {
                seconds: self.state_since.elapsed().as_secs() as i64,
                nanos: self.state_since.elapsed().subsec_nanos() as i32,
            }),
            trip_count: self.trip_count,
            avg_response_time_ms: 0, // TODO: Track response times
            sliding_window_metrics: if self.config.sliding_window.is_some() {
                Some(SlidingWindowMetrics {
                    window_requests: self.request_window.len() as u32,
                    window_failures: self.count_failures_in_window(),
                    window_start: None,
                    window_end: None,
                })
            } else {
                None
            },
        }
    }
}

/// Circuit breaker for fault tolerance
#[derive(Clone)]
pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitBreakerState>>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with given configuration
    ///
    /// ## Arguments
    /// * `config` - Circuit breaker configuration
    ///
    /// ## Returns
    /// A new `CircuitBreaker` instance
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # use plexspaces_proto::circuitbreaker::prv::*;
    /// let config = CircuitBreakerConfig {
    ///     name: "test".to_string(),
    ///     failure_threshold: 5,
    ///     success_threshold: 2,
    ///     ..Default::default()
    /// };
    /// let circuit = CircuitBreaker::new(config);
    /// ```
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitBreakerState::new(config))),
        }
    }

    /// Check if request should be allowed
    ///
    /// ## Returns
    /// `true` if request should be allowed, `false` if circuit is open
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// if circuit.is_request_allowed().await {
    ///     // Execute request
    /// } else {
    ///     // Fail fast - circuit open
    /// }
    /// # }
    /// ```
    pub async fn is_request_allowed(&self) -> bool {
        let mut state = self.state.write().await;
        let allowed = state.is_request_allowed();

        if !allowed {
            state.rejected_requests += 1;
        }

        allowed
    }

    /// Record successful request
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// circuit.record_success().await;
    /// # }
    /// ```
    pub async fn record_success(&self) {
        self.state.write().await.record_success();
    }

    /// Record failed request
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// circuit.record_failure().await;
    /// # }
    /// ```
    pub async fn record_failure(&self) {
        self.state.write().await.record_failure();
    }

    /// Get current state
    pub async fn get_state(&self) -> CircuitState {
        self.state.read().await.state.clone()
    }

    /// Get circuit breaker metrics
    ///
    /// ## Returns
    /// Current metrics including state, counters, and error rate
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// let metrics = circuit.get_metrics().await;
    /// println!("State: {:?}", metrics.state);
    /// println!("Error rate: {:.2}%", metrics.error_rate * 100.0);
    /// # }
    /// ```
    pub async fn get_metrics(&self) -> CircuitBreakerMetrics {
        self.state.read().await.get_metrics()
    }

    /// Manually trip the circuit (for testing/maintenance)
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// circuit.trip().await;
    /// # }
    /// ```
    pub async fn trip(&self) {
        self.state.write().await.transition_to_open();
    }

    /// Manually reset the circuit
    ///
    /// ## Examples
    /// ```rust
    /// # use plexspaces_circuit_breaker::*;
    /// # async fn example(circuit: &CircuitBreaker) {
    /// circuit.reset().await;
    /// # }
    /// ```
    pub async fn reset(&self) {
        self.state.write().await.transition_to_closed();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> CircuitBreakerConfig {
        CircuitBreakerConfig {
            name: "test-circuit".to_string(),
            failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
            failure_threshold: 3,
            success_threshold: 2,
            timeout: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
            max_half_open_requests: 5,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn test_create_circuit_breaker() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        let state = circuit.get_state().await;
        assert_eq!(state, CircuitState::CircuitStateClosed);
    }

    #[tokio::test]
    async fn test_consecutive_failures_trip() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Record 2 failures - should stay closed
        circuit.record_failure().await;
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateClosed);

        // 3rd failure - should trip to open
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);

        // Requests should be rejected
        assert!(!circuit.is_request_allowed().await);
    }

    #[tokio::test]
    async fn test_success_resets_consecutive_failures() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Record 2 failures
        circuit.record_failure().await;
        circuit.record_failure().await;

        // Success resets counter
        circuit.record_success().await;

        // Should need 3 more failures to trip
        circuit.record_failure().await;
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateClosed);

        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);
    }

    #[tokio::test]
    async fn test_half_open_recovery() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Trip circuit
        circuit.record_failure().await;
        circuit.record_failure().await;
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);

        // Manually transition to half-open (simulate timeout)
        circuit.state.write().await.transition_to_half_open();
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateHalfOpen);

        // One success
        circuit.record_success().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateHalfOpen);

        // Second success - should close
        circuit.record_success().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateClosed);
    }

    #[tokio::test]
    async fn test_half_open_failure_reopens() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Trip and transition to half-open
        circuit.trip().await;
        circuit.state.write().await.transition_to_half_open();
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateHalfOpen);

        // Any failure in half-open → back to open
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);
    }

    #[tokio::test]
    async fn test_manual_trip_and_reset() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Manual trip
        circuit.trip().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);

        // Manual reset
        circuit.reset().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateClosed);
    }

    #[tokio::test]
    async fn test_metrics() {
        let config = create_test_config();
        let circuit = CircuitBreaker::new(config);

        // Record some requests
        circuit.record_success().await;
        circuit.record_success().await;
        circuit.record_failure().await;

        let metrics = circuit.get_metrics().await;
        assert_eq!(metrics.total_requests, 3);
        assert_eq!(metrics.successful_requests, 2);
        assert_eq!(metrics.failed_requests, 1);
        assert_eq!(metrics.consecutive_failures, 1);

        // Error rate should be 1/3 ≈ 0.33
        assert!((metrics.error_rate - 0.333).abs() < 0.01);
    }

    #[tokio::test]
    async fn test_error_rate_strategy() {
        let mut config = create_test_config();
        config.failure_strategy = FailureStrategy::FailureStrategyErrorRate as i32;
        config.failure_threshold = 50; // 50% error rate
        config.sliding_window = Some(SlidingWindowConfig {
            window_size: 10,
            minimum_requests: 5,
            ..Default::default()
        });

        let circuit = CircuitBreaker::new(config);

        // 3 successes, 2 failures (40% error rate) - should stay closed
        circuit.record_success().await;
        circuit.record_success().await;
        circuit.record_success().await;
        circuit.record_failure().await;
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateClosed);

        // 2 more failures (4 failures / 7 total ≈ 57% error rate) - should trip
        circuit.record_failure().await;
        circuit.record_failure().await;
        assert_eq!(circuit.get_state().await, CircuitState::CircuitStateOpen);
    }
}
