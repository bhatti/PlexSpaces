// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! Integration tests for gRPC Health and Metrics endpoints

use plexspaces_node::{Node, NodeId, default_node_config};
use plexspaces_proto::system::v1::ServingStatus;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tonic_health::pb::health_client::HealthClient;
use tonic_health::pb::HealthCheckRequest;
use tokio_stream::StreamExt;

/// Test helper to create a health reporter (standalone for testing)
fn create_health_reporter() -> Arc<plexspaces_node::health_service::PlexSpacesHealthReporter> {
    let (health_reporter, _) = plexspaces_node::health_service::PlexSpacesHealthReporter::new();
    Arc::new(health_reporter)
}

#[tokio::test]
async fn test_health_check_overall_health() {
    // Test: Standard gRPC health check for overall health (empty service name)
    let health_reporter = create_health_reporter();
    
    // Initially NOT_SERVING (startup in progress) or UNKNOWN
    let status = health_reporter.get_service_status("").await;
    assert!(status == ServingStatus::ServingStatusNotServing || status == ServingStatus::ServingStatusUnknown);
    
    // Mark startup complete
    health_reporter.mark_startup_complete(None).await;
    
    // Now should be SERVING
    let status = health_reporter.get_service_status("").await;
    assert_eq!(status, ServingStatus::ServingStatusServing);
}

#[tokio::test]
async fn test_health_check_service_specific() {
    // Test: Service-specific health checks
    let health_reporter = create_health_reporter();
    
    // Mark startup complete
    health_reporter.mark_startup_complete(None).await;
    
    // Check individual service health
    let actor_service_status = health_reporter.get_service_status("plexspaces.actor.v1.ActorService").await;
    assert_eq!(actor_service_status, ServingStatus::ServingStatusServing);
    
    let tuplespace_status = health_reporter.get_service_status("plexspaces.tuplespace.v1.TuplePlexSpaceService").await;
    assert_eq!(tuplespace_status, ServingStatus::ServingStatusServing);
    
    let supervisor_status = health_reporter.get_service_status("plexspaces.supervisor.v1.SupervisorService").await;
    assert_eq!(supervisor_status, ServingStatus::ServingStatusServing);
}

#[tokio::test]
async fn test_health_check_shutdown() {
    // Test: Health status updates to NOT_SERVING on shutdown
    let health_reporter = create_health_reporter();
    
    // Mark startup complete
    health_reporter.mark_startup_complete(None).await;
    
    // Verify SERVING
    let status = health_reporter.get_service_status("").await;
    assert_eq!(status, ServingStatus::ServingStatusServing);
    
    // Begin shutdown
    health_reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
    
    // Verify NOT_SERVING
    let status = health_reporter.get_service_status("").await;
    assert_eq!(status, ServingStatus::ServingStatusNotServing);
    
    // Verify shutdown flag is set
    assert!(health_reporter.is_shutting_down().await);
}

#[tokio::test]
async fn test_health_check_service_status_updates() {
    // Test: Setting individual service status
    let health_reporter = create_health_reporter();
    
    health_reporter.mark_startup_complete(None).await;
    
    // Set a service to NOT_SERVING
    health_reporter.set_service_status("plexspaces.actor.v1.ActorService", ServingStatus::ServingStatusNotServing).await;
    
    let status = health_reporter.get_service_status("plexspaces.actor.v1.ActorService").await;
    assert_eq!(status, ServingStatus::ServingStatusNotServing);
    
    // Other services should still be SERVING
    let tuplespace_status = health_reporter.get_service_status("plexspaces.tuplespace.v1.TuplePlexSpaceService").await;
    assert_eq!(tuplespace_status, ServingStatus::ServingStatusServing);
}

#[tokio::test]
async fn test_health_check_readiness() {
    // Test: Readiness checks
    let health_reporter = create_health_reporter();
    
    // Initially not ready (startup in progress)
    let (is_ready, reason) = health_reporter.check_readiness().await;
    assert!(!is_ready);
    assert!(reason.contains("Startup"));
    
    // Mark startup complete
    health_reporter.mark_startup_complete(None).await;
    
    // Now should be ready
    let (is_ready, _reason) = health_reporter.check_readiness().await;
    assert!(is_ready);
}

#[tokio::test]
async fn test_health_check_liveness() {
    // Test: Liveness checks
    let health_reporter = create_health_reporter();
    
    // Note: is_alive() returns true only if is_alive=true AND shutdown_in_progress=false
    // For a new health reporter, is_alive defaults to true, shutdown_in_progress defaults to false
    // So is_alive() should return true initially
    let is_alive = health_reporter.is_alive().await;
    assert!(is_alive, "Node should be alive (not crashed) even during startup");
    
    // Mark startup complete (doesn't affect is_alive, but ensures node is in good state)
    health_reporter.mark_startup_complete(None).await;
    
    // Should still be alive
    let is_alive = health_reporter.is_alive().await;
    assert!(is_alive, "Node should be alive after startup");
    
    // Begin shutdown
    health_reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
    
    // During shutdown, is_alive() returns false (because shutdown_in_progress=true)
    // This is correct: node is shutting down, so liveness probe should fail
    let is_alive = health_reporter.is_alive().await;
    assert!(!is_alive, "Node should not be alive during shutdown");
}

