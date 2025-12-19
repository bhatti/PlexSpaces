// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Comprehensive tests for Application trait to improve coverage

use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use plexspaces_proto::v1::application::{ApplicationConfig, HealthStatus, ShutdownStrategy};
use async_trait::async_trait;
use std::sync::Arc;

struct MockNode {
    id: String,
    addr: String,
}

#[async_trait]
impl ApplicationNode for MockNode {
    fn id(&self) -> &str {
        &self.id
    }

    fn listen_addr(&self) -> &str {
        &self.addr
    }

}

struct TestApplication {
    name: String,
    version: String,
    start_fails: bool,
    stop_fails: bool,
    health_status: HealthStatus,
}

#[async_trait]
impl Application for TestApplication {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        if self.start_fails {
            Err(ApplicationError::StartupFailed("startup error".to_string()))
        } else {
            Ok(())
        }
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        if self.stop_fails {
            Err(ApplicationError::ShutdownFailed("shutdown error".to_string()))
        } else {
            Ok(())
        }
    }

    async fn health_check(&self) -> HealthStatus {
        self.health_status.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn test_application_start_failure() {
    let node = Arc::new(MockNode {
        id: "test-node".to_string(),
        addr: "0.0.0.0:9000".to_string(),
    });

    let mut app = TestApplication {
        name: "test-app".to_string(),
        stop_fails: false,
        version: "0.1.0".to_string(),
        start_fails: true,
        health_status: HealthStatus::HealthStatusHealthy,
    };

    let result = app.start(node).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ApplicationError::StartupFailed(msg) => {
            assert!(msg.contains("startup error"));
        },
        _ => panic!("Expected StartupFailed"),
    }
}

#[tokio::test]
async fn test_application_stop_failure() {
    let mut app = TestApplication {
        name: "test-app".to_string(),
        stop_fails: false,
        version: "0.1.0".to_string(),
        start_fails: false,
        health_status: HealthStatus::HealthStatusHealthy,
    };

    let result = app.stop().await;
    assert!(result.is_err());
    match result.unwrap_err() {
        ApplicationError::ShutdownFailed(msg) => {
            assert!(msg.contains("shutdown error"));
        },
        _ => panic!("Expected ShutdownFailed"),
    }
}

#[tokio::test]
async fn test_application_health_status_variants() {
    let healthy_app = TestApplication {
        name: "app".to_string(),
        version: "1.0".to_string(),
        start_fails: false,
        stop_fails: false,
        health_status: HealthStatus::HealthStatusHealthy,
    };
    assert_eq!(healthy_app.health_check().await, HealthStatus::HealthStatusHealthy);

    let degraded_app = TestApplication {
        name: "app".to_string(),
        version: "1.0".to_string(),
        start_fails: false,
        stop_fails: false,
        health_status: HealthStatus::HealthStatusDegraded,
    };
    assert_eq!(degraded_app.health_check().await, HealthStatus::HealthStatusDegraded);

    let unhealthy_app = TestApplication {
        name: "app".to_string(),
        version: "1.0".to_string(),
        start_fails: false,
        stop_fails: false,
        health_status: HealthStatus::HealthStatusUnhealthy,
    };
    assert_eq!(unhealthy_app.health_check().await, HealthStatus::HealthStatusUnhealthy);
}

// Note: spawn_actor and stop_actor methods removed from ApplicationNode trait
// Use ActorFactory directly via ServiceLocator instead

#[tokio::test]
async fn test_application_error_all_variants() {
    // Test all ApplicationError variants
    let errors = vec![
        ApplicationError::StartupFailed("startup".to_string()),
        ApplicationError::ShutdownFailed("shutdown".to_string()),
        ApplicationError::DependencyFailed("dependency".to_string()),
        ApplicationError::ConfigError("config".to_string()),
        ApplicationError::ShutdownTimeout(prost_types::Duration {
            seconds: 30,
            nanos: 0,
        }),
        ApplicationError::ActorSpawnFailed("actor".to_string(), "error".to_string()),
        ApplicationError::ActorStopFailed("actor".to_string(), "error".to_string()),
        ApplicationError::Other("other".to_string()),
    ];

    for error in errors {
        let error_msg = error.to_string();
        assert!(!error_msg.is_empty());
    }
}

