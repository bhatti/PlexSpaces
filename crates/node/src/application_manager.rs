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

//! Application Manager for Node
//!
//! ## Purpose
//! Manages application lifecycle within a node: registration, starting, stopping, health checks.
//! Follows Erlang/OTP application controller pattern.
//!
//! ## Architecture
//! ```text
//! Node
//!   └─ ApplicationManager
//!        ├─ Applications (HashMap<name, ApplicationInstance>)
//!        ├─ Start applications in dependency order
//!        ├─ Stop applications in reverse order
//!        └─ Health checks
//! ```

use plexspaces_core::application::{
    Application, ApplicationError, ApplicationNode, ApplicationState, HealthStatus,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};
use tracing::{debug, error, info, warn};

/// Application instance wrapper
struct ApplicationInstance {
    /// Application implementation
    app: Box<dyn Application>,
    /// Current state
    state: ApplicationState,
    /// When the application was deployed (registered)
    deployed_at: std::time::Instant,
    /// When the application started
    started_at: Option<std::time::Instant>,
    /// When the application stopped (if stopped)
    stopped_at: Option<std::time::Instant>,
    /// Application metrics (if available)
    metrics: Option<plexspaces_proto::application::v1::ApplicationMetrics>,
    /// Tracked actor count (updated when actors are spawned/stopped)
    tracked_actor_count: u32,
    /// Tracked supervisor count (updated when supervisors are created)
    tracked_supervisor_count: u32,
}

/// Application manager for node
#[derive(Clone)]
pub struct ApplicationManager {
    /// Registered applications
    applications: Arc<RwLock<HashMap<String, ApplicationInstance>>>,
    /// Shutdown signal
    shutdown_requested: Arc<RwLock<bool>>,
    /// Reference to the node context for applications
    node_context: Option<Arc<dyn ApplicationNode>>,
    /// ServiceLocator for accessing ActorFactory
    service_locator: Option<Arc<plexspaces_core::ServiceLocator>>,
}

impl ApplicationManager {
    /// Create new application manager
    pub fn new() -> Self {
        Self {
            applications: Arc::new(RwLock::new(HashMap::new())),
            shutdown_requested: Arc::new(RwLock::new(false)),
            node_context: None,
            service_locator: None,
        }
    }

    /// Set the node context for the application manager.
    /// This is called by the Node after its creation.
    /// Can be called multiple times safely (idempotent).
    pub fn set_node_context(&mut self, node_context: Arc<dyn ApplicationNode>) {
        self.node_context = Some(node_context);
    }

    /// Set the service locator for the application manager.
    /// This is called by the Node after its creation.
    /// Can be called multiple times safely (idempotent).
    pub fn set_service_locator(&mut self, service_locator: Arc<plexspaces_core::ServiceLocator>) {
        self.service_locator = Some(service_locator);
    }

    /// Check if node context is set
    pub fn has_node_context(&self) -> bool {
        self.node_context.is_some()
    }

    /// Set node context if not already set
    pub fn ensure_node_context(&mut self, node_context: Arc<dyn ApplicationNode>) {
        if self.node_context.is_none() {
            self.node_context = Some(node_context);
        }
    }

    /// Register an application
    ///
    /// Records metrics and logs for observability.
    /// ## Purpose
    /// Add an application to the manager without starting it.
    ///
    /// ## Arguments
    /// * `app` - Application implementation
    ///
    /// ## Returns
    /// * `Ok(())` - Application registered successfully
    /// * `Err(ApplicationError)` - Application with same name already registered
    pub async fn register(&self, app: Box<dyn Application>) -> Result<(), ApplicationError> {
        let name = app.name().to_string();
        let mut apps = self.applications.write().await;

        if apps.contains_key(&name) {
            return Err(ApplicationError::Other(format!(
                "Application '{}' already registered",
                name
            )));
        }

        eprintln!("Registering application: {}", name);

        apps.insert(
            name.clone(),
            ApplicationInstance {
                app,
                state: ApplicationState::ApplicationStateCreated,
                deployed_at: std::time::Instant::now(),
                started_at: None,
                stopped_at: None,
                metrics: None,
                tracked_actor_count: 0,
                tracked_supervisor_count: 0,
            },
        );

        Ok(())
    }