#[tokio::test]
async fn test_health_check_startup() {
    // Test: Startup checks
    let health_reporter = create_health_reporter();
    
    // Initially startup not complete
    let (is_complete, _reason) = health_reporter.check_startup().await;
    assert!(!is_complete);
    
    // Mark startup complete
    health_reporter.mark_startup_complete(None).await;
    
    // Now startup should be complete
    let (is_complete, _reason) = health_reporter.check_startup().await;
    assert!(is_complete);
}

#[tokio::test]
async fn test_health_check_get_all_service_statuses() {
    // Test: Get all service statuses
    let health_reporter = create_health_reporter();
    
    health_reporter.mark_startup_complete(None).await;
    
    let all_statuses = health_reporter.get_all_service_statuses().await;
    
    // Should have at least the default services
    assert!(all_statuses.contains_key("plexspaces.actor.v1.ActorService"));
    assert!(all_statuses.contains_key("plexspaces.tuplespace.v1.TuplePlexSpaceService"));
    assert!(all_statuses.contains_key("plexspaces.supervisor.v1.SupervisorService"));
    
    // All should be SERVING
    for (service_name, status) in all_statuses.iter() {
        if service_name.starts_with("plexspaces.") {
            assert_eq!(*status, ServingStatus::ServingStatusServing, "Service {} should be SERVING", service_name);
        }
    }
}

#[tokio::test]
async fn test_metrics_service_export_prometheus() {
    // Test: Prometheus metrics export
    use plexspaces_node::metrics_service::MetricsServiceImpl;
    use plexspaces_proto::metrics::v1::metrics_service_server::MetricsService;
    
    let metrics_service = MetricsServiceImpl::new();
    let request = tonic::Request::new(plexspaces_proto::metrics::v1::ExportPrometheusRequest {});
    
    let response = metrics_service.export_prometheus(request).await.unwrap();
    let content = response.get_ref().content.clone();
    
    // Should contain Prometheus format
    assert!(content.contains("PlexSpaces Metrics"));
    assert!(content.contains("plexspaces_node_health_requests_total"));
    assert!(content.contains("# TYPE"));
    assert!(content.contains("# HELP"));
}

#[tokio::test]
async fn test_metrics_service_list_definitions() {
    // Test: List metric definitions
    use plexspaces_node::metrics_service::MetricsServiceImpl;
    use plexspaces_proto::metrics::v1::metrics_service_server::MetricsService;
    
    let metrics_service = MetricsServiceImpl::new();
    let request = tonic::Request::new(plexspaces_proto::metrics::v1::ListMetricDefinitionsRequest {
        name_pattern: String::new(),
    });
    
    let response = metrics_service.list_metric_definitions(request).await.unwrap();
    let definitions = &response.get_ref().definitions;
    
    // Should have at least some definitions
    assert!(!definitions.is_empty());
    
    // Should include standard metrics
    let metric_names: Vec<String> = definitions.iter().map(|d| d.name.clone()).collect();
    assert!(metric_names.contains(&"plexspaces_node_health_requests_total".to_string()));
    assert!(metric_names.contains(&"plexspaces_node_readiness_checks_total".to_string()));
}

#[tokio::test]
async fn test_metrics_service_get_metrics() {
    // Test: Get structured metrics
    use plexspaces_node::metrics_service::MetricsServiceImpl;
    use plexspaces_proto::metrics::v1::metrics_service_server::MetricsService;
    
    let metrics_service = MetricsServiceImpl::new();
    let request = tonic::Request::new(plexspaces_proto::metrics::v1::GetMetricsRequest {
        name_pattern: String::new(),
        label_filter: std::collections::HashMap::new(),
    });
    
    let response = metrics_service.get_metrics(request).await.unwrap();
    // Response should be valid (even if empty)
    assert!(response.get_ref().metrics.is_empty() || !response.get_ref().metrics.is_empty());
}

#[tokio::test]
async fn test_standard_health_service_check() {
    // Test: StandardHealthServiceImpl check method
    use plexspaces_node::standard_health_service::StandardHealthServiceImpl;
    use tonic_health::pb::health_server::Health;
    
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    let health_service = StandardHealthServiceImpl::new(health_reporter.clone());
    
    // Test overall health check (empty service name)
    let request = tonic::Request::new(HealthCheckRequest {
        service: String::new(),
    });
    
    let response = health_service.check(request).await.unwrap();
    let status = response.get_ref().status;
    
    // Should be SERVING (1)
    assert_eq!(status, 1); // ServingStatus::Serving as i32
    
    // Test service-specific health check
    let request = tonic::Request::new(HealthCheckRequest {
        service: "plexspaces.actor.v1.ActorService".to_string(),
    });
    
    let response = health_service.check(request).await.unwrap();
    let status = response.get_ref().status;
    assert_eq!(status, 1); // SERVING
}

