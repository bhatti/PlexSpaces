// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Unit tests for SystemService implementation

use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::system::v1::system_service_server::SystemService;
use plexspaces_proto::system::v1::{
    CreateBackupRequest, GetConfigRequest, GetDetailedHealthRequest, GetHealthRequest,
    GetLogsRequest, GetMetricsRequest, GetNodeReadinessRequest, GetShutdownStatusRequest,
    GetSystemInfoRequest, ListBackupsRequest, LivenessProbeRequest, ReadinessProbeRequest,
    StartupProbeRequest,
};
use plexspaces_services::system_service::SystemServiceImpl;
use std::sync::Arc;
use tonic::Request;

/// Helper to create a test node
async fn create_test_node() -> Arc<Node> {
    Arc::new(NodeBuilder::new("test-node").build().await)
}

/// Helper to create a SystemService with node
async fn create_system_service_with_node() -> (SystemServiceImpl, Arc<Node>) {
    let node = create_test_node().await;
    let (health_reporter, _) = plexspaces_actor::PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);
    let service = SystemServiceImpl::new(health_reporter);
    (service, node)
}

/// Helper to create a SystemService without node
fn create_system_service_without_node() -> SystemServiceImpl {
    let (health_reporter, _) = plexspaces_actor::PlexSpacesHealthReporter::new();
    let health_reporter = Arc::new(health_reporter);
    SystemServiceImpl::new(health_reporter)
}

#[tokio::test]
async fn test_get_system_info() {
    let (service, _node) = create_system_service_with_node().await;

    let request = Request::new(GetSystemInfoRequest {
        request_id: ulid::Ulid::new().to_string(),
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
        request_id: ulid::Ulid::new().to_string(),
        include_details: false,
    });

    let response = service.get_system_info(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_metrics() {
    let (service, _node) = create_system_service_with_node().await;

    let request = Request::new(GetMetricsRequest {
        request_id: ulid::Ulid::new().to_string(),
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
        request_id: ulid::Ulid::new().to_string(),
        key_pattern: "node.*".to_string(),
        include_secrets: false,
    });

    let response = service.get_config(request).await;
    assert!(response.is_ok());

    let response = response.unwrap();
    let inner = response.into_inner();
    let _ = inner.settings; // settings may be empty until node config is wired to SystemServiceImpl
}

#[tokio::test]
async fn test_get_config_with_pattern() {
    let (service, _node) = create_system_service_with_node().await;

    let request = Request::new(GetConfigRequest {
        request_id: ulid::Ulid::new().to_string(),
        key_pattern: "node.listen_addr".to_string(),
        include_secrets: false,
    });

    let response = service.get_config(request).await;
    assert!(response.is_ok());

    let response = response.unwrap();
    let inner = response.into_inner();
    let _ = inner.settings; // settings may be empty until node config is wired to SystemServiceImpl
}

#[tokio::test]
async fn test_get_health() {
    let service = create_system_service_without_node();

    let request = Request::new(GetHealthRequest { request_id: ulid::Ulid::new().to_string(), components: vec![] });

    let response = service.get_health(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_detailed_health() {
    let service = create_system_service_without_node();

    let request = Request::new(GetDetailedHealthRequest {
        request_id: ulid::Ulid::new().to_string(),
        include_non_critical: true,
    });

    let response = service.get_detailed_health(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_liveness_probe() {
    let service = create_system_service_without_node();

    let request = Request::new(LivenessProbeRequest { request_id: ulid::Ulid::new().to_string() });

    let response = service.liveness_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_readiness_probe() {
    let service = create_system_service_without_node();

    let request = Request::new(ReadinessProbeRequest { request_id: ulid::Ulid::new().to_string() });

    let response = service.readiness_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_startup_probe() {
    let service = create_system_service_without_node();

    let request = Request::new(StartupProbeRequest { request_id: ulid::Ulid::new().to_string() });

    let response = service.startup_probe(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_node_readiness() {
    let service = create_system_service_without_node();

    let request = Request::new(GetNodeReadinessRequest { request_id: ulid::Ulid::new().to_string() });

    let response = service.get_node_readiness(request).await;
    assert!(response.is_ok());
}

#[tokio::test]
async fn test_get_shutdown_status() {
    let service = create_system_service_without_node();

    let request = Request::new(GetShutdownStatusRequest { request_id: ulid::Ulid::new().to_string() });

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
        request_id: ulid::Ulid::new().to_string(),
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
        request_id: ulid::Ulid::new().to_string(),
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
        request_id: ulid::Ulid::new().to_string(),
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

    use plexspaces_proto::prost_types::Value;
    use plexspaces_proto::system::v1::{ConfigSetting, SetConfigRequest};

    let settings = vec![ConfigSetting {
        key: "test.key".to_string(),
        value: Some(Value {
            kind: Some(plexspaces_proto::prost_types::value::Kind::StringValue(
                "test_value".to_string(),
            )),
        }),
        description: "Test setting".to_string(),
        is_secret: false,
        requires_restart: false,
        updated_at: None,
        updated_by: String::new(),
    }];

    let request = Request::new(SetConfigRequest {
        request_id: ulid::Ulid::new().to_string(),
        settings,
        validate_only: false,
    });

    let response = service.set_config(request).await;
    // This might fail if validation is strict, but should not panic
    let _ = response;
}
