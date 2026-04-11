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

//! Monitoring and Observability Helpers
//!
//! ## Purpose
//! Centralized helper functions for metrics, tracing, and observability.
//! This module provides reusable functions for recording metrics consistently
//! across the codebase, reducing duplication and ensuring consistent naming.

use async_trait::async_trait;
use std::time::Duration;

/// Record node-scoped message routing totals (Prometheus counters, hot path).
#[inline]
pub fn record_node_messages_routed(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_messages_routed_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Record a successful local delivery on this node.
#[inline]
pub fn record_node_local_delivery(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_local_deliveries_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Record a successful remote delivery initiated from this node.
#[inline]
pub fn record_node_remote_delivery(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_remote_deliveries_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Record a failed delivery on this node.
#[inline]
pub fn record_node_failed_delivery(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_failed_deliveries_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Shard group created on this node.
#[inline]
pub fn record_node_shard_groups_created(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_shard_groups_created_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Message sent to a shard actor from this node.
#[inline]
pub fn record_node_shard_messages_sent(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_shard_messages_sent_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Message received from shard processing on this node.
#[inline]
pub fn record_node_shard_messages_received(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_shard_messages_received_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Shard collective operation on this node.
#[inline]
pub fn record_node_shard_operation(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_shard_operations_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Failed shard collective operation on this node.
#[inline]
pub fn record_node_shard_operation_failed(node_id: &str) {
    metrics::counter!(
        "plexspaces_node_shard_operations_failed_total",
        "node_id" => node_id.to_string()
    )
    .increment(1);
}

/// Trait for accessing node connection information
///
/// This allows components to access node connection information (connected nodes list)
/// without directly depending on the Node type.
#[async_trait]
pub trait NodeConnectionInfo: Send + Sync {
    /// Get list of connected node IDs (as strings)
    async fn connected_nodes(&self) -> Vec<String>;
}

/// Record message routing R.E.D. metrics (unified Prometheus pipeline, namespace dimension).
///
/// Must stay aligned with `plexspaces_services::metrics_service::record_message_routing_red`
/// (core cannot depend on services, so names and labels are duplicated intentionally).
#[allow(unused_variables)]
pub fn record_message_routing_metrics(
    actor_id: &str,
    namespace: &str,
    duration: Duration,
    success: bool,
    error_type: Option<&str>,
) {
    let ns = if namespace.is_empty() {
        "default".to_string()
    } else {
        namespace.to_string()
    };
    let dur = duration.as_secs_f64();
    metrics::counter!("plexspaces_messages_routed_total", "namespace" => ns.clone()).increment(1);
    if success {
        metrics::histogram!(
            "plexspaces_message_routing_duration_seconds",
            "namespace" => ns.clone(),
        )
        .record(dur);
    } else {
        let et = error_type.unwrap_or("unknown").to_string();
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
        .record(dur);
    }

    #[cfg(feature = "tracing")]
    {
        if success {
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    actor_id = %actor_id,
                    duration_ms = duration.as_millis(),
                    "Message routed successfully"
                );
            }
        } else {
            tracing::error!(
                actor_id = %actor_id,
                duration_ms = duration.as_millis(),
                error_type = error_type.unwrap_or("unknown"),
                "Message routing failed"
            );
        }
    }
}

/// Record actor activation R.E.D. metrics (aligned with MetricsService `RecordActorActivation`).
#[allow(unused_variables)]
pub fn record_actor_activation_metrics(
    actor_id: &str,
    namespace: &str,
    activation_type: &str,
    duration: Duration,
    success: bool,
) {
    let ns = if namespace.is_empty() {
        "default".to_string()
    } else {
        namespace.to_string()
    };
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
    .record(duration.as_secs_f64());

    #[cfg(feature = "tracing")]
    if tracing::enabled!(tracing::Level::DEBUG) {
        tracing::debug!(
            actor_id = %actor_id,
            activation_type = %activation_type,
            duration_ms = duration.as_millis(),
            success = success,
            "Actor activation"
        );
    }
}

/// Record connection metrics
///
///
/// ## Arguments
/// * `node_id` - Node ID
/// * `remote_node_id` - Remote node ID
/// * `event_type` - Event type ("connected", "disconnected", "error")
/// * `duration` - Connection duration (if applicable)
pub fn record_connection_metrics(
    node_id: &str,
    remote_node_id: &str,
    event_type: &str,
    duration: Option<Duration>,
) {
    metrics::counter!(
        "plexspaces_connections_total",
        "node_id" => node_id.to_string(),
        "remote_node_id" => remote_node_id.to_string(),
        "event_type" => event_type.to_string(),
    )
    .increment(1);

    if let Some(dur) = duration {
        metrics::histogram!(
            "plexspaces_connection_duration_seconds",
            "node_id" => node_id.to_string(),
            "remote_node_id" => remote_node_id.to_string(),
        )
        .record(dur.as_secs_f64());
    }

    #[cfg(feature = "tracing")]
    tracing::info!(
        node_id = %node_id,
        remote_node_id = %remote_node_id,
        event_type = %event_type,
        "Connection event"
    );
}
