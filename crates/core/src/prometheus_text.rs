// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Parse Prometheus text exposition for aggregate counter reads.
//!
//! Used to hydrate [`plexspaces_proto::node::v1::NodeMetrics`] operational fields from the
//! same recorder that powers `/metrics`, avoiding duplicate atomics.

use std::collections::HashMap;

use plexspaces_proto::metrics::v1::ActorMetrics;
use plexspaces_proto::node::v1::NodeMetrics;

/// Sums counter samples whose name equals `metric_name` and whose labels satisfy `required`.
///
/// Non-finite or negative sample values are skipped. Histogram `_bucket` / `_sum` lines are
/// ignored because their names do not equal `metric_name`.
pub fn sum_counter_for_labels(
    exposition: &str,
    metric_name: &str,
    required: &[(&str, &str)],
) -> u64 {
    let mut total = 0u64;
    for line in exposition.lines() {
        let Some((name, label_src, val)) = parse_counter_sample_line(line) else {
            continue;
        };
        if name != metric_name {
            continue;
        }
        let labels = parse_label_pairs(label_src);
        if !labels_match(&labels, required) {
            continue;
        }
        total = total.saturating_add(val);
    }
    total
}

/// Sums all counter samples for a metric name across every label set.
pub fn sum_counter_all_label_sets(exposition: &str, metric_name: &str) -> u64 {
    let mut total = 0u64;
    for line in exposition.lines() {
        let Some((name, _label_src, val)) = parse_counter_sample_line(line) else {
            continue;
        };
        if name == metric_name {
            total = total.saturating_add(val);
        }
    }
    total
}

/// Sums all sample values for a metric name across every label set (float, for histogram `_sum` / gauges).
pub fn sum_sample_values_all_series(exposition: &str, metric_name: &str) -> f64 {
    let mut total = 0.0_f64;
    for line in exposition.lines() {
        let Some((name, _label_src, value)) = parse_sample_line(line) else {
            continue;
        };
        if name == metric_name {
            total += value;
        }
    }
    total
}

/// Chart-oriented aggregates from one Prometheus text snapshot (local process recorder).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LocalPrometheusRecorderChartSummary {
    /// Weighted mean routing latency (`plexspaces_message_routing_duration_seconds`), milliseconds.
    pub message_routing_latency_avg_ms: f64,
    /// Max finite `le` with positive count across all routing histogram series, milliseconds.
    pub message_routing_latency_max_ms: f64,
    /// Weighted mean actor handler latency (`plexspaces_actor_message_processing_duration_seconds`), ms.
    pub actor_message_processing_latency_avg_ms: f64,
    /// Max finite `le` with positive count across all actor-processing histogram series, ms.
    pub actor_message_processing_latency_max_ms: f64,
    /// Sum of `plexspaces_application_tracked_supervisors` gauge samples (all applications).
    pub application_supervisors_total: u32,
}

/// Derives dashboard chart inputs from exposition (same text as `GetDashboardMetrics` / `/metrics`).
pub fn local_prometheus_recorder_chart_summary(exposition: &str) -> LocalPrometheusRecorderChartSummary {
    let mut out = LocalPrometheusRecorderChartSummary::default();
    let routing_base = "plexspaces_message_routing_duration_seconds";
    let actor_base = "plexspaces_actor_message_processing_duration_seconds";
    let sum_r = sum_sample_values_all_series(exposition, &format!("{routing_base}_sum"));
    let cnt_r = sum_sample_values_all_series(exposition, &format!("{routing_base}_count"));
    if cnt_r > 0.0 {
        out.message_routing_latency_avg_ms = (sum_r / cnt_r) * 1000.0;
    }
    if let Some(le) = max_histogram_finite_bucket_le_globally(exposition, routing_base) {
        out.message_routing_latency_max_ms = le * 1000.0;
    }
    let sum_a = sum_sample_values_all_series(exposition, &format!("{actor_base}_sum"));
    let cnt_a = sum_sample_values_all_series(exposition, &format!("{actor_base}_count"));
    if cnt_a > 0.0 {
        out.actor_message_processing_latency_avg_ms = (sum_a / cnt_a) * 1000.0;
    }
    if let Some(le) = max_histogram_finite_bucket_le_globally(exposition, actor_base) {
        out.actor_message_processing_latency_max_ms = le * 1000.0;
    }
    let sup = sum_sample_values_all_series(exposition, "plexspaces_application_tracked_supervisors");
    out.application_supervisors_total = sup.round().clamp(0.0, u32::MAX as f64) as u32;
    out
}

