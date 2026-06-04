// SPDX-License-Identifier: AGPL-3.0-or-later
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
use std::{collections::VecDeque, num::NonZeroU32, sync::Arc};

type BucketLimiter = Arc<RateLimiter<NotKeyed, InMemoryState, governor::clock::DefaultClock>>;

const MAX_CLIENT_LIMITERS: usize = 10_000;

/// Rate limiting interceptor using token bucket algorithm
pub struct RateLimitInterceptor {
    /// Global rate limiter (if per_client = false)
    global_limiter: Option<BucketLimiter>,

    /// Per-client rate limiters with bounded LRU eviction (if per_client = true)
    client_limiters: Arc<tokio::sync::RwLock<BoundedLimiterMap>>,

    /// Configuration
    config: RateLimitMiddlewareConfig,
}

struct BoundedLimiterMap {
    map: std::collections::HashMap<String, BucketLimiter>,
    order: VecDeque<String>,
}

impl BoundedLimiterMap {
    fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn get(&self, key: &str) -> Option<&BucketLimiter> {
        self.map.get(key)
    }

    fn insert(&mut self, key: String, limiter: BucketLimiter) {
        if self.map.len() >= MAX_CLIENT_LIMITERS {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(key.clone());
        self.map.insert(key, limiter);
    }
}

fn build_quota(config: &RateLimitMiddlewareConfig) -> Quota {
    let refill_rate = (config.refill_rate as u32).max(1);
    let burst_size = config.burst_size.max(1);
    Quota::per_second(NonZeroU32::new(refill_rate).unwrap())
        .allow_burst(NonZeroU32::new(burst_size).unwrap())
}

impl RateLimitInterceptor {
    /// Create new rate limit interceptor from proto config
    pub fn new(config: RateLimitMiddlewareConfig) -> Self {
        let global_limiter = if !config.per_client {
            let quota = build_quota(&config);
            Some(Arc::new(RateLimiter::direct(quota)))
        } else {
            None
        };

        Self {
            global_limiter,
            client_limiters: Arc::new(tokio::sync::RwLock::new(BoundedLimiterMap::new())),
            config,
        }
    }

    /// Get or create rate limiter for a client IP
    async fn get_client_limiter(&self, client_ip: &str) -> BucketLimiter {
        {
            let limiters = self.client_limiters.read().await;
            if let Some(limiter) = limiters.get(client_ip) {
                return Arc::clone(limiter);
            }
        }

        let quota = build_quota(&self.config);
        let limiter = Arc::new(RateLimiter::direct(quota));

        let mut limiters = self.client_limiters.write().await;
        // Double-check after acquiring write lock
        if let Some(existing) = limiters.get(client_ip) {
            return Arc::clone(existing);
        }
        limiters.insert(client_ip.to_string(), Arc::clone(&limiter));
        limiter
    }

    /// Check if request should be rate limited
    fn check_rate_limit(&self, limiter: &BucketLimiter) -> Result<(), InterceptorError> {
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
        let limiter = if self.config.per_client {
            let client_ip = context.remote_addr.split(':').next().unwrap_or("unknown");
            self.get_client_limiter(client_ip).await
        } else {
            match &self.global_limiter {
                Some(limiter) => Arc::clone(limiter),
                None => {
                    return Ok(InterceptorResult {
                        decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                        error_message: String::new(),
                        modified_headers: std::collections::HashMap::new(),
                        metrics: vec![],
                    });
                }
            }
        };

        match self.check_rate_limit(&limiter) {
            Ok(()) => Ok(InterceptorResult {
                decision: InterceptorDecision::InterceptorDecisionAllow as i32,
                error_message: String::new(),
                modified_headers: std::collections::HashMap::new(),
                metrics: vec![],
            }),
            Err(e) => Ok(InterceptorResult {
                decision: InterceptorDecision::InterceptorDecisionDeny as i32,
                error_message: e.to_string(),
                modified_headers: std::collections::HashMap::new(),
                metrics: vec![],
            }),
        }
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
        "rate_limit"
    }

    fn priority(&self) -> i32 {
        40
    }
}
