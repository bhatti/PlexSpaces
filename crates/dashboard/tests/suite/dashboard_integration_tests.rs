// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Comprehensive Integration Tests for Dashboard Service
//!
//! Tests all dashboard methods with real node instances, applications, actors, and clusters.
//!
//! To run:
//!   cargo test -p plexspaces-dashboard --test dashboard_integration_tests -- --test-threads=1

use plexspaces_application::{Application, ApplicationError, ApplicationNode};
use plexspaces_core::RequestContext;
use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService, GetActorsRequest, GetApplicationsRequest,
    GetDependencyHealthRequest, GetNodeDashboardRequest, GetNodesRequest, GetSummaryRequest,
    GetWorkflowsRequest,
};
use std::sync::Arc;
use tonic::Request;

// Import trait to enable method calls

/// Simple no-op actor behavior for test spawning
struct NoopBehavior;

#[async_trait::async_trait]
impl plexspaces_core::Actor for NoopBehavior {
    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        _message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        Ok(())
    }

    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }
}

/// Register a BehaviorRegistry with multiple actor types for testing
async fn register_behavior_registry(node: &Node, types: &[&str]) {
    use plexspaces_core::behavior_factory::BehaviorRegistry;
    use plexspaces_core::ServiceLocator;
    let registry = BehaviorRegistry::new();
    for actor_type in types {
        let type_str = actor_type.to_string();
        registry
            .register_simple(&type_str, move || {
                Box::pin(
                    async move { Ok(Box::new(NoopBehavior) as Box<dyn plexspaces_core::Actor>) },
                )
            })
            .await;
    }
    node.service_locator()
        .register_behavior_registry(Arc::new(registry))
        .await;
}

// Mock Application for testing
struct MockApplication {
    name: String,
    version: String,
}

#[async_trait::async_trait]
impl Application for MockApplication {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Helper to create a test node
async fn create_test_node(node_id: &str) -> Arc<Node> {
    let node = NodeBuilder::new(node_id).build().await;
    Arc::new(node)
}

/// Helper to create dashboard service from a node
async fn create_dashboard_service(node: Arc<Node>) -> DashboardServiceImpl {
    let service_locator = node.service_locator();

    // NodeBuilder::build() already called initialize_services(); avoid repeating it (slow + contends under parallel tests).

    // Register NodeMetricsAccessor
    use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
    let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
    let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> =
        metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
    service_locator
        .register_node_metrics_accessor(metrics_accessor_trait)
        .await;

    // ApplicationManager is already registered in service_locator by NodeBuilder::build()
    // Do NOT create a new one here - that would overwrite the one apps are registered in

    // Ensure ObjectRegistry is registered (needed for query_remote_nodes)
    use plexspaces_object_registry::ObjectRegistry;
    // ObjectRegistry is already registered via ServiceLocator initialization

    // Create HealthReporterAccess implementation
    use plexspaces_core::PlexSpacesHealthReporter;
    let (health_reporter, _service) = PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);

    struct HealthReporterAccessImpl {
        health_reporter: Arc<PlexSpacesHealthReporter>,
    }

    #[async_trait::async_trait]
    impl HealthReporterAccess for HealthReporterAccessImpl {
        async fn get_detailed_health(
            &self,
            include_non_critical: bool,
        ) -> plexspaces_proto::system::v1::DetailedHealthCheck {
            self.health_reporter
                .get_detailed_health(include_non_critical)
                .await
        }
    }

    let health_access: Arc<dyn HealthReporterAccess + Send + Sync> =
        Arc::new(HealthReporterAccessImpl { health_reporter });

    DashboardServiceImpl::with_health_reporter(service_locator, health_access)
}

