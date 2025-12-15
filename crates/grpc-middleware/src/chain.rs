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

//! # InterceptorChain - Composable gRPC Middleware
//!
//! ## Purpose
//! Provides a composable middleware chain for gRPC services using proto-defined types.
//!
//! ## Proto-First Design
//! This module implements the Rust side of the proto-defined InterceptorService.
//! All request/response types, decisions, and metrics are defined in:
//! `proto/plexspaces/v1/grpc/middleware.proto`
//!
//! ## Architecture Context
//! - **PlexSpacesNode**: Loads middleware chain from release.toml configuration
//! - **ActorService**: All gRPC services use the middleware chain
//! - **Proto Types**: InterceptorRequest, InterceptorResponse, InterceptorResult

use async_trait::async_trait;
use plexspaces_proto::grpc::v1::{
    InterceptorDecision, InterceptorRequest, InterceptorResponse, InterceptorResult,
    MiddlewareConfig, MiddlewareType,
};
use std::sync::Arc;
use thiserror::Error;
use tonic::Status;

/// Errors that can occur during interceptor execution
#[derive(Debug, Error)]
pub enum InterceptorError {
    /// Request was rejected by interceptor
    #[error("Request rejected: {0}")]
    Rejected(String),

    /// Authentication failed
    #[error("Authentication failed: {0}")]
    AuthenticationFailed(String),

    /// Authorization failed
    #[error("Authorization failed: {0}")]
    AuthorizationFailed(String),

    /// Rate limit exceeded
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// Compression/decompression failed
    #[error("Compression error: {0}")]
    CompressionFailed(String),

    /// Tracing error
    #[error("Tracing error: {0}")]
    TracingFailed(String),

    /// Generic error
    #[error("Interceptor error: {0}")]
    Other(String),
}

impl From<InterceptorError> for Status {
    fn from(err: InterceptorError) -> Status {
        match err {
            InterceptorError::Rejected(msg) => Status::invalid_argument(msg),
            InterceptorError::AuthenticationFailed(msg) => Status::unauthenticated(msg),
            InterceptorError::AuthorizationFailed(msg) => Status::permission_denied(msg),
            InterceptorError::RateLimitExceeded(msg) => Status::resource_exhausted(msg),
            InterceptorError::CompressionFailed(msg) => Status::internal(msg),
            InterceptorError::TracingFailed(msg) => Status::internal(msg),
            InterceptorError::Other(msg) => Status::internal(msg),
        }
    }
}

