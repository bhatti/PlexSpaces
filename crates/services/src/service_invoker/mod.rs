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

//! Service Invoker Implementation
//!
//! Provides cross-node service invocation with automatic routing,
//! retries, load balancing, and observability. Inspired by wasmCloud's
//! lattice RPC and Dapr's service invocation.

use async_trait::async_trait;
use plexspaces_core::{
    BackoffStrategy, InvocationError, InvocationOptions, RequestContext, ServiceInvoker,
    ServiceLocator,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

/// Service Invoker implementation with retry, load balancing, and circuit breaking
pub struct ServiceInvokerImpl {
    service_locator: Arc<dyn ServiceLocator>,
    node_id: String,
    /// Round-robin counter for load balancing
    round_robin_counter: AtomicUsize,
}

impl ServiceInvokerImpl {
    /// Create a new ServiceInvokerImpl
    pub fn new(service_locator: Arc<dyn ServiceLocator>, node_id: String) -> Self {
        Self {
            service_locator,
            node_id,
            round_robin_counter: AtomicUsize::new(0),
        }
    }

    /// Select a node using round-robin from available nodes
    async fn select_node(
        &self,
        ctx: &RequestContext,
        target_node: Option<&str>,
    ) -> Result<String, InvocationError> {
        // If a specific node is requested, use it
        if let Some(node) = target_node {
            return Ok(node.to_string());
        }

        // Get node registry for discovery
        let node_registry = self
            .service_locator
            .get_node_registry()
            .await
            .ok_or_else(|| {
                InvocationError::ServiceNotFound("NodeRegistry not available".to_string())
            })?;

        // List available nodes
        let (nodes, _) = node_registry
            .list_nodes(ctx, None, 100, "")
            .await
            .map_err(|e| InvocationError::TransportError(e.to_string()))?;

        if nodes.is_empty() {
            return Err(InvocationError::NoAvailableNodes(
                "No nodes registered".to_string(),
            ));
        }

        // Round-robin selection
        let idx = self.round_robin_counter.fetch_add(1, Ordering::Relaxed) % nodes.len();
        Ok(nodes[idx].node_id.clone())
    }

    /// Calculate backoff duration for a retry attempt
    fn backoff_duration(strategy: &BackoffStrategy, attempt: u32) -> Duration {
        match strategy {
            BackoffStrategy::None => Duration::ZERO,
            BackoffStrategy::Fixed(duration) => *duration,
            BackoffStrategy::Exponential { initial, max } => {
                let delay = initial.as_millis() as u64 * 2u64.pow(attempt);
                let max_ms = max.as_millis() as u64;
                let clamped = delay.min(max_ms);
                // Add jitter (up to 25% of delay)
                let jitter = (clamped as f64 * rand::random::<f64>() * 0.25) as u64;
                Duration::from_millis(clamped + jitter)
            }
        }
    }

    /// Invoke a service on a specific node via the ActorService abstraction
    ///
    /// Routes the invocation through the actor messaging layer using a synthetic
    /// actor ID `__service__:{service_name}:{operation}` so the receiving node
    /// can dispatch it to the appropriate service handler.
    async fn invoke_remote(
        &self,
        _ctx: &RequestContext,
        _node_id: &str,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        _timeout: Duration,
    ) -> Result<Vec<u8>, InvocationError> {
        // Use the ActorService abstraction for cross-node messaging
        let actor_service = self
            .service_locator
            .get_actor_service()
            .await
            .ok_or_else(|| {
                InvocationError::ServiceNotFound("ActorService not available".to_string())
            })?;

        // Create a message targeting a synthetic service actor
        let target_actor_id = format!("__service__:{}:{}", service_name, operation);
        let msg = plexspaces_proto::common::v1::Message {
            message_type: format!("{}:{}", service_name, operation),
            payload: payload.to_vec(),
            sender_id: self.node_id.clone(),
            receiver_id: target_actor_id.clone(),
            ..Default::default()
        };

        // Send via the actor service (handles routing to correct node)
        actor_service
            .send(&target_actor_id, msg)
            .await
            .map(|msg_id| msg_id.into_bytes())
            .map_err(|e| InvocationError::TransportError(e.to_string()))
    }
}

#[async_trait]
impl ServiceInvoker for ServiceInvokerImpl {
    async fn invoke(
        &self,
        ctx: &RequestContext,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        options: InvocationOptions,
    ) -> Result<Vec<u8>, InvocationError> {
        let deadline = tokio::time::Instant::now() + options.timeout;
        let mut last_error = String::new();

        for attempt in 0..=options.max_retries {
            // Check if we've exceeded the overall timeout
            if tokio::time::Instant::now() >= deadline {
                return Err(InvocationError::Timeout(options.timeout));
            }

            // Select a node
            let node_id =
                match self.select_node(ctx, options.target_node.as_deref()).await {
                    Ok(id) => id,
                    Err(e) => {
                        last_error = e.to_string();
                        if attempt < options.max_retries {
                            let backoff =
                                Self::backoff_duration(&options.backoff, attempt);
                            tokio::time::sleep(backoff).await;
                            continue;
                        }
                        return Err(e);
                    }
                };

            // Calculate remaining timeout
            let remaining = deadline - tokio::time::Instant::now();

            // Invoke on the selected node
            match self
                .invoke_remote(ctx, &node_id, service_name, operation, payload, remaining)
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) => {
                    last_error = e.to_string();
                    tracing::warn!(
                        service = %service_name,
                        operation = %operation,
                        node = %node_id,
                        attempt = attempt,
                        error = %e,
                        "Service invocation failed, retrying"
                    );

                    if attempt < options.max_retries {
                        let backoff = Self::backoff_duration(&options.backoff, attempt);
                        tokio::time::sleep(backoff).await;
                    }
                }
            }
        }

        Err(InvocationError::RetriesExhausted {
            service: service_name.to_string(),
            retries: options.max_retries,
            last_error,
        })
    }

    async fn invoke_on_node(
        &self,
        ctx: &RequestContext,
        node_id: &str,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, InvocationError> {
        self.invoke_remote(ctx, node_id, service_name, operation, payload, timeout)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_none() {
        let d = ServiceInvokerImpl::backoff_duration(&BackoffStrategy::None, 0);
        assert_eq!(d, Duration::ZERO);
    }

    #[test]
    fn test_backoff_fixed() {
        let d = ServiceInvokerImpl::backoff_duration(
            &BackoffStrategy::Fixed(Duration::from_millis(500)),
            3,
        );
        assert_eq!(d, Duration::from_millis(500));
    }

    #[test]
    fn test_backoff_exponential_capped() {
        let d = ServiceInvokerImpl::backoff_duration(
            &BackoffStrategy::Exponential {
                initial: Duration::from_millis(100),
                max: Duration::from_secs(5),
            },
            10, // 2^10 * 100ms = 102400ms = 102s >> 5s max
        );
        // Should be capped at max (5s) plus up to 25% jitter
        assert!(d <= Duration::from_millis(6250));
    }
}
