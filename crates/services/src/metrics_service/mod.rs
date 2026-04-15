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
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the GNU Lesser
// General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Metrics Service: unified Prometheus export via `metrics` + `metrics-exporter-prometheus`.
//!
//! Typed `Record*` RPCs map to R.E.D. series with namespace labels. Recording is synchronous
//! and allocation-light on the hot path; gRPC handlers return as soon as updates complete.
//!
//! ## Global recorder and tests
//!
//! The process-wide handle lives in a module-level `OnceLock`. Service startup calls
//! [`install_metrics_recorder`] before registering other services (see
//! `service_locator::initialize_services_impl`).

use std::collections::HashMap;
use std::sync::OnceLock;

pub use metrics_exporter_prometheus::PrometheusHandle;
use plexspaces_proto::metrics::v1::metric::Value as MetricValue;
use plexspaces_proto::metrics::v1::metrics_service_server::MetricsService;
use plexspaces_proto::metrics::v1::*;
use prost_types::Timestamp;
use tonic::{Request, Response, Status};

static PROM_HANDLE: OnceLock<PrometheusHandle> = OnceLock::new();

/// Install the global Prometheus recorder once per process; returns a handle for scrape/render.
///
/// # Panics
///
/// Panics if installation fails (for example a non-Prometheus global recorder was already
/// installed).
pub fn install_metrics_recorder() -> PrometheusHandle {
    PROM_HANDLE
        .get_or_init(|| {
            metrics_exporter_prometheus::PrometheusBuilder::new()
                .install_recorder()
                .unwrap_or_else(|e| panic!("failed to install metrics recorder: {e}"))
        })
        .clone()
}

/// Bridges [`PrometheusHandle`] to [`plexspaces_core::MetricsPrometheusRenderer`].
pub struct PrometheusHandleRenderer(PrometheusHandle);

impl PrometheusHandleRenderer {
    /// Creates a renderer that shares the same underlying recorder as [`MetricsServiceImpl`].
    pub fn new(handle: PrometheusHandle) -> Self {
        Self(handle)
    }
}

impl plexspaces_core::MetricsPrometheusRenderer for PrometheusHandleRenderer {
    fn render_prometheus_text(&self) -> String {
        self.0.render()
    }
}

fn prost_duration_secs(d: &Option<prost_types::Duration>) -> f64 {
    d.as_ref()
        .map(|x| x.seconds as f64 + f64::from(x.nanos) / 1_000_000_000.0)
        .unwrap_or(0.0)
}

/// Record message routing R.E.D. metrics (shared by RPC and in-process helpers).
pub fn record_message_routing_red(
    namespace: &str,
    success: bool,
    duration_secs: f64,
    error_type: &str,
) {
    let ns = namespace.to_string();
    metrics::counter!("plexspaces_messages_routed_total", "namespace" => ns.clone()).increment(1);
    if success {
        metrics::histogram!(
            "plexspaces_message_routing_duration_seconds",
            "namespace" => ns.clone(),
        )
        .record(duration_secs);
    } else {
        let et = if error_type.is_empty() {
            "unknown".to_string()
        } else {
            error_type.to_string()
        };
        metrics::counter!(
            "plexspaces_messages_failed_total",
            "namespace" => ns.clone(),
            "error_type" => et,
        )
        .increment(1);
        metrics::histogram!(
            "plexspaces_message_routing_duration_seconds",
            "namespace" => ns,
        )
        .record(duration_secs);
    }
}

/// Record actor activation R.E.D. metrics.
pub fn record_actor_activation_red(
    namespace: &str,
    activation_type: &str,
    success: bool,
    duration_secs: f64,
) {
    let ns = namespace.to_string();
    let at = activation_type.to_string();
    metrics::counter!(
        "plexspaces_actor_activations_total",
        "namespace" => ns.clone(),
        "activation_type" => at.clone(),
    )
    .increment(1);
    if !success {
        metrics::counter!(
            "plexspaces_actor_activation_errors_total",
            "namespace" => ns.clone(),
        )
        .increment(1);
    }
    metrics::histogram!(
        "plexspaces_actor_activation_duration_seconds",
        "namespace" => ns,
        "activation_type" => at,
    )
    .record(duration_secs);
}