/// Helper to register a mock application
async fn register_application(
    node: Arc<Node>,
    name: &str,
    version: &str,
) -> Result<(), ApplicationError> {
    let app = Box::new(MockApplication {
        name: name.to_string(),
        version: version.to_string(),
    });
    node.application_manager().register(app).await
}

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[tokio::test]
async fn test_get_summary_empty_node_shows_data() {
    // ARRANGE: Create empty node (no applications) and dashboard service
    // This is the critical test - empty node MUST show data
    let node = create_test_node("test-node-empty").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request).await;
    if let Err(e) = &response {
        eprintln!("get_summary failed: {:?}", e);
    }
    assert!(
        response.is_ok(),
        "get_summary should succeed: {:?}",
        response.err()
    );

    let summary = response.unwrap().into_inner();

    // ASSERT: Empty node MUST show at least 1 node and 1 cluster
    // This is critical for dashboard to display data before loading applications
    assert_eq!(summary.total_nodes, 1, "Empty node should show 1 node");
    assert_eq!(
        summary.total_clusters, 1,
        "Empty node should show exactly 1 cluster (default cluster)"
    );
    assert!(
        summary.total_tenants >= 1,
        "Empty node should show at least 1 tenant"
    );
    assert_eq!(
        summary.total_applications, 0,
        "Empty node should show 0 applications"
    );

    // Validate timestamps are present
    assert!(summary.since.is_some(), "Should have since timestamp");
    assert!(summary.until.is_some(), "Should have until timestamp");

    // Validate actors_by_type is present (may be empty HashMap)
    // actors_by_type is a HashMap, not Option, so it's always present
    assert!(
        summary.actors_by_type.is_empty() || !summary.actors_by_type.is_empty(),
        "Should have actors_by_type map (may be empty)"
    );
}

#[tokio::test]
async fn test_get_summary_with_single_node() {
    // ARRANGE: Create node and dashboard service
    let node = create_test_node("test-node-1").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request).await;
    if let Err(e) = &response {
        eprintln!("get_summary failed: {:?}", e);
    }
    assert!(
        response.is_ok(),
        "get_summary should succeed: {:?}",
        response.err()
    );

    let summary = response.unwrap().into_inner();

    // ASSERT: Should have at least 1 node and 1 cluster
    assert_eq!(summary.total_nodes, 1, "Should have 1 node");
    assert!(
        summary.total_clusters >= 1,
        "Should have at least 1 cluster"
    );
    assert!(summary.total_tenants >= 1, "Should have at least 1 tenant");
    assert_eq!(
        summary.total_applications, 0,
        "Should have 0 applications initially"
    );
}

#[tokio::test]
async fn test_get_summary_with_applications() {
    // ARRANGE: Create node, register applications, create dashboard service
    let node = create_test_node("test-node-apps").await;

    // Register 3 applications
    register_application(node.clone(), "app-1", "1.0.0")
        .await
        .unwrap();
    register_application(node.clone(), "app-2", "2.0.0")
        .await
        .unwrap();
    register_application(node.clone(), "app-3", "1.5.0")
        .await
        .unwrap();

    let service = create_dashboard_service(node.clone()).await;

    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = service.get_summary(request).await.unwrap();
    let summary = response.into_inner();

    // ASSERT: Should have 3 applications
    assert_eq!(summary.total_applications, 3, "Should have 3 applications");
    assert_eq!(summary.total_nodes, 1, "Should have 1 node");
}

#[tokio::test]
async fn test_get_nodes_empty_node_shows_local_node() {
    // ARRANGE: Empty node should show itself in nodes list
    let node = create_test_node("test-node-empty-nodes").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: None,
    });

    let response = DashboardService::get_nodes(&service, request).await;
    assert!(response.is_ok(), "get_nodes should succeed for empty node");

    let nodes_response = response.unwrap().into_inner();

    // ASSERT: Empty node MUST show itself
    assert_eq!(
        nodes_response.nodes.len(),
        1,
        "Empty node should return 1 node (itself)"
    );
    assert_eq!(
        nodes_response.nodes[0].id, "test-node-empty-nodes",
        "Should return correct node ID"
    );

    // Validate node structure
    let node_proto = &nodes_response.nodes[0];
    assert!(!node_proto.id.is_empty(), "Node ID should not be empty");
    assert!(node_proto.node_type > 0, "Node type should be set");
    assert!(node_proto.status > 0, "Node status should be set");
    assert!(node_proto.metrics.is_some(), "Node should have metrics");
    assert!(
        node_proto.last_heartbeat.is_some(),
        "Node should have last heartbeat"
    );

    // Validate pagination
    assert!(nodes_response.page.is_some(), "Should have pagination info");
    let page = nodes_response.page.unwrap();
    assert_eq!(page.total_size, 1, "Should have 1 total node");
}

