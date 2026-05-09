// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! In-process access to the unified metrics pipeline.

use async_trait::async_trait;
use plexspaces_proto::metrics::v1::{Metric, MetricDefinition};
use std::collections::HashMap;

/// Local metrics API mirroring read paths of gRPC `MetricsService`.
///
/// Implemented by `plexspaces_services::metrics_service::MetricsServiceImpl`.
#[async_trait]
pub trait MetricsServiceAccess: Send + Sync {
    /// Full Prometheus text exposition for this process.
    async fn export_prometheus_text(&self) -> String;

    /// Structured metric samples parsed from exposition (counters/gauges; histogram buckets omitted).
    async fn get_metrics_filtered(
        &self,
        name_pattern: String,
        label_filter: HashMap<String, String>,
    ) -> Vec<Metric>;

    /// Metric definitions (metadata) optionally filtered by name glob.
    async fn list_metric_definitions_filtered(
        &self,
        name_pattern: String,
    ) -> Vec<MetricDefinition>;
}
