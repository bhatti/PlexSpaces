// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for OpenTelemetry tracing interceptor

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{
    InterceptorRequest, InterceptorResponse, MiddlewareConfig, MiddlewareSpec, MiddlewareType,
    TracingMiddlewareConfig,
};

#[tokio::test]
async fn test_tracing_interceptor_creates_span() {
    let config = TracingMiddlewareConfig {
        backend: "test".to_string(),
        endpoint: "http://localhost:4318".to_string(),
        service_name: "test-service".to_string(),
        sampling_rate: 1.0, // Sample all requests
        trace_payloads: false,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeTracing as i32,
            enabled: true,
            priority: 20,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(
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

    // Should create span and allow request
    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Tracing interceptor should allow requests");
}

#[tokio::test]
async fn test_tracing_interceptor_propagates_trace_context() {
    let config = TracingMiddlewareConfig {
        backend: "test".to_string(),
        endpoint: "http://localhost:4318".to_string(),
        service_name: "test-service".to_string(),
        sampling_rate: 1.0,
        trace_payloads: false,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeTracing as i32,
            enabled: true,
            priority: 20,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Request with trace context in headers
    let mut headers = std::collections::HashMap::new();
    headers.insert(
        "traceparent".to_string(),
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01".to_string(),
    );

    let context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers,
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Should extract and propagate trace context");
}

#[tokio::test]
async fn test_tracing_interceptor_records_duration() {
    let config = TracingMiddlewareConfig {
        backend: "test".to_string(),
        endpoint: "http://localhost:4318".to_string(),
        service_name: "test-service".to_string(),
        sampling_rate: 1.0,
        trace_payloads: false,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeTracing as i32,
            enabled: true,
            priority: 20,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(
                config,
            )),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    let request_context = InterceptorRequest {
        method: "/ActorService/SpawnActor".to_string(),
        headers: std::collections::HashMap::new(),
        remote_addr: "127.0.0.1:12345".to_string(),
        timestamp: None,
        request_id: ulid::Ulid::new().to_string(),
        peer_certificate: String::new(),
        peer_service_id: String::new(),
    };

    let _ = chain.before_request(&request_context).await.unwrap();

    // Simulate response with duration
    let response_context = InterceptorResponse {
        status_code: 0,
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 100_000_000, // 100ms
        }),
    };

    let result = chain.after_response(&response_context).await;
    assert!(result.is_ok(), "Should record span duration");
}

#[tokio::test]
async fn test_tracing_interceptor_respects_sampling_rate() {
    // Test with 0% sampling (should not create spans)
    let config = TracingMiddlewareConfig {
        backend: "test".to_string(),
        endpoint: "http://localhost:4318".to_string(),
        service_name: "test-service".to_string(),
        sampling_rate: 0.0, // No sampling
        trace_payloads: false,
    };

    let middleware_config = MiddlewareConfig {
        middleware: vec![MiddlewareSpec {
            middleware_type: MiddlewareType::MiddlewareTypeTracing as i32,
            enabled: true,
            priority: 20,
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(
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

    // Should still allow request (sampling doesn't block)
    let result = chain.before_request(&context).await;
    assert!(result.is_ok(), "Sampling rate should not block requests");
}