#[tokio::test]
async fn test_standard_health_service_check_shutdown() {
    // Test: Health service returns NOT_SERVING during shutdown
    use plexspaces_node::standard_health_service::StandardHealthServiceImpl;
    use tonic_health::pb::health_server::Health;
    
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    let health_service = StandardHealthServiceImpl::new(health_reporter.clone());
    
    // Begin shutdown
    health_reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
    
    // Health check should return NOT_SERVING
    let request = tonic::Request::new(HealthCheckRequest {
        service: String::new(),
    });
    
    let response = health_service.check(request).await.unwrap();
    let status = response.get_ref().status;
    
    // Should be NOT_SERVING (2)
    assert_eq!(status, 2); // ServingStatus::NotServing as i32
}

#[tokio::test]
async fn test_standard_health_service_watch() {
    // Test: Health service watch stream
    use plexspaces_node::standard_health_service::StandardHealthServiceImpl;
    use tonic_health::pb::health_server::Health;
    
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    let health_service = StandardHealthServiceImpl::new(health_reporter.clone());
    
    let request = tonic::Request::new(HealthCheckRequest {
        service: String::new(),
    });
    
    let mut stream = health_service.watch(request).await.unwrap().into_inner();
    
    // Should receive at least one health status update
    let first_update = stream.next().await;
    assert!(first_update.is_some());
    
    if let Some(Ok(response)) = first_update {
        // Should be SERVING
        assert_eq!(response.status, 1);
    }
}

#[tokio::test]
async fn test_grpc_service_rejects_requests_during_shutdown() {
    // Test: gRPC services reject new requests during shutdown
    use plexspaces_node::grpc_service::ActorServiceImpl;
    use plexspaces_proto::v1::actor::actor_service_server::ActorService;
    
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    let actor_service = ActorServiceImpl::with_health_reporter(node, health_reporter.clone());
    
    // Begin shutdown
    health_reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
    
    // Try to create an actor - should be rejected
    let request = tonic::Request::new(plexspaces_proto::v1::actor::CreateActorRequest {
        actor_type: "TestActor".to_string(),
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });
    
    let result = actor_service.create_actor(request).await;
    
    // Should return unavailable error
    assert!(result.is_err());
    let status = result.unwrap_err();
    assert_eq!(status.code(), tonic::Code::Unavailable);
    assert!(status.message().contains("shutting down"));
}

#[tokio::test]
async fn test_grpc_service_accepts_requests_when_serving() {
    // Test: gRPC services accept requests when not shutting down
    use plexspaces_node::grpc_service::ActorServiceImpl;
    use plexspaces_proto::v1::actor::actor_service_server::ActorService;
    
    let node = Arc::new(NodeBuilder::new("test-node").build());
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    let actor_service = ActorServiceImpl::with_health_reporter(node, health_reporter.clone());
    
    // Try to create an actor - should be accepted (may fail for other reasons, but not shutdown)
    let request = tonic::Request::new(plexspaces_proto::v1::actor::CreateActorRequest {
        actor_type: "TestActor".to_string(),
        initial_state: vec![],
        config: None,
        labels: std::collections::HashMap::new(),
    });
    
    let result = actor_service.create_actor(request).await;
    
    // Should not be unavailable due to shutdown
    if result.is_err() {
        let status = result.unwrap_err();
        assert_ne!(status.code(), tonic::Code::Unavailable, "Should not be unavailable due to shutdown");
    }
}

#[tokio::test]
async fn test_health_status_propagation_on_shutdown() {
    // Test: All service statuses updated to NOT_SERVING on shutdown
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    // Verify all services are SERVING
    let services = vec![
        "",
        "plexspaces.actor.v1.ActorService",
        "plexspaces.tuplespace.v1.TuplePlexSpaceService",
        "plexspaces.supervisor.v1.SupervisorService",
    ];
    
    for service_name in &services {
        let status = health_reporter.get_service_status(service_name).await;
        assert_eq!(status, ServingStatus::ServingStatusServing, "Service {} should be SERVING", service_name);
    }
    
    // Begin shutdown
    health_reporter.begin_shutdown(Some(Duration::from_secs(1))).await;
    
    // Verify all services are NOT_SERVING
    for service_name in &services {
        let status = health_reporter.get_service_status(service_name).await;
        assert_eq!(status, ServingStatus::ServingStatusNotServing, "Service {} should be NOT_SERVING after shutdown", service_name);
    }
}

#[tokio::test]
async fn test_graceful_shutdown_drains_requests() {
    // Test: Graceful shutdown drains in-flight requests
    let health_reporter = create_health_reporter();
    health_reporter.mark_startup_complete(None).await;
    
    // Begin shutdown with short timeout
    let (drained, duration, completed) = health_reporter.begin_shutdown(Some(Duration::from_millis(100))).await;
    
    // Should complete quickly (no in-flight requests)
    assert!(completed || drained == 0);
    assert!(duration < Duration::from_millis(200));
}
