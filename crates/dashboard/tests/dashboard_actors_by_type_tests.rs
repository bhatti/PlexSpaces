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

//! Integration tests for dashboard "Actors by Type" functionality
//!
//! Tests verify that:
//! 1. Actors are registered with actor_type
//! 2. Dashboard shows actors grouped by type
//! 3. Both home page and node page show actors by type correctly

use plexspaces_dashboard::{DashboardServiceImpl, HealthReporterAccess};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_core::RequestContext;
use plexspaces_proto::dashboard::v1::{
    dashboard_service_server::DashboardService,
    GetSummaryRequest, GetNodeDashboardRequest,
};
use plexspaces_actor::get_actor_factory;
use std::sync::Arc;
use std::collections::HashMap;
use tonic::Request;

/// Helper to create a test node
async fn create_test_node(node_id: &str) -> Arc<Node> {
    let node = NodeBuilder::new(node_id).build().await;
    Arc::new(node)
}

/// Helper to create dashboard service from a node
async fn create_dashboard_service(node: Arc<Node>) -> DashboardServiceImpl {
    let service_locator = node.service_locator();
    
    // Initialize services
    node.initialize_services().await.expect("Failed to initialize services");
    
    // Register NodeMetricsAccessor
    use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
    let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
    service_locator.register_service(metrics_accessor.clone()).await;
    let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = 
        metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
    service_locator.register_node_metrics_accessor(metrics_accessor_trait).await;
    
    // Create HealthReporterAccess implementation
    use plexspaces_node::health_service::PlexSpacesHealthReporter;
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
            self.health_reporter.get_detailed_health(include_non_critical).await
        }
    }
    
    let health_reporter_access: Arc<dyn HealthReporterAccess + Send + Sync> = 
        Arc::new(HealthReporterAccessImpl { health_reporter });
    
    DashboardServiceImpl::with_health_reporter(service_locator, health_reporter_access)
}


#[tokio::test]
async fn test_actors_by_type_on_home_page() {
    // ARRANGE: Create node and spawn actors with different types
    let node = create_test_node("test-node-actors").await;
    let service_locator = node.service_locator();
    node.initialize_services().await.expect("Failed to initialize services");
    
    // Spawn actors with different types
    let ctx = RequestContext::internal();
    let actor_factory = get_actor_factory(service_locator.as_ref()).await
        .expect("ActorFactory should be available");
    
    // Spawn 3 Counter actors
    for i in 0..3 {
        let actor_id = format!("counter-{}@test-node-actors", i);
        actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "Counter",
            vec![],
            None,
            HashMap::new(),
            vec![],
        ).await.expect("Should spawn counter actor");
    }
    
    // Spawn 2 Worker actors
    for i in 0..2 {
        let actor_id = format!("worker-{}@test-node-actors", i);
        actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "Worker",
            vec![],
            None,
            HashMap::new(),
            vec![],
        ).await.expect("Should spawn worker actor");
    }
    
    // Spawn 1 GenServer actor
    let gen_server_id = "genserver-0@test-node-actors".to_string();
    actor_factory.spawn_actor(
        &ctx,
        &gen_server_id,
        "GenServer",
        vec![],
        None,
        HashMap::new(),
        vec![],
    ).await.expect("Should spawn GenServer actor");
    
    // Wait for actors to be registered and started
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // Verify actors are registered by checking ActorRegistry directly
    use plexspaces_core::service_locator::service_names;
    let actor_registry: Arc<plexspaces_core::ActorRegistry> = service_locator
        .get_service_by_name::<plexspaces_core::ActorRegistry>(service_names::ACTOR_REGISTRY)
        .await
        .expect("ActorRegistry should be available");
    
    let index = actor_registry.actor_type_index().read().await;
    eprintln!("DEBUG: actor_type_index has {} entries", index.len());
    for ((tenant, namespace, actor_type), actor_ids) in index.iter() {
        eprintln!("DEBUG: actor_type={}, tenant={}, namespace={}, count={}", 
            actor_type, tenant, namespace, actor_ids.len());
    }
    drop(index);
    
    // ACT: Get summary from dashboard
    // Note: create_dashboard_service creates a NEW dashboard service instance
    // but it should use the same ServiceLocator, so it should see the same ActorRegistry
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    
    let response = DashboardService::get_summary(&dashboard_service, request).await;
    assert!(response.is_ok(), "get_summary should succeed");
    
    let summary = response.unwrap().into_inner();
    
    eprintln!("DEBUG: summary.actors_by_type has {} entries: {:?}", 
        summary.actors_by_type.len(), summary.actors_by_type);
    
    // ASSERT: Verify actors_by_type is populated
    assert!(!summary.actors_by_type.is_empty(), 
        "Actors by type should not be empty after spawning actors (found {} entries)",
        summary.actors_by_type.len());
    
    // Verify specific actor types
    assert_eq!(summary.actors_by_type.get("Counter"), Some(&3u32),
        "Should have 3 Counter actors");
    assert_eq!(summary.actors_by_type.get("Worker"), Some(&2u32),
        "Should have 2 Worker actors");
    assert_eq!(summary.actors_by_type.get("GenServer"), Some(&1u32),
        "Should have 1 GenServer actor");
    
    // Verify total actor count
    let total_actors: u32 = summary.actors_by_type.values().sum();
    assert_eq!(total_actors, 6, "Should have 6 total actors");
}

