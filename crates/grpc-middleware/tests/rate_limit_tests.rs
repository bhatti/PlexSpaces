// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for rate limiting interceptor

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{
    InterceptorRequest, MiddlewareConfig, MiddlewareSpec, MiddlewareType,
    RateLimitMiddlewareConfig,
};

#[tokio::test]
async fn test_rate_limit_allows_requests_within_limit() {
    let config = RateLimitMiddlewareConfig {
        refill_rate: 10.0, // 10 tokens per second
        burst_size: 20,
        per_client: true, // Per-IP
        status_code: 429, // Too Many Requests
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRateLimit as i32,
            enabled: true,
            priority: 40,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::RateLimit(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Send 5 requests (within limit of 10/sec)
    for i in 0..5 {
        let context = InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: format!("req-{}", i),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = chain.before_request(&context).await;
        assert!(
            result.is_ok(),
            "Request {} should be allowed within rate limit",
            i
        );
    }
}

#[tokio::test]
async fn test_rate_limit_rejects_requests_exceeding_limit() {
    let config = RateLimitMiddlewareConfig {
        refill_rate: 2.0, // Very low rate (2 tokens/sec)
        burst_size: 3,
        per_client: true,
        status_code: 429,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRateLimit as i32,
            enabled: true,
            priority: 40,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::RateLimit(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Send requests rapidly to exceed limit
    let mut allowed = 0;
    let mut rejected = 0;

    for i in 0..10 {
        let context = InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: format!("req-{}", i),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = chain.before_request(&context).await;
        if result.is_ok() {
            allowed += 1;
        } else {
            rejected += 1;
        }
    }

    // Should have some rejections
    assert!(
        rejected > 0,
        "Should reject some requests when exceeding rate limit, allowed: {}, rejected: {}",
        allowed,
        rejected
    );
}

#[tokio::test]
async fn test_rate_limit_per_ip_strategy() {
    let config = RateLimitMiddlewareConfig {
        refill_rate: 5.0,
        burst_size: 10,
        per_client: true, // Per-IP
        status_code: 429,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRateLimit as i32,
            enabled: true,
            priority: 40,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::RateLimit(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // IP 1 should have separate limit from IP 2
    let context1 = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: "req-1".to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let context2 = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "192.168.1.1:12345".to_string(), // Different IP
        timestamp: None,
        request_id: "req-2".to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    // Both should be allowed (separate rate limits)
    let result1 = chain.before_request(&context1).await;
    let result2 = chain.before_request(&context2).await;

    assert!(result1.is_ok(), "IP 1 should be allowed");
    assert!(result2.is_ok(), "IP 2 should be allowed (separate limit)");
}

#[tokio::test]
async fn test_rate_limit_burst_size() {
    let config = RateLimitMiddlewareConfig {
        refill_rate: 1.0, // Very low rate (1 token/sec)
        burst_size: 5, // But allow burst of 5
        per_client: true,
        status_code: 429,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRateLimit as i32,
            enabled: true,
            priority: 40,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::RateLimit(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Should allow burst of 5 requests
    for i in 0..5 {
        let context = InterceptorRequest {
            method: "/ActorService/SpawnActor".to_string(),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: format!("req-{}", i),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = chain.before_request(&context).await;
        assert!(
            result.is_ok(),
            "Burst request {} should be allowed",
            i
        );
    }
}

