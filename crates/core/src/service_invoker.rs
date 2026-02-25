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

//! Service Invoker trait for cross-node service invocation
//!
//! ## Purpose
//! Provides transparent cross-node service invocation with built-in
//! retries, timeouts, load balancing, and observability.
//!
//! ## Design
//! Inspired by wasmCloud's lattice RPC and Dapr's service invocation:
//! - Uses ObjectRegistry for service discovery
//! - Uses GrpcConnectionManager for transport
//! - Automatic retry with configurable backoff
//! - Circuit breaker integration
//! - Distributed tracing context propagation

use async_trait::async_trait;
use std::time::Duration;

use crate::RequestContext;

/// Options for a service invocation
#[derive(Debug, Clone)]
pub struct InvocationOptions {
    /// Timeout for the entire invocation (including retries)
    pub timeout: Duration,
    /// Maximum number of retries (0 = no retries)
    pub max_retries: u32,
    /// Retry backoff strategy
    pub backoff: BackoffStrategy,
    /// Target node preference (None = any available node)
    pub target_node: Option<String>,
}

impl Default for InvocationOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            max_retries: 3,
            backoff: BackoffStrategy::Exponential {
                initial: Duration::from_millis(100),
                max: Duration::from_secs(5),
            },
            target_node: None,
        }
    }
}

/// Retry backoff strategy
#[derive(Debug, Clone)]
pub enum BackoffStrategy {
    /// Fixed delay between retries
    Fixed(Duration),
    /// Exponential backoff with jitter
    Exponential {
        /// Initial backoff duration
        initial: Duration,
        /// Maximum backoff duration
        max: Duration,
    },
    /// No delay between retries
    None,
}

/// Invocation errors
#[derive(Debug, thiserror::Error)]
pub enum InvocationError {
    /// Service not found in registry
    #[error("Service not found: {0}")]
    ServiceNotFound(String),

    /// No available nodes for the service
    #[error("No available nodes for service: {0}")]
    NoAvailableNodes(String),

    /// All retries exhausted
    #[error("All {retries} retries exhausted for {service}: {last_error}")]
    RetriesExhausted {
        /// Service name
        service: String,
        /// Number of retries attempted
        retries: u32,
        /// Last error encountered
        last_error: String,
    },

    /// Invocation timed out
    #[error("Invocation timed out after {0:?}")]
    Timeout(Duration),

    /// Circuit breaker is open
    #[error("Circuit breaker open for service: {0}")]
    CircuitBreakerOpen(String),

    /// Transport error (gRPC, network, etc.)
    #[error("Transport error: {0}")]
    TransportError(String),

    /// Remote service returned an error
    #[error("Remote error from {service}: {message}")]
    RemoteError {
        /// Service name
        service: String,
        /// Error message from remote
        message: String,
    },
}

/// Cross-node service invocation with automatic routing, retries, and observability.
///
/// ## Purpose
/// Provides a high-level interface for invoking services across nodes.
/// Handles service discovery, load balancing, retries, and circuit breaking.
///
/// ## Example
/// ```rust,ignore
/// let invoker = service_locator.get_service_invoker().await?;
/// let result = invoker.invoke(
///     &ctx,
///     "ActorService",
///     "spawn",
///     &spawn_request_bytes,
///     InvocationOptions::default(),
/// ).await?;
/// ```
#[async_trait]
pub trait ServiceInvoker: Send + Sync {
    /// Invoke a service operation on any available node
    ///
    /// ## Behavior
    /// 1. Discovers available nodes via ObjectRegistry
    /// 2. Selects a node (round-robin or target_node preference)
    /// 3. Sends request via GrpcConnectionManager
    /// 4. On failure, retries with backoff per InvocationOptions
    /// 5. If circuit breaker is open, fails fast
    ///
    /// ## Arguments
    /// * `ctx` - Request context with tenant/namespace and tracing info
    /// * `service_name` - Name of the service to invoke (e.g., "ActorService")
    /// * `operation` - Operation name (e.g., "spawn", "send")
    /// * `payload` - Serialized request payload (protobuf bytes)
    /// * `options` - Invocation options (timeout, retries, backoff)
    ///
    /// ## Returns
    /// Serialized response payload on success
    async fn invoke(
        &self,
        ctx: &RequestContext,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        options: InvocationOptions,
    ) -> Result<Vec<u8>, InvocationError>;

    /// Invoke a service on a specific node (no discovery, no load balancing)
    ///
    /// ## Arguments
    /// * `ctx` - Request context
    /// * `node_id` - Target node ID
    /// * `service_name` - Name of the service
    /// * `operation` - Operation name
    /// * `payload` - Serialized request payload
    /// * `timeout` - Request timeout
    ///
    /// ## Returns
    /// Serialized response payload on success
    async fn invoke_on_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, InvocationError>;
}