/// Interceptor trait for gRPC middleware (proto-first design)
///
/// ## Purpose
/// Defines the interface for gRPC request/response interceptors using proto types.
///
/// ## Proto Integration
/// - Uses `InterceptorRequest` for request context (all metadata from proto)
/// - Uses `InterceptorResponse` for response context (all metadata from proto)
/// - Returns `InterceptorResult` with decision (ALLOW, DENY, MODIFY)
///
/// ## Design Note
/// **No generics!** Trait is dyn-compatible for use as `Arc<dyn Interceptor>`.
/// All request/response data passed via proto types instead of generic parameters.
///
/// ## Lifecycle
/// 1. `before_request()` - Called before handler (can reject request)
/// 2. Handler executes
/// 3. `after_response()` - Called after handler (can modify response)
/// 4. `on_error()` - Called if handler or interceptor fails
#[async_trait]
pub trait Interceptor: Send + Sync {
    /// Called before request reaches handler
    ///
    /// ## Arguments
    /// * `context` - Proto InterceptorRequest with ALL request metadata
    ///   (method, headers, remote_addr, timestamp, request_id)
    ///
    /// ## Returns
    /// `InterceptorResult` with decision (ALLOW, DENY, MODIFY)
    ///
    /// ## Design Note
    /// No generic `Request<T>` parameter - all data in proto context.
    /// This makes the trait dyn-compatible!
    async fn before_request(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError>;

    /// Called after handler produces response
    ///
    /// ## Arguments
    /// * `context` - Proto InterceptorResponse with ALL response metadata
    ///   (status_code, headers, timestamp, duration)
    ///
    /// ## Returns
    /// `InterceptorResult` with optional modifications
    ///
    /// ## Design Note
    /// No generic `Response<T>` parameter - all data in proto context.
    /// This makes the trait dyn-compatible!
    async fn after_response(
        &self,
        context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError>;

    /// Called when handler or another interceptor fails
    ///
    /// ## Arguments
    /// * `error` - The error that occurred
    async fn on_error(&self, error: &Status) {
        // Default: do nothing
        let _ = error;
    }

    /// Interceptor name for logging/debugging
    fn name(&self) -> &str;

    /// Priority for execution order (higher = earlier)
    ///
    /// ## Default Priorities
    /// - Metrics: 10 (always first)
    /// - Tracing: 20
    /// - Auth: 30
    /// - Rate Limit: 40
    /// - Compression: 50
    fn priority(&self) -> i32 {
        50
    }
}

/// Composable chain of interceptors
///
/// ## Purpose
/// Manages multiple interceptors and executes them in priority order.
/// Uses proto-defined types for all request/response handling.
///
/// ## Design
/// - Interceptors sorted by priority (highest first)
/// - Metrics interceptor always included
/// - Disabled interceptors skipped
/// - Errors in one interceptor stop the chain
pub struct InterceptorChain {
    /// Ordered list of interceptors (sorted by priority)
    interceptors: Vec<Arc<dyn Interceptor>>,
}

impl InterceptorChain {
    /// Create empty interceptor chain
    pub fn new() -> Self {
        Self {
            interceptors: Vec::new(),
        }
    }

    /// Add interceptor to chain
    pub fn add(&mut self, interceptor: Arc<dyn Interceptor>) {
        self.interceptors.push(interceptor);
    }

    /// Sort interceptors by priority (highest first)
    fn sort_by_priority(&mut self) {
        self.interceptors
            .sort_by_key(|b| std::cmp::Reverse(b.priority()));
    }

    /// Create interceptor chain from proto config
    ///
    /// ## Purpose
    /// Factory method to build InterceptorChain from MiddlewareConfig proto.
    /// Ensures metrics interceptor is always included.
    ///
    /// ## Arguments
    /// * `config` - Middleware configuration from proto
    ///
    /// ## Returns
    /// Configured interceptor chain with metrics always enabled
    pub fn from_config(config: &MiddlewareConfig) -> Result<Self, InterceptorError> {
        let mut chain = Self::new();

        // CRITICAL: Always add metrics interceptor first (cannot be disabled)
        chain.add(Arc::new(crate::metrics::MetricsInterceptor::new()));

        // Add other interceptors based on config
        for spec in &config.middleware {
            // Skip if disabled (except metrics which is always enabled)
            if !spec.enabled && spec.middleware_type != (MiddlewareType::MiddlewareTypeMetrics as i32) {
                continue;
            }

            // Create interceptor based on type
            let interceptor: Arc<dyn Interceptor> = match spec.middleware_type {
                x if x == MiddlewareType::MiddlewareTypeMetrics as i32 => {
                    // Already added above, skip
                    continue;
                }
                x if x == MiddlewareType::MiddlewareTypeAuth as i32 => {
                    // Extract auth config from spec
                    let auth_config = match &spec.config {
                        Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Auth(cfg)) => {
                            cfg.clone()
                        }
                        _ => {
                            // No config provided, use default (permissive)
                            plexspaces_proto::grpc::v1::AuthMiddlewareConfig::default()
                        }
                    };
                    Arc::new(crate::auth::AuthInterceptor::new(auth_config)?)
                }
                x if x == MiddlewareType::MiddlewareTypeRateLimit as i32 => {
                    // Extract rate limit config from spec
                    let rate_limit_config = match &spec.config {
                        Some(plexspaces_proto::grpc::v1::middleware_spec::Config::RateLimit(
                            cfg,
                        )) => cfg.clone(),
                        _ => plexspaces_proto::grpc::v1::RateLimitMiddlewareConfig::default(),
                    };
                    Arc::new(crate::rate_limit::RateLimitInterceptor::new(
                        rate_limit_config,
                    ))
                }
                x if x == MiddlewareType::MiddlewareTypeCompression as i32 => {
                    // Extract compression config from spec
                    let compression_config = match &spec.config {
                        Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Compression(
                            cfg,
                        )) => cfg.clone(),
                        _ => plexspaces_proto::grpc::v1::CompressionMiddlewareConfig::default(),
                    };
                    Arc::new(crate::compression::CompressionInterceptor::new(
                        compression_config,
                    ))
                }
                x if x == MiddlewareType::MiddlewareTypeTracing as i32 => {
                    // Extract tracing config from spec
                    let tracing_config = match &spec.config {
                        Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(cfg)) => {
                            cfg.clone()
                        }
                        _ => plexspaces_proto::grpc::v1::TracingMiddlewareConfig::default(),
                    };
                    Arc::new(crate::tracing_interceptor::TracingInterceptor::new(
                        tracing_config,
                    ))
                }
                x if x == MiddlewareType::MiddlewareTypeRetry as i32 => {
                    // Extract retry config from spec
                    let retry_config = match &spec.config {
                        Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Retry(cfg)) => {
                            cfg.clone()
                        }
                        _ => plexspaces_proto::grpc::v1::RetryMiddlewareConfig::default(),
                    };
                    Arc::new(crate::retry::RetryInterceptor::new(retry_config))
                }
                _ => {
                    // Unknown middleware type, skip
                    continue;
                }
            };