fn max_histogram_finite_bucket_le_globally(exposition: &str, histogram_name: &str) -> Option<f64> {
    let bucket_metric_name = format!("{histogram_name}_bucket");
    let mut max_bucket = None;
    for line in exposition.lines() {
        let Some((name, label_src, value)) = parse_sample_line(line) else {
            continue;
        };
        if name != bucket_metric_name || value <= 0.0 {
            continue;
        }
        let labels = parse_label_pairs(label_src);
        let Some(le_raw) = labels.get("le") else {
            continue;
        };
        if le_raw == "+Inf" {
            continue;
        }
        let Ok(le) = le_raw.parse::<f64>() else {
            continue;
        };
        if !le.is_finite() || le < 0.0 {
            continue;
        }
        max_bucket = Some(max_bucket.map_or(le, |current: f64| current.max(le)));
    }
    max_bucket
}

/// Sums all non-negative sample values for a metric name whose labels satisfy `required`.
///
/// Unlike [`sum_counter_for_labels`], this keeps floating-point precision so histogram `_sum`
/// series can be aggregated without truncating sub-millisecond values to zero.
pub fn sum_sample_values_for_labels(
    exposition: &str,
    metric_name: &str,
    required: &[(&str, &str)],
) -> f64 {
    let mut total = 0.0_f64;
    for line in exposition.lines() {
        let Some((name, label_src, value)) = parse_sample_line(line) else {
            continue;
        };
        if name != metric_name {
            continue;
        }
        let labels = parse_label_pairs(label_src);
        if !labels_match(&labels, required) {
            continue;
        }
        total += value;
    }
    total
}

/// Returns the largest finite histogram bucket upper bound (`le`) that has a positive count.
pub fn max_histogram_bucket_upper_bound_for_labels(
    exposition: &str,
    histogram_name: &str,
    required: &[(&str, &str)],
) -> Option<f64> {
    let bucket_metric_name = format!("{histogram_name}_bucket");
    let mut max_bucket = None;
    for line in exposition.lines() {
        let Some((name, label_src, value)) = parse_sample_line(line) else {
            continue;
        };
        if name != bucket_metric_name || value <= 0.0 {
            continue;
        }
        let labels = parse_label_pairs(label_src);
        if !labels_match(&labels, required) {
            continue;
        }
        let Some(le_raw) = labels.get("le") else {
            continue;
        };
        let Ok(le) = le_raw.parse::<f64>() else {
            continue;
        };
        if !le.is_finite() || le < 0.0 {
            continue;
        }
        max_bucket = Some(max_bucket.map_or(le, |current: f64| current.max(le)));
    }
    max_bucket
}

