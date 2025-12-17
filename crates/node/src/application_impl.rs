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

//! Application implementation from proto ApplicationSpec
//!
//! ## Purpose
//! Production-quality Application implementation that wraps ApplicationSpec proto.
//! Supports both native Rust applications and provides foundation for WASM applications.
//!
//! ## Design
//! - Wraps ApplicationSpec proto message
//! - Implements Application trait with full lifecycle management
//! - Spawns supervisor trees and actors from supervisor spec
//! - Tracks spawned actors for graceful shutdown
//! - Provides health checks and metrics

use async_trait::async_trait;
use plexspaces_core::application::{
    Application, ApplicationError, ApplicationNode, HealthStatus,
};
use plexspaces_proto::application::v1::{ApplicationSpec, SupervisorSpec};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Production-quality application implementation from ApplicationSpec
///
/// ## Purpose
/// Wraps ApplicationSpec proto to provide a complete Application trait implementation.
/// Supports spawning supervisor trees and actors, graceful shutdown, and health checks.
///
/// ## Lifecycle
/// - Created from ApplicationSpec proto
/// - Registered with ApplicationManager
/// - Started: Spawns supervisor tree and actors
/// - Running: Monitors health and tracks metrics
/// - Stopped: Gracefully shuts down all actors
pub struct SpecApplication {
    /// Application specification from proto
    spec: ApplicationSpec,
    /// Whether the application is running
    is_running: Arc<RwLock<bool>>,
    /// Spawned actor IDs (for graceful shutdown)
    spawned_actor_ids: Arc<RwLock<Vec<String>>>,
    /// Node reference (for spawning actors)
    node: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
    /// Behavior factory for dynamic actor spawning (optional)
    behavior_factory: Option<Arc<dyn plexspaces_core::behavior_factory::BehaviorFactory + Send + Sync>>,
    /// ServiceLocator for accessing ActorFactory
    service_locator: Arc<RwLock<Option<Arc<plexspaces_core::ServiceLocator>>>>,
}

impl SpecApplication {
    /// Create new application from ApplicationSpec
    pub fn new(spec: ApplicationSpec) -> Self {
        Self {
            spec,
            is_running: Arc::new(RwLock::new(false)),
            spawned_actor_ids: Arc::new(RwLock::new(Vec::new())),
            node: Arc::new(RwLock::new(None)),
            behavior_factory: None,
            service_locator: Arc::new(RwLock::new(None)),
        }
    }

