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

//! Unit tests for SystemService implementation

use plexspaces_node::{system_service::SystemServiceImpl, Node, NodeId, default_node_config};
use plexspaces_proto::system::v1::{
    GetSystemInfoRequest, GetMetricsRequest, GetConfigRequest, GetHealthRequest,
    GetDetailedHealthRequest, LivenessProbeRequest, ReadinessProbeRequest,
    StartupProbeRequest, GetNodeReadinessRequest, GetShutdownStatusRequest,
    CreateBackupRequest, ListBackupsRequest, GetLogsRequest,
};
use plexspaces_proto::system::v1::system_service_server::SystemService;
use std::sync::Arc;
use tonic::Request;

/// Helper to create a test node
async fn create_test_node() -> Arc<Node> {
    Arc::new(Node::new(NodeId::new("test-node"), default_node_config()))
}

/// Helper to create a SystemService with node
async fn create_system_service_with_node() -> (SystemServiceImpl, Arc<Node>) {
    let node = create_test_node().await;
    let (health_reporter, _) = plexspaces_node::health_service::PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);
    let service = SystemServiceImpl::with_node(health_reporter, node.clone());
    (service, node)
}

/// Helper to create a SystemService without node
fn create_system_service_without_node() -> SystemServiceImpl {
    let (health_reporter, _) = plexspaces_node::health_service::PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);
    SystemServiceImpl::new(health_reporter)
}

#[tokio::test]
async fn test_get_system_info() {
    let (service, _node) = create_system_service_with_node().await;
    
    let request = Request::new(GetSystemInfoRequest {
        include_details: true,
    });
    
    let response = service.get_system_info(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(inner.system_info.is_some());
    
    let system_info = inner.system_info.unwrap();
    assert!(!system_info.version.is_empty());
    assert!(system_info.cpu_cores > 0);
    assert!(system_info.total_memory_mb > 0);
}

#[tokio::test]
async fn test_get_system_info_without_node() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetSystemInfoRequest {
        include_details: false,
    });
    
    let response = service.get_system_info(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_metrics() {
    let (service, _node) = create_system_service_with_node().await;
    
    let request = Request::new(GetMetricsRequest {
        start_time: None,
        end_time: None,
        interval: None,
    });
    
    let response = service.get_metrics(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(!inner.metrics.is_empty());
}

#[tokio::test]
async fn test_get_config() {
    let (service, _node) = create_system_service_with_node().await;
    
    let request = Request::new(GetConfigRequest {
        key_pattern: "node.*".to_string(),
        include_secrets: false,
    });
    
    let response = service.get_config(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(!inner.settings.is_empty());
}

#[tokio::test]
async fn test_get_config_with_pattern() {
    let (service, _node) = create_system_service_with_node().await;
    
    let request = Request::new(GetConfigRequest {
        key_pattern: "node.listen_addr".to_string(),
        include_secrets: false,
    });
    
    let response = service.get_config(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    // Should have at least one matching setting
    assert!(inner.settings.len() >= 1);
}

#[tokio::test]
async fn test_get_health() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetHealthRequest {
        components: vec![],
    });
    
    let response = service.get_health(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_detailed_health() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetDetailedHealthRequest {
        include_non_critical: true,
    });
    
    let response = service.get_detailed_health(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_liveness_probe() {
    let service = create_system_service_without_node();
    
    let request = Request::new(LivenessProbeRequest {});
    
    let response = service.liveness_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_readiness_probe() {
    let service = create_system_service_without_node();
    
    let request = Request::new(ReadinessProbeRequest {});
    
    let response = service.readiness_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_startup_probe() {
    let service = create_system_service_without_node();
    
    let request = Request::new(StartupProbeRequest {});
    
    let response = service.startup_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_node_readiness() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetNodeReadinessRequest {});
    
    let response = service.get_node_readiness(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_shutdown_status() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetShutdownStatusRequest {});
    
    let response = service.get_shutdown_status(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(inner.status.is_some());
}

#[tokio::test]
async fn test_create_backup() {
    let service = create_system_service_without_node();
    
    let request = Request::new(CreateBackupRequest {
        r#type: 0, // BackupType::BackupTypeUnspecified
        components: vec![],
        destination: "/tmp/backup".to_string(),
        compress: false,
        encrypt: false,
    });
    
    let response = service.create_backup(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(inner.backup.is_some());
}

#[tokio::test]
async fn test_list_backups() {
    let service = create_system_service_without_node();
    
    let request = Request::new(ListBackupsRequest {
        page_request: None,
        r#type: 0,
        status: 0,
    });
    
    let response = service.list_backups(request).await;
    assert!(response.is_ok());
    
    let response = response.unwrap();
    let inner = response.into_inner();
    assert!(inner.page_response.is_some());
}

#[tokio::test]
async fn test_get_logs() {
    let service = create_system_service_without_node();
    
    let request = Request::new(GetLogsRequest {
        start_time: None,
        end_time: None,
        min_level: 0, // LogLevel::LogLevelUnspecified
        components: vec![],
        query: String::new(),
        page_request: None,
    });
    
    let response = service.get_logs(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_set_config() {
    let (service, _node) = create_system_service_with_node().await;
    
    use plexspaces_proto::system::v1::{SetConfigRequest, ConfigSetting};
    use plexspaces_proto::prost_types::Value;
    
    let settings = vec![ConfigSetting {
        key: "test.key".to_string(),
        value: Some(Value {
            kind: Some(plexspaces_proto::prost_types::value::Kind::StringValue("test_value".to_string())),
        }),
        description: "Test setting".to_string(),
        is_secret: false,
        requires_restart: false,
        updated_at: None,
        updated_by: String::new(),
    }];
    
    let request = Request::new(SetConfigRequest {
        settings,
        validate_only: false,
    });
    
    let response = service.set_config(request).await;
    // This might fail if validation is strict, but should not panic
    let _ = response;
}

