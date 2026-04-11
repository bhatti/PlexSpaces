// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Abstraction for rendering the current process metrics as Prometheus text.
//!
//! Implementations live in higher layers (for example `plexspaces-services`) that own
//! `metrics-exporter-prometheus`. Core depends only on this trait so `NodeMetrics` overlays
//! can parse exposition without pulling the exporter into `plexspaces-core`.

/// Renders Prometheus text exposition for the in-process `metrics` recorder.
///
/// # Purpose
///
/// Provides a single integration point for dashboards and `Node::metrics()` to read
/// operational counters that are recorded on hot paths via the `metrics` crate.
pub trait MetricsPrometheusRenderer: Send + Sync {
    /// Returns a Prometheus text exposition snapshot (possibly empty).
    fn render_prometheus_text(&self) -> String;
}
