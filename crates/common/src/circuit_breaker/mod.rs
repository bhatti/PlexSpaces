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

mod circuit_breaker;

pub use circuit_breaker::*;

// Re-export proto types for convenience
pub use plexspaces_proto::circuitbreaker::prv::{
    CircuitBreakerConfig, CircuitBreakerEvent, CircuitBreakerInfo, CircuitBreakerMetrics,
    CircuitOpenError, CircuitState, FailureStrategy, HalfOpenConfig, RequestResult,
    SlidingWindowConfig, SlidingWindowMetrics,
};
