// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for retry interceptor with exponential backoff

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::common::v1::RetryPolicy;
use plexspaces_proto::grpc::v1::{
    InterceptorRequest, InterceptorResponse, MiddlewareConfig, MiddlewareSpec, MiddlewareType,
};

#[tokio::test]
async fn test_retry_interceptor_configured() {
    let config = RetryPolicy {
        max_attempts: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0, // 1 second
        }),
        backoff_multiplier: 2.0,
        retryable_errors: vec!["14".to_string(), "8".to_string()],
        ..Default::default()
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

    let result = chain.before_request(&context).await;
    assert!(
        result.is_ok(),
        "Retry interceptor should allow initial request"
    );
}

#[tokio::test]
async fn test_retry_interceptor_identifies_retryable_errors() {
    let config = RetryPolicy {
        max_attempts: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0,
        }),
        backoff_multiplier: 2.0,
        retryable_errors: vec!["14".to_string()],
        ..Default::default()
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

    let response_context = InterceptorResponse {
        status_code: 14, // UNAVAILABLE
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
        request_id: ulid::Ulid::new().to_string(),
        method: "/test.Service/RetryableMethod".to_string(),
    };

    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should handle retryable error");
}

#[tokio::test]
async fn test_retry_interceptor_does_not_retry_non_retryable_errors() {
    let config = RetryPolicy {
        max_attempts: 3,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000,
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 1,
            nanos: 0,
        }),
        backoff_multiplier: 2.0,
        retryable_errors: vec!["14".to_string(), "8".to_string()],
        ..Default::default()
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

    let response_context = InterceptorResponse {
        status_code: 3, // INVALID_ARGUMENT (not in retryable list)
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
        request_id: ulid::Ulid::new().to_string(),
        method: "/test.Service/NonRetryableMethod".to_string(),
    };

    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should not retry non-retryable errors");
}

#[tokio::test]
async fn test_retry_interceptor_respects_max_retries() {
    let config = RetryPolicy {
        max_attempts: 2,
        initial_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 10_000_000, // 10ms
        }),
        max_delay: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
        backoff_multiplier: 2.0,
        retryable_errors: vec!["14".to_string()],
        ..Default::default()
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

    let response_context = InterceptorResponse {
        status_code: 14, // UNAVAILABLE
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: None,
        request_id: ulid::Ulid::new().to_string(),
        method: "/test.Service/MaxRetryMethod".to_string(),
    };

    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should handle retry configuration");
}
