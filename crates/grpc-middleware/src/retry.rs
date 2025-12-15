// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! # RetryInterceptor - Exponential Backoff Retry Logic
//!
//! ## Purpose
//! Identifies retryable errors and provides retry configuration for client-side retries.
//! Note: Actual retry logic is typically implemented in the client, not the server interceptor.
//!
//! ## Design
//! - **Retryable Status Codes**: Configurable list of gRPC status codes that should be retried
//! - **Exponential Backoff**: Calculates backoff delays based on configuration
//! - **Max Retries**: Limits number of retry attempts
//!
//! ## Note
//! This interceptor primarily serves to identify retryable errors. The actual retry
//! logic with exponential backoff would be implemented in the gRPC client layer.

use crate::chain::{Interceptor, InterceptorError};
use async_trait::async_trait;
use plexspaces_proto::grpc::v1::{
    InterceptorDecision, InterceptorRequest, InterceptorResponse, InterceptorResult,
    RetryMiddlewareConfig,
};

/// Retry interceptor for identifying retryable errors
pub struct RetryInterceptor {
    /// Configuration
    config: RetryMiddlewareConfig,
}

impl RetryInterceptor {
    /// Create RetryInterceptor from proto config
    pub fn new(config: RetryMiddlewareConfig) -> Self {
        Self { config }
    }

    /// Check if a status code is retryable
    fn is_retryable(&self, status_code: i32) -> bool {
        self.config.retryable_status_codes.contains(&status_code)
    }

    /// Calculate backoff delay for retry attempt
    ///
    /// ## Arguments
    /// * `attempt` - Retry attempt number (0 = first retry)
    ///
    /// ## Returns
    /// Backoff delay in milliseconds
    pub fn calculate_backoff(&self, attempt: u32) -> u64 {
        if attempt >= self.config.max_retries {
            return 0; // No more retries
        }

        let initial_delay_ms = self
            .config
            .initial_delay
            .as_ref()
            .map(|d| (d.seconds as u64 * 1000) + (d.nanos as u64 / 1_000_000))
            .unwrap_or(100); // Default 100ms

        let max_delay_ms = self
            .config
            .max_delay
            .as_ref()
            .map(|d| (d.seconds as u64 * 1000) + (d.nanos as u64 / 1_000_000))
            .unwrap_or(5000); // Default 5 seconds

        // Exponential backoff: initial_delay * (backoff_factor ^ attempt)
        let delay = (initial_delay_ms as f64 * self.config.backoff_factor.powi(attempt as i32)) as u64;

        // Cap at max_delay
        delay.min(max_delay_ms)
    }
}

impl Default for RetryInterceptor {
    fn default() -> Self {
        Self::new(RetryMiddlewareConfig::default())
    }
}

#[async_trait]
impl Interceptor for RetryInterceptor {
    async fn before_request(
        &self,
        _context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Retry interceptor doesn't block requests
        // It only identifies retryable errors in after_response
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    async fn after_response(
        &self,
        context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Check if error is retryable
        if self.is_retryable(context.status_code) {
            // Add retry hint to headers (for client-side retry logic)
            // In a real implementation, this would be added to response metadata
        }

        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    fn name(&self) -> &str {
        "retry"
    }

    fn priority(&self) -> i32 {
        35 // After auth, before compression
    }
}