#[tokio::test]
async fn test_actors_by_type_on_node_page() {
    // ARRANGE: Create node and spawn actors
    let node = create_test_node("test-node-actors-node").await;
    let service_locator = node.service_locator();
    node.initialize_services().await.expect("Failed to initialize services");
    
    // Spawn actors
    let ctx = RequestContext::internal();
    let actor_factory = get_actor_factory(service_locator.as_ref()).await
        .expect("ActorFactory should be available");
    
    // Spawn 2 Calculator actors
    for i in 0..2 {
        let actor_id = format!("calculator-{}@test-node-actors-node", i);
        actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "Calculator",
            vec![],
            None,
            HashMap::new(),
            vec![],
        ).await.expect("Should spawn calculator actor");
    }
    
    // Wait for actors to be registered and started
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    // ACT: Get node dashboard
    let dashboard_service = create_dashboard_service(node.clone()).await;
    let request = Request::new(GetNodeDashboardRequest {
        node_id: "test-node-actors-node".to_string(),
        since: None,
    });
    
    let response = DashboardService::get_node_dashboard(&dashboard_service, request).await;
    assert!(response.is_ok(), "get_node_dashboard should succeed");
    
    let dashboard = response.unwrap().into_inner();
    
    // ASSERT: Verify summary has actors_by_type
    assert!(dashboard.summary.is_some(), "Node dashboard should have summary");
    let summary = dashboard.summary.unwrap();
    
    assert!(!summary.actors_by_type.is_empty(),
        "Actors by type should not be empty on node page");
    assert_eq!(summary.actors_by_type.get("Calculator"), Some(&2u32),
        "Should have 2 Calculator actors on node page");
}

#[tokio::test]
async fn test_actors_by_type_empty_initially() {
    // ARRANGE: Create empty node (no actors)
    let node = create_test_node("test-node-empty-actors").await;
    let dashboard_service = create_dashboard_service(node.clone()).await;
    
    // ACT: Get summary
    let request = Request::new(GetSummaryRequest {
        tenant_id: String::new(),
        node_id: String::new(),
        cluster_id: String::new(),
        since: None,
    });
    
    let response = DashboardService::get_summary(&dashboard_service, request).await;
    assert!(response.is_ok(), "get_summary should succeed for empty node");
    
    let summary = response.unwrap().into_inner();
    
    // ASSERT: Actors by type should be empty (not None, but empty HashMap)
    assert!(summary.actors_by_type.is_empty(),
        "Actors by type should be empty when no actors are spawned");
}

