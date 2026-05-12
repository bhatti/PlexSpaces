// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Outbound HTTP client trait (implementation in `plexspaces-http-client`).

use async_trait::async_trait;
use thiserror::Error;

pub use plexspaces_proto::common::v1::{HttpHeader, OutboundHttpRequest, OutboundHttpResponse};

/// Errors surfaced by [`OutboundHttpClient`].
#[derive(Debug, Error)]
pub enum OutboundHttpClientError {
    #[error("unknown service link: {0}")]
    UnknownLink(String),

    #[error("circuit open for link {link}: {detail}")]
    CircuitOpen { link: String, detail: String },

    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    #[error("HTTP request failed: {0}")]
    RequestFailed(String),

    #[error("response body too large")]
    BodyTooLarge,
}

impl OutboundHttpClientError {
    /// Returns the proto error code for this error variant.
    pub fn code(&self) -> plexspaces_proto::common::v1::OutboundHttpClientErrorCode {
        use plexspaces_proto::common::v1::OutboundHttpClientErrorCode;
        match self {
            OutboundHttpClientError::UnknownLink(_) => {
                OutboundHttpClientErrorCode::OutboundHttpErrorUnknownLink
            }
            OutboundHttpClientError::CircuitOpen { .. } => {
                OutboundHttpClientErrorCode::OutboundHttpErrorCircuitOpen
            }
            OutboundHttpClientError::InvalidUrl(_) => {
                OutboundHttpClientErrorCode::OutboundHttpErrorInvalidUrl
            }
            OutboundHttpClientError::RequestFailed(_) => {
                OutboundHttpClientErrorCode::OutboundHttpErrorRequestFailed
            }
            OutboundHttpClientError::BodyTooLarge => {
                OutboundHttpClientErrorCode::OutboundHttpErrorBodyTooLarge
            }
        }
    }
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
