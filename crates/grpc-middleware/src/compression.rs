// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! # CompressionInterceptor - Stub with Config
//!
//! ## Status
//! Stub implementation - always allows requests.
//! Accepts proto `CompressionMiddlewareConfig` for future implementation.

use crate::chain::{Interceptor, InterceptorError};
use async_trait::async_trait;
use plexspaces_proto::grpc::v1::{
    CompressionMiddlewareConfig, InterceptorDecision, InterceptorRequest, InterceptorResponse,
    InterceptorResult,
};

/// Compression interceptor (stub)
pub struct CompressionInterceptor {
    _config: CompressionMiddlewareConfig,
}

impl CompressionInterceptor {
    /// Create CompressionInterceptor from proto config
    pub fn new(config: CompressionMiddlewareConfig) -> Self {
        Self { _config: config }
    }
}

impl Default for CompressionInterceptor {
    fn default() -> Self {
        Self::new(CompressionMiddlewareConfig::default())
    }
}

#[async_trait]
impl Interceptor for CompressionInterceptor {
    async fn before_request(
        &self,
        _context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // TODO: Decompress request if compressed
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
        // TODO: Compress response if size > threshold
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    fn name(&self) -> &str {
        "compression"
    }

    fn priority(&self) -> i32 {
        50
    }
}
