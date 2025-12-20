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

//! Observability
//!
//! ## Purpose
//! Comprehensive observability integration for production deployments:
//! - Prometheus metrics export (HTTP endpoint)
//! - OpenTelemetry tracing with tenant context
//! - Structured logging with tenant isolation
//! - Request context propagation
//!
//! ## Design Principles
//! 1. **Tenant-Aware**: All metrics/traces/logs include tenant_id
//! 2. **Low Overhead**: Async, batched, sampled
//! 3. **Production-Ready**: Follows industry best practices
//! 4. **Comprehensive**: Metrics for all operations

use std::collections::HashMap;
use std::sync::Arc;
use thiserror::Error;
use prometheus::{Encoder, Registry, TextEncoder};
use opentelemetry::{
    trace::{Span, SpanKind, Tracer, TraceContextExt},
    Context, KeyValue,
};
use tracing::{info, warn, error, debug};

/// Observability errors
#[derive(Debug, Error)]
pub enum ObservabilityError {
    /// Metrics export failed
    #[error("Metrics export failed: {0}")]
    MetricsExportFailed(String),

    /// Tracing setup failed
    #[error("Tracing setup failed: {0}")]
    TracingSetupFailed(String),
}

/// observability manager
pub struct ObservabilityManager {
    /// Prometheus registry
    registry: Arc<Registry>,
}

impl ObservabilityManager {
    /// Create a new observability manager
    pub fn new() -> Self {
        Self {
            registry: Arc::new(Registry::new()),
        }
    }

    /// Export metrics in Prometheus format
    ///
    /// ## Returns
    /// Prometheus text format string
    pub fn export_metrics(&self) -> Result<String, ObservabilityError> {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .map_err(|e| ObservabilityError::MetricsExportFailed(format!("Failed to encode metrics: {}", e)))?;
        String::from_utf8(buffer)
            .map_err(|e| ObservabilityError::MetricsExportFailed(format!("Failed to convert metrics: {}", e)))
    }

    /// Create a span with tenant context using global tracer
    ///
    /// ## Arguments
    /// * `name` - Span name (must be static string)
    /// * `tenant_id` - Tenant ID for context (required)
    /// * `attributes` - Additional span attributes
    ///
    /// ## Returns
    /// Span context
    pub fn create_span(
        &self,
        name: &'static str,
        tenant_id: &str,
        attributes: Vec<KeyValue>,
    ) -> Context {
        let tracer = opentelemetry::global::tracer("plexspaces");
        let mut span = tracer
            .span_builder(name)
            .with_kind(SpanKind::Server)
            .start_with_context(&tracer, &Context::current());

        // Add tenant_id attribute (required)
        span.set_attribute(KeyValue::new("tenant_id", tenant_id.to_string()));

        // Add custom attributes
        for attr in attributes {
            span.set_attribute(attr);
        }

        Context::current().with_span(span)
    }

    /// Log with tenant context (tenant_id is required)
    ///
    /// ## Arguments
    /// * `level` - Log level
    /// * `message` - Log message
    /// * `tenant_id` - Tenant ID (required for tenant isolation)
    /// * `fields` - Additional structured fields
    pub fn log_with_context(
        &self,
        level: LogLevel,
        message: &str,
        tenant_id: &str,
        fields: HashMap<String, String>,
    ) {
        match level {
            LogLevel::Debug => {
                debug!(tenant_id = %tenant_id, ?fields, "{}", message);
            }
            LogLevel::Info => {
                info!(tenant_id = %tenant_id, ?fields, "{}", message);
            }
            LogLevel::Warn => {
                warn!(tenant_id = %tenant_id, ?fields, "{}", message);
            }
            LogLevel::Error => {
                error!(tenant_id = %tenant_id, ?fields, "{}", message);
            }
        }
    }

    /// Get Prometheus registry
    pub fn registry(&self) -> &Registry {
        &self.registry
    }
}

impl Default for ObservabilityManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Log levels
#[derive(Debug, Clone, Copy)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

/// HTTP handler for Prometheus metrics endpoint
///
/// ## Usage
/// ```rust,ignore
/// use axum::{Router, routing::get};
/// let app = Router::new()
///     .route("/metrics", get(metrics_handler))
///     .with_state(observability_manager);
/// ```
pub async fn metrics_handler(
    manager: Arc<ObservabilityManager>,
) -> Result<String, ObservabilityError> {
    manager.export_metrics()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_observability_manager_new() {
        let manager = ObservabilityManager::new();
        assert!(manager.export_metrics().is_ok());
    }

    #[test]
    fn test_export_metrics() {
        let manager = ObservabilityManager::new();
        let metrics = manager.export_metrics().unwrap();
        
        // Should return valid Prometheus format (even if empty)
        assert!(metrics.contains("# TYPE") || metrics.is_empty());
    }

    #[test]
    fn test_log_with_context() {
        let manager = ObservabilityManager::new();
        let mut fields = HashMap::new();
        fields.insert("key1".to_string(), "value1".to_string());
        
        // Should not panic
        manager.log_with_context(LogLevel::Info, "test message", "tenant-123", fields);
    }

    #[test]
    fn test_create_span() {
        let manager = ObservabilityManager::new();
        let _span = manager.create_span("test-span", "tenant-123", vec![]);
        
        // Should create span context
        // Note: This uses global tracer, so it may not work in all test environments
    }
}

