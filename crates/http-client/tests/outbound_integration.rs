// SPDX-License-Identifier: LGPL-2.1-or-later

//! Integration tests: resilient client against a local axum server.

use axum::{routing::get, Router};
use plexspaces_core::{OutboundHttpClient, OutboundHttpRequest};
use plexspaces_http_client::ResilientOutboundHttpClient;
use plexspaces_proto::circuitbreaker::prv::{CircuitBreakerConfig, FailureStrategy};
use plexspaces_proto::node::v1::{
    ClientTransportPolicy, HttpRetryPolicy, OutboundTransport, RuntimeConfig, ServiceLinkConfig,
};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

/// Binds an ephemeral port and serves `app` until graceful shutdown (long sleep; tests finish first).
async fn spawn_axum_test_server(app: Router) -> (SocketAddr, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("local_addr");
    let jh = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            })
            .await;
    });
    tokio::task::yield_now().await;
    (addr, jh)
}

#[tokio::test]
async fn outbound_http_get_success() {
    let hits = Arc::new(AtomicU32::new(0));
    let hits_srv = hits.clone();
    let app = Router::new().route(
        "/ping",
        get(move || async move {
            hits_srv.fetch_add(1, Ordering::SeqCst);
            "pong"
        }),
    );
    let (addr, _jh) = spawn_axum_test_server(app).await;

    let base = format!("http://{}", addr);
    let mut rt = RuntimeConfig::default();
    rt.service_links.push(ServiceLinkConfig {
        name: "test-api".to_string(),
        transport: OutboundTransport::OutboundTransportHttp as i32,
        base_url: base,
        ..Default::default()
    });

    let client = ResilientOutboundHttpClient::from_runtime_config(&rt).expect("build client");
    let resp = client
        .execute(
            "test-api",
            OutboundHttpRequest {
                method: "GET".to_string(),
                path_and_query: "/ping".to_string(),
                headers: vec![],
                body: vec![],
            },
        )
        .await
        .expect("execute");
    assert_eq!(resp.status, 200);
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn outbound_http_retries_on_503_then_succeeds() {
    let attempt = Arc::new(AtomicU32::new(0));
    let a = attempt.clone();
    let app = Router::new().route(
        "/flaky",
        get(move || async move {
            let n = a.fetch_add(1, Ordering::SeqCst);
            if n < 2 {
                axum::http::StatusCode::SERVICE_UNAVAILABLE
            } else {
                axum::http::StatusCode::OK
            }
        }),
    );
    let (addr, _jh) = spawn_axum_test_server(app).await;

    let base = format!("http://{}", addr);
    let mut rt = RuntimeConfig::default();
    rt.default_outbound_client_policy = Some(ClientTransportPolicy {
        retry: Some(HttpRetryPolicy {
            max_attempts: 4,
            initial_backoff: Some(prost_types::Duration {
                seconds: 0,
                nanos: 1_000_000,
            }),
            max_backoff: Some(prost_types::Duration {
                seconds: 0,
                nanos: 5_000_000,
            }),
            backoff_multiplier: 2.0,
            jitter_ratio: 0.0,
            allow_non_idempotent_retry: false,
        }),
        ..Default::default()
    });
    rt.service_links.push(ServiceLinkConfig {
        name: "flaky".to_string(),
        transport: OutboundTransport::OutboundTransportHttp as i32,
        base_url: base,
        ..Default::default()
    });

    let client = ResilientOutboundHttpClient::from_runtime_config(&rt).expect("build client");
    let resp = client
        .execute(
            "flaky",
            OutboundHttpRequest {
                method: "GET".to_string(),
                path_and_query: "/flaky".to_string(),
                headers: vec![],
                body: vec![],
            },
        )
        .await
        .expect("execute after retries");
    assert_eq!(resp.status, 200);
    assert!(attempt.load(Ordering::SeqCst) >= 3);
}

#[tokio::test]
async fn circuit_opens_after_failures() {
    let app = Router::new().route(
        "/boom",
        get(|| async { axum::http::StatusCode::INTERNAL_SERVER_ERROR }),
    );
    let (addr, _jh) = spawn_axum_test_server(app).await;

    let base = format!("http://{}", addr);
    let mut rt = RuntimeConfig::default();
    rt.service_links.push(ServiceLinkConfig {
        name: "bad".to_string(),
        transport: OutboundTransport::OutboundTransportHttp as i32,
        base_url: base,
        policy_template: None,
        ..Default::default()
    });
    rt.default_outbound_client_policy = Some(ClientTransportPolicy {
        retry: Some(HttpRetryPolicy {
            max_attempts: 1,
            ..Default::default()
        }),
        circuit_breaker: Some(CircuitBreakerConfig {
            name: String::new(),
            failure_strategy: FailureStrategy::FailureStrategyConsecutive as i32,
            failure_threshold: 2,
            success_threshold: 1,
            timeout: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
            half_open_config: None,
            sliding_window: None,
            request_timeout: None,
            max_half_open_requests: 5,
        }),
        ..Default::default()
    });

    let client = ResilientOutboundHttpClient::from_runtime_config(&rt).expect("build client");
    let req = OutboundHttpRequest {
        method: "GET".to_string(),
        path_and_query: "/boom".to_string(),
        headers: vec![],
        body: vec![],
    };
    // HTTP 500 is still `Ok` with status set; the breaker records failures for status >= 400.
    let r1 = client.execute("bad", req.clone()).await.expect("first");
    assert_eq!(r1.status, 500);
    let r2 = client.execute("bad", req.clone()).await.expect("second");
    assert_eq!(r2.status, 500);
    let third = client.execute("bad", req).await;
    assert!(
        matches!(
            third,
            Err(plexspaces_core::OutboundHttpClientError::CircuitOpen { .. })
        ),
        "expected circuit open, got {:?}",
        third
    );
}

#[tokio::test]
async fn outbound_http_client_supports_concurrent_calls() {
    let hits = Arc::new(AtomicU32::new(0));
    let hits_srv = hits.clone();
    let app = Router::new().route(
        "/forecast",
        get(move || async move {
            hits_srv.fetch_add(1, Ordering::SeqCst);
            "sunny"
        }),
    );
    let (addr, _jh) = spawn_axum_test_server(app).await;

    let mut rt = RuntimeConfig::default();
    rt.service_links.push(ServiceLinkConfig {
        name: "weather-api".to_string(),
        transport: OutboundTransport::OutboundTransportHttp as i32,
        base_url: format!("http://{addr}"),
        ..Default::default()
    });

    let client =
        Arc::new(ResilientOutboundHttpClient::from_runtime_config(&rt).expect("build client"));

    let mut handles = Vec::new();
    for _ in 0..24 {
        let client = client.clone();
        handles.push(tokio::spawn(async move {
            client
                .execute(
                    "weather-api",
                    OutboundHttpRequest {
                        method: "GET".to_string(),
                        path_and_query: "/forecast".to_string(),
                        headers: vec![],
                        body: vec![],
                    },
                )
                .await
                .expect("execute")
        }));
    }

    for handle in handles {
        let resp = handle.await.expect("task should complete");
        assert_eq!(resp.status, 200);
        assert_eq!(resp.body, b"sunny");
    }

    assert_eq!(hits.load(Ordering::SeqCst), 24);
}