/// Record channel operation R.E.D. metrics (`delivery_count` / `reason` are label dimensions).
pub fn record_channel_metrics_red(
    namespace: &str,
    operation: &str,
    backend: &str,
    success: bool,
    duration_secs: f64,
    delivery_count: u32,
    reason: &str,
) {
    let ns = namespace.to_string();
    let op = operation.to_string();
    let be = backend.to_string();
    let dc = delivery_count.to_string();
    let rs = reason.to_string();
    metrics::counter!(
        "plexspaces_channel_operations_total",
        "namespace" => ns.clone(),
        "operation" => op.clone(),
        "backend" => be.clone(),
    )
    .increment(1);
    if !success {
        metrics::counter!(
            "plexspaces_channel_errors_total",
            "namespace" => ns.clone(),
            "operation" => op.clone(),
            "backend" => be.clone(),
        )
        .increment(1);
    }
    metrics::histogram!(
        "plexspaces_channel_operation_duration_seconds",
        "namespace" => ns.clone(),
        "operation" => op.clone(),
        "backend" => be.clone(),
    )
    .record(duration_secs);
    // High-cardinality-safe auxiliary counters for DLQ / retry analysis
    if delivery_count > 0 {
        metrics::counter!(
            "plexspaces_channel_delivery_attempts_total",
            "namespace" => ns.clone(),
            "operation" => op.clone(),
            "backend" => be.clone(),
            "delivery_count" => dc,
        )
        .increment(1);
    }
    if !rs.is_empty() {
        metrics::counter!(
            "plexspaces_channel_operation_reason_total",
            "namespace" => ns.clone(),
            "operation" => op,
            "backend" => be,
            "reason" => rs,
        )
        .increment(1);
    }
}

pub(crate) fn metric_name_matches(pattern: &str, name: &str) -> bool {
    let pattern = pattern.trim();
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    if let Some(prefix) = pattern.strip_suffix('*') {
        return name.starts_with(prefix);
    }
    if let Some(suffix) = pattern.strip_prefix('*') {
        return name.ends_with(suffix);
    }
    name == pattern
}

fn labels_match_filter(labels: &HashMap<String, String>, filter: &HashMap<String, String>) -> bool {
    filter
        .iter()
        .all(|(k, v)| labels.get(k).map(|lv| lv == v).unwrap_or(false))
}

fn split_prometheus_metric_line(line: &str) -> Option<(&str, &str)> {
    let line = line.trim();
    if let Some(pos) = line.rfind('}') {
        let after = line[pos + 1..].trim_start();
        let v = after.split_whitespace().next()?;
        return Some((line.get(..=pos)?, v));
    }
    let mut it = line.split_whitespace();
    let left = it.next()?;
    let v = it.next()?;
    Some((left, v))
}