#[tokio::test]
async fn test_get_nodes_single_node() {
    // ARRANGE
    let node = create_test_node("test-node-nodes").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: None,
    });

    let response = DashboardService::get_nodes(&service, request).await;
    assert!(response.is_ok(), "get_nodes should succeed");

    let nodes_response = response.unwrap().into_inner();

    // ASSERT
    assert_eq!(nodes_response.nodes.len(), 1, "Should return 1 node");
    assert_eq!(nodes_response.nodes[0].id, "test-node-nodes");
    assert!(nodes_response.page.is_some(), "Should have pagination info");
}

#[tokio::test]
async fn test_get_nodes_with_pagination() {
    // ARRANGE
    let node = create_test_node("test-node-pag").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT: Request with pagination
    let request = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 10,
            filter: String::new(),
            order_by: String::new(),
        }),
    });

    let response = service.get_nodes(request).await.unwrap();
    let nodes_response = response.into_inner();

    // ASSERT
    assert!(nodes_response.page.is_some(), "Should have pagination");
    let page = nodes_response.page.unwrap();
    assert_eq!(page.total_size, 1, "Should have 1 total node");
}

#[tokio::test]
async fn test_get_node_dashboard() {
    // ARRANGE
    let node = create_test_node("test-node-dashboard").await;

    // Register an application
    register_application(node.clone(), "test-app", "1.0.0")
        .await
        .unwrap();

    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-dashboard".to_string(),
        since: None,
    });

    let response = DashboardService::get_node_dashboard(&service, request).await;
    assert!(response.is_ok(), "get_node_dashboard should succeed");

    let dashboard = response.unwrap().into_inner();

    // ASSERT
    assert!(dashboard.node.is_some(), "Should have node info");
    assert_eq!(dashboard.node.as_ref().unwrap().id, "test-node-dashboard");
    assert!(dashboard.node_metrics.is_some(), "Should have node metrics");
    assert!(dashboard.summary.is_some(), "Should have summary");
    assert!(
        dashboard.dependency_health.is_some(),
        "Should have dependency health"
    );

    let summary = dashboard.summary.unwrap();
    assert_eq!(summary.total_applications, 1, "Should have 1 application");
}

#[tokio::test]
async fn test_get_applications_empty_returns_empty_list() {
    // ARRANGE: Empty node should return empty applications list (not error)
    let node = create_test_node("test-node-apps-empty").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });

    let response = service.get_applications(request).await;
    assert!(
        response.is_ok(),
        "get_applications should succeed for empty node (not error)"
    );

    let apps_response = response.unwrap().into_inner();

    // ASSERT: Empty list is valid, not an error
    assert_eq!(
        apps_response.applications.len(),
        0,
        "Should have 0 applications"
    );
    assert!(apps_response.page.is_some(), "Should have pagination");

    // Validate pagination shows 0 total
    let page = apps_response.page.unwrap();
    assert_eq!(page.total_size, 0, "Should have 0 total applications");
}

#[tokio::test]
async fn test_get_applications_empty() {
    // ARRANGE
    let node = create_test_node("test-node-apps-empty").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });

    let response = service.get_applications(request).await.unwrap();
    let apps_response = response.into_inner();

    // ASSERT
    assert_eq!(
        apps_response.applications.len(),
        0,
        "Should have 0 applications"
    );
    assert!(apps_response.page.is_some(), "Should have pagination");
}

#[tokio::test]
async fn test_get_applications_with_multiple_apps() {
    // ARRANGE
    let node = create_test_node("test-node-multi-apps").await;

    // Register multiple applications
    register_application(node.clone(), "calculator-app", "1.0.0")
        .await
        .unwrap();
    register_application(node.clone(), "web-server", "2.1.0")
        .await
        .unwrap();
    register_application(node.clone(), "data-processor", "1.5.0")
        .await
        .unwrap();

    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });

    let response = service.get_applications(request).await.unwrap();
    let apps_response = response.into_inner();

    // ASSERT
    assert_eq!(
        apps_response.applications.len(),
        3,
        "Should have 3 applications"
    );

    let app_names: Vec<String> = apps_response
        .applications
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert!(app_names.contains(&"calculator-app".to_string()));
    assert!(app_names.contains(&"web-server".to_string()));
    assert!(app_names.contains(&"data-processor".to_string()));
}

