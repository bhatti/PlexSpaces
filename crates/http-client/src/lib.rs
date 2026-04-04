// SPDX-License-Identifier: LGPL-2.1-or-later

//! Resilient outbound HTTP client for PlexSpaces [`RuntimeConfig::service_links`](plexspaces_proto::node::v1::RuntimeConfig).
//!
//! Features: connection/request timeouts, exponential backoff with full jitter, circuit breaker
//! integration (`plexspaces-circuit-breaker`), Prometheus-style metrics, and structured tracing.

#![warn(missing_docs)]

mod client;
mod error;
mod policy;
mod retry;

pub use client::ResilientOutboundHttpClient;
pub use error::OutboundHttpClientError;
pub use policy::{merge_client_transport_policy, validate_application_service_links};