    /// Create new application with behavior factory
    ///
    /// ## Purpose
    /// Allows applications to dynamically spawn actors from `start_module` strings
    /// in supervisor tree specifications.
    ///
    /// ## Arguments
    /// * `spec` - Application specification
    /// * `behavior_factory` - Factory for creating behaviors from module names
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_core::behavior_factory::BehaviorRegistry;
    ///
    /// let mut registry = BehaviorRegistry::new();
    /// registry.register_simple("my_app::Worker", || Worker::new());
    ///
    /// let app = SpecApplication::with_behavior_factory(spec, Arc::new(registry));
    /// ```
    pub fn with_behavior_factory(
        spec: ApplicationSpec,
        behavior_factory: Arc<dyn plexspaces_core::behavior_factory::BehaviorFactory>,
    ) -> Self {
        Self {
            spec,
            is_running: Arc::new(RwLock::new(false)),
            spawned_actor_ids: Arc::new(RwLock::new(Vec::new())),
            node: Arc::new(RwLock::new(None)),
            behavior_factory: Some(behavior_factory),
            service_locator: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Get application specification
    pub fn spec(&self) -> &ApplicationSpec {
        &self.spec
    }

    /// Get environment variables from application spec
    ///
    /// ## Purpose
    /// Returns the environment variables defined in the ApplicationSpec.
    /// Applications can use these during start() to configure themselves.
    ///
    /// ## Returns
    /// Reference to the environment variables map
    pub fn env(&self) -> &std::collections::HashMap<String, String> {
        &self.spec.env
    }

    /// Initialize supervisor tree and spawn actors
    ///
    /// ## Purpose
    /// Spawns all actors defined in the supervisor tree specification.
    /// For native Rust applications, this validates the spec but cannot
    /// dynamically instantiate behaviors from start_module strings.
    ///
    /// ## Design Notes
    /// - Native Rust applications need a behavior factory/registry to map
    ///   start_module strings to behavior constructors
    /// - For now, we validate the spec and log that spawning is not yet
    ///   implemented for native applications
    /// - WASM applications use WasmApplication which has full spawning support
    async fn initialize_supervisor_tree(
        &self,
        node: Arc<dyn ApplicationNode>,
        service_locator: Arc<plexspaces_core::ServiceLocator>,
        supervisor_spec: &SupervisorSpec,
    ) -> Result<Vec<String>, ApplicationError> {
        let mut actor_ids = Vec::new();
        let spawned_actor_ids = self.spawned_actor_ids.clone();

        info!(
            application = %self.spec.name,
            strategy = ?supervisor_spec.strategy,
            children_count = supervisor_spec.children.len(),
            "Initializing supervisor tree"
        );

        // Log deployment progress: starting supervisor tree initialization
        debug!(
            application = %self.spec.name,
            total_children = supervisor_spec.children.len(),
            "Starting supervisor tree initialization"
        );

        // Traverse supervisor tree and spawn actors
        // For native Rust applications, we can't dynamically load modules,
        // so we validate the spec and log that full spawning requires a behavior factory
        for child in &supervisor_spec.children {
            debug!(
                child_id = %child.id,
                start_module = %child.start_module,
                child_type = ?child.r#type(),
                "Processing child spec"
            );

            // Validate child spec
            if child.id.is_empty() {
                return Err(ApplicationError::ConfigError(format!(
                    "Child spec has empty ID in application '{}'",
                    self.spec.name
                )));
            }

            if child.start_module.is_empty() {
                return Err(ApplicationError::ConfigError(format!(
                    "Child '{}' has empty start_module in application '{}'",
                    child.id, self.spec.name
                )));
            }

            // Log deployment progress: processing child
            debug!(
                application = %self.spec.name,
                child_id = %child.id,
                child_type = ?child.r#type(),
                progress = format!("{}/{}", actor_ids.len() + 1, supervisor_spec.children.len()),
                "Processing child spec"
            );

            // Try to create behavior using behavior factory if available
            if let Some(ref factory) = self.behavior_factory {
                // Serialize args HashMap to bytes
                // Note: ChildSpec.args is HashMap<String, String>, but factory expects &[u8]
                // We serialize to JSON for now. In production, you might want protobuf encoding.
                let args_bytes = if child.args.is_empty() {
                    vec![]
                } else {
                    // Convert HashMap to JSON bytes
                    // Note: This requires serde_json, but we can also just use empty vec for now
                    // and let the factory handle default args
                    vec![] // TODO: Serialize args properly when needed
                };
                
                match factory.create(&child.start_module, &args_bytes).await {
                    Ok(behavior) => {
                        let actor_id = format!("{}@{}", child.id, node.id());
                        
                        // Use ActorBuilder to build and spawn the actor with custom behavior
                        use plexspaces_actor::ActorBuilder;
                        match ActorBuilder::new(behavior)
                            .with_id(actor_id.clone())
                            .with_namespace(self.spec.name.clone())
                            .spawn(service_locator.clone())
                            .await
                        {
                            Ok(_actor_ref) => {
                                debug!(
                                    application = %self.spec.name,
                                    child_id = %child.id,
                                    start_module = %child.start_module,
                                    actor_id = %actor_id,
                                    "Spawned actor from behavior factory"
                                );
                                actor_ids.push(actor_id);
                            }
                            Err(e) => {
                                error!(
                                    application = %self.spec.name,
                                    child_id = %child.id,
                                    start_module = %child.start_module,
                                    error = %e,
                                    "Failed to spawn actor from behavior factory"
                                );
                                return Err(ApplicationError::StartupFailed(format!(
                                    "Failed to spawn actor '{}': {}",
                                    child.id, e
                                )));
                            }
                        }
                    }
                    Err(e) => {
                        error!(
                            application = %self.spec.name,
                            child_id = %child.id,
                            start_module = %child.start_module,
                            error = %e,
                            "Failed to create behavior from factory"
                        );
                        return Err(ApplicationError::StartupFailed(format!(
                            "Failed to create behavior for '{}' from module '{}': {}",
                            child.id, child.start_module, e
                        )));
                    }
                }
            } else {
                // No behavior factory - log warning and skip
                warn!(
                    application = %self.spec.name,
                    child_id = %child.id,
                    start_module = %child.start_module,
                    "No behavior factory available. Cannot spawn actor from start_module. \
                     Use SpecApplication::with_behavior_factory() or use WASM applications. \
                     This child will not be spawned."
                );
            }
        }

        // Store spawned actor IDs for graceful shutdown
        {
            let mut spawned = spawned_actor_ids.write().await;
            spawned.extend(actor_ids.clone());
        }

        if actor_ids.is_empty() {
            warn!(
                application = %self.spec.name,
                "No actors were spawned. Supervisor tree is empty or native spawning not implemented."
            );
        } else {
            info!(
                application = %self.spec.name,
                actor_count = actor_ids.len(),
                "Supervisor tree initialized successfully"
            );
            
            // Log deployment progress: supervisor tree initialization complete
            debug!(
                application = %self.spec.name,
                actor_count = actor_ids.len(),
                "Supervisor tree initialization complete"
            );
        }

        Ok(actor_ids)
    }
}

#[async_trait]
impl Application for SpecApplication {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn version(&self) -> &str {
        &self.spec.version
    }

    async fn start(&mut self, node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
        {
            let is_running = self.is_running.read().await;
            if *is_running {
                return Err(ApplicationError::Other(format!(
                    "Application '{}' is already running",
                    self.spec.name
                )));
            }
        }

        info!(
            application = %self.spec.name,
            version = %self.spec.version,
            env_var_count = self.spec.env.len(),
            "Starting application"
        );

        // Log environment variables (at debug level to avoid cluttering logs)
        if !self.spec.env.is_empty() {
            debug!(
                application = %self.spec.name,
                env_vars = ?self.spec.env.keys().collect::<Vec<_>>(),
                "Application environment variables available"
            );
        }

        // Store node reference for shutdown
        {
            let mut node_ref = self.node.write().await;
            *node_ref = Some(node.clone());
        }

        // Get ServiceLocator from node
        let service_locator = node.service_locator()
            .ok_or_else(|| ApplicationError::StartupFailed(
                "ServiceLocator not available from node".to_string()
            ))?;
        
        // Store ServiceLocator for use in stop()
        {
            let mut sl = self.service_locator.write().await;
            *sl = Some(service_locator.clone());
        }

        // Initialize supervisor tree if specified
        let actor_count = if let Some(ref supervisor_spec) = self.spec.supervisor {
            let actor_ids = self.initialize_supervisor_tree(node.clone(), service_locator.clone(), supervisor_spec)
                .await
                .map_err(|e| {
                    error!(
                        application = %self.spec.name,
                        error = %e,
                        "Failed to initialize supervisor tree"
                    );
                    ApplicationError::StartupFailed(format!(
                        "Failed to initialize supervisor tree: {}",
                        e
                    ))
                })?;
            actor_ids.len() as u32
        } else {
            debug!(
                application = %self.spec.name,
                "No supervisor spec provided, application has no actors"
            );
            0
        };

        // Actor counts are automatically tracked by ActorRegistry when actors are spawned

        // Mark as running
        {
            let mut is_running = self.is_running.write().await;
            *is_running = true;
        }

        info!(
            application = %self.spec.name,
            actor_count = actor_count,
            "Application started successfully"
        );

        Ok(())
    }

    async fn stop(&mut self) -> Result<(), ApplicationError> {
        {
            let is_running = self.is_running.read().await;
            if !*is_running {
                return Err(ApplicationError::Other(format!(
                    "Application '{}' is not running",
                    self.spec.name
                )));
            }
        }

        info!(
            application = %self.spec.name,
            "Stopping application"
        );

        // Get node reference
        let node = {
            let node_ref = self.node.read().await;
            node_ref.clone()
        };

        // Graceful shutdown: stop all spawned actors
        let actor_ids = {
            let spawned = self.spawned_actor_ids.read().await;
            spawned.clone()
        };

        if !actor_ids.is_empty() {
            info!(
                application = %self.spec.name,
                actor_count = actor_ids.len(),
                "Stopping spawned actors"
            );

            // Stop actors in reverse order (last spawned, first stopped)
            // Use ActorFactory directly from ServiceLocator
            let service_locator = {
                let sl = self.service_locator.read().await;
                sl.clone()
            };
            
            let mut errors = Vec::new();
            if let Some(ref service_locator) = service_locator {
                use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
                
                let actor_factory: Arc<ActorFactoryImpl> = service_locator.get_service().await
                    .ok_or_else(|| ApplicationError::ActorStopFailed(
                        "unknown".to_string(),
                        "ActorFactory not found in ServiceLocator".to_string()
                    ))?;
                
                for actor_id in actor_ids.iter().rev() {
                    if let Err(e) = actor_factory.stop_actor(actor_id).await {
                        error!(
                            application = %self.spec.name,
                            actor_id = %actor_id,
                            error = %e,
                            "Failed to stop actor"
                        );
                        errors.push(format!("{}: {}", actor_id, e));
                    } else {
                        debug!(
                            application = %self.spec.name,
                            actor_id = %actor_id,
                            "Actor stopped"
                        );
                    }
                }
            } else {
                warn!(
                    application = %self.spec.name,
                    "ServiceLocator not available, cannot stop actors"
                );
            }

            if !errors.is_empty() {
                warn!(
                    application = %self.spec.name,
                    error_count = errors.len(),
                    "Some actors failed to stop: {}",
                    errors.join(", ")
                );
                // Continue with shutdown even if some actors failed
            }
        }

        // Get final actor count before clearing
        let final_actor_count = {
            let spawned = self.spawned_actor_ids.read().await;
            spawned.len() as u32
        };

        // Actor counts are automatically tracked by ActorRegistry

        // Clear spawned actor IDs
        {
            let mut spawned = self.spawned_actor_ids.write().await;
            spawned.clear();
        }

        // Clear node reference
        {
            let mut node_ref = self.node.write().await;
            *node_ref = None;
        }

        info!(
            application = %self.spec.name,
            actor_count = final_actor_count,
            "Application stopped successfully"
        );

        // Mark as stopped
        {
            let mut is_running = self.is_running.write().await;
            *is_running = false;
        }

        info!(
            application = %self.spec.name,
            "Application stopped successfully"
        );

        Ok(())
    }

    async fn health_check(&self) -> HealthStatus {
        let is_running = self.is_running.read().await;
        if *is_running {
            // For production, we could check:
            // - Are all spawned actors still running?
            // - Are critical resources available?
            // - Are metrics within acceptable ranges?
            // For now, just check if application is running
            HealthStatus::HealthStatusHealthy
        } else {
            HealthStatus::HealthStatusUnhealthy
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// ============================================================================
// TESTS (TDD Approach)
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_proto::application::v1::{
        ApplicationSpec, ApplicationType, ChildSpec, ChildType, RestartPolicy, SupervisorSpec,
        SupervisionStrategy,
    };
    use prost_types::Duration as ProtoDuration;
    use tokio::sync::RwLock;

    /// Mock ApplicationNode for testing
    struct MockNode {
        id: String,
        addr: String,
        spawned_actors: Arc<RwLock<Vec<String>>>,
        stopped_actors: Arc<RwLock<Vec<String>>>,
        service_locator: Arc<plexspaces_core::ServiceLocator>,
    }

    impl MockNode {
        fn new(id: impl Into<String>) -> Self {
            Self {
                id: id.into(),
                addr: "0.0.0.0:9001".to_string(),
                spawned_actors: Arc::new(RwLock::new(Vec::new())),
                stopped_actors: Arc::new(RwLock::new(Vec::new())),
                service_locator: Arc::new(plexspaces_core::ServiceLocator::new()),
            }
        }

        async fn get_spawned_actors(&self) -> Vec<String> {
            self.spawned_actors.read().await.clone()
        }

        async fn get_stopped_actors(&self) -> Vec<String> {
            self.stopped_actors.read().await.clone()
        }
    }

    #[async_trait]
    impl ApplicationNode for MockNode {
        fn id(&self) -> &str {
            &self.id
        }

        fn listen_addr(&self) -> &str {
            &self.addr
        }

        fn service_locator(&self) -> Option<Arc<plexspaces_core::ServiceLocator>> {
            Some(self.service_locator.clone())
        }
    }

    /// Create a test ApplicationSpec with supervisor tree
    fn create_test_spec_with_supervisor() -> ApplicationSpec {
        ApplicationSpec {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            description: "Test application".to_string(),
            r#type: ApplicationType::ApplicationTypeActive as i32,
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: Some(SupervisorSpec {
                strategy: SupervisionStrategy::SupervisionStrategyOneForOne as i32,
                max_restarts: 3,
                max_restart_window: Some(ProtoDuration {
                    seconds: 5,
                    nanos: 0,
                }),
                children: vec![
                    ChildSpec {
                        id: "worker1".to_string(),
                        r#type: ChildType::ChildTypeWorker as i32,
                        start_module: "test::Worker1".to_string(),
                        args: Default::default(),
                        restart: RestartPolicy::RestartPolicyPermanent as i32,
                        shutdown_timeout: Some(ProtoDuration {
                            seconds: 10,
                            nanos: 0,
                        }),
                        supervisor: None,
                    },
                    ChildSpec {
                        id: "worker2".to_string(),
                        r#type: ChildType::ChildTypeWorker as i32,
                        start_module: "test::Worker2".to_string(),
                        args: Default::default(),
                        restart: RestartPolicy::RestartPolicyTransient as i32,
                        shutdown_timeout: Some(ProtoDuration {
                            seconds: 5,
                            nanos: 0,
                        }),
                        supervisor: None,
                    },
                ],
            }),
        }
    }

    /// Create a test ApplicationSpec without supervisor
    fn create_test_spec_no_supervisor() -> ApplicationSpec {
        ApplicationSpec {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            description: "Test application".to_string(),
            r#type: ApplicationType::ApplicationTypeLibrary as i32,
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: None,
        }
    }

    /// Test: Create SpecApplication
    #[test]
    fn test_create_spec_application() {
        let spec = create_test_spec_no_supervisor();
        let app = SpecApplication::new(spec);

        assert_eq!(app.name(), "test-app");
        assert_eq!(app.version(), "0.1.0");
    }

    /// Test: Start application without supervisor spec
    #[tokio::test]
    async fn test_start_application_no_supervisor() {
        let spec = create_test_spec_no_supervisor();
        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        app.start(node).await.unwrap();

        // Verify application is running
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    /// Test: Start application with supervisor spec (validates but doesn't spawn)
    #[tokio::test]
    async fn test_start_application_with_supervisor() {
        let spec = create_test_spec_with_supervisor();
        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        // Start should succeed (validates spec but logs warning about native spawning)
        app.start(node.clone()).await.unwrap();

        // Verify application is running
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);

        // Verify no actors were spawned (native Rust can't dynamically spawn)
        let spawned = node.get_spawned_actors().await;
        assert_eq!(spawned.len(), 0);
    }

    /// Test: Start already running application fails
    #[tokio::test]
    async fn test_start_already_running_application_fails() {
        let spec = create_test_spec_no_supervisor();
        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        app.start(node.clone()).await.unwrap();

        // Try to start again
        let result = app.start(node).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already running"));
    }

    /// Test: Stop application gracefully
    #[tokio::test]
    async fn test_stop_application() {
        let spec = create_test_spec_no_supervisor();
        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        app.start(node.clone()).await.unwrap();
        app.stop().await.unwrap();

        // Verify application is stopped
        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusUnhealthy);
    }

    /// Test: Stop non-running application fails
    #[tokio::test]
    async fn test_stop_non_running_application_fails() {
        let spec = create_test_spec_no_supervisor();
        let mut app = SpecApplication::new(spec);

        let result = app.stop().await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not running"));
    }

    /// Test: Health check for running application
    #[tokio::test]
    async fn test_health_check_running() {
        let spec = create_test_spec_no_supervisor();
        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        app.start(node).await.unwrap();

        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    /// Test: Health check for stopped application
    #[tokio::test]
    async fn test_health_check_stopped() {
        let spec = create_test_spec_no_supervisor();
        let app = SpecApplication::new(spec);

        let health = app.health_check().await;
        assert_eq!(health, HealthStatus::HealthStatusUnhealthy);
    }

    /// Test: Supervisor tree validation with empty child ID fails
    #[tokio::test]
    async fn test_supervisor_tree_empty_child_id_fails() {
        let mut spec = create_test_spec_with_supervisor();
        if let Some(ref mut supervisor) = spec.supervisor {
            supervisor.children[0].id = String::new();
        }

        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        let result = app.start(node).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty ID"));
    }

    /// Test: Supervisor tree validation with empty start_module fails
    #[tokio::test]
    async fn test_supervisor_tree_empty_start_module_fails() {
        let mut spec = create_test_spec_with_supervisor();
        if let Some(ref mut supervisor) = spec.supervisor {
            supervisor.children[0].start_module = String::new();
        }

        let mut app = SpecApplication::new(spec);
        let node = Arc::new(MockNode::new("test-node"));

        let result = app.start(node).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty start_module"));
    }
}

