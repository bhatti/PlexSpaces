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
        local_deliveries: 0,
        remote_deliveries: 0,
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
    m.messages_routed = sum_counter_for_labels(exposition, "plexspaces_node_messages_routed_total", &req);
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
    m.shard_operations_total = sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_operations_total",
        &req,
    );
    m.shard_operations_failed = sum_counter_for_labels(
        exposition,
        "plexspaces_node_shard_operations_failed_total",
        &req,
    );
}

fn parse_counter_sample_line(line: &str) -> Option<(&str, &str, u64)> {
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
    Some((name, labels, v as u64))
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
            sum_counter_for_labels(text, "plexspaces_node_messages_routed_total", &[("node_id", "a")]),
            3
        );
        assert_eq!(
            sum_counter_for_labels(text, "plexspaces_node_messages_routed_total", &[("node_id", "b")]),
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
plexspaces_messages_failed_total{namespace="app1",error_type="x"} 1
plexspaces_node_shard_operations_total{node_id="n1"} 4
"#;
        let m = actor_metrics_from_exposition_for_namespace(text, "app1", "n1", true);
        assert_eq!(m.spawn_total, 2);
        assert_eq!(m.active, 1);
        assert_eq!(m.messages_routed, 10);
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
            sum_counter_for_labels(text, "plexspaces_node_messages_routed_total", &[("node_id", "n")]),
            1
        );
    }
}
