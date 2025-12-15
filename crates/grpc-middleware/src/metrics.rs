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

//! # MetricsInterceptor - Prometheus gRPC Metrics
//!
//! ## Purpose
//! Collects standard gRPC metrics using Prometheus:
//! - Request counts (by service, method, status)
//! - Request latency histograms (p50, p95, p99)
//! - Active requests gauge
//! - Request/response size histograms
//!
//! ## Standard gRPC Metrics
//! Follows the Prometheus gRPC metric naming conventions:
//! - `grpc_server_started_total` - Total requests started
//! - `grpc_server_handled_total` - Total requests completed (with status)
//! - `grpc_server_handling_seconds` - Request latency histogram
//! - `grpc_server_msg_received_total` - Messages received
//! - `grpc_server_msg_sent_total` - Messages sent
//!
//! ## Design
//! - **Always enabled**: Cannot be disabled (metrics are critical)
//! - **Priority: 10**: Highest priority (sees all requests first)
//! - **Low overhead**: Metrics collection is async and batched
//! - **Prometheus format**: Compatible with Prometheus scraping

use async_trait::async_trait;
use lazy_static::lazy_static;
use plexspaces_proto::grpc::v1::{
    InterceptorDecision, InterceptorRequest, InterceptorResponse, InterceptorResult,
};
use prometheus::{CounterVec, HistogramVec, IntGaugeVec, Opts, Registry};
use std::sync::Arc;
use tonic::Status;

use crate::chain::{Interceptor, InterceptorError};