/// Parse Prometheus exposition text into simple counter/gauge [`Metric`] values (skips histogram buckets).
pub(crate) fn parse_prometheus_text(
    text: &str,
    name_pat: &str,
    label_filter: &HashMap<String, String>,
) -> Vec<Metric> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((left, value_str)) = split_prometheus_metric_line(line) else {
            continue;
        };
        let value_str = value_str.trim();
        if value_str == "NaN" {
            continue;
        }
        let Ok(value) = value_str.parse::<f64>() else {
            continue;
        };
        let left = left.trim();
        let (name, labels) = if let Some(i) = left.find('{') {
            let name = left[..i].to_string();
            let Some(rest) = left.get(i..) else {
                continue;
            };
            let Some(close) = rest.rfind('}') else {
                continue;
            };
            let label_src = &rest[1..close];
            let mut map = HashMap::new();
            for part in label_src.split(',') {
                let part = part.trim();
                if part.is_empty() {
                    continue;
                }
                if let Some((k, v)) = part.split_once('=') {
                    let v = v.trim().trim_matches('"');
                    map.insert(k.trim().to_string(), v.to_string());
                }
            }
            (name, map)
        } else {
            (left.to_string(), HashMap::new())
        };
        if name.ends_with("_bucket") || name.ends_with("_sum") || name.ends_with("_count") {
            continue;
        }
        if !metric_name_matches(name_pat, &name) {
            continue;
        }
        if !labels_match_filter(&labels, label_filter) {
            continue;
        }
        out.push(Metric {
            name,
            labels,
            timestamp: Some(Timestamp {
                seconds: time_now_secs(),
                nanos: 0,
            }),
            value: Some(MetricValue::GaugeValue(value)),
        });
    }
    out
}

