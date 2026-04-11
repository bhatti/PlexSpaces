// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration test to reproduce and verify fix for "tried to clone a span that already closed" panic
//
// ## Purpose
// This test reproduces the span cloning panic that occurs when:
// 1. OpenTelemetry spans are created in interceptors
// 2. Spans are stored in Context objects
// 3. Spans are ended/closed
// 4. Context objects are cloned later (in async code)
// 5. OpenTelemetry tries to clone already-closed spans, causing panic
//
// ## Test Strategy
// - Create a scenario that triggers span cloning after span is closed
// - Verify that the fix prevents the panic
// - Test both with and without the fix to ensure it works

use plexspaces_grpc_middleware::chain::InterceptorChain;
use plexspaces_proto::grpc::v1::{
    InterceptorRequest, InterceptorResponse, MiddlewareConfig, MiddlewareSpec, MiddlewareType,
    TracingMiddlewareConfig,
};
use std::sync::Arc;
use tokio::sync::Barrier;

#[tokio::test]
async fn test_span_cloning_panic_reproduction() {
    // This test reproduces the scenario that causes "tried to clone a span that already closed" panic
    // The panic occurs when OpenTelemetry Context objects with spans are cloned after spans are closed.

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
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(config)),
        }],
    };

    let chain = Arc::new(InterceptorChain::from_config(&middleware_config).unwrap());

    // Create multiple concurrent requests to trigger span cloning issues
    // The issue occurs when spans are created, ended, and then contexts are cloned in async code
    let barrier = Arc::new(Barrier::new(10));
    let mut handles = vec![];

    for i in 0..10 {
        let chain_clone = chain.clone();
        let barrier_clone = barrier.clone();

        let handle = tokio::spawn(async move {
            // Wait for all tasks to be ready
            barrier_clone.wait().await;

            let context = InterceptorRequest {
                method: format!("/ActorService/Method{}", i),
                headers: std::collections::HashMap::new(),
                remote_addr: "127.0.0.1:12345".to_string(),
                timestamp: None,
                request_id: ulid::Ulid::new().to_string(),
                peer_certificate: String::new(),
                peer_service_id: String::new(),
            };

            // Call before_request - this should NOT panic even with concurrent requests
            let result = chain_clone.before_request(&context).await;
            assert!(result.is_ok(), "before_request should not panic");

            // Simulate async work that might clone contexts
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            // Call after_response - this should NOT panic even if spans were closed
            let response_context = InterceptorResponse {
                status_code: 0,
                headers: std::collections::HashMap::new(),
                timestamp: None,
                duration: Some(plexspaces_proto::prost_types::Duration {
                    seconds: 0,
                    nanos: 100_000_000,
                }),
                request_id: ulid::Ulid::new().to_string(),
                method: "/test.Service/ConcurrentMethod".to_string(),
            };

            let result = chain_clone.after_response(&response_context).await;
            assert!(result.is_ok(), "after_response should not panic");
        });

        handles.push(handle);
    }

    // Wait for all tasks to complete - none should panic
    for handle in handles {
        let result = handle.await;
        assert!(result.is_ok(), "Task should complete without panicking");
    }
}

#[tokio::test]
async fn test_span_cloning_with_error_handling() {
    // Test that error handling doesn't trigger span cloning panics
    // This reproduces the scenario where errors occur and spans might be cloned during error handling

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
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(config)),
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

    // Call before_request
    let _ = chain.before_request(&context).await.unwrap();

    // Simulate error - call on_error
    // This should NOT panic even if spans were created and closed
    let error = tonic::Status::internal("Test error");
    chain.on_error(&error).await;

    // Call after_response - should not panic
    let response_context = InterceptorResponse {
        status_code: 500,
        headers: std::collections::HashMap::new(),
        timestamp: None,
        duration: Some(plexspaces_proto::prost_types::Duration {
            seconds: 0,
            nanos: 50_000_000,
        }),
        request_id: ulid::Ulid::new().to_string(),
        method: "/test.Service/ErrorMethod".to_string(),
    };

    let result = chain.after_response(&response_context).await;
    assert!(
        result.is_ok(),
        "after_response should not panic even after error"
    );
}

#[tokio::test]
async fn test_tracing_interceptor_no_panic_on_rapid_requests() {
    // Test rapid sequential requests to ensure spans don't cause panics
    // This reproduces the scenario from the migrating_orbit example where
    // multiple concurrent requests trigger span cloning issues

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
            config: Some(plexspaces_proto::grpc::v1::middleware_spec::Config::Tracing(config)),
        }],
    };

    let chain = InterceptorChain::from_config(&middleware_config).unwrap();

    // Rapid sequential requests (like in migrating_orbit example)
    for i in 0..100 {
        let context = InterceptorRequest {
            method: format!("/ActorService/Method{}", i),
            headers: std::collections::HashMap::new(),
            remote_addr: "127.0.0.1:12345".to_string(),
            timestamp: None,
            request_id: ulid::Ulid::new().to_string(),
            peer_certificate: String::new(),
            peer_service_id: String::new(),
        };

        let result = chain.before_request(&context).await;
        assert!(result.is_ok(), "Request {} should not panic", i);

        // Immediately call after_response to simulate rapid request/response cycle
        let response_context = InterceptorResponse {
            status_code: 0,
            headers: std::collections::HashMap::new(),
            timestamp: None,
            duration: Some(plexspaces_proto::prost_types::Duration {
                seconds: 0,
                nanos: 10_000_000, // 10ms
            }),
            request_id: ulid::Ulid::new().to_string(),
            method: format!("/ActorService/Method{}", i),
        };

        let result = chain.after_response(&response_context).await;
        assert!(result.is_ok(), "Response {} should not panic", i);
    }
}
