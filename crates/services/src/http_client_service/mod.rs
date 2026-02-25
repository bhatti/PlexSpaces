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

//! HTTP Client Service Implementation
//!
//! Provides outbound HTTP request capabilities for WASM actors.
//! Includes security controls: URL allowlist/denylist, rate limiting,
//! response size limits.

use async_trait::async_trait;
use plexspaces_core::{
    HttpClientError, HttpClientService, HttpMethod, HttpRequest, HttpResponse, RequestContext,
};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::RwLock;

/// Configuration for the HTTP client service
#[derive(Debug, Clone)]
pub struct HttpClientConfig {
    /// Default timeout in milliseconds (used when request timeout is 0)
    pub default_timeout_ms: u64,
    /// Maximum response body size in bytes (default: 10MB)
    pub max_response_size: usize,
    /// Allowed URL patterns (regex strings). Empty = allow all (except denied).
    pub allowed_url_patterns: Vec<String>,
    /// Denied URL patterns (regex strings). Checked before allowed.
    /// Default: deny localhost, 127.0.0.1, 169.254.x.x (link-local), 10.x.x.x, 172.16-31.x.x, 192.168.x.x
    pub denied_url_patterns: Vec<String>,
    /// Maximum requests per second per actor (0 = unlimited)
    pub max_requests_per_second: u32,
}

impl Default for HttpClientConfig {
    fn default() -> Self {
        Self {
            default_timeout_ms: 30_000,
            max_response_size: 10 * 1024 * 1024, // 10 MB
            allowed_url_patterns: Vec::new(),
            denied_url_patterns: vec![
                r"^https?://localhost".to_string(),
                r"^https?://127\.".to_string(),
                r"^https?://\[::1\]".to_string(),
                r"^https?://169\.254\.".to_string(),
                r"^https?://10\.".to_string(),
                r"^https?://172\.(1[6-9]|2[0-9]|3[01])\.".to_string(),
                r"^https?://192\.168\.".to_string(),
            ],
            max_requests_per_second: 100,
        }
    }
}

/// HTTP Client Service implementation
///
/// Provides outbound HTTP capabilities for WASM actors with security controls:
/// - URL allowlist/denylist (prevent SSRF)
/// - Per-actor rate limiting
/// - Response size limits
/// - Configurable timeouts
pub struct HttpClientServiceImpl {
    client: reqwest::Client,
    config: HttpClientConfig,
    denied_patterns: Vec<regex::Regex>,
    allowed_patterns: Vec<regex::Regex>,
    /// Per-actor request count tracking for rate limiting (actor_id -> (count, epoch_second))
    rate_limits: Arc<RwLock<HashMap<String, (AtomicU64, u64)>>>,
}

impl HttpClientServiceImpl {
    /// Create a new HttpClientServiceImpl with the given configuration
    pub fn new(config: HttpClientConfig) -> Self {
        let denied_patterns: Vec<regex::Regex> = config
            .denied_url_patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect();

        let allowed_patterns: Vec<regex::Regex> = config
            .allowed_url_patterns
            .iter()
            .filter_map(|p| regex::Regex::new(p).ok())
            .collect();

        let client = reqwest::Client::builder()
            .pool_max_idle_per_host(10)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            client,
            config,
            denied_patterns,
            allowed_patterns,
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(HttpClientConfig::default())
    }