fn time_now_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) fn unified_metric_definitions() -> Vec<MetricDefinition> {
    let mut defs = vec![
        MetricDefinition {
            name: "plexspaces_messages_routed_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Total messages routed (rate)".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_messages_failed_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Failed message routings".to_string(),
            labels: vec!["namespace".to_string(), "error_type".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_local_deliveries_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Local message deliveries by namespace".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_remote_deliveries_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Remote message deliveries by namespace".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_message_routing_duration_seconds".to_string(),
            r#type: MetricType::MetricTypeHistogram as i32,
            help: "Message routing duration".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![0.000_1, 0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        },
        MetricDefinition {
            name: "plexspaces_actor_activations_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Actor activations by type".to_string(),
            labels: vec!["namespace".to_string(), "activation_type".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_actor_activation_errors_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Failed actor activations".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_actor_activation_duration_seconds".to_string(),
            r#type: MetricType::MetricTypeHistogram as i32,
            help: "Actor activation duration".to_string(),
            labels: vec!["namespace".to_string(), "activation_type".to_string()],
            buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        },
        MetricDefinition {
            name: "plexspaces_actor_spawn_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Total actors registered on this node (spawn/register)".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_actor_active".to_string(),
            r#type: MetricType::MetricTypeGauge as i32,
            help: "Currently registered active actors on this node".to_string(),
            labels: vec!["namespace".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_operations_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel operations".to_string(),
            labels: vec![
                "namespace".to_string(),
                "operation".to_string(),
                "backend".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_errors_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel operation errors".to_string(),
            labels: vec![
                "namespace".to_string(),
                "operation".to_string(),
                "backend".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_operation_duration_seconds".to_string(),
            r#type: MetricType::MetricTypeHistogram as i32,
            help: "Channel operation duration".to_string(),
            labels: vec![
                "namespace".to_string(),
                "operation".to_string(),
                "backend".to_string(),
            ],
            buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0],
        },
        MetricDefinition {
            name: "plexspaces_channel_ack_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel ACK events".to_string(),
            labels: vec!["channel".to_string(), "backend".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_nack_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel NACK events".to_string(),
            labels: vec![
                "channel".to_string(),
                "backend".to_string(),
                "requeue".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_dlq_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel DLQ events".to_string(),
            labels: vec![
                "channel".to_string(),
                "backend".to_string(),
                "reason".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "plexspaces_channel_error_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "Channel errors".to_string(),
            labels: vec![
                "channel".to_string(),
                "backend".to_string(),
                "operation".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "grpc_server_started_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "gRPC requests started".to_string(),
            labels: vec!["grpc_service".to_string(), "grpc_method".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "grpc_server_handled_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "gRPC requests completed".to_string(),
            labels: vec![
                "grpc_service".to_string(),
                "grpc_method".to_string(),
                "grpc_code".to_string(),
            ],
            buckets: vec![],
        },
        MetricDefinition {
            name: "grpc_server_handling_seconds".to_string(),
            r#type: MetricType::MetricTypeHistogram as i32,
            help: "gRPC handling latency".to_string(),
            labels: vec!["grpc_service".to_string(), "grpc_method".to_string()],
            buckets: vec![0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0],
        },
        MetricDefinition {
            name: "grpc_server_active_requests".to_string(),
            r#type: MetricType::MetricTypeGauge as i32,
            help: "Active gRPC requests".to_string(),
            labels: vec!["grpc_service".to_string(), "grpc_method".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "grpc_server_msg_received_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "gRPC stream messages received".to_string(),
            labels: vec!["grpc_service".to_string(), "grpc_method".to_string()],
            buckets: vec![],
        },
        MetricDefinition {
            name: "grpc_server_msg_sent_total".to_string(),
            r#type: MetricType::MetricTypeCounter as i32,
            help: "gRPC stream messages sent".to_string(),
            labels: vec!["grpc_service".to_string(), "grpc_method".to_string()],
            buckets: vec![],
        },
    ];
    defs.extend(legacy_node_metric_definitions());
    defs
}

fn legacy_node_metric_definitions() -> Vec<MetricDefinition> {
    vec![
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
            labels: vec![
                "supervisor_id".to_string(),
                "child_type".to_string(),
                "restart_policy".to_string(),
            ],
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
            help: "Total size of actor subtree".to_string(),
            labels: vec!["actor_id".to_string()],
            buckets: vec![],
        },
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
    ]
}

/// Metrics Service implementation backed by a shared Prometheus handle.
#[derive(Clone)]
pub struct MetricsServiceImpl {
    prometheus_handle: PrometheusHandle,
}

impl MetricsServiceImpl {
    pub fn new(prometheus_handle: PrometheusHandle) -> Self {
        Self { prometheus_handle }
    }
}

#[async_trait::async_trait]
impl plexspaces_core::MetricsServiceAccess for MetricsServiceImpl {
    async fn export_prometheus_text(&self) -> String {
        self.prometheus_handle.render()
    }

    async fn get_metrics_filtered(
        &self,
        name_pattern: String,
        label_filter: HashMap<String, String>,
    ) -> Vec<Metric> {
        let text = self.prometheus_handle.render();
        parse_prometheus_text(&text, &name_pattern, &label_filter)
    }

    async fn list_metric_definitions_filtered(
        &self,
        name_pattern: String,
    ) -> Vec<MetricDefinition> {
        unified_metric_definitions()
            .into_iter()
            .filter(|d| metric_name_matches(&name_pattern, &d.name))
            .collect()
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsServiceImpl {
    async fn export_prometheus(
        &self,
        _request: Request<ExportPrometheusRequest>,
    ) -> Result<Response<ExportPrometheusResponse>, Status> {
        let content = self.prometheus_handle.render();
        Ok(Response::new(ExportPrometheusResponse { content }))
    }

    async fn get_metrics(
        &self,
        request: Request<GetMetricsRequest>,
    ) -> Result<Response<GetMetricsResponse>, Status> {
        let req = request.into_inner();
        let text = self.prometheus_handle.render();
        let metrics = parse_prometheus_text(&text, &req.name_pattern, &req.label_filter);
        Ok(Response::new(GetMetricsResponse { metrics }))
    }

    async fn list_metric_definitions(
        &self,
        request: Request<ListMetricDefinitionsRequest>,
    ) -> Result<Response<ListMetricDefinitionsResponse>, Status> {
        let pat = request.into_inner().name_pattern;
        let definitions: Vec<MetricDefinition> = unified_metric_definitions()
            .into_iter()
            .filter(|d| metric_name_matches(&pat, &d.name))
            .collect();
        Ok(Response::new(ListMetricDefinitionsResponse { definitions }))
    }

    async fn record_metric(
        &self,
        request: Request<RecordMetricRequest>,
    ) -> Result<Response<plexspaces_proto::common::v1::Empty>, Status> {
        let req = request.into_inner();
        let Some(m) = req.metric else {
            return Err(Status::invalid_argument("metric required"));
        };
        let name = m.name;
        if name.is_empty() {
            return Err(Status::invalid_argument("metric.name required"));
        }
        let labels: Vec<metrics::Label> = m
            .labels
            .iter()
            .map(|(k, v)| metrics::Label::new(k.clone(), v.clone()))
            .collect();
        match m.value {
            Some(MetricValue::CounterValue(v)) => {
                metrics::counter!(name.clone(), labels.clone()).increment(v as u64);
            }
            Some(MetricValue::GaugeValue(v)) => {
                metrics::gauge!(name, labels).set(v);
            }
            Some(MetricValue::HistogramValue(_)) => {
                return Err(Status::unimplemented(
                    "histogram recording via RecordMetric is not supported",
                ));
            }
            Some(MetricValue::SummaryValue(_)) => {
                return Err(Status::unimplemented(
                    "summary recording via RecordMetric is not supported",
                ));
            }
            None => return Err(Status::invalid_argument("metric.value required")),
        }
        Ok(Response::new(plexspaces_proto::common::v1::Empty {}))
    }

    async fn record_message_routing(
        &self,
        request: Request<RecordMessageRoutingRequest>,
    ) -> Result<Response<plexspaces_proto::common::v1::Empty>, Status> {
        let r = request.into_inner();
        let ns = if r.namespace.is_empty() {
            "default"
        } else {
            r.namespace.as_str()
        };
        let dur = prost_duration_secs(&r.duration);
        let err = r.error_type.as_str();
        record_message_routing_red(ns, r.success, dur, err);
        let _ = r.actor_id;
        Ok(Response::new(plexspaces_proto::common::v1::Empty {}))
    }

    async fn record_actor_activation(
        &self,
        request: Request<RecordActorActivationRequest>,
    ) -> Result<Response<plexspaces_proto::common::v1::Empty>, Status> {
        let r = request.into_inner();
        let ns = if r.namespace.is_empty() {
            "default"
        } else {
            r.namespace.as_str()
        };
        let at = if r.activation_type.is_empty() {
            "unknown"
        } else {
            r.activation_type.as_str()
        };
        record_actor_activation_red(ns, at, r.success, prost_duration_secs(&r.duration));
        let _ = r.actor_id;
        Ok(Response::new(plexspaces_proto::common::v1::Empty {}))
    }

    async fn record_channel_metrics(
        &self,
        request: Request<RecordChannelMetricsRequest>,
    ) -> Result<Response<plexspaces_proto::common::v1::Empty>, Status> {
        let r = request.into_inner();
        let ns = if r.namespace.is_empty() {
            "default"
        } else {
            r.namespace.as_str()
        };
        record_channel_metrics_red(
            ns,
            r.operation.as_str(),
            r.backend.as_str(),
            r.success,
            prost_duration_secs(&r.duration),
            r.delivery_count,
            r.reason.as_str(),
        );
        let _ = r.channel_name;
        Ok(Response::new(plexspaces_proto::common::v1::Empty {}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_service() -> MetricsServiceImpl {
        let h = install_metrics_recorder();
        MetricsServiceImpl::new(h)
    }

    #[tokio::test]
    async fn test_export_prometheus_includes_recorded_counter() {
        let service = test_service();
        metrics::counter!("plexspaces_messages_routed_total", "namespace" => "test-ns")
            .increment(1);
        let request = Request::new(ExportPrometheusRequest {});
        let response = service.export_prometheus(request).await.unwrap();
        let body = &response.get_ref().content;
        assert!(
            body.contains("plexspaces_messages_routed_total"),
            "body={}",
            body
        );
        assert!(body.contains("test-ns"), "body={}", body);
    }

    #[tokio::test]
    async fn test_record_metric_counter_visible_in_export() {
        let service = test_service();
        let request = Request::new(RecordMetricRequest {
            metric: Some(Metric {
                name: "plexspaces_manual_test_counter".to_string(),
                labels: std::collections::HashMap::from([("env".to_string(), "test".to_string())]),
                timestamp: None,
                value: Some(MetricValue::CounterValue(3.0)),
            }),
        });
        service.record_metric(request).await.unwrap();
        let exp = service
            .export_prometheus(Request::new(ExportPrometheusRequest {}))
            .await
            .unwrap();
        assert!(exp
            .get_ref()
            .content
            .contains("plexspaces_manual_test_counter"));
    }

    #[tokio::test]
    async fn test_record_message_routing_rpc() {
        let service = test_service();
        let request = Request::new(RecordMessageRoutingRequest {
            actor_id: "a1".to_string(),
            namespace: "app1".to_string(),
            success: true,
            duration: Some(prost_types::Duration {
                seconds: 0,
                nanos: 1_000_000,
            }),
            error_type: String::new(),
        });
        service.record_message_routing(request).await.unwrap();
        let exp = service
            .export_prometheus(Request::new(ExportPrometheusRequest {}))
            .await
            .unwrap();
        assert!(exp
            .get_ref()
            .content
            .contains("plexspaces_messages_routed_total"));
        assert!(exp.get_ref().content.contains("app1"));
    }

    #[tokio::test]
    async fn test_list_metric_definitions_includes_unified() {
        let service = test_service();
        let request = Request::new(ListMetricDefinitionsRequest {
            name_pattern: "plexspaces_messages_*".to_string(),
        });
        let response = service.list_metric_definitions(request).await.unwrap();
        let names: Vec<_> = response
            .get_ref()
            .definitions
            .iter()
            .map(|d| d.name.as_str())
            .collect();
        assert!(names.contains(&"plexspaces_messages_routed_total"));
        assert!(names.contains(&"plexspaces_messages_failed_total"));
    }

    #[tokio::test]
    async fn test_get_metrics_filters_by_name() {
        let service = test_service();
        metrics::counter!("plexspaces_messages_routed_total", "namespace" => "z9").increment(1);
        let request = Request::new(GetMetricsRequest {
            name_pattern: "plexspaces_messages_routed*".to_string(),
            label_filter: std::collections::HashMap::new(),
        });
        let response = service.get_metrics(request).await.unwrap();
        assert!(response
            .get_ref()
            .metrics
            .iter()
            .any(|m| m.name == "plexspaces_messages_routed_total"));
    }

    #[tokio::test]
    async fn test_metrics_service_access_matches_grpc_read_paths() {
        use plexspaces_core::MetricsServiceAccess;

        let service = test_service();
        metrics::counter!("plexspaces_messages_routed_total", "namespace" => "trait-ns")
            .increment(3);
        let text = MetricsServiceAccess::export_prometheus_text(&service).await;
        assert!(text.contains("trait-ns"));
        let mut lf = std::collections::HashMap::new();
        lf.insert("namespace".to_string(), "trait-ns".to_string());
        let samples = service
            .get_metrics_filtered("plexspaces_messages_routed_total".to_string(), lf)
            .await;
        let sum: f64 = samples
            .iter()
            .filter_map(|m| match &m.value {
                Some(MetricValue::GaugeValue(v)) => Some(*v),
                Some(MetricValue::CounterValue(v)) => Some(*v),
                _ => None,
            })
            .sum();
        assert!(sum >= 3.0, "samples={:?}", samples);
        let defs = service
            .list_metric_definitions_filtered("plexspaces_actor_spawn_total".to_string())
            .await;
        assert!(
            defs.iter()
                .any(|d| d.name == "plexspaces_actor_spawn_total"),
            "defs={:?}",
            defs
        );
    }
}
