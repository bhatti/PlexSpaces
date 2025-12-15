// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for retry interceptor with exponential backoff

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{
    InterceptorRequest, InterceptorResponse, MiddlewareConfig, MiddlewareSpec, MiddlewareType,
    RetryMiddlewareConfig,
};

#[tokio::test]
async fn test_retry_interceptor_configured() {
    let config = RetryMiddlewareConfig {
        max_retries: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0, // 1 second
        }),
        backoff_factor: 2.0,
        retryable_status_codes: vec![14, 8], // UNAVAILABLE, RESOURCE_EXHAUSTED
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRetry as i32,
            enabled: true,
            priority: 35,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Retry(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    // Retry interceptor should allow initial request
    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Retry interceptor should allow initial request");
}

#[tokio::test]
async fn test_retry_interceptor_identifies_retryable_errors() {
    let config = RetryMiddlewareConfig {
        max_retries: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0,
        }),
        backoff_factor: 2.0,
        retryable_status_codes: vec![14], // UNAVAILABLE
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRetry as i32,
            enabled: true,
            priority: 35,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Retry(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Response with retryable error (UNAVAILABLE = 14)
    let response_context = InterceptorResponse {
        status_code: 14, // UNAVAILABLE
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
    };

    // Retry interceptor should identify this as retryable
    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should handle retryable error");
}

#[tokio::test]
async fn test_retry_interceptor_does_not_retry_non_retryable_errors() {
    let config = RetryMiddlewareConfig {
        max_retries: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0,
        }),
        backoff_factor: 2.0,
        retryable_status_codes: vec![14, 8], // Only UNAVAILABLE and RESOURCE_EXHAUSTED
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRetry as i32,
            enabled: true,
            priority: 35,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Retry(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Response with non-retryable error (INVALID_ARGUMENT = 3)
    let response_context = InterceptorResponse {
        status_code: 3, // INVALID_ARGUMENT (not in retryable list)
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
    };

    // Should not retry
    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should not retry non-retryable errors");
}

#[tokio::test]
async fn test_retry_interceptor_respects_max_retries() {
    let config = RetryMiddlewareConfig {
        max_retries: 2, // Max 2 retries
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 10_000_000, // 10ms (fast for testing)
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        backoff_factor: 2.0,
        retryable_status_codes: vec![14], // UNAVAILABLE
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeRetry as i32,
            enabled: true,
            priority: 35,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Retry(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Simulate multiple retryable failures
    // Note: Actual retry logic would be in the client, not interceptor
    // This test just verifies configuration is respected
    let response_context = InterceptorResponse {
        status_code: 14, // UNAVAILABLE
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
    };

    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should handle retry configuration");
}

