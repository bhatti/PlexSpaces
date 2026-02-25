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

    /// Invoke a service on a specific node via gRPC
    async fn invoke_remote(
        &self,
        _ctx: &RequestContext,
        node_id: &str,
        service_name: &str,
        operation: &str,
        payload: &[u8],
        timeout: Duration,
    ) -> Result<Vec<u8>, InvocationError> {
        // Get gRPC channel to the target node
        let channel = self
            .service_locator
            .get_actor_service_client(node_id)
            .await
            .map_err(|e| InvocationError::TransportError(e.to_string()))?;

        // Create a generic service invocation request
        // Using the existing actor service proto for generic invocation
        use plexspaces_proto::common::v1::Message;
        let request_msg = Message {
            msg_type: format!("{}:{}", service_name, operation),
            payload: payload.to_vec(),
            sender_id: self.node_id.clone(),
            ..Default::default()
        };

        // Send via the gRPC channel with timeout
        use plexspaces_proto::v1::actors::actor_service_client::ActorServiceClient;
        let mut client = ActorServiceClient::new(channel);
        client = client.max_decoding_message_size(64 * 1024 * 1024); // 64MB

        let mut tonic_request =
            tonic::Request::new(plexspaces_proto::v1::actors::InvokeActorRequest {
                actor_id: format!("__service__:{}:{}", service_name, operation),
                message: Some(request_msg),
                timeout_ms: timeout.as_millis() as u64,
                ..Default::default()
            });

        tonic_request.set_timeout(timeout);

        let response = client
            .invoke_actor(tonic_request)
            .await
            .map_err(|e| InvocationError::TransportError(e.to_string()))?;

        let inner = response.into_inner();
        if !inner.error.is_empty() {
            return Err(InvocationError::RemoteError {
                service: service_name.to_string(),
                message: inner.error,
            });
        }

        Ok(inner
            .response
            .map(|m| m.payload)
            .unwrap_or_default())
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
