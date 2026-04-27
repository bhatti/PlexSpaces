// SPDX-License-Identifier: AGPL-3.0-or-later

//! Errors for outbound HTTP execution.

use thiserror::Error;

/// Failures from resilient outbound HTTP calls.
#[derive(Debug, Error)]
pub enum OutboundHttpClientError {
    /// No service link with this name in the runtime catalog.
    #[error("unknown service link: {0}")]
    UnknownLink(String),

    /// Link exists but transport is not HTTP (or unspecified).
    #[error("service link {0} does not support HTTP transport")]
    UnsupportedTransport(String),

    /// Circuit breaker is open for this link.
    #[error("circuit open for link {link}: {detail}")]
    CircuitOpen {
        /// Logical link name.
        link: String,
        /// Reason or breaker name.
        detail: String,
    },

    /// Invalid base URL or joined URL.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// reqwest or I/O failure after retries exhausted.
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),

    /// Response exceeded configured size cap (if any).
    #[error("response body too large")]
    BodyTooLarge,
}
