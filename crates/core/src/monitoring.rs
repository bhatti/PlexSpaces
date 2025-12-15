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

use std::sync::Arc;
use std::time::Duration;
use async_trait::async_trait;
use crate::{Service, ActorMetricsHandle};

/// Trait for updating NodeMetrics from message routing
///
/// This allows the monitoring helper to update NodeMetrics without
/// directly depending on the Node type.
#[async_trait]
pub trait NodeMetricsUpdater: Send + Sync {
    /// Increment messages_routed counter
    async fn increment_messages_routed(&self);
    
    /// Increment local_deliveries counter
    async fn increment_local_deliveries(&self);
    
    /// Increment remote_deliveries counter
    async fn increment_remote_deliveries(&self);
    
    /// Increment failed_deliveries counter
    async fn increment_failed_deliveries(&self);
}

/// Record message routing metrics
///
///
/// ## Arguments
/// * `actor_id` - Target actor ID
/// * `node_id` - Source node ID
/// * `is_local` - Whether the message is being routed locally
/// * `remote_node_id` - Remote node ID (if routing remotely)
/// * `duration` - Time taken for routing
/// * `success` - Whether routing succeeded
/// * `error_type` - Error type if routing failed
/// * `metrics_updater` - Optional NodeMetricsUpdater to update NodeMetrics (for backward compatibility)
/// * `actor_metrics` - Optional ActorMetricsHandle to update ActorMetrics (preferred)
pub async fn record_message_routing_metrics(
    actor_id: &str,
    node_id: &str,
    is_local: bool,
    remote_node_id: Option<&str>,
    duration: Duration,
    success: bool,
    error_type: Option<&str>,
    metrics_updater: Option<Arc<dyn NodeMetricsUpdater + Send + Sync>>,
    actor_metrics: Option<ActorMetricsHandle>,
) {
    let location = if is_local { "local" } else { "remote" };
    
    // Update ActorMetrics if available (preferred - ActorRegistry tracks metrics directly)
    if let Some(ref actor_metrics_handle) = actor_metrics {
        use crate::message_metrics::ActorMetricsExt;
        let mut metrics = actor_metrics_handle.write().await;
        metrics.increment_messages_routed();
        if success {
            if is_local {
                metrics.increment_local_deliveries();
            } else {
                metrics.increment_remote_deliveries();
            }
        } else {
            metrics.increment_failed_deliveries();
            metrics.increment_error_total();
        }
    }
    
    // Update NodeMetrics if updater is available (for backward compatibility)
    if let Some(ref updater) = metrics_updater {
        updater.increment_messages_routed().await;
        if success {
            if is_local {
                updater.increment_local_deliveries().await;
            } else {
                updater.increment_remote_deliveries().await;
            }
        } else {
            updater.increment_failed_deliveries().await;
        }
    }
    
    // Counter: Total messages routed
    metrics::counter!(
        "plexspaces_messages_routed_total",
        "node_id" => node_id.to_string(),
        "location" => location.to_string(),
    )
    .increment(1);
    
    // Counter: Messages delivered (success)
    if success {
        metrics::counter!(
            "plexspaces_messages_delivered_total",
            "node_id" => node_id.to_string(),
            "location" => location.to_string(),
        )
        .increment(1);
    } else {
        // Counter: Messages failed
        metrics::counter!(
            "plexspaces_messages_failed_total",
            "node_id" => node_id.to_string(),
            "location" => location.to_string(),
            "error_type" => error_type.unwrap_or("unknown").to_string(),
        )
        .increment(1);
    }
    
    // Histogram: Routing duration
    let mut labels = vec![
        ("node_id", node_id.to_string()),
        ("location", location.to_string()),
    ];
    
    if let Some(remote_node) = remote_node_id {
        labels.push(("remote_node_id", remote_node.to_string()));
    }
    
    // Convert labels to key-value pairs for metrics macro
    let mut metric_labels = Vec::new();
    for (key, value) in labels {
        metric_labels.push((key, value));
    }
    
    // Record histogram with dynamic labels
    // Note: metrics crate doesn't support dynamic labels easily, so we use a simpler approach
    if is_local {
        metrics::histogram!(
            "plexspaces_message_routing_duration_seconds",
            "node_id" => node_id.to_string(),
            "location" => "local".to_string(),
        )
        .record(duration.as_secs_f64());
    } else if let Some(remote_node) = remote_node_id {
        metrics::histogram!(
            "plexspaces_message_routing_duration_seconds",
            "node_id" => node_id.to_string(),
            "location" => "remote".to_string(),
            "remote_node_id" => remote_node.to_string(),
        )
        .record(duration.as_secs_f64());
    }
    
    // Tracing (if tracing feature is enabled)
    #[cfg(feature = "tracing")]
    {
        if success {
            tracing::debug!(
                actor_id = %actor_id,
                node_id = %node_id,
                location = %location,
                duration_ms = duration.as_millis(),
                "Message routed successfully"
            );
        } else {
            tracing::error!(
                actor_id = %actor_id,
                node_id = %node_id,
                location = %location,
                duration_ms = duration.as_millis(),
                error_type = error_type.unwrap_or("unknown"),
                "Message routing failed"
            );
        }
    }
}

/// Record actor activation metrics
///
///
/// ## Arguments
/// * `actor_id` - Actor ID
/// * `node_id` - Node ID
/// * `activation_type` - Type of activation (e.g., "lazy", "eager", "virtual")
/// * `duration` - Time taken for activation
/// * `success` - Whether activation succeeded
pub fn record_actor_activation_metrics(
    actor_id: &str,
    node_id: &str,
    activation_type: &str,
    duration: Duration,
    success: bool,
) {
    metrics::counter!(
        "plexspaces_actor_activations_total",
        "node_id" => node_id.to_string(),
        "activation_type" => activation_type.to_string(),
        "status" => if success { "success" } else { "failed" }.to_string(),
    )
    .increment(1);
    
    if success {
        metrics::histogram!(
            "plexspaces_actor_activation_duration_seconds",
            "node_id" => node_id.to_string(),
            "activation_type" => activation_type.to_string(),
        )
        .record(duration.as_secs_f64());
    }
    
    #[cfg(feature = "tracing")]
    tracing::debug!(
        actor_id = %actor_id,
        node_id = %node_id,
        activation_type = %activation_type,
        duration_ms = duration.as_millis(),
        success = success,
        "Actor activation"
    );
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