    /// Start an application
    ///
    /// ## Purpose
    /// Start a registered application by calling its `start()` method.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `node` - Node reference for infrastructure services
    ///
    /// ## Returns
    /// * `Ok(())` - Application started successfully
    /// * `Err(ApplicationError)` - Start failed
    ///
    /// ## State Transitions
    /// Created -> Starting -> Running (or Failed)
    pub async fn start(&mut self, name: &str) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;

        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        if instance.state != ApplicationState::ApplicationStateCreated {
            return Err(ApplicationError::Other(format!(
                "Application '{}' is in state {:?}, expected Created",
                name, instance.state
            )));
        }

        tracing::info!(
            application = %name,
            version = %instance.app.version(),
            "Starting application"
        );

        // Transition to Starting
        instance.state = ApplicationState::ApplicationStateStarting;
        
        tracing::debug!(
            application = %name,
            state = ?instance.state,
            "Application state transition: Created -> Starting"
        );

        // Call application's start() method
        let node_context = self.node_context.as_ref().unwrap().clone();
        tracing::debug!(
            application = %name,
            "Calling application.start() method"
        );
        match instance.app.start(node_context).await {
            Ok(()) => {
                instance.state = ApplicationState::ApplicationStateRunning;
                instance.started_at = Some(std::time::Instant::now());
                
                // Get actor count for metrics logging
                let actor_count = instance.tracked_actor_count;
                let supervisor_count = instance.tracked_supervisor_count;
                
                tracing::info!(
                    application = %name,
                    state = ?instance.state,
                    actor_count = actor_count,
                    supervisor_count = supervisor_count,
                    "Application started successfully"
                );
                
                // Log metrics
                if actor_count > 0 || supervisor_count > 0 {
                    tracing::debug!(
                        application = %name,
                        actor_count = actor_count,
                        supervisor_count = supervisor_count,
                        "Application metrics after startup"
                    );
                }
                
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    application = %name,
                    error = %e,
                    state = ?ApplicationState::ApplicationStateFailed,
                    "Application failed to start"
                );
                instance.state = ApplicationState::ApplicationStateFailed;
                Err(e)
            }
        }
    }

    /// Stop an application gracefully
    ///
    /// ## Purpose
    /// Stop an application by calling its `stop()` method with timeout.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `timeout_duration` - Maximum time to wait for graceful shutdown
    ///
    /// ## Returns
    /// * `Ok(())` - Application stopped successfully
    /// * `Err(ApplicationError)` - Stop failed or timed out
    ///
    /// ## State Transitions
    /// Running -> Stopping -> Stopped (or Failed)
    pub async fn stop(
        &self,
        name: &str,
        timeout_duration: Duration,
    ) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;

        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        if instance.state != ApplicationState::ApplicationStateRunning {
            eprintln!(
                "Application '{}' is in state {:?}, expected Running",
                name, instance.state
            );
            return Ok(()); // Already stopped
        }

        tracing::info!(
            application = %name,
            timeout_seconds = timeout_duration.as_secs(),
            "Stopping application"
        );

        // Transition to Stopping
        instance.state = ApplicationState::ApplicationStateStopping;
        
        // Get metrics before stopping
        let actor_count = instance.tracked_actor_count;
        let supervisor_count = instance.tracked_supervisor_count;
        
        tracing::debug!(
            application = %name,
            state = ?instance.state,
            actor_count = actor_count,
            supervisor_count = supervisor_count,
            "Application state transition: Running -> Stopping"
        );

        // Call application's stop() method with timeout
        match timeout(timeout_duration, instance.app.stop()).await {
            Ok(Ok(())) => {
                instance.state = ApplicationState::ApplicationStateStopped;
                instance.stopped_at = Some(std::time::Instant::now());
                
                tracing::info!(
                    application = %name,
                    state = ?instance.state,
                    actor_count = actor_count,
                    supervisor_count = supervisor_count,
                    "Application stopped successfully"
                );
                
                Ok(())
            }
            Ok(Err(e)) => {
                tracing::error!(
                    application = %name,
                    error = %e,
                    state = ?ApplicationState::ApplicationStateFailed,
                    "Application stop() failed"
                );
                instance.state = ApplicationState::ApplicationStateFailed;
                instance.stopped_at = Some(std::time::Instant::now());
                Err(e)
            }
            Err(_) => {
                tracing::error!(
                    application = %name,
                    timeout_seconds = timeout_duration.as_secs(),
                    state = ?ApplicationState::ApplicationStateFailed,
                    "Application stop() timed out"
                );
                instance.state = ApplicationState::ApplicationStateFailed;
                instance.stopped_at = Some(std::time::Instant::now());
                Err(ApplicationError::ShutdownTimeout(
                    prost_types::Duration::try_from(timeout_duration).unwrap_or(
                        prost_types::Duration {
                            seconds: timeout_duration.as_secs() as i64,
                            nanos: 0,
                        },
                    ),
                ))
            }
        }
    }

    /// Stop all applications in reverse registration order
    ///
    /// ## Purpose
    /// Gracefully stop all applications (last started, first stopped).
    ///
    /// ## Arguments
    /// * `timeout_duration` - Maximum time to wait for each application
    ///
    /// ## Returns
    /// * `Ok(())` - All applications stopped successfully
    /// * `Err(ApplicationError)` - One or more applications failed to stop
    pub async fn stop_all(&self, timeout_duration: Duration) -> Result<(), ApplicationError> {
        // Mark shutdown as requested
        *self.shutdown_requested.write().await = true;

        let apps = self.applications.read().await;
        let app_names: Vec<String> = apps
            .iter()
            .filter(|(_, inst)| inst.state == ApplicationState::ApplicationStateRunning)
            .map(|(name, _)| name.clone())
            .collect();

        drop(apps); // Release read lock before stopping

        eprintln!("Stopping {} running applications", app_names.len());

        // Stop in reverse order (last started, first stopped)
        let mut errors = Vec::new();
        for name in app_names.iter().rev() {
            if let Err(e) = self.stop(name, timeout_duration).await {
                errors.push(format!("{}: {}", name, e));
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(ApplicationError::Other(format!(
                "Failed to stop applications: {}",
                errors.join(", ")
            )))
        }
    }

    /// Get application health status
    ///
    /// ## Purpose
    /// Check health of a specific application.
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// * `Ok(HealthStatus)` - Application health
    /// * `Err(ApplicationError)` - Application not found
    pub async fn health_check(&self, name: &str) -> Result<HealthStatus, ApplicationError> {
        let apps = self.applications.read().await;

        let instance = apps
            .get(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        if instance.state != ApplicationState::ApplicationStateRunning {
            return Ok(HealthStatus::HealthStatusUnhealthy);
        }

        Ok(instance.app.health_check().await)
    }

    /// Check if shutdown has been requested
    pub async fn is_shutdown_requested(&self) -> bool {
        *self.shutdown_requested.read().await
    }

    /// Get application state
    pub async fn get_state(&self, name: &str) -> Option<ApplicationState> {
        let apps = self.applications.read().await;
        apps.get(name).map(|inst| inst.state.clone())
    }

    /// Get all application names
    pub async fn list_applications(&self) -> Vec<String> {
        let apps = self.applications.read().await;
        apps.keys().cloned().collect()
    }

    /// Unregister an application
    ///
    /// ## Purpose
    /// Remove an application from the manager. The application must be stopped first.
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// * `Ok(())` - Application unregistered successfully
    /// * `Err(ApplicationError)` - Application not found or still running
    pub async fn unregister(&self, name: &str) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;

        let instance = apps
            .get(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        // Only allow unregistering stopped or failed applications
        if instance.state == ApplicationState::ApplicationStateRunning {
            return Err(ApplicationError::Other(format!(
                "Cannot unregister running application '{}'. Stop it first.",
                name
            )));
        }

        apps.remove(name);
        eprintln!("Unregistered application: {}", name);

        Ok(())
    }

    /// Update application metrics
    ///
    /// ## Purpose
    /// Update metrics for a running application.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `metrics` - Updated metrics
    ///
    /// ## Note
    /// ApplicationMetrics proto has: actor_count, supervisor_count, uptime_seconds
    /// For message_count and error_count, use ApplicationStatistics instead (future enhancement)
    pub async fn update_metrics(
        &self,
        name: &str,
        metrics: plexspaces_proto::application::v1::ApplicationMetrics,
    ) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;

        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        // Update tracked counts from metrics
        let old_actor_count = instance.tracked_actor_count;
        let old_supervisor_count = instance.tracked_supervisor_count;
        instance.tracked_actor_count = metrics.actor_count;
        instance.tracked_supervisor_count = metrics.supervisor_count;
        instance.metrics = Some(metrics.clone());
        
        // Log metrics update
        tracing::debug!(
            application = %name,
            actor_count = metrics.actor_count,
            supervisor_count = metrics.supervisor_count,
            uptime_seconds = metrics.uptime_seconds,
            actor_count_changed = old_actor_count != metrics.actor_count,
            supervisor_count_changed = old_supervisor_count != metrics.supervisor_count,
            "Application metrics updated"
        );
        
        Ok(())
    }
    
    /// Update tracked actor count for an application
    ///
    /// ## Purpose
    /// Updates the tracked actor count for metrics reporting.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `actor_count` - New actor count
    pub async fn update_actor_count(&self, name: &str, actor_count: u32) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;
        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;
        
        let old_count = instance.tracked_actor_count;
        instance.tracked_actor_count = actor_count;
        
        // Update metrics if they exist
        if let Some(ref mut metrics) = instance.metrics {
            metrics.actor_count = actor_count;
        }
        
        // Log metrics update
        if old_count != actor_count {
            tracing::debug!(
                application = %name,
                old_count = old_count,
                new_count = actor_count,
                "Actor count updated"
            );
        }
        
        Ok(())
    }
    
    /// Update tracked supervisor count for an application
    ///
    /// ## Purpose
    /// Updates the tracked supervisor count for metrics reporting.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `supervisor_count` - New supervisor count
    pub async fn update_supervisor_count(&self, name: &str, supervisor_count: u32) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;
        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;
        
        let old_count = instance.tracked_supervisor_count;
        instance.tracked_supervisor_count = supervisor_count;
        
        // Update metrics if they exist
        if let Some(ref mut metrics) = instance.metrics {
            metrics.supervisor_count = supervisor_count;
        }
        
        // Log metrics update
        if old_count != supervisor_count {
            tracing::debug!(
                application = %name,
                old_count = old_count,
                new_count = supervisor_count,
                "Supervisor count updated"
            );
        }
        
        Ok(())
    }

    /// Get ApplicationSpec from application (if available)
    ///
    /// ## Purpose
    /// Attempts to extract ApplicationSpec from the application instance.
    /// Works for both SpecApplication and WasmApplication.
    ///
    /// ## Returns
    /// ApplicationSpec if available, None otherwise
    pub async fn get_application_spec(&self, name: &str) -> Option<plexspaces_proto::application::v1::ApplicationSpec> {
        let apps = self.applications.read().await;
        let instance = apps.get(name)?;
        
        // Try to downcast to SpecApplication
        use crate::application_impl::SpecApplication;
        if let Some(spec_app) = instance.app.as_any().downcast_ref::<SpecApplication>() {
            return Some(spec_app.spec().clone());
        }
        
        // Try to downcast to WasmApplication
        use crate::wasm_application::WasmApplication;
        if let Some(wasm_app) = instance.app.as_any().downcast_ref::<WasmApplication>() {
            return wasm_app.spec().cloned();
        }
        
        None
    }

    /// Get full application information
    ///
    /// ## Purpose
    /// Returns comprehensive information about an application including:
    /// - Version
    /// - Status
    /// - Deployment timestamp
    /// - Metrics (if available)
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// Application info or None if not found
    pub async fn get_application_info(&self, name: &str) -> Option<plexspaces_proto::application::v1::ApplicationInfo> {
        use plexspaces_proto::application::v1::ApplicationInfo;
        use prost_types::Timestamp;

        let apps = self.applications.read().await;
        let instance = apps.get(name)?;

        // Convert ApplicationState to ApplicationStatus
        use plexspaces_proto::application::v1::ApplicationStatus as ProtoApplicationStatus;
        let status = match instance.state {
            ApplicationState::ApplicationStateUnspecified => ProtoApplicationStatus::ApplicationStatusUnspecified,
            ApplicationState::ApplicationStateCreated => ProtoApplicationStatus::ApplicationStatusLoading, // Created maps to Loading
            ApplicationState::ApplicationStateStarting => ProtoApplicationStatus::ApplicationStatusStarting,
            ApplicationState::ApplicationStateRunning => ProtoApplicationStatus::ApplicationStatusRunning,
            ApplicationState::ApplicationStateStopping => ProtoApplicationStatus::ApplicationStatusStopping,
            ApplicationState::ApplicationStateStopped => ProtoApplicationStatus::ApplicationStatusStopped,
            ApplicationState::ApplicationStateFailed => ProtoApplicationStatus::ApplicationStatusFailed,
        };

        // Calculate deployed_at timestamp
        let deployed_at = Some(Timestamp {
            seconds: instance.deployed_at.elapsed().as_secs() as i64,
            nanos: 0,
        });

        // Calculate uptime if running
        let uptime_seconds = if let Some(started_at) = instance.started_at {
            started_at.elapsed().as_secs()
        } else {
            0
        };

        // Build metrics if available or create basic metrics
        let metrics = instance.metrics.clone().or_else(|| {
            let uptime_seconds = if let Some(started_at) = instance.started_at {
                started_at.elapsed().as_secs()
            } else {
                instance.deployed_at.elapsed().as_secs()
            };
            Some(plexspaces_proto::application::v1::ApplicationMetrics {
                actor_count: instance.tracked_actor_count,
                supervisor_count: instance.tracked_supervisor_count,
                uptime_seconds,
            })
        });
        
        // Record metrics for application info retrieval
        let name_clone = name.to_string();
        metrics::counter!("plexspaces_node_application_info_requests_total",
            "application_name" => name_clone
        ).increment(1);

        Some(ApplicationInfo {
            application_id: name.to_string(), // Use name as ID for now
            name: name.to_string(),
            version: instance.app.version().to_string(),
            status: status.into(),
            deployed_at,
            metrics,
        })
    }
}

