// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Actor metrics wire type (proto).
//!
//! Runtime values come from the process Prometheus recorder; see
//! [`crate::prometheus_text::actor_metrics_from_exposition_for_namespace`].

/// Re-export proto-generated `ActorMetrics` for dashboards and gRPC.
pub use plexspaces_proto::metrics::v1::ActorMetrics;