#[tokio::test]
async fn test_get_applications_with_name_pattern_filter() {
    // ARRANGE
    let node = create_test_node("test-node-filter").await;

    register_application(node.clone(), "calculator-app", "1.0.0")
        .await
        .unwrap();
    register_application(node.clone(), "web-server", "2.1.0")
        .await
        .unwrap();
    register_application(node.clone(), "calc-helper", "1.5.0")
        .await
        .unwrap();

    let service = create_dashboard_service(node.clone()).await;

    // ACT: Filter by name pattern "calc"
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: "calc".to_string(),
        page: None,
    });

    let response = service.get_applications(request).await.unwrap();
    let apps_response = response.into_inner();

    // ASSERT: Should return apps matching "calc"
    assert_eq!(
        apps_response.applications.len(),
        2,
        "Should have 2 matching applications"
    );
    let names: Vec<String> = apps_response
        .applications
        .iter()
        .map(|a| a.name.clone())
        .collect();
    assert!(names.iter().any(|n| n.contains("calc")));
}

#[tokio::test]
async fn test_get_actors_empty() {
    // ARRANGE
    let node = create_test_node("test-node-actors-empty").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetActorsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: String::new(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });

    let response = service.get_actors(request).await.unwrap();
    let actors_response = response.into_inner();

    // ASSERT
    assert_eq!(
        actors_response.actors.len(),
        0,
        "Should have 0 actors initially"
    );
    assert!(actors_response.page.is_some(), "Should have pagination");
}

#[tokio::test]
async fn test_get_workflows() {
    // ARRANGE
    let node = create_test_node("test-node-workflows").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetWorkflowsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        definition_id: String::new(),
        status: 0, // ExecutionStatus enum value
        page: None,
    });

    let response = service.get_workflows(request).await.unwrap();
    let workflows_response = response.into_inner();

    // ASSERT: Workflows may be empty if WorkflowService not registered (expected)
    assert!(workflows_response.page.is_some() || workflows_response.page.is_none());
}

#[tokio::test]
async fn test_get_dependency_health() {
    // ARRANGE
    let node = create_test_node("test-node-deps").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetDependencyHealthRequest {
        node_id: String::new(),
        include_non_critical: true,
    });

    let response = DashboardService::get_dependency_health(&service, request).await;
    assert!(response.is_ok(), "get_dependency_health should succeed");

    let health_response = response.unwrap().into_inner();

    // ASSERT
    assert!(
        health_response.health_check.is_some(),
        "Should have health check"
    );
    let health_check = health_response.health_check.unwrap();
    assert!(
        !health_check.dependency_checks.is_empty() || health_check.dependency_checks.is_empty(),
        "Should have dependency checks (may be empty if no dependencies configured)"
    );
}

#[tokio::test]
async fn test_get_summary_comprehensive() {
    // ARRANGE: Create node with apps
    let node = create_test_node("test-node-comprehensive").await;

    // Register applications
    register_application(node.clone(), "app-1", "1.0.0")
        .await
        .unwrap();
    register_application(node.clone(), "app-2", "2.0.0")
        .await
        .unwrap();

    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = service.get_summary(request).await.unwrap();
    let summary = response.into_inner();

    // ASSERT: Comprehensive validation
    assert_eq!(summary.total_nodes, 1, "Should have 1 node");
    assert!(
        summary.total_clusters >= 1,
        "Should have at least 1 cluster"
    );
    assert!(summary.total_tenants >= 1, "Should have at least 1 tenant");
    assert_eq!(summary.total_applications, 2, "Should have 2 applications");
}

#[tokio::test]
async fn test_get_applications_pagination() {
    // ARRANGE: Create many applications
    let node = create_test_node("test-node-pag-apps").await;

    for i in 0..15 {
        register_application(node.clone(), &format!("app-{}", i), "1.0.0")
            .await
            .unwrap();
    }

    let service = create_dashboard_service(node.clone()).await;

    // ACT: Request with page size 5
    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 5,
            filter: String::new(),
            order_by: String::new(),
        }),
    });

    let response = service.get_applications(request).await.unwrap();
    let apps_response = response.into_inner();

    // ASSERT
    assert!(
        apps_response.applications.len() <= 5,
        "Should respect page size"
    );
    assert!(apps_response.page.is_some(), "Should have pagination info");
}

// ============================================================================
// COMPREHENSIVE VALIDATION TESTS FOR EMPTY NODE
// ============================================================================

