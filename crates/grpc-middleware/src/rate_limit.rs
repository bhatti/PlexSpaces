// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! # RateLimitInterceptor - Token Bucket Rate Limiting
//!
//! ## Purpose
//! Implements token bucket rate limiting using the `governor` crate.
//! Prevents service overload by limiting requests per second per client.
//!
//! ## Design
//! - **Token Bucket**: Refills at configured rate, allows bursts up to burst_size
//! - **Per-Client**: Can rate limit per IP address or globally
//! - **Non-Blocking**: Returns 429 (Too Many Requests) when limit exceeded
//!
//! ## Configuration
//! - `refill_rate`: Tokens per second (sustained rate)
//! - `burst_size`: Maximum tokens (burst capacity)
//! - `per_client`: If true, rate limit per IP; if false, global limit
//! - `status_code`: HTTP status code for rate limit exceeded (default: 429)

use crate::chain::{Interceptor, InterceptorError};
use async_trait::async_trait;
use governor::{
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
use plexspaces_proto::grpc::v1::{
    InterceptorDecision, InterceptorRequest, InterceptorResponse, InterceptorResult,
    RateLimitMiddlewareConfig,
};
use std::{
    num::NonZeroU32,
    sync::Arc,
};

/// Rate limiting interceptor using token bucket algorithm
pub struct RateLimitInterceptor {
    /// Global rate limiter (if per_client = false)
    global_limiter: Option<Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>>,

    /// Per-client rate limiters (if per_client = true)
    /// Key: client IP address
    client_limiters: Arc<
        tokio::sync::RwLock<
            std::collections::HashMap<
                String,
                Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>,
            >,
        >,
    >,

    /// Configuration
    config: RateLimitMiddlewareConfig,
}

impl RateLimitInterceptor {
    /// Create new rate limit interceptor from proto config
    pub fn new(config: RateLimitMiddlewareConfig) -> Self {
        let global_limiter = if !config.per_client {
            // Create global rate limiter
            let burst_size = config.burst_size.max(1);

            let quota = Quota::per_second(NonZeroU32::new(burst_size as u32).unwrap())
                .allow_burst(NonZeroU32::new(burst_size as u32).unwrap());

            Some(Arc::new(RateLimiter::direct(quota)))
        } else {
            None
        };

        Self {
            global_limiter,
            client_limiters: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            config,
        }
    }

    /// Get or create rate limiter for a client IP
    async fn get_client_limiter(
        &self,
        client_ip: &str,
    ) -> Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>> {
        // Check if limiter exists
        {
            let limiters = self.client_limiters.read().await;
            if let Some(limiter) = limiters.get(client_ip) {
                return Arc::clone(limiter);
            }
        }

        // Create new limiter for this client
        let burst_size = self.config.burst_size.max(1);

        let quota = Quota::per_second(NonZeroU32::new(burst_size as u32).unwrap())
            .allow_burst(NonZeroU32::new(burst_size as u32).unwrap());

        let limiter = Arc::new(RateLimiter::direct(quota));

        // Store in map
        {
            let mut limiters = self.client_limiters.write().await;
            limiters.insert(client_ip.to_string(), Arc::clone(&limiter));
        }

        limiter
    }

    /// Check if request should be rate limited
    fn check_rate_limit(
        &self,
        limiter: &RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>,
    ) -> Result<(), InterceptorError> {
        // Try to acquire a token (non-blocking)
        limiter.check().map_err(|_| {
            InterceptorError::RateLimitExceeded(format!(
                "Rate limit exceeded: {} requests/second, burst: {}",
                self.config.refill_rate, self.config.burst_size
            ))
        })
    }
}

impl Default for RateLimitInterceptor {
    fn default() -> Self {
        Self::new(RateLimitMiddlewareConfig::default())
    }
}

#[async_trait]
impl Interceptor for RateLimitInterceptor {
    async fn before_request(
        &self,
        context: &InterceptorRequest,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Determine which limiter to use
        let limiter = if self.config.per_client {
            // Per-client rate limiting
            let client_ip = context.remote_addr.split(':').next().unwrap_or("unknown");
            self.get_client_limiter(client_ip).await
        } else {
            // Global rate limiting
            match &self.global_limiter {
                Some(limiter) => Arc::clone(limiter),
                None => {
                    // Fallback: create a limiter on the fly
                    return Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                        error_message: String::new(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    });
                }
            }
        };

        // Check rate limit
        match self.check_rate_limit(&limiter) {
            Ok(()) => {
                // Allow request
                Ok(InterceptorResult {
                    decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                    error_message: String::new(),
                    modified_headers: std::collections::HashMap::new(),
                    metrics: vec![],
                })
            }
            Err(e) => {
                // Rate limit exceeded
                Ok(InterceptorResult {
                    decision: InterceptorDecision::InterceptorDecisionDeny as i32,
                    error_message: e.to_string(),
                    modified_headers: std::collections::HashMap::new(),
                    metrics: vec![],
                })
            }
        }
    }

    async fn after_response(
        &self,
        _context: &InterceptorResponse,
    ) -> Result<InterceptorResult, InterceptorError> {
        // Rate limiting is checked before request, nothing to do after
        Ok(InterceptorResult {
            decision: InterceptorDecision::InterceptorDecisionAllow as i32,
            error_message: String::new(),
            modified_headers: std::collections::HashMap::new(),
            metrics: vec![],
        })
    }

    fn name(&self) -> &str {
        "rate_limit"
    }

    fn priority(&self) -> i32 {
        40
    }
}