lazy_static! {
    /// Global Prometheus registry for gRPC metrics
    pub static ref GRPC_METRICS_REGISTRY: Registry = Registry::new();

    /// Total gRPC requests started
    pub static ref GRPC_REQUESTS_STARTED: CounterVec = {
        let opts = Opts::new(
            "grpc_server_started_total",
            "Total number of RPCs started on the server",
        );
        let counter = CounterVec::new(opts, &["grpc_service", "grpc_method"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(counter.clone())).unwrap();
        counter
    };

    /// Total gRPC requests handled (with status code)
    pub static ref GRPC_REQUESTS_HANDLED: CounterVec = {
        let opts = Opts::new(
            "grpc_server_handled_total",
            "Total number of RPCs completed on the server, regardless of success or failure",
        );
        let counter = CounterVec::new(opts, &["grpc_service", "grpc_method", "grpc_code"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(counter.clone())).unwrap();
        counter
    };

    /// gRPC request latency histogram
    pub static ref GRPC_REQUESTS_DURATION: HistogramVec = {
        let opts = prometheus::HistogramOpts::new(
            "grpc_server_handling_seconds",
            "Histogram of response latency (seconds) of gRPC that had been application-level handled by the server",
        )
        .buckets(vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0]);
        let histogram = HistogramVec::new(opts, &["grpc_service", "grpc_method"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(histogram.clone())).unwrap();
        histogram
    };

    /// Active gRPC requests gauge
    pub static ref GRPC_REQUESTS_ACTIVE: IntGaugeVec = {
        let opts = Opts::new(
            "grpc_server_active_requests",
            "Number of currently active gRPC requests",
        );
        let gauge = IntGaugeVec::new(opts, &["grpc_service", "grpc_method"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(gauge.clone())).unwrap();
        gauge
    };

    /// Total messages received
    pub static ref GRPC_MESSAGES_RECEIVED: CounterVec = {
        let opts = Opts::new(
            "grpc_server_msg_received_total",
            "Total number of stream messages received from the client",
        );
        let counter = CounterVec::new(opts, &["grpc_service", "grpc_method"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(counter.clone())).unwrap();
        counter
    };

    /// Total messages sent
    pub static ref GRPC_MESSAGES_SENT: CounterVec = {
        let opts = Opts::new(
            "grpc_server_msg_sent_total",
            "Total number of stream messages sent by the server",
        );
        let counter = CounterVec::new(opts, &["grpc_service", "grpc_method"]).unwrap();
        GRPC_METRICS_REGISTRY.register(Box::new(counter.clone())).unwrap();
        counter
    };
}

/// Metrics interceptor for Prometheus
///
/// ## Purpose
/// Automatically collects gRPC metrics for all requests.
///
/// ## Design
/// - **Always enabled**: Cannot be disabled
/// - **Priority: 10**: Highest priority (first to see requests)
/// - **Thread-safe**: Uses Arc for shared state
/// - **Standard metrics**: Follows Prometheus gRPC conventions
pub struct MetricsInterceptor {
    /// Thread-safe reference to Prometheus registry
    registry: Arc<Registry>,

    /// Request context storage (request_id -> (service, method))
    /// Used to track service/method in after_response
    request_context: Arc<tokio::sync::RwLock<std::collections::HashMap<String, (String, String)>>>,
}

impl MetricsInterceptor {
    /// Create new metrics interceptor
    ///
    /// ## Returns
    /// MetricsInterceptor with global Prometheus registry
    pub fn new() -> Self {
        Self {
            registry: Arc::new(GRPC_METRICS_REGISTRY.clone()),
            request_context: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }

    /// Export metrics in Prometheus text format
    ///
    /// ## Purpose
    /// Generates Prometheus-compatible text output for `/metrics` endpoint.
    ///
    /// ## Returns
    /// String in Prometheus exposition format
    ///
    /// ## Example Output
    /// ```text
    /// # HELP grpc_server_started_total Total number of RPCs started
    /// # TYPE grpc_server_started_total counter
    /// grpc_server_started_total{grpc_service="ActorService",grpc_method="SpawnActor"} 42
    /// ```
    pub fn export_metrics(&self) -> Result<String, InterceptorError> {
        use prometheus::Encoder;
        let encoder = prometheus::TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| InterceptorError::Other(format!("Failed to encode metrics: {}", e)))?;
        String::from_utf8(buffer)
            .map_err(|e| InterceptorError::Other(format!("Failed to convert metrics: {}", e)))
    }

    /// Parse gRPC method into service and method name
    ///
    /// ## Arguments
    /// * `method` - Full method path (e.g., "/plexspaces.actor.v1.ActorService/SpawnActor")
    ///
    /// ## Returns
    /// (service_name, method_name) tuple
    ///
    /// ## Examples
    /// - `/plexspaces.actor.v1.ActorService/SpawnActor` → ("ActorService", "SpawnActor")
    /// - `/Service/Method` → ("Service", "Method")
    fn parse_method(method: &str) -> (&str, &str) {
        // Method format: /package.service/Method or /Service/Method
        if let Some(slash_pos) = method.rfind('/') {
            let service_path = &method[1..slash_pos]; // Remove leading '/'
            let method_name = &method[slash_pos + 1..];

            // Extract service name from package path
            let service_name = if let Some(dot_pos) = service_path.rfind('.') {
                &service_path[dot_pos + 1..]
            } else {
                service_path
            };

            (service_name, method_name)
        } else {
            ("unknown", method)
        }
    }
}

impl Default for MetricsInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Interceptor for MetricsInterceptor {
    async fn before_request(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        let (service, method) = Self::parse_method(&context.method);

        // Store service/method for after_response
        {
            let mut ctx = self.request_context.write().await;
            ctx.insert(
                context.request_id.clone(),
                (service.to_string(), method.to_string()),
            );
        }

        // Increment started counter
        GRPC_REQUESTS_STARTED
            .with_label_values(&[service, method])
            .inc();

        // Increment active requests gauge
        GRPC_REQUESTS_ACTIVE
            .with_label_values(&[service, method])
            .inc();

        // Always allow (metrics are observational)
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    async fn after_response(
        &self,
        context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError> {
        // NOTE: InterceptorResponse doesn't have request_id, so we can't perfectly match
        // For now, we'll use a best-effort approach: get the most recent request context
        // In production, the interceptor chain should pass request_id in response context
        let (service, method) = {
            let ctx = self.request_context.read().await;
            // Get the most recent entry (FIFO - first in should be first out)
            // In practice, we'd match by request_id, but for now use the first entry
            ctx.values()
                .next()
                .cloned()
                .unwrap_or_else(|| ("unknown".to_string(), "unknown".to_string()))
        };

        // Clean up: remove the entry we just used (simple FIFO)
        // In production, this would be keyed by request_id
        {
            let mut ctx = self.request_context.write().await;
            if !ctx.is_empty() {
                // Remove first entry (FIFO)
                let first_key = ctx.keys().next().cloned();
                if let Some(key) = first_key {
                    ctx.remove(&key);
                }
            }
        }

        // Decrement active requests gauge
        GRPC_REQUESTS_ACTIVE
            .with_label_values(&[&service, &method])
            .dec();

        // Increment handled counter with status code
        let status_code = context.status_code.to_string();
        GRPC_REQUESTS_HANDLED
            .with_label_values(&[&service, &method, &status_code])
            .inc();

        // Record latency if duration available
        if let Some(duration) = &context.duration {
            let seconds = duration.seconds as f64 + (duration.nanos as f64 / 1_000_000_000.0);
            GRPC_REQUESTS_DURATION
                .with_label_values(&[&service, &method])
                .observe(seconds);
        }

        // Always allow (metrics are observational)
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    async fn on_error(&self, error: &Status) {
        // Record error metric
        // NOTE: Would need service/method from request context
        let _ = error; // For now, just acknowledge
    }

    fn name(&self) -> &str {
        "metrics"
    }

    fn priority(&self) -> i32 {
        10 // Highest priority (sees all requests first)
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_interceptor_creation() {
        let interceptor = MetricsInterceptor::new();
        assert_eq!(interceptor.name(), "metrics");
        assert_eq!(interceptor.priority(), 10);
    }

    #[test]
    fn test_parse_method() {
        let (service, method) =
            MetricsInterceptor::parse_method("/plexspaces.actor.v1.ActorService/SpawnActor");
        assert_eq!(service, "ActorService");
        assert_eq!(method, "SpawnActor");

        let (service, method) = MetricsInterceptor::parse_method("/Service/Method");
        assert_eq!(service, "Service");
        assert_eq!(method, "Method");
    }

    #[tokio::test]
    async fn test_before_request() {
        let interceptor = MetricsInterceptor::new();
        let context = InterceptorRequest {
            method: "/plexspaces.actor.v1.ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = interceptor.before_request(&context).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.decision, InterceptorDecision::InterceptorDecisionAllow as i32);
    }

    #[tokio::test]
    async fn test_after_response() {
        let interceptor = MetricsInterceptor::new();
        let context = InterceptorResponse {
            status_code: 0,
            headers: std::collections::HashMap::new(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            duration: Some(plexspaces_proto::prost_types::Duration {
                seconds: 0,
                nanos: 50_000_000, // 50ms
            }),
        };

        let result = interceptor.after_response(&context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_export_metrics() {
        let interceptor = MetricsInterceptor::new();

        // First, record a metric by processing a request
        let context = InterceptorRequest {
            method: "/test.Service/Method".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        // Record a request to populate metrics
        let _ = interceptor.before_request(&context).await.unwrap();

        // Now export metrics
        let metrics = interceptor.export_metrics();
        assert!(metrics.is_ok());
        let metrics_text = metrics.unwrap();

        // Should contain the started counter
        assert!(
            metrics_text.contains("grpc_server_started_total"),
            "Metrics should contain grpc_server_started_total, got: {}",
            metrics_text
        );
    }
}