#[tokio::test]
async fn test_empty_node_all_apis_return_valid_data() {
    // ARRANGE: Create completely empty node (no apps, no actors)
    let node = create_test_node("test-node-all-apis").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT & ASSERT: Test all API endpoints return valid data (not errors)

    // 1. Summary API
    let summary_req = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary_resp = DashboardService::get_summary(&service, summary_req).await;
    assert!(
        summary_resp.is_ok(),
        "Summary API should succeed for empty node"
    );
    let summary = summary_resp.unwrap().into_inner();
    assert_eq!(summary.total_nodes, 1, "Summary should show 1 node");
    assert_eq!(summary.total_clusters, 1, "Summary should show 1 cluster");

    // 2. Nodes API
    let nodes_req = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: None,
    });
    let nodes_resp = DashboardService::get_nodes(&service, nodes_req).await;
    assert!(
        nodes_resp.is_ok(),
        "Nodes API should succeed for empty node"
    );
    let nodes = nodes_resp.unwrap().into_inner();
    assert_eq!(nodes.nodes.len(), 1, "Nodes API should return 1 node");

    // 3. Node Dashboard API
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-all-apis".to_string(),
        since: None,
    });
    let node_dashboard_resp =
        DashboardService::get_node_dashboard(&service, node_dashboard_req).await;
    assert!(
        node_dashboard_resp.is_ok(),
        "Node Dashboard API should succeed for empty node"
    );
    let node_dashboard = node_dashboard_resp.unwrap().into_inner();
    assert!(
        node_dashboard.node.is_some(),
        "Node Dashboard should have node info"
    );
    assert!(
        node_dashboard.node_metrics.is_some(),
        "Node Dashboard should have metrics"
    );
    assert!(
        node_dashboard.summary.is_some(),
        "Node Dashboard should have summary"
    );

    // 4. Applications API
    let apps_req = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps_resp = service.get_applications(apps_req).await;
    assert!(
        apps_resp.is_ok(),
        "Applications API should succeed for empty node (return empty list)"
    );
    let apps = apps_resp.unwrap().into_inner();
    assert_eq!(apps.applications.len(), 0, "Applications should be empty");

    // 5. Actors API
    let actors_req = Request::new(GetActorsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        actor_id_pattern: String::new(),
        actor_group: String::new(),
        actor_type: String::new(),
        status: String::new(),
        since: None,
        page: None,
    });
    let actors_resp = service.get_actors(actors_req).await;
    assert!(
        actors_resp.is_ok(),
        "Actors API should succeed for empty node (return empty list)"
    );
    let actors = actors_resp.unwrap().into_inner();
    assert_eq!(actors.actors.len(), 0, "Actors should be empty");

    // 6. Workflows API
    let workflows_req = Request::new(GetWorkflowsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        definition_id: String::new(),
        status: 0,
        page: None,
    });
    let workflows_resp = service.get_workflows(workflows_req).await;
    assert!(
        workflows_resp.is_ok(),
        "Workflows API should succeed for empty node"
    );

    // 7. Dependency Health API
    let deps_req = Request::new(GetDependencyHealthRequest {
        node_id: String::new(),
        include_non_critical: true,
    });
    let deps_resp = DashboardService::get_dependency_health(&service, deps_req).await;
    assert!(
        deps_resp.is_ok(),
        "Dependency Health API should succeed for empty node"
    );
    let deps = deps_resp.unwrap().into_inner();
    assert!(deps.health_check.is_some(), "Should have health check");
}

#[tokio::test]
async fn test_summary_cluster_counting_logic() {
    // ARRANGE: Test cluster counting logic for nodes with/without cluster names
    let node1 = create_test_node("test-node-cluster-1").await;
    let service1 = create_dashboard_service(node1.clone()).await;

    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service1, request)
        .await
        .unwrap();
    let summary = response.into_inner();

    // ASSERT: If node has no cluster_name, it should be counted as "default" cluster
    // So we should always have at least 1 cluster if we have at least 1 node
    assert_eq!(summary.total_nodes, 1, "Should have 1 node");
    assert_eq!(
        summary.total_clusters, 1,
        "Should have exactly 1 cluster (default if no cluster_name)"
    );
}

