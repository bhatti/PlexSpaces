// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! In-process access to the unified metrics pipeline (same backend as gRPC [`MetricsService`]).
//!
//! Register [`MetricsServiceAccess`] on [`crate::ServiceLocator`] so node internals, dashboard,
//! and other services can query Prometheus text and structured samples without a network hop.

use std::collections::HashMap;

use async_trait::async_trait;
use plexspaces_proto::metrics::v1::{Metric, MetricDefinition};

/// Local metrics API mirroring read paths of gRPC `MetricsService`.
///
/// Implemented by [`plexspaces_services::metrics_service::MetricsServiceImpl`].
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
    async fn list_metric_definitions_filtered(&self, name_pattern: String)
        -> Vec<MetricDefinition>;
}
