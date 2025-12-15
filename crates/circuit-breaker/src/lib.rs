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

//! # PlexSpaces Circuit Breaker
//!
//! ## Purpose
//! Provides fault tolerance and resilience patterns for microservices through
//! the circuit breaker pattern, preventing cascading failures in distributed systems.
//!
//! ## Architecture Context
//! CircuitBreaker is essential for resilient microservices in PlexSpaces:
//! - **Fault Isolation**: Prevent failing services from bringing down the whole system
//! - **Fast Failure**: Fail fast instead of waiting for timeouts
//! - **Automatic Recovery**: Self-healing through half-open state testing
//! - **Resource Protection**: Prevent resource exhaustion from repeated failures
//!
//! ### Component Diagram
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Circuit Breaker                           │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │            State Machine                             │  │
//! │  │                                                       │  │
//! │  │   ┌─────────┐  failures≥threshold  ┌──────────┐    │  │
//! │  │   │ Closed  │────────────────────>│   Open   │    │  │
//! │  │   │         │<────────────────────│          │    │  │
//! │  │   └────┬────┘    reset/timeout    └─────┬────┘    │  │
//! │  │        │                                 │         │  │
//! │  │        │ success≥threshold               │timeout │  │
//! │  │        │                                 │         │  │
//! │  │   ┌────▼────────┐                  ┌────▼────┐    │  │
//! │  │   │  Half-Open  │──────────────────│         │    │  │
//! │  │   │  (Testing)  │  failures>0      │  Open   │    │  │
//! │  │   └─────────────┘                  └─────────┘    │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! │                                                             │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │       Request Processing                              │  │
//! │  │  1. Check state (Closed → pass, Open → reject)        │  │
//! │  │  2. Execute request (or reject if Open)               │  │
//! │  │  3. Record result (success/failure)                   │  │
//! │  │  4. Update counters and check thresholds              │  │
//! │  │  5. Transition state if threshold exceeded            │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! │                                                             │
//! │  ┌──────────────────────────────────────────────────────┐  │
//! │  │       Metrics Tracking                                │  │
//! │  │  - Total/successful/failed/rejected requests          │  │
//! │  │  - Error rate calculation                             │  │
//! │  │  - Response time tracking                             │  │
//! │  │  - State transition history                           │  │
//! │  └──────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Key Components
//! - [`CircuitBreaker`][]: Main circuit breaker structure managing state and requests
//! - [`CircuitState`][]: States (Closed, Open, Half-Open, Disabled)
//! - [`FailureStrategy`][]: Detection strategies (Consecutive, Sliding Window, Error Rate)
//! - [`CircuitBreakerMetrics`][]: Real-time metrics for monitoring
//!
//! ## Dependencies
//! This crate depends on:
//! - [`plexspaces_proto`]: Protocol buffer definitions (CircuitBreakerConfig, etc.)
//! - [`tokio`]: Async runtime for timeouts and background tasks
//!
//! ## Dependents
//! This crate is used by:
//! - **Order-Processing Example**: Protect external service calls
//! - **ElasticPool**: Integrate with pool health monitoring
//! - **ObjectRegistry**: Circuit state affects service routing (replaces ServiceRegistry)
//! - **API Gateway**: Fail fast on unhealthy backend services
//!
//! ## Examples
//!
//! ### Basic Usage (Protect External Service Call)
//! ```rust,no_run
//! # use plexspaces_circuit_breaker::*;
//! # use plexspaces_proto::circuitbreaker::prv::*;
//! # async fn example() -> Result<(), CircuitBreakerError> {
//! // Configure circuit breaker
//! let config = CircuitBreakerConfig {
//!     name: "payment-service".to_string(),
//!     failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
//!     failure_threshold: 5,  // Trip after 5 consecutive failures
//!     success_threshold: 2,  // Close after 2 consecutive successes
//!     timeout: Some(prost_types::Duration { seconds: 60, nanos: 0 }),
//!     max_half_open_requests: 3,
//!     ..Default::default()
//! };
//!
//! // Create circuit breaker
//! let circuit = CircuitBreaker::new(config);
//!
//! // Check if request allowed
//! if circuit.is_request_allowed().await {
//!     // Execute request
//!     match call_payment_service().await {
//!         Ok(result) => {
//!             circuit.record_success().await;
//!             // Process result...
//!         }
//!         Err(e) => {
//!             circuit.record_failure().await;
//!             // Handle error...
//!         }
//!     }
//! } else {
//!     // Circuit open - fail fast
//!     return Err(CircuitBreakerError::CircuitOpen);
//! }
//! # Ok(())
//! # }
//! # async fn call_payment_service() -> Result<(), String> { Ok(()) }
//! ```
//!
//! ### Error Rate Strategy
//! ```rust,no_run
//! # use plexspaces_circuit_breaker::*;
//! # use plexspaces_proto::circuitbreaker::prv::*;
//! # async fn example() -> Result<(), CircuitBreakerError> {
//! // Trip when error rate exceeds 50% over last 100 requests
//! let config = CircuitBreakerConfig {
//!     name: "api-gateway".to_string(),
//!     failure_strategy: FailureStrategy::FailureStrategyErrorRate as i32,
//!     failure_threshold: 50,  // 50% error rate
//!     sliding_window: Some(SlidingWindowConfig {
//!         window_size: 100,
//!         minimum_requests: 10,  // Need at least 10 requests before tripping
//!         ..Default::default()
//!     }),
//!     ..Default::default()
//! };
//!
//! let circuit = CircuitBreaker::new(config);
//! # Ok(())
//! # }
//! ```
//!
//! ### Half-Open State Sampling
//! ```rust,no_run
//! # use plexspaces_circuit_breaker::*;
//! # use plexspaces_proto::circuitbreaker::prv::*;
//! # use plexspaces_proto::circuitbreaker::prv::half_open_config::SelectionStrategy;
//! # async fn example() -> Result<(), CircuitBreakerError> {
//! // Allow 10% of requests through in half-open state
//! let config = CircuitBreakerConfig {
//!     name: "database".to_string(),
//!     failure_threshold: 3,
//!     success_threshold: 5,
//!     half_open_config: Some(HalfOpenConfig {
//!         max_requests: 10,
//!         selection_strategy: SelectionStrategy::Percentage as i32,
//!         sample_percentage: 0.1,  // 10% sampling
//!         ..Default::default()
//!     }),
//!     ..Default::default()
//! };
//!
//! let circuit = CircuitBreaker::new(config);
//! # Ok(())
//! # }
//! ```
//!
//! ### Monitoring and Metrics
//! ```rust,no_run
//! # use plexspaces_circuit_breaker::*;
//! # async fn example() -> Result<(), CircuitBreakerError> {
//! # let circuit = CircuitBreaker::new(Default::default());
//! // Get circuit metrics
//! let metrics = circuit.get_metrics().await;
//!
//! println!("State: {:?}", metrics.state);
//! println!("Error rate: {:.2}%", metrics.error_rate * 100.0);
//! println!("Total requests: {}", metrics.total_requests);
//! println!("Rejected: {}", metrics.rejected_requests);
//! println!("Trip count: {}", metrics.trip_count);
//! # Ok(())
//! # }
//! ```
//!
//! ## Design Principles
//!
//! ### Proto-First
//! All types come from `circuitbreaker.proto` (CircuitBreakerConfig, CircuitState, etc.).
//! This ensures type safety, versioning, and language-agnostic interoperability.
//!
//! ### Three-State Machine
//! - **Closed**: Normal operation, all requests pass through
//! - **Open**: Circuit tripped, requests fail fast without execution
//! - **Half-Open**: Testing recovery, limited requests allowed through
//!
//! ### Failure Detection Strategies
//! - **Consecutive**: Count consecutive failures (simple, fast)
//! - **Sliding Window**: Count failures in time window (smoothed, realistic)
//! - **Error Rate**: Percentage-based (best for high-traffic services)
//!
//! ### Automatic Recovery
//! After timeout in Open state, transition to Half-Open to test recovery.
//! Gradual increase in allowed requests prevents overwhelming failed service.
//!
//! ## Testing
//! ```bash
//! # Run tests
//! cargo test -p plexspaces-circuit-breaker
//!
//! # Check coverage
//! cargo tarpaulin -p plexspaces-circuit-breaker
//! ```
//!
//! ## Performance Characteristics
//! - **Decision latency**: < 1μs (simple state check)
//! - **Memory per circuit**: ~1KB (counters and timestamps)
//! - **Throughput**: > 1M decisions/sec (lock-free reads in Closed state)
//! - **State transitions**: < 10μs (atomic updates)
//!
//! ## Known Limitations
//! - **No Distributed State**: Each instance maintains local state (use ObjectRegistry for distributed)
//! - **No Persistence**: State lost on restart (future: persist to Redis/DB)
//! - **No Custom Strategies**: Only built-in strategies supported
//! - **No Adaptive Thresholds**: Fixed thresholds (future: ML-based adjustment)

#![warn(missing_docs)]
#![warn(clippy::all)]

mod circuit_breaker;

pub use circuit_breaker::*;

// Re-export proto types for convenience
pub use plexspaces_proto::circuitbreaker::prv::{
    circuit_breaker_service_client::CircuitBreakerServiceClient,
    circuit_breaker_service_server::CircuitBreakerService,
    circuit_breaker_service_server::CircuitBreakerServiceServer, CircuitBreakerConfig,
    CircuitBreakerEvent, CircuitBreakerInfo, CircuitBreakerMetrics, CircuitOpenError, CircuitState,
    FailureStrategy, HalfOpenConfig, RequestResult, SlidingWindowConfig, SlidingWindowMetrics,
};