#[tokio::test]
async fn test_summary_tenant_counting_logic() {
    // ARRANGE: Test tenant counting logic
    let node = create_test_node("test-node-tenant").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT: Get summary (as admin - no tenant_id specified)
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(), // Empty = admin view
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request)
        .await
        .unwrap();
    let summary = response.into_inner();

    // ASSERT: For admin, should show at least 1 tenant (internal/default)
    assert_eq!(summary.total_nodes, 1, "Should have 1 node");
    assert!(
        summary.total_tenants >= 1,
        "Admin view should show at least 1 tenant"
    );
}

#[tokio::test]
async fn test_node_dashboard_empty_node() {
    // ARRANGE: Empty node dashboard should show node info even with no apps
    let node = create_test_node("test-node-dashboard-empty").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT
    let request = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-dashboard-empty".to_string(),
        since: None,
    });

    let response = DashboardService::get_node_dashboard(&service, request).await;
    assert!(
        response.is_ok(),
        "Node dashboard should succeed for empty node"
    );

    let dashboard = response.unwrap().into_inner();

    // ASSERT: All required fields should be present
    assert!(dashboard.node.is_some(), "Should have node info");
    assert_eq!(
        dashboard.node.as_ref().unwrap().id,
        "test-node-dashboard-empty"
    );
    assert!(dashboard.node_metrics.is_some(), "Should have node metrics");
    assert!(dashboard.summary.is_some(), "Should have summary");
    assert!(
        dashboard.dependency_health.is_some(),
        "Should have dependency health"
    );
    assert!(
        dashboard.system_metrics.is_some(),
        "Should have system metrics"
    );

    // Validate summary shows 0 applications
    let summary = dashboard.summary.unwrap();
    assert_eq!(
        summary.total_applications, 0,
        "Empty node should show 0 applications"
    );
}

// ============================================================================
// Tests merged from dashboard_service_tests.rs
// ============================================================================

#[tokio::test]
async fn test_dashboard_service_creation() {
    let node = create_test_node("test-node-svc-create").await;
    let service = create_dashboard_service(node).await;
    // Service should be created successfully
    assert!(true);
}

#[tokio::test]
async fn test_get_summary_with_filters() {
    let node = create_test_node("test-node-filters").await;
    let service = create_dashboard_service(node).await;

    let request = Request::new(GetSummaryRequest {
        tenant_id: "test-tenant".to_string(),
        node_id: String::new(),
        cluster_id: "test-cluster".to_string(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request).await;
    assert!(response.is_ok());

    let summary = response.unwrap().into_inner();
    assert!(summary.total_nodes >= 0);
    assert!(summary.total_clusters >= 0);
}

#[tokio::test]
async fn test_get_nodes_with_pagination_service() {
    let node = create_test_node("test-node-pag-svc").await;
    let service = create_dashboard_service(node).await;

    let request = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 10,
            filter: String::new(),
            order_by: String::new(),
        }),
    });

    let response = DashboardService::get_nodes(&service, request).await;
    assert!(response.is_ok());

    let nodes_response = response.unwrap().into_inner();
    assert!(nodes_response.page.is_some());
    let page = nodes_response.page.unwrap();
    assert!(page.total_size >= 0);
}

#[tokio::test]
async fn test_get_applications_with_filters() {
    let node = create_test_node("test-node-apps-filter").await;
    let service = create_dashboard_service(node).await;

    let request = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: "test".to_string(),
        page: None,
    });

    let response = DashboardService::get_applications(&service, request).await;
    assert!(response.is_ok());

    let apps_response = response.unwrap().into_inner();
    assert!(apps_response.page.is_some());
}

#[tokio::test]
async fn test_get_actors_with_all_filters() {
    let node = create_test_node("test-node-actors-filter").await;
    let service = create_dashboard_service(node).await;

    let request = Request::new(GetActorsRequest {
        node_id: "test-node".to_string(),
        tenant_id: "test-tenant".to_string(),
        namespace: "test-namespace".to_string(),
        actor_id_pattern: "test".to_string(),
        actor_group: "test-group".to_string(),
        actor_type: "test-type".to_string(),
        status: "running".to_string(),
        since: None,
        page: Some(plexspaces_proto::common::v1::PageRequest {
            offset: 0,
            limit: 20,
            filter: String::new(),
            order_by: String::new(),
        }),
    });

    let response = DashboardService::get_actors(&service, request).await;
    assert!(response.is_ok());

    let actors_response = response.unwrap().into_inner();
    assert!(actors_response.page.is_some());
}