/// Overwrites operational counter fields on `m` from exposition filtered by `node_id`.
///
/// System fields (CPU, memory, uptime, actor counts) are left unchanged.
/// Builds an [`ActorMetrics`] row for dashboard APIs from Prometheus exposition.
///
/// Namespace-scoped counters use the `namespace` label; shard counters use `node_id`.
pub fn actor_metrics_from_exposition_for_namespace(
    exposition: &str,
    namespace: &str,
    node_id: &str,
    is_live: bool,
) -> ActorMetrics {
    let ns = [("namespace", namespace)];
    let nid = [("node_id", node_id)];
    let failed = sum_counter_for_labels(exposition, "plexspaces_messages_failed_total", &ns);
    ActorMetrics {
        spawn_total: sum_counter_for_labels(exposition, "plexspaces_actor_spawn_total", &ns),
        active: u64::from(is_live),
        messages_routed: sum_counter_for_labels(
            exposition,
            "plexspaces_messages_routed_total",
            &ns,
        ),
        local_deliveries: sum_counter_for_labels(
            exposition,
            "plexspaces_local_deliveries_total",
            &ns,
        ),
        remote_deliveries: sum_counter_for_labels(
            exposition,
            "plexspaces_remote_deliveries_total",
            &ns,
        ),
        failed_deliveries: failed,
        error_total: failed,
        init_total: 0,
        init_errors_total: 0,
        terminate_total: 0,
        terminate_errors_total: 0,
        exit_handled_total: 0,
        exit_propagated_total: 0,
        exit_handle_errors_total: 0,
        parent_child_registered_total: 0,
        parent_child_unregistered_total: 0,
        shard_groups_created_total: sum_counter_for_labels(
            exposition,
            "plexspaces_node_shard_groups_created_total",
            &nid,
        ),
        shard_messages_sent_total: sum_counter_for_labels(
            exposition,
            "plexspaces_node_shard_messages_sent_total",
            &nid,
        ),
        shard_messages_received_total: sum_counter_for_labels(
            exposition,
            "plexspaces_node_shard_messages_received_total",
            &nid,
        ),
        shard_operations_total: sum_counter_for_labels(
            exposition,
            "plexspaces_node_shard_operations_total",
            &nid,
        ),
        shard_operations_failed_total: sum_counter_for_labels(
            exposition,
            "plexspaces_node_shard_operations_failed_total",
            &nid,
        ),
    }
}

pub fn overlay_node_operational_counters_from_exposition(
    exposition: &str,
    node_id: &str,
    m: &mut NodeMetrics,
) {
    let req = [("node_id", node_id)];
    m.messages_routed =
        sum_counter_for_labels(exposition, "plexspaces_node_messages_routed_total", &req);
    m.local_deliveries =
        sum_counter_for_labels(exposition, "plexspaces_node_local_deliveries_total", &req);
    m.remote_deliveries =
        sum_counter_for_labels(exposition, "plexspaces_node_remote_deliveries_total", &req);
    m.failed_deliveries =
        sum_counter_for_labels(exposition, "plexspaces_node_failed_deliveries_total", &req);
    m.shard_groups_created = u32::try_from(sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_groups_created_total",
        &req,
    ))
    .unwrap_or(u32::MAX);
    m.shard_messages_sent = sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_messages_sent_total",
        &req,
    );
    m.shard_messages_received = sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_messages_received_total",
        &req,
    );
    m.shard_operations_total =
        sum_counter_for_labels(exposition, "plexspaces_node_shard_operations_total", &req);
    m.shard_operations_failed = sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_operations_failed_total",
        &req,
    );
}

fn parse_counter_sample_line(line: &str) -> Option<(&str, &str, u64)> {
    let (name, labels, value) = parse_sample_line(line)?;
    Some((name, labels, value as u64))
}

fn parse_sample_line(line: &str) -> Option<(&str, &str, f64)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let len = parts.len();
    let (value_str, metric_token_idx_end) = if len >= 3 && parts[len - 1].parse::<i64>().is_ok() {
        (&parts[len - 2], len - 3)
    } else {
        (&parts[len - 1], len - 2)
    };
    let v: f64 = value_str.parse().ok()?;
    if !v.is_finite() || v < 0.0 {
        return None;
    }
    let metric_token = *parts.get(metric_token_idx_end)?;
    let (name, labels) = split_metric_name_and_labels(metric_token);
    Some((name, labels, v))
}

fn split_metric_name_and_labels(metric_token: &str) -> (&str, &str) {
    if let Some(open) = metric_token.find('{') {
        if metric_token.ends_with('}') {
            return (
                metric_token[..open].trim(),
                &metric_token[open + 1..metric_token.len() - 1],
            );
        }
    }
    (metric_token.trim(), "")
}