            chain.add(interceptor);
        }

        // Sort by priority
        chain.sort_by_priority();

        Ok(chain)
    }

    /// Execute all interceptors before request
    ///
    /// ## Purpose
    /// Runs all interceptors' before_request() in priority order.
    /// Stops on first DENY decision.
    ///
    /// ## Arguments
    /// * `context` - Proto InterceptorRequest with request metadata
    ///
    /// ## Returns
    /// `Ok(())` if all interceptors pass, error otherwise
    pub async fn before_request(&self, context: &InterceptorRequest) -> Result<(), Status> {
        // Execute interceptors in priority order
        for interceptor in &self.interceptors {
            let result = interceptor.before_request(context).await?;

            // Check decision
            match result.decision() {
                InterceptorDecision::InterceptorDecisionAllow => {
                    // Continue to next interceptor
                    continue;
                }
                InterceptorDecision::InterceptorDecisionDeny => {
                    // Reject request
                    return Err(Status::permission_denied(result.error_message));
                }
                InterceptorDecision::InterceptorDecisionModify => {
                    // TODO: Apply modified headers to request
                    continue;
                }
                InterceptorDecision::InterceptorDecisionUnspecified => {
                    // Treat as ALLOW
                    continue;
                }
            }
        }

        Ok(())
    }

    /// Execute all interceptors after response
    ///
    /// ## Purpose
    /// Runs all interceptors' after_response() in REVERSE priority order.
    ///
    /// ## Arguments
    /// * `context` - Proto InterceptorResponse with response metadata
    ///
    /// ## Returns
    /// `Ok(())` if all interceptors pass, error otherwise
    pub async fn after_response(&self, context: &InterceptorResponse) -> Result<(), Status> {
        // Execute interceptors in reverse order
        for interceptor in self.interceptors.iter().rev() {
            let _result = interceptor.after_response(context).await?;
            // TODO: Apply modified headers if decision=MODIFY
        }

        Ok(())
    }

    /// Notify all interceptors of error
    pub async fn on_error(&self, error: &Status) {
        for interceptor in &self.interceptors {
            interceptor.on_error(error).await;
        }
    }
}

impl Default for InterceptorChain {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    struct TestInterceptor {
        name: String,
        priority: i32,
    }

    impl TestInterceptor {
        fn new(name: &str, priority: i32) -> Self {
            Self {
                name: name.to_string(),
                priority,
            }
        }
    }

    #[async_trait]
    impl Interceptor for TestInterceptor {
        async fn before_request(
            &self,
            _context: &InterceptorRequest,
        ) -> Result<InterceptorResult, InterceptorError> {
            Ok(InterceptorResult {
                decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                error_message: String::new(),
                modified_headers: std::collections::HashMap::new(),
                metrics: vec![],
            })
        }

        async fn after_response(
            &self,
            _context: &InterceptorResponse,
        ) -> Result<InterceptorResult, InterceptorError> {
            Ok(InterceptorResult {
                decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                error_message: String::new(),
                modified_headers: std::collections::HashMap::new(),
                metrics: vec![],
            })
        }

        fn name(&self) -> &str {
            &self.name
        }

        fn priority(&self) -> i32 {
            self.priority
        }
    }

    #[test]
    fn test_interceptor_chain_creation() {
        let chain = InterceptorChain::new();
        assert_eq!(chain.interceptors.len(), 0);
    }

    #[test]
    fn test_add_interceptor() {
        let mut chain = InterceptorChain::new();
        chain.add(Arc::new(TestInterceptor::new("test", 50)));
        assert_eq!(chain.interceptors.len(), 1);
    }

    #[test]
    fn test_sort_by_priority() {
        let mut chain = InterceptorChain::new();
        chain.add(Arc::new(TestInterceptor::new("low", 10)));
        chain.add(Arc::new(TestInterceptor::new("high", 90)));
        chain.add(Arc::new(TestInterceptor::new("medium", 50)));

        chain.sort_by_priority();

        assert_eq!(chain.interceptors[0].name(), "high");
        assert_eq!(chain.interceptors[1].name(), "medium");
        assert_eq!(chain.interceptors[2].name(), "low");
    }

    #[tokio::test]
    async fn test_before_request_chain() {
        let mut chain = InterceptorChain::new();
        chain.add(Arc::new(TestInterceptor::new("test1", 50)));
        chain.add(Arc::new(TestInterceptor::new("test2", 60)));

        let context = InterceptorRequest {
            method: "/test.Service/Method".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = chain.before_request(&context).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_after_response_chain() {
        let mut chain = InterceptorChain::new();
        chain.add(Arc::new(TestInterceptor::new("test1", 50)));
        chain.add(Arc::new(TestInterceptor::new("test2", 60)));

        let context = InterceptorResponse {
            status_code: 0,
            headers: std::collections::HashMap::new(),
            timestamp: Some(plexspaces_proto::prost_types::Timestamp::from(
                std::time::SystemTime::now(),
            )),
            duration: None,
        };

        let result = chain.after_response(&context).await;
        assert!(result.is_ok());
    }
}