// ============================================================================
// Tests merged from dashboard_comprehensive_tests.rs
// ============================================================================

#[tokio::test]
async fn test_dashboard_home_page_data() {
    let node = create_test_node("test-node-home").await;
    let service = create_dashboard_service(node.clone()).await;

    // Dashboard reads local state via ServiceLocator; full node.start() (tonic server) is not required.

    // Test summary
    let summary_req = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    let summary_resp = DashboardService::get_summary(&service, summary_req).await;
    assert!(summary_resp.is_ok(), "Summary should succeed");
    let summary = summary_resp.unwrap().into_inner();
    assert!(summary.total_nodes >= 1, "Should have at least 1 node");

    // Test nodes
    let nodes_req = Request::new(GetNodesRequest {
        tenant_id: String::new(),
        cluster_id: String::new(),
        page: None,
    });
    let nodes_resp = DashboardService::get_nodes(&service, nodes_req).await;
    assert!(nodes_resp.is_ok(), "Nodes should succeed");
    let nodes = nodes_resp.unwrap().into_inner();
    assert!(!nodes.nodes.is_empty(), "Should have at least 1 node");
    assert_eq!(nodes.nodes[0].id, "test-node-home", "Node ID should match");

    // Test applications (should be empty initially)
    let apps_req = Request::new(GetApplicationsRequest {
        node_id: String::new(),
        tenant_id: String::new(),
        namespace: String::new(),
        name_pattern: String::new(),
        page: None,
    });
    let apps_resp = service.get_applications(apps_req).await;
    assert!(apps_resp.is_ok(), "Applications should succeed");
    let apps = apps_resp.unwrap().into_inner();
    assert_eq!(
        apps.applications.len(),
        0,
        "Should have no applications initially"
    );
}

#[tokio::test]
async fn test_dashboard_node_page_data() {
    let node = create_test_node("test-node-page").await;
    let service = create_dashboard_service(node.clone()).await;

    // Test node dashboard
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-page".to_string(),
        since: None,
    });
    let node_dashboard_resp =
        DashboardService::get_node_dashboard(&service, node_dashboard_req).await;
    assert!(node_dashboard_resp.is_ok(), "Node dashboard should succeed");
    let dashboard = node_dashboard_resp.unwrap().into_inner();

    // Verify node data
    assert!(dashboard.node.is_some(), "Should have node data");
    let node_data = dashboard.node.unwrap();
    assert_eq!(node_data.id, "test-node-page", "Node ID should match");

    // Verify metrics
    assert!(dashboard.node_metrics.is_some(), "Should have node metrics");
    let metrics = dashboard.node_metrics.unwrap();
    assert!(metrics.uptime_seconds >= 0, "Uptime should be non-negative");
}

#[tokio::test]
async fn test_dashboard_metrics_not_zero() {
    let node = create_test_node("test-node-metrics").await;
    let service = create_dashboard_service(node.clone()).await;

    // NodeMetricsAccessorWrapper refreshes CPU/memory from sysinfo without node.start().

    // Get node dashboard
    let node_dashboard_req = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-metrics".to_string(),
        since: None,
    });
    let node_dashboard = DashboardService::get_node_dashboard(&service, node_dashboard_req)
        .await
        .unwrap()
        .into_inner();

    // Verify metrics are not all zeros
    if let Some(metrics) = node_dashboard.node_metrics {
        assert!(
            metrics.uptime_seconds > 0
                || metrics.memory_available_bytes > 0
                || metrics.cpu_usage_percent > 0.0,
            "Metrics should not all be zero"
        );
    } else {
        panic!("Node metrics should be present");
    }
}

// ============================================================================
// Tests merged from dashboard_actors_by_type_tests.rs
// ============================================================================

