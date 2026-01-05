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

//! Integration tests for Dashboard Service

#[cfg(test)]
mod tests {
    use plexspaces_dashboard::DashboardServiceImpl;
    use plexspaces_proto::dashboard::v1::dashboard_service_server::DashboardService;
    use plexspaces_node::{Node, NodeBuilder};
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    use tonic::Request;

    async fn create_test_node() -> Arc<Node> {
        let node = NodeBuilder::new("test-node").build().await;
        Arc::new(node)
    }

    async fn create_test_service(node: Arc<Node>) -> DashboardServiceImpl {
        let service_locator = node.service_locator();
        
        // Initialize services (normally done in node.start())
        node.initialize_services().await.expect("Failed to initialize services");
        
        // Register NodeMetricsAccessor
        use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
        let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
        service_locator.register_service(metrics_accessor.clone()).await;
        let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
        service_locator.register_node_metrics_accessor(metrics_accessor_trait).await;
        
        // Metrics are updated in initialize_services() - no need to update manually
        
        // Ensure ApplicationManager is registered as both by-name and by-type
        use plexspaces_core::ApplicationManager;
        use plexspaces_core::service_locator::service_names;
        if let Some(app_manager) = service_locator.get_service_by_name::<ApplicationManager>(service_names::APPLICATION_MANAGER).await {
            // Also register as generic service for get_service() lookup
            service_locator.register_service(app_manager.clone()).await;
        }
        
        DashboardServiceImpl::new(service_locator)
    }

    #[tokio::test]
    async fn test_dashboard_service_creation() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        // Service should be created successfully
        assert!(true);
    }

    #[tokio::test]
    async fn test_get_summary_with_filters() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(plexspaces_proto::dashboard::v1::GetSummaryRequest {
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
    async fn test_get_nodes_with_pagination() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(plexspaces_proto::dashboard::v1::GetNodesRequest {
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
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(plexspaces_proto::dashboard::v1::GetApplicationsRequest {
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
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(plexspaces_proto::dashboard::v1::GetActorsRequest {
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
}




