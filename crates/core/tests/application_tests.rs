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

    async fn spawn_actor(
        &self,
        actor_id: String,
        _behavior: Box<dyn plexspaces_core::ActorBehavior>,
        _namespace: String,
    ) -> Result<String, ApplicationError> {
        self.spawned_actors.lock().unwrap().push(actor_id.clone());
        Ok(actor_id)
    }

    async fn stop_actor(&self, actor_id: &str) -> Result<(), ApplicationError> {
        self.stopped_actors.lock().unwrap().push(actor_id.to_string());
        Ok(())
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

#[tokio::test]
async fn test_application_node_spawn_actor() {
    let spawned = Arc::new(std::sync::Mutex::new(Vec::new()));
    let node = Arc::new(MockNode {
        id: "test-node".to_string(),
        addr: "0.0.0.0:9000".to_string(),
        spawned_actors: spawned.clone(),
        stopped_actors: Arc::new(std::sync::Mutex::new(Vec::new())),
    });

    // Create a dummy behavior
    struct DummyBehavior;
    #[async_trait]
    impl plexspaces_core::ActorBehavior for DummyBehavior {
        async fn handle_message(&mut self, _ctx: &plexspaces_core::ActorContext, _msg: plexspaces_mailbox::Message) -> Result<(), plexspaces_core::BehaviorError> {
            Ok(())
        }
        fn behavior_type(&self) -> plexspaces_core::BehaviorType {
            plexspaces_core::BehaviorType::GenServer
        }
    }

    let behavior: Box<dyn plexspaces_core::ActorBehavior> = Box::new(DummyBehavior);
    let result = node.spawn_actor("actor-1".to_string(), behavior, "default".to_string()).await;

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "actor-1");
    let spawned_list = spawned.lock().unwrap();
    assert_eq!(spawned_list.len(), 1);
    assert_eq!(spawned_list[0], "actor-1");
}

#[tokio::test]
async fn test_application_node_stop_actor() {
    let stopped = Arc::new(std::sync::Mutex::new(Vec::new()));
    let node = Arc::new(MockNode {
        id: "test-node".to_string(),
        addr: "0.0.0.0:9000".to_string(),
        spawned_actors: Arc::new(std::sync::Mutex::new(Vec::new())),
        stopped_actors: stopped.clone(),
    });

    let result = node.stop_actor("actor-1").await;

    assert!(result.is_ok());
    let stopped_list = stopped.lock().unwrap();
    assert_eq!(stopped_list.len(), 1);
    assert_eq!(stopped_list[0], "actor-1");
}

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