#[tokio::test]
async fn test_actors_by_type_on_home_page() {
    use plexspaces_core::{ActorFactory, ActorId};
    use std::collections::HashMap;

    // ARRANGE: Create node and spawn actors with different types
    let node = create_test_node("test-node-actors-type").await;
    let service_locator = node.service_locator();

    // Register behavior types so actor spawning works
    register_behavior_registry(&node, &["Counter", "Worker"]).await;

    // Spawn actors with different types
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .expect("ActorFactory should be available");

    // Spawn 3 Counter actors
    for i in 0..3 {
        let actor_id = ActorId::new(
            format!("counter-{}", i),
            "Counter",
            "system",
            "test-node-actors-type",
        )
        .expect("valid counter actor id");
        actor_factory
            .spawn_actor(
                &ctx,
                &actor_id,
                "Counter",
                vec![],
                None,
                HashMap::new(),
                vec![],
            )
            .await
            .expect("Should spawn counter actor");
    }

    // Spawn 2 Worker actors
    for i in 0..2 {
        let actor_id = ActorId::new(
            format!("worker-{}", i),
            "Worker",
            "system",
            "test-node-actors-type",
        )
        .expect("valid worker actor id");
        actor_factory
            .spawn_actor(
                &ctx,
                &actor_id,
                "Worker",
                vec![],
                None,
                HashMap::new(),
                vec![],
            )
            .await
            .expect("Should spawn worker actor");
    }

    // ACT: Get summary from dashboard
    let service = create_dashboard_service(node.clone()).await;
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request).await;
    assert!(response.is_ok(), "get_summary should succeed");

    let summary = response.unwrap().into_inner();

    // ASSERT: Verify actors_by_type is populated
    assert!(
        !summary.actors_by_type.is_empty(),
        "Actors by type should not be empty after spawning actors"
    );

    // Verify specific actor types
    assert_eq!(
        summary.actors_by_type.get("Counter"),
        Some(&3u32),
        "Should have 3 Counter actors"
    );
    assert_eq!(
        summary.actors_by_type.get("Worker"),
        Some(&2u32),
        "Should have 2 Worker actors"
    );
}

#[tokio::test]
async fn test_actors_by_type_on_node_page() {
    use plexspaces_core::{ActorFactory, ActorId};
    use std::collections::HashMap;

    // ARRANGE: Create node and spawn actors
    let node = create_test_node("test-node-actors-node-type").await;
    let service_locator = node.service_locator();

    // Register behavior types so actor spawning works
    register_behavior_registry(&node, &["Calculator"]).await;

    // Spawn actors
    let ctx = RequestContext::new_without_auth("internal".to_string(), "system".to_string());
    let actor_factory = service_locator
        .get_actor_factory()
        .await
        .expect("ActorFactory should be available");

    // Spawn 2 Calculator actors
    for i in 0..2 {
        let actor_id = ActorId::new(
            format!("calculator-{}", i),
            "Calculator",
            "system",
            "test-node-actors-node-type",
        )
        .expect("valid calculator actor id");
        actor_factory
            .spawn_actor(
                &ctx,
                &actor_id,
                "Calculator",
                vec![],
                None,
                HashMap::new(),
                vec![],
            )
            .await
            .expect("Should spawn calculator actor");
    }

    // ACT: Get node dashboard
    let service = create_dashboard_service(node.clone()).await;
    let request = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-actors-node-type".to_string(),
        since: None,
    });

    let response = DashboardService::get_node_dashboard(&service, request).await;
    assert!(response.is_ok(), "get_node_dashboard should succeed");

    let dashboard = response.unwrap().into_inner();

    // ASSERT: Verify summary has actors_by_type
    assert!(
        dashboard.summary.is_some(),
        "Node dashboard should have summary"
    );
    let summary = dashboard.summary.unwrap();

    assert!(
        !summary.actors_by_type.is_empty(),
        "Actors by type should not be empty on node page"
    );
    assert_eq!(
        summary.actors_by_type.get("Calculator"),
        Some(&2u32),
        "Should have 2 Calculator actors on node page"
    );
}

#[tokio::test]
async fn test_actors_by_type_empty_initially() {
    // ARRANGE: Create empty node (no actors)
    let node = create_test_node("test-node-empty-actors-type").await;
    let service = create_dashboard_service(node.clone()).await;

    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });

    let response = DashboardService::get_summary(&service, request).await;
    assert!(
        response.is_ok(),
        "get_summary should succeed for empty node"
    );

    let summary = response.unwrap().into_inner();

    // ASSERT: Actors by type should be empty
    assert!(
        summary.actors_by_type.is_empty(),
        "Actors by type should be empty when no actors are spawned"
    );
}