    /// Check if a URL is allowed by the security policy
    fn is_url_allowed(&self, url: &str) -> Result<(), HttpClientError> {
        // Check denied patterns first
        for pattern in &self.denied_patterns {
            if pattern.is_match(url) {
                return Err(HttpClientError::UrlNotAllowed(format!(
                    "URL matches denied pattern: {}",
                    pattern.as_str()
                )));
            }
        }

        // If allowed patterns are specified, URL must match at least one
        if !self.allowed_patterns.is_empty() {
            let allowed = self.allowed_patterns.iter().any(|p| p.is_match(url));
            if !allowed {
                return Err(HttpClientError::UrlNotAllowed(
                    "URL does not match any allowed pattern".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Check and update rate limit for an actor
    async fn check_rate_limit(&self, actor_id: &str) -> Result<(), HttpClientError> {
        if self.config.max_requests_per_second == 0 {
            return Ok(());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut limits = self.rate_limits.write().await;
        let entry = limits
            .entry(actor_id.to_string())
            .or_insert_with(|| (AtomicU64::new(0), now));

        // Reset counter if we're in a new second
        if entry.1 != now {
            entry.0.store(0, Ordering::Relaxed);
            entry.1 = now;
        }

        let count = entry.0.fetch_add(1, Ordering::Relaxed);
        if count >= self.config.max_requests_per_second as u64 {
            return Err(HttpClientError::RateLimitExceeded(actor_id.to_string()));
        }

        Ok(())
    }

    /// Convert HttpMethod to reqwest::Method
    fn to_reqwest_method(method: HttpMethod) -> reqwest::Method {
        match method {
            HttpMethod::Get => reqwest::Method::GET,
            HttpMethod::Post => reqwest::Method::POST,
            HttpMethod::Put => reqwest::Method::PUT,
            HttpMethod::Delete => reqwest::Method::DELETE,
            HttpMethod::Patch => reqwest::Method::PATCH,
            HttpMethod::Head => reqwest::Method::HEAD,
            HttpMethod::Options => reqwest::Method::OPTIONS,
        }
    }
}

#[async_trait]
impl HttpClientService for HttpClientServiceImpl {
    async fn request(
        &self,
        _ctx: &RequestContext,
        actor_id: &str,
        request: HttpRequest,
    ) -> Result<HttpResponse, HttpClientError> {
        // Check URL security policy
        self.is_url_allowed(&request.url)?;

        // Check rate limit
        self.check_rate_limit(actor_id).await?;

        // Determine timeout
        let timeout_ms = if request.timeout_ms > 0 {
            request.timeout_ms
        } else {
            self.config.default_timeout_ms
        };

        // Build reqwest request
        let method = Self::to_reqwest_method(request.method);
        let mut req_builder = self
            .client
            .request(method, &request.url)
            .timeout(std::time::Duration::from_millis(timeout_ms));

        // Add headers
        for (name, value) in &request.headers {
            req_builder = req_builder.header(name.as_str(), value.as_str());
        }

        // Add body if present
        if let Some(body) = request.body {
            req_builder = req_builder.body(body);
        }

        // Send request
        let response = req_builder.send().await.map_err(|e| {
            if e.is_timeout() {
                HttpClientError::Timeout(timeout_ms)
            } else {
                HttpClientError::NetworkError(e.to_string())
            }
        })?;

        // Extract status and headers
        let status = response.status().as_u16();
        let headers: HashMap<String, String> = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|v| (name.to_string(), v.to_string()))
            })
            .collect();

        // Check content-length before reading body
        if let Some(content_length) = response.content_length() {
            if content_length as usize > self.config.max_response_size {
                return Err(HttpClientError::ResponseTooLarge(
                    self.config.max_response_size,
                ));
            }
        }

        // Read body with size limit
        let body = response.bytes().await.map_err(|e| {
            HttpClientError::NetworkError(format!("Failed to read response body: {}", e))
        })?;

        if body.len() > self.config.max_response_size {
            return Err(HttpClientError::ResponseTooLarge(
                self.config.max_response_size,
            ));
        }

        Ok(HttpResponse {
            status,
            headers,
            body: body.to_vec(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_security_denies_localhost() {
        let service = HttpClientServiceImpl::with_defaults();
        assert!(service.is_url_allowed("http://localhost:8080/api").is_err());
        assert!(service.is_url_allowed("https://localhost/api").is_err());
        assert!(service.is_url_allowed("http://127.0.0.1:8080/api").is_err());
        assert!(service.is_url_allowed("http://[::1]:8080/api").is_err());
    }

    #[test]
    fn test_url_security_denies_private_networks() {
        let service = HttpClientServiceImpl::with_defaults();
        assert!(service.is_url_allowed("http://10.0.0.1/api").is_err());
        assert!(service.is_url_allowed("http://172.16.0.1/api").is_err());
        assert!(service.is_url_allowed("http://192.168.1.1/api").is_err());
        assert!(service.is_url_allowed("http://169.254.169.254/latest/meta-data").is_err());
    }

    #[test]
    fn test_url_security_allows_public_urls() {
        let service = HttpClientServiceImpl::with_defaults();
        assert!(service.is_url_allowed("https://api.example.com/v1/data").is_ok());
        assert!(service.is_url_allowed("https://httpbin.org/get").is_ok());
    }

    #[test]
    fn test_url_security_with_allowlist() {
        let config = HttpClientConfig {
            allowed_url_patterns: vec![r"^https://api\.example\.com/".to_string()],
            ..HttpClientConfig::default()
        };
        let service = HttpClientServiceImpl::new(config);
        assert!(service.is_url_allowed("https://api.example.com/v1/data").is_ok());
        assert!(service.is_url_allowed("https://other.com/api").is_err());
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let config = HttpClientConfig {
            max_requests_per_second: 2,
            ..HttpClientConfig::default()
        };
        let service = HttpClientServiceImpl::new(config);
        assert!(service.check_rate_limit("actor1").await.is_ok());
        assert!(service.check_rate_limit("actor1").await.is_ok());
        assert!(service.check_rate_limit("actor1").await.is_err());
        // Different actor should still be ok
        assert!(service.check_rate_limit("actor2").await.is_ok());
    }
}
