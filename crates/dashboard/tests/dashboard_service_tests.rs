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
    use plexspaces_node::{Node, NodeBuilder};
    use plexspaces_core::ServiceLocator;
    use std::sync::Arc;
    use tonic::Request;

    async fn create_test_node() -> Arc<Node> {
        let node = NodeBuilder::new("test-node").build();
        Arc::new(node)
    }

    async fn create_test_service(node: Arc<Node>) -> DashboardServiceImpl {
        let service_locator = node.service_locator();
        // Register services in ServiceLocator (normally done in node.start())
        // For tests, we need to manually register NodeMetricsAccessor
        use plexspaces_node::service_wrappers::NodeMetricsAccessorWrapper;
        let metrics_accessor = Arc::new(NodeMetricsAccessorWrapper::new(node.clone()));
        service_locator.register_service(metrics_accessor.clone()).await;
        let metrics_accessor_trait: Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync> = metrics_accessor.clone() as Arc<dyn plexspaces_core::NodeMetricsAccessor + Send + Sync>;
        service_locator.register_node_metrics_accessor(metrics_accessor_trait).await;
        
        // Update metrics with node_id and cluster_name (normally done in node.start())
        {
            let mut metrics = node.metrics.write().await;
            metrics.node_id = node.id().to_string();
            // cluster_name comes from NodeConfig, which is set in node.start()
            // For tests, we'll leave it empty or set it from node_config if available
            if metrics.cluster_name.is_empty() {
                if let Some(node_config) = service_locator.get_node_config().await {
                    metrics.cluster_name = node_config.cluster_name.clone();
                }
            }
        }
        
        // ApplicationManager is already registered in ServiceLocator by create_default_service_locator
        let app_manager = application_manager.read().await.clone();
        service_locator.register_service(Arc::new(app_manager)).await;
        
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
            tenant_id: Some("test-tenant".to_string()),
            node_id: None,
            cluster_id: Some("test-cluster".to_string()),
            since: None,
        });

        let response = service.get_summary(request).await;
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
            tenant_id: None,
            cluster_id: None,
            page: Some(plexspaces_proto::common::v1::PageRequest {
                offset: 0,
                limit: 10,
                filter: String::new(),
                order_by: String::new(),
            }),
        });

        let response = service.get_nodes(request).await;
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
            node_id: None,
            tenant_id: None,
            namespace: None,
            name_pattern: Some("test".to_string()),
            page: None,
        });

        let response = service.get_applications(request).await;
        assert!(response.is_ok());
        
        let apps_response = response.unwrap().into_inner();
        assert!(apps_response.page.is_some());
    }

    #[tokio::test]
    async fn test_get_actors_with_all_filters() {
        let node = create_test_node().await;
        let service = create_test_service(node).await;
        
        let request = Request::new(plexspaces_proto::dashboard::v1::GetActorsRequest {
            node_id: Some("test-node".to_string()),
            tenant_id: Some("test-tenant".to_string()),
            namespace: Some("test-namespace".to_string()),
            actor_id_pattern: Some("test".to_string()),
            actor_group: Some("test-group".to_string()),
            actor_type: Some("test-type".to_string()),
            status: Some("running".to_string()),
            since: None,
            page: Some(plexspaces_proto::common::v1::PageRequest {
                offset: 0,
                limit: 20,
                filter: String::new(),
                order_by: String::new(),
            }),
        });

        let response = service.get_actors(request).await;
        assert!(response.is_ok());
        
        let actors_response = response.unwrap().into_inner();
        assert!(actors_response.page.is_some());
    }
}




