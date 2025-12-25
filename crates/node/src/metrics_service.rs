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

//! # Metrics Service Implementation
//!
//! ## Purpose
//! Implements the MetricsService gRPC service for exporting metrics in Prometheus format.
//!
//! ## Architecture Context
//! This service provides:
//! - Prometheus text format export (`/metrics` endpoint)
//! - Structured metrics retrieval
//! - Metric definition listing
//!
//! ## Design Notes
//! Uses the `metrics` crate for metric collection and formats output in Prometheus text format.

use plexspaces_proto::metrics::v1::metrics_service_server::MetricsService;
use plexspaces_proto::metrics::v1::*;
use std::sync::Arc;
use tonic::{Request, Response, Status};

/// Metrics Service implementation
pub struct MetricsServiceImpl {
    /// Node reference for accessing metrics
    node: Option<Arc<crate::Node>>,
}

impl MetricsServiceImpl {
    /// Create new metrics service
    pub fn new() -> Self {
        Self { node: None }
    }

    /// Create metrics service with node reference
    pub fn with_node(node: Arc<crate::Node>) -> Self {
        Self { node: Some(node) }
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsServiceImpl {
    async fn export_prometheus(
        &self,
        _request: Request<ExportPrometheusRequest>,
    ) -> Result<Response<ExportPrometheusResponse>, Status> {
        // Export metrics in Prometheus text format
        // The metrics crate provides a recorder that we can query
        // For now, we'll use a simple implementation that formats known metrics
        
        let mut output = String::new();
        
        // Export all metrics from the metrics crate
        // Note: The metrics crate doesn't provide a direct way to export all metrics
        // in Prometheus format. We'll need to use metrics-exporter-prometheus or
        // implement a custom exporter.
        
        // For now, return a placeholder that indicates metrics are available
        // In production, this should use metrics-exporter-prometheus or a custom exporter
        output.push_str("# PlexSpaces Metrics (Prometheus format)\n");
        output.push_str("# Note: Full Prometheus export requires metrics-exporter-prometheus\n");
        output.push_str("# Current metrics are available via metrics crate recorder\n");
        
        // Add some example metrics to show the format
        output.push_str("# HELP plexspaces_node_health_requests_total Total health check requests\n");
        output.push_str("# TYPE plexspaces_node_health_requests_total counter\n");
        output.push_str("plexspaces_node_health_requests_total 0\n");
        
        output.push_str("# HELP plexspaces_node_readiness_checks_total Total readiness checks\n");
        output.push_str("# TYPE plexspaces_node_readiness_checks_total counter\n");
        output.push_str("plexspaces_node_readiness_checks_total 0\n");
        
        output.push_str("# HELP plexspaces_node_liveness_checks_total Total liveness checks\n");
        output.push_str("# TYPE plexspaces_node_liveness_checks_total counter\n");
        output.push_str("plexspaces_node_liveness_checks_total 0\n");

        Ok(Response::new(ExportPrometheusResponse {
            content: output,
        }))
    }

    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let _req = request.into_inner();
        
        // Return structured metrics
        // For now, return empty list - full implementation would query metrics crate
        Ok(Response::new(GetMetricsResponse {
            metrics: vec![],
        }))
    }

    async fn list_metric_definitions(
        &self,
        _request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        // Return standard metric definitions including all actor system metrics (Phase 8)
        let definitions = vec![
            // Node metrics
            MetricDefinition {
                name: "plexspaces_node_health_requests_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total health check requests".to_string(),
                labels: vec!["components_count".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_node_readiness_checks_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total readiness checks".to_string(),
                labels: vec![],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_node_liveness_checks_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total liveness checks".to_string(),
                labels: vec![],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_node_application_deploy_attempts_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total application deployment attempts".to_string(),
                labels: vec![],
                buckets: vec![],
            },
            // Actor lifecycle metrics (Phase 8)
            MetricDefinition {
                name: "plexspaces_actor_init_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total actor initializations".to_string(),
                labels: vec!["actor_type".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_actor_init_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Actor initialization duration in seconds".to_string(),
                labels: vec!["actor_type".to_string()],
                buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
            },
            MetricDefinition {
                name: "plexspaces_actor_terminate_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total actor terminations".to_string(),
                labels: vec!["actor_type".to_string(), "reason".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_actor_terminate_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Actor termination duration in seconds".to_string(),
                labels: vec!["actor_type".to_string()],
                buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
            },
            MetricDefinition {
                name: "plexspaces_actor_exit_handled_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total exit signals handled by actors".to_string(),
                labels: vec!["actor_type".to_string(), "exit_reason".to_string()],
                buckets: vec![],
            },
            // Supervisor metrics (Phase 8)
            MetricDefinition {
                name: "plexspaces_supervisor_child_started_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total supervisor child starts".to_string(),
                labels: vec!["supervisor_id".to_string(), "child_type".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_supervisor_child_stopped_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total supervisor child stops".to_string(),
                labels: vec!["supervisor_id".to_string(), "child_type".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_supervisor_child_restarted_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total supervisor child restarts".to_string(),
                labels: vec!["supervisor_id".to_string(), "child_type".to_string(), "restart_policy".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_supervisor_startup_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Supervisor startup duration in seconds".to_string(),
                labels: vec!["supervisor_id".to_string()],
                buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
            },
            MetricDefinition {
                name: "plexspaces_supervisor_shutdown_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Supervisor shutdown duration in seconds".to_string(),
                labels: vec!["supervisor_id".to_string()],
                buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
            },
            // Parent-child relationship metrics (Phase 8)
            MetricDefinition {
                name: "plexspaces_actor_children_count".to_string(),
                r#type: MetricType::MetricTypeGauge as i32,
                help: "Number of children for an actor/supervisor".to_string(),
                labels: vec!["actor_id".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_actor_subtree_size".to_string(),
                r#type: MetricType::MetricTypeGauge as i32,
                help: "Total size of actor subtree (including self and all descendants)".to_string(),
                labels: vec!["actor_id".to_string()],
                buckets: vec![],
            },
            // Application lifecycle metrics (Phase 8)
            MetricDefinition {
                name: "plexspaces_application_startup_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total application startup attempts".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_application_startup_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Application startup duration in seconds".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0],
            },
            MetricDefinition {
                name: "plexspaces_application_startup_success_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total successful application startups".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_application_startup_errors_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total failed application startup attempts".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_application_shutdown_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total application shutdown attempts".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_application_shutdown_duration_seconds".to_string(),
                r#type: MetricType::MetricTypeHistogram as i32,
                help: "Application shutdown duration in seconds".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![0.1, 0.5, 1.0, 5.0, 10.0, 30.0, 60.0],
            },
            MetricDefinition {
                name: "plexspaces_application_shutdown_success_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total successful application shutdowns".to_string(),
                labels: vec!["application".to_string()],
                buckets: vec![],
            },
            MetricDefinition {
                name: "plexspaces_application_shutdown_errors_total".to_string(),
                r#type: MetricType::MetricTypeCounter as i32,
                help: "Total failed application shutdown attempts".to_string(),
                labels: vec!["application".to_string(), "error_type".to_string()],
                buckets: vec![],
            },
        ];

        Ok(Response::new(ListMetricDefinitionsResponse { definitions }))
    }

    async fn record_metric(
        &self,
        request: Request<RecordMetricRequest>,
    ) -> Result<Response<plexspaces_proto::common::v1::Empty>, Status> {
        let _req = request.into_inner();
        // Record metric via metrics crate
        // For now, just return success
        Ok(Response::new(plexspaces_proto::common::v1::Empty {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_export_prometheus() {
        let service = MetricsServiceImpl::new();
        let request = Request::new(ExportPrometheusRequest {});
        let response = service.export_prometheus(request).await.unwrap();
        
        assert!(response.get_ref().content.contains("PlexSpaces Metrics"));
        assert!(response.get_ref().content.contains("plexspaces_node_health_requests_total"));
    }

    #[tokio::test]
    async fn test_list_metric_definitions() {
        let service = MetricsServiceImpl::new();
        let request = Request::new(ListMetricDefinitionsRequest {
            name_pattern: String::new(),
        });
        let response = service.list_metric_definitions(request).await.unwrap();
        
        assert!(!response.get_ref().definitions.is_empty());
        assert!(response.get_ref().definitions.iter().any(|d| d.name == "plexspaces_node_health_requests_total"));
    }
}