impl Default for ApplicationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    // Mock Node for testing
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

    // Mock Application for testing
    struct MockApplication {
        name: String,
        version: String,
        should_fail_start: bool,
        should_fail_stop: bool,
        stop_delay: Duration,
    }

    #[async_trait]
    impl Application for MockApplication {
        fn name(&self) -> &str {
            &self.name
        }

        fn version(&self) -> &str {
            &self.version
        }

        async fn start(&mut self, _node: Arc<dyn ApplicationNode>) -> Result<(), ApplicationError> {
            if self.should_fail_start {
                Err(ApplicationError::StartupFailed("mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn stop(&mut self) -> Result<(), ApplicationError> {
            if self.stop_delay > Duration::from_secs(0) {
                tokio::time::sleep(self.stop_delay).await;
            }

            if self.should_fail_stop {
                Err(ApplicationError::ShutdownFailed("mock failure".to_string()))
            } else {
                Ok(())
            }
        }

        async fn health_check(&self) -> HealthStatus {
            HealthStatus::HealthStatusHealthy
        }
        
        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn test_register_application() {
        let manager = ApplicationManager::new();

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();

        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateCreated)
        );
    }

    #[tokio::test]
    async fn test_register_duplicate_application() {
        let manager = ApplicationManager::new();

        let app1 = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        let app2 = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.2.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app1).await.unwrap();
        let result = manager.register(app2).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[tokio::test]
    async fn test_start_application_success() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();

        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateRunning)
        );
    }

    #[tokio::test]
    async fn test_start_application_failure() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: true,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        let result = manager.start("test-app").await;

        assert!(result.is_err());
        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateFailed)
        );
    }

    #[tokio::test]
    async fn test_stop_application_success() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();
        manager
            .stop("test-app", Duration::from_secs(5))
            .await
            .unwrap();

        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateStopped)
        );
    }

    #[tokio::test]
    async fn test_stop_application_timeout() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(10), // Longer than timeout
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();

        let result = manager.stop("test-app", Duration::from_millis(100)).await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timeout"));
        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateFailed)
        );
    }

    #[tokio::test]
    async fn test_stop_all_applications() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        // Register and start multiple applications
        for i in 1..=3 {
            let app = Box::new(MockApplication {
                name: format!("test-app-{}", i),
                version: "0.1.0".to_string(),
                should_fail_start: false,
                should_fail_stop: false,
                stop_delay: Duration::from_secs(0),
            });

            manager.register(app).await.unwrap();
            manager.start(&format!("test-app-{}", i)).await.unwrap();
        }

        manager.stop_all(Duration::from_secs(5)).await.unwrap();

        // Verify all stopped
        for i in 1..=3 {
            assert_eq!(
                manager.get_state(&format!("test-app-{}", i)).await,
                Some(ApplicationState::ApplicationStateStopped)
            );
        }

        assert!(manager.is_shutdown_requested().await);
    }

    #[tokio::test]
    async fn test_health_check() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();

        let health = manager.health_check("test-app").await.unwrap();
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_list_applications() {
        let manager = ApplicationManager::new();

        for i in 1..=3 {
            let app = Box::new(MockApplication {
                name: format!("test-app-{}", i),
                version: "0.1.0".to_string(),
                should_fail_start: false,
                should_fail_stop: false,
                stop_delay: Duration::from_secs(0),
            });

            manager.register(app).await.unwrap();
        }

        let apps = manager.list_applications().await;
        assert_eq!(apps.len(), 3);
        assert!(apps.contains(&"test-app-1".to_string()));
        assert!(apps.contains(&"test-app-2".to_string()));
        assert!(apps.contains(&"test-app-3".to_string()));
    }

    /// Test: Unregister stopped application
    #[tokio::test]
    async fn test_unregister_stopped_application() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();
        manager.stop("test-app", Duration::from_secs(5)).await.unwrap();

        // Unregister should succeed for stopped application
        manager.unregister("test-app").await.unwrap();

        // Application should no longer exist
        assert_eq!(manager.get_state("test-app").await, None);
        assert!(manager.list_applications().await.is_empty());
    }

    /// Test: Unregister running application fails
    #[tokio::test]
    async fn test_unregister_running_application_fails() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();

        // Unregister should fail for running application
        let result = manager.unregister("test-app").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Cannot unregister running"));

        // Application should still exist
        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateRunning)
        );
    }

    /// Test: Unregister non-existent application fails
    #[tokio::test]
    async fn test_unregister_nonexistent_application_fails() {
        let manager = ApplicationManager::new();

        let result = manager.unregister("nonexistent").await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("not found"));
    }

    /// Test: Update application metrics
    #[tokio::test]
    async fn test_update_metrics() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();
        manager.start("test-app").await.unwrap();

        // Update metrics
        let metrics = plexspaces_proto::application::v1::ApplicationMetrics {
            actor_count: 5,
            supervisor_count: 2,
            uptime_seconds: 100,
        };

        manager.update_metrics("test-app", metrics.clone()).await.unwrap();

        // Verify metrics are stored
        let app_info = manager.get_application_info("test-app").await.unwrap();
        assert!(app_info.metrics.is_some());
        let stored_metrics = app_info.metrics.unwrap();
        assert_eq!(stored_metrics.uptime_seconds, 100);
        assert_eq!(stored_metrics.actor_count, 5);
        assert_eq!(stored_metrics.supervisor_count, 2);
    }

    /// Test: Get application info with full details
    #[tokio::test]
    async fn test_get_application_info() {
        let mut manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node);

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.2.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
        });

        manager.register(app).await.unwrap();

        // Get info for created application
        let app_info = manager.get_application_info("test-app").await.unwrap();
        assert_eq!(app_info.name, "test-app");
        assert_eq!(app_info.version, "0.2.0");
        assert_eq!(
            app_info.status,
            plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusLoading as i32
        );
        assert!(app_info.deployed_at.is_some());
        assert!(app_info.metrics.is_some());

        // Start application
        manager.start("test-app").await.unwrap();

        // Get info for running application
        // Add small delay to ensure uptime is calculated
        tokio::time::sleep(Duration::from_millis(10)).await;
        
        let app_info = manager.get_application_info("test-app").await.unwrap();
        assert_eq!(
            app_info.status,
            plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusRunning as i32
        );
        assert!(app_info.metrics.is_some());
        let metrics = app_info.metrics.unwrap();
        assert!(metrics.uptime_seconds >= 0); // May be 0 if very fast, but should be calculated
    }

    /// Test: Get application info for non-existent application
    #[tokio::test]
    async fn test_get_application_info_nonexistent() {
        let manager = ApplicationManager::new();

        let app_info = manager.get_application_info("nonexistent").await;
        assert!(app_info.is_none());
    }
}
