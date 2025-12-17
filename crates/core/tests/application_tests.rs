// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Additional tests for Application trait to improve coverage

use plexspaces_core::application::{Application, ApplicationNode, ApplicationError};
use plexspaces_proto::v1::application::{ApplicationConfig, HealthStatus, ShutdownStrategy};
use async_trait::async_trait;
use std::sync::Arc;

struct MockNode {
    id: String,
    addr: String,
    spawned_actors: Arc<std::sync::Mutex<Vec<String>>>,
    stopped_actors: Arc<std::sync::Mutex<Vec<String>>>,
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
    start_called: Arc<std::sync::Mutex<bool>>,
    stop_called: Arc<std::sync::Mutex<bool>>,
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
        *self.start_called.lock().unwrap() = true;
        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        *self.stop_called.lock().unwrap() = true;
        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        HealthStatus::HealthStatusHealthy
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[tokio::test]
async fn test_application_lifecycle() {
    let node = Arc::new(MockNode {
        id: "test-node".to_string(),
        addr: "0.0.0.0:9000".to_string(),
        spawned_actors: Arc::new(std::sync::Mutex::new(Vec::new())),
        stopped_actors: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    let start_called = Arc::new(std::sync::Mutex::new(false));
    let stop_called = Arc::new(std::sync::Mutex::new(false));

    let mut app = TestApplication {
        name: "test-app".to_string(),
        version: "0.1.0".to_string(),
        start_called: start_called.clone(),
        stop_called: stop_called.clone(),
    };

    assert_eq!(app.name(), "test-app");
    assert_eq!(app.version(), "0.1.0");

    app.start(node.clone()).await.unwrap();
    assert!(*start_called.lock().unwrap());

    assert_eq!(app.health_check().await, HealthStatus::HealthStatusHealthy);

    app.stop().await.unwrap();
    assert!(*stop_called.lock().unwrap());
}

// Note: spawn_actor and stop_actor methods removed from ApplicationNode trait
// Use ActorFactory directly via ServiceLocator instead

#[tokio::test]
async fn test_application_error_variants() {
    let errors = vec![
        ApplicationError::StartupFailed("startup error".to_string()),
        ApplicationError::ShutdownFailed("shutdown error".to_string()),
        ApplicationError::DependencyFailed("dependency error".to_string()),
        ApplicationError::ConfigError("config error".to_string()),
        ApplicationError::ShutdownTimeout(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        ApplicationError::ActorSpawnFailed("actor-1".to_string(), "spawn error".to_string()),
        ApplicationError::ActorStopFailed("actor-1".to_string(), "stop error".to_string()),
        ApplicationError::Other("other error".to_string()),
    ];

    for error in errors {
        let error_msg = error.to_string();
        assert!(!error_msg.is_empty());
    }
}

#[tokio::test]
async fn test_application_health_status() {
    let app = TestApplication {
        name: "test-app".to_string(),
        version: "0.1.0".to_string(),
        start_called: Arc::new(std::sync::Mutex::new(false)),
        stop_called: Arc::new(std::sync::Mutex::new(false)),
    };

    let health = app.health_check().await;
    assert_eq!(health, HealthStatus::HealthStatusHealthy);
}

