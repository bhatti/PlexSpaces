// SPDX-License-Identifier: AGPL-3.0-or-later

//! Outbound HTTP client trait (implementation in `plexspaces-http-client`).

use async_trait::async_trait;
use thiserror::Error;

/// HTTP request to an external service (path relative to link base URL).
#[derive(Debug, Clone)]
pub struct OutboundHttpRequest {
    /// Method (GET, POST, ...).
    pub method: String,
    /// Path and optional query, e.g. `/v1/items` or `/v1/items?a=1`.
    pub path_and_query: String,
    /// Extra headers (merged with link default and auth headers).
    pub headers: Vec<(String, String)>,
    /// Request body.
    pub body: Vec<u8>,
}

/// HTTP response from an outbound call.
#[derive(Debug, Clone)]
pub struct OutboundHttpResponse {
    /// HTTP status code.
    pub status: u16,
    /// Response headers (string pairs).
    pub headers: Vec<(String, String)>,
    /// Response body.
    pub body: Vec<u8>,
}

/// Errors surfaced by [`OutboundHttpClient`].
#[derive(Debug, Error)]
pub enum OutboundHttpClientError {
    /// Unknown service link name.
    #[error("unknown service link: {0}")]
    UnknownLink(String),

    /// Circuit breaker open.
    #[error("circuit open for link {link}: {detail}")]
    CircuitOpen {
        /// Link name.
        link: String,
        /// Detail message.
        detail: String,
    },

    /// Bad URL.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// Network or HTTP layer failure.
    #[error("HTTP request failed: {0}")]
    RequestFailed(String),

    /// Response body over limit.
    #[error("response body too large")]
    BodyTooLarge,
}

/// Resilient outbound HTTP access by configured service link name.
#[async_trait]
pub trait OutboundHttpClient: Send + Sync {
    /// Perform one logical request (may retry internally).
    async fn execute(
        &self,
        link_name: &str,
        request: OutboundHttpRequest,
    ) -> Result<OutboundHttpResponse, OutboundHttpClientError>;
}
