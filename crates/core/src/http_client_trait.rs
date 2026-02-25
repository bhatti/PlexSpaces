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

//! HTTP Client trait for outbound HTTP requests from WASM actors
//!
//! ## Purpose
//! Defines the interface for making outbound HTTP requests.
//! WASM actors use this via the `http-client` WIT interface.
//! The host-side implementation provides security controls
//! (URL allowlist/denylist, rate limiting, response size limits).
//!
//! ## Security
//! - URL allowlist/denylist to prevent SSRF attacks
//! - Response size limits to prevent OOM
//! - Per-actor rate limiting
//! - No localhost/internal network access by default

use async_trait::async_trait;
use std::collections::HashMap;

use crate::RequestContext;

/// HTTP method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HttpMethod {
    /// HTTP GET
    Get,
    /// HTTP POST
    Post,
    /// HTTP PUT
    Put,
    /// HTTP DELETE
    Delete,
    /// HTTP PATCH
    Patch,
    /// HTTP HEAD
    Head,
    /// HTTP OPTIONS
    Options,
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HttpMethod::Get => write!(f, "GET"),
            HttpMethod::Post => write!(f, "POST"),
            HttpMethod::Put => write!(f, "PUT"),
            HttpMethod::Delete => write!(f, "DELETE"),
            HttpMethod::Patch => write!(f, "PATCH"),
            HttpMethod::Head => write!(f, "HEAD"),
            HttpMethod::Options => write!(f, "OPTIONS"),
        }
    }
}

/// HTTP request
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// HTTP method
    pub method: HttpMethod,
    /// Request URL (must be absolute)
    pub url: String,
    /// Request headers
    pub headers: HashMap<String, String>,
    /// Request body (optional)
    pub body: Option<Vec<u8>>,
    /// Request timeout in milliseconds (0 = use default)
    pub timeout_ms: u64,
}

/// HTTP response
#[derive(Debug, Clone)]
pub struct HttpResponse {
    /// HTTP status code
    pub status: u16,
    /// Response headers
    pub headers: HashMap<String, String>,
    /// Response body
    pub body: Vec<u8>,
}

/// HTTP client errors
#[derive(Debug, thiserror::Error)]
pub enum HttpClientError {
    /// URL is not allowed by security policy
    #[error("URL not allowed: {0}")]
    UrlNotAllowed(String),

    /// Request timed out
    #[error("Request timed out after {0}ms")]
    Timeout(u64),

    /// Response body too large
    #[error("Response body exceeds maximum size of {0} bytes")]
    ResponseTooLarge(usize),

    /// Rate limit exceeded
    #[error("Rate limit exceeded for actor {0}")]
    RateLimitExceeded(String),

    /// Network error
    #[error("Network error: {0}")]
    NetworkError(String),

    /// Invalid request
    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    /// Other error
    #[error("HTTP client error: {0}")]
    Other(String),
}

/// Trait for making outbound HTTP requests.
///
/// ## Purpose
/// Allows WASM actors to make outbound HTTP requests through a host-provided
/// capability. The host implementation provides security controls.
///
/// ## Security Model
/// - URL allowlist/denylist checked before every request
/// - Per-actor rate limiting
/// - Response size limits
/// - No localhost access by default (configurable)
#[async_trait]
pub trait HttpClientService: Send + Sync {
    /// Send an HTTP request
    ///
    /// ## Arguments
    /// * `ctx` - Request context with tenant/namespace
    /// * `actor_id` - ID of the actor making the request (for rate limiting)
    /// * `request` - The HTTP request to send
    ///
    /// ## Returns
    /// HTTP response or error
    async fn request(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        request: HttpRequest,
    ) -> Result<HttpResponse, HttpClientError>;

    /// Convenience: Send a GET request
    async fn get(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        url: &str,
        headers: HashMap<String, String>,
        timeout_ms: u64,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(
            ctx,
            actor_id,
            HttpRequest {
                method: HttpMethod::Get,
                url: url.to_string(),
                headers,
                body: None,
                timeout_ms,
            },
        )
        .await
    }

    /// Convenience: Send a POST request
    async fn post(
        &self,
        ctx: &RequestContext,
        actor_id: &str,
        url: &str,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        timeout_ms: u64,
    ) -> Result<HttpResponse, HttpClientError> {
        self.request(
            ctx,
            actor_id,
            HttpRequest {
                method: HttpMethod::Post,
                url: url.to_string(),
                headers,
                body: Some(body),
                timeout_ms,
            },
        )
        .await
    }
}