fn parse_label_pairs(label_src: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if label_src.is_empty() {
        return out;
    }
    let mut current = String::new();
    let mut in_string = false;
    for ch in label_src.chars() {
        if ch == '"' {
            in_string = !in_string;
            current.push(ch);
        } else if ch == ',' && !in_string {
            push_label_pair(&current, &mut out);
            current.clear();
        } else {
            current.push(ch);
        }
    }
    push_label_pair(&current, &mut out);
    out
}

fn push_label_pair(raw: &str, out: &mut HashMap<String, String>) {
    let pair = raw.trim();
    if pair.is_empty() {
        return;
    }
    let Some((k, vraw)) = pair.split_once('=') else {
        return;
    };
    let key = k.trim().to_string();
    let v = vraw.trim();
    let value = if let Some(inner) = v.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
        inner.to_string()
    } else {
        v.to_string()
    };
    out.insert(key, value);
}

fn labels_match(labels: &HashMap<String, String>, required: &[(&str, &str)]) -> bool {
    required
        .iter()
        .all(|(k, v)| labels.get(*k).map(|lv| lv == *v).unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sum_counter_respects_node_id() {
        let text = r#"
# HELP x x
plexspaces_node_messages_routed_total{node_id="a"} 3
plexspaces_node_messages_routed_total{node_id="b"} 7
"#;
        assert_eq!(
            sum_counter_for_labels(
                text,
                "plexspaces_node_messages_routed_total",
                &[("node_id", "a")]
            ),
            3
        );
        assert_eq!(
            sum_counter_for_labels(
                text,
                "plexspaces_node_messages_routed_total",
                &[("node_id", "b")]
            ),
            7
        );
    }

    #[test]
    fn sum_counter_all_series() {
        let text = r#"plexspaces_node_shard_operations_total{node_id="n1"} 2
plexspaces_node_shard_operations_total{node_id="n2"} 5"#;
        assert_eq!(
            sum_counter_all_label_sets(text, "plexspaces_node_shard_operations_total"),
            7
        );
    }

    #[test]
    fn actor_metrics_from_exposition_respects_namespace_and_node() {
        let text = r#"
plexspaces_actor_spawn_total{namespace="app1"} 2
plexspaces_messages_routed_total{namespace="app1"} 10
plexspaces_local_deliveries_total{namespace="app1"} 7
plexspaces_remote_deliveries_total{namespace="app1"} 3
plexspaces_messages_failed_total{namespace="app1",error_type="x"} 1
plexspaces_node_shard_operations_total{node_id="n1"} 4
"#;
        let m = actor_metrics_from_exposition_for_namespace(text, "app1", "n1", true);
        assert_eq!(m.spawn_total, 2);
        assert_eq!(m.active, 1);
        assert_eq!(m.messages_routed, 10);
        assert_eq!(m.local_deliveries, 7);
        assert_eq!(m.remote_deliveries, 3);
        assert_eq!(m.shard_operations_total, 4);
    }

    #[test]
    fn overlay_updates_node_metrics() {
        let text = r#"plexspaces_node_messages_routed_total{node_id="nid"} 10
plexspaces_node_local_deliveries_total{node_id="nid"} 4
plexspaces_node_remote_deliveries_total{node_id="nid"} 5
plexspaces_node_failed_deliveries_total{node_id="nid"} 1
plexspaces_node_shard_groups_created_total{node_id="nid"} 2
plexspaces_node_shard_messages_sent_total{node_id="nid"} 8
plexspaces_node_shard_messages_received_total{node_id="nid"} 9
plexspaces_node_shard_operations_total{node_id="nid"} 11
plexspaces_node_shard_operations_failed_total{node_id="nid"} 1"#;
        let mut m = NodeMetrics {
            memory_used_bytes: 0,
            memory_available_bytes: 0,
            cpu_usage_percent: 0.0,
            uptime_seconds: 0,
            messages_routed: 0,
            local_deliveries: 0,
            remote_deliveries: 0,
            failed_deliveries: 0,
            active_actors: 0,
            connected_nodes: 0,
            shard_groups_created: 0,
            shard_messages_sent: 0,
            shard_messages_received: 0,
            shard_operations_total: 0,
            shard_operations_failed: 0,
            node_id: "nid".into(),
            cluster_name: String::new(),
        };
        overlay_node_operational_counters_from_exposition(text, "nid", &mut m);
        assert_eq!(m.messages_routed, 10);
        assert_eq!(m.local_deliveries, 4);
        assert_eq!(m.remote_deliveries, 5);
        assert_eq!(m.failed_deliveries, 1);
        assert_eq!(m.shard_groups_created, 2);
        assert_eq!(m.shard_messages_sent, 8);
        assert_eq!(m.shard_messages_received, 9);
        assert_eq!(m.shard_operations_total, 11);
        assert_eq!(m.shard_operations_failed, 1);
    }

    #[test]
    fn skips_comments_and_histogram_buckets() {
        let text = r#"# TYPE x counter
plexspaces_node_messages_routed_total{node_id="n"} 1
plexspaces_node_messages_routed_total_bucket{node_id="n",le="1"} 0
"#;
        assert_eq!(
            sum_counter_for_labels(
                text,
                "plexspaces_node_messages_routed_total",
                &[("node_id", "n")]
            ),
            1
        );
    }

    #[test]
    fn sums_histogram_sum_samples_without_truncating_sub_millisecond_values() {
        let text = r#"
plexspaces_actor_message_processing_duration_seconds_sum{actor_id="a@app"} 0.0004
plexspaces_actor_message_processing_duration_seconds_sum{actor_id="a@app"} 0.0006
"#;
        let total = sum_sample_values_for_labels(
            text,
            "plexspaces_actor_message_processing_duration_seconds_sum",
            &[("actor_id", "a@app")],
        );
        assert!((total - 0.001).abs() < f64::EPSILON);
    }

    #[test]
    fn finds_max_finite_histogram_bucket_upper_bound() {
        let text = r#"
plexspaces_actor_message_processing_duration_seconds_bucket{actor_id="a@app",le="0.001"} 2
plexspaces_actor_message_processing_duration_seconds_bucket{actor_id="a@app",le="0.01"} 4
plexspaces_actor_message_processing_duration_seconds_bucket{actor_id="a@app",le="+Inf"} 4
"#;
        let max_bucket = max_histogram_bucket_upper_bound_for_labels(
            text,
            "plexspaces_actor_message_processing_duration_seconds",
            &[("actor_id", "a@app")],
        );
        assert_eq!(max_bucket, Some(0.01));
    }

    #[test]
    fn local_recorder_chart_summary_aggregates_histograms_and_supervisor_gauges() {
        let text = r#"
plexspaces_message_routing_duration_seconds_sum{namespace="app"} 0.06
plexspaces_message_routing_duration_seconds_count{namespace="app"} 3
plexspaces_message_routing_duration_seconds_bucket{namespace="app",le="0.05"} 2
plexspaces_message_routing_duration_seconds_bucket{namespace="app",le="0.1"} 3
plexspaces_actor_message_processing_duration_seconds_sum{actor_id="a",message_type="m"} 0.02
plexspaces_actor_message_processing_duration_seconds_count{actor_id="a",message_type="m"} 4
plexspaces_actor_message_processing_duration_seconds_bucket{actor_id="a",message_type="m",le="0.01"} 4
plexspaces_application_tracked_supervisors{application="app"} 2
plexspaces_application_tracked_supervisors{application="b"} 1
"#;
        let s = local_prometheus_recorder_chart_summary(text);
        assert!((s.message_routing_latency_avg_ms - 20.0).abs() < 0.001);
        assert!((s.message_routing_latency_max_ms - 100.0).abs() < 0.001);
        assert!((s.actor_message_processing_latency_avg_ms - 5.0).abs() < 0.001);
        assert!((s.actor_message_processing_latency_max_ms - 10.0).abs() < 0.001);
        assert_eq!(s.application_supervisors_total, 3);
    }
}
