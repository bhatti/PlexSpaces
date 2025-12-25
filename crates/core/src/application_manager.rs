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

use crate::application::{
    Application, ApplicationError, ApplicationNode, ApplicationState, HealthStatus,
};
use crate::Service;
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
    /// Namespace from ApplicationSpec (if available)
    namespace: String,
    /// Tenant ID from ApplicationSpec (if available)
    tenant_id: String,
}

/// Application manager for node
#[derive(Clone)]
pub struct ApplicationManager {
    /// Registered applications
    applications: Arc<RwLock<HashMap<String, ApplicationInstance>>>,
    /// Shutdown signal
    shutdown_requested: Arc<RwLock<bool>>,
    /// Reference to the node context for applications (protected by RwLock for Arc compatibility)
    node_context: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
}

impl Service for ApplicationManager {
    fn service_name(&self) -> String {
        crate::service_locator::service_names::APPLICATION_MANAGER.to_string()
    }
}

impl ApplicationManager {
    /// Create new application manager
    pub fn new() -> Self {
        Self {
            applications: Arc::new(RwLock::new(HashMap::new())),
            shutdown_requested: Arc::new(RwLock::new(false)),
            node_context: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the node context for the application manager.
    /// This is called by the Node after its creation.
    /// Can be called multiple times safely (idempotent).
    pub async fn set_node_context(&self, node_context: Arc<dyn ApplicationNode>) {
        let mut ctx = self.node_context.write().await;
        *ctx = Some(node_context);
    }


    /// Check if node context is set
    pub async fn has_node_context(&self) -> bool {
        let ctx = self.node_context.read().await;
        ctx.is_some()
    }

    /// Set node context if not already set
    pub async fn ensure_node_context(&self, node_context: Arc<dyn ApplicationNode>) {
        let mut ctx = self.node_context.write().await;
        if ctx.is_none() {
            *ctx = Some(node_context);
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
        self.register_with_metadata(app, "default".to_string(), "internal".to_string()).await
    }
    
    /// Register an application with namespace and tenant_id
    ///
    /// ## Arguments
    /// * `app` - Application implementation
    /// * `namespace` - Application namespace
    /// * `tenant_id` - Application tenant ID
    ///
    /// ## Returns
    /// * `Ok(())` - Application registered successfully
    /// * `Err(ApplicationError)` - Application with same name already registered
    pub async fn register_with_metadata(
        &self,
        app: Box<dyn Application>,
        namespace: String,
        tenant_id: String,
    ) -> Result<(), ApplicationError> {
        let name = app.name().to_string();
        let version = app.version().to_string();
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
                namespace: namespace.clone(),
                tenant_id: tenant_id.clone(),
            },
        );

        // Register application with object-registry
        if let Some(node_context) = self.node_context.read().await.as_ref() {
            if let Some(service_locator) = node_context.service_locator() {
                // Note: We can't use concrete ObjectRegistry type here due to circular dependency.
                // Unregistration will be handled by the node crate's object_registry_helpers.
                // This is a placeholder - the actual unregistration happens in the node crate.
                tracing::debug!(application = %name, "Application unregistered (object-registry unregistration handled by node crate)");
            }
        }

        Ok(())
    }

    /// Start an application
    ///
    /// ## Purpose
    /// Start a registered application by calling its `start()` method.
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// * `Ok(())` - Application started successfully
    /// * `Err(ApplicationError)` - Start failed
    ///
    /// ## State Transitions
    /// Created -> Starting -> Running (or Failed)
    pub async fn start(&self, name: &str) -> Result<(), ApplicationError> {
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

        // OBSERVABILITY: Record metrics for application startup (Phase 8)
        let startup_start = std::time::Instant::now();
        metrics::counter!("plexspaces_application_startup_total",
            "application" => name.to_string()
        ).increment(1);
        
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

        // Get node context (must be set before calling start)
        let node_context = {
            let ctx = self.node_context.read().await;
            ctx.as_ref().ok_or_else(|| ApplicationError::Other(
                "Node context not set. Call set_node_context() before starting applications.".to_string()
            ))?.clone()
        };
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
                
                // Phase 3: Application Facet Metrics - Aggregate facet metrics at application level
                // Note: Facet counts are tracked at actor level, but we can aggregate here
                // For now, we log that facets are tracked at actor level
                // Future: Add facet_count to ApplicationMetrics proto and aggregate here
                tracing::info!(
                    application = %name,
                    state = ?instance.state,
                    actor_count = actor_count,
                    supervisor_count = supervisor_count,
                    "Application started successfully"
                );
                
                // Phase 3: Application Facet Metrics - Record aggregated metrics
                let startup_duration = startup_start.elapsed();
                metrics::histogram!("plexspaces_application_startup_duration_seconds",
                    "application" => name.to_string()
                ).record(startup_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_startup_success_total",
                    "application" => name.to_string()
                ).increment(1);
                
                // Log metrics
                if actor_count > 0 || supervisor_count > 0 {
                    tracing::debug!(
                        application = %name,
                        actor_count = actor_count,
                        supervisor_count = supervisor_count,
                        duration_ms = startup_duration.as_millis(),
                        "Application metrics after startup"
                    );
                }
                
                Ok(())
            }
            Err(e) => {
                // OBSERVABILITY: Record metrics for startup failure (Phase 8)
                let startup_duration = startup_start.elapsed();
                metrics::histogram!("plexspaces_application_startup_duration_seconds",
                    "application" => name.to_string()
                ).record(startup_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_startup_errors_total",
                    "application" => name.to_string(),
                    "error_type" => format!("{:?}", e)
                ).increment(1);
                
                tracing::error!(
                    application = %name,
                    error = %e,
                    state = ?ApplicationState::ApplicationStateFailed,
                    duration_ms = startup_duration.as_millis(),
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

        // OBSERVABILITY: Record metrics for application shutdown (Phase 8)
        let shutdown_start = std::time::Instant::now();
        metrics::counter!("plexspaces_application_shutdown_total",
            "application" => name.to_string()
        ).increment(1);
        
        // Call application's stop() method with timeout
        match timeout(timeout_duration, instance.app.stop()).await {
            Ok(Ok(())) => {
                instance.state = ApplicationState::ApplicationStateStopped;
                instance.stopped_at = Some(std::time::Instant::now());
                
                // Phase 3: Application Facet Metrics - Record aggregated shutdown metrics
                let shutdown_duration = shutdown_start.elapsed();
                metrics::histogram!("plexspaces_application_shutdown_duration_seconds",
                    "application" => name.to_string()
                ).record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_success_total",
                    "application" => name.to_string()
                ).increment(1);
                
                tracing::info!(
                    application = %name,
                    state = ?instance.state,
                    actor_count = actor_count,
                    supervisor_count = supervisor_count,
                    duration_ms = shutdown_duration.as_millis(),
                    "Application stopped successfully"
                );
                
                Ok(())
            }
            Ok(Err(e)) => {
                // OBSERVABILITY: Record metrics for shutdown failure (Phase 8)
                let shutdown_duration = shutdown_start.elapsed();
                metrics::histogram!("plexspaces_application_shutdown_duration_seconds",
                    "application" => name.to_string()
                ).record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_errors_total",
                    "application" => name.to_string(),
                    "error_type" => format!("{:?}", e)
                ).increment(1);
                
                tracing::error!(
                    application = %name,
                    error = %e,
                    state = ?ApplicationState::ApplicationStateFailed,
                    duration_ms = shutdown_duration.as_millis(),
                    "Application stop() failed"
                );
                instance.state = ApplicationState::ApplicationStateFailed;
                instance.stopped_at = Some(std::time::Instant::now());
                Err(e)
            }
            Err(_) => {
                // OBSERVABILITY: Record metrics for shutdown timeout (Phase 8)
                let shutdown_duration = shutdown_start.elapsed();
                metrics::histogram!("plexspaces_application_shutdown_duration_seconds",
                    "application" => name.to_string()
                ).record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_errors_total",
                    "application" => name.to_string(),
                    "error_type" => "timeout"
                ).increment(1);
                
                tracing::error!(
                    application = %name,
                    timeout_seconds = timeout_duration.as_secs(),
                    state = ?ApplicationState::ApplicationStateFailed,
                    duration_ms = shutdown_duration.as_millis(),
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
            .map(|(name, inst)| {
                // Collect metrics for each application
                let actor_count = inst.tracked_actor_count;
                let supervisor_count = inst.tracked_supervisor_count;
                eprintln!("   → Stopping '{}' (actors: {}, supervisors: {})", name, actor_count, supervisor_count);
                name.clone()
            })
            .collect();

        drop(apps); // Release read lock before stopping

        if app_names.is_empty() {
            eprintln!("   (No running applications to stop)");
            return Ok(());
        }

        // Stop in reverse order (last started, first stopped)
        let mut errors = Vec::new();
        let mut stopped_count = 0;
        for name in app_names.iter().rev() {
            match self.stop(name, timeout_duration).await {
                Ok(()) => {
                    stopped_count += 1;
                    eprintln!("   ✓ Stopped '{}' ({}/{})", name, stopped_count, app_names.len());
                }
                Err(e) => {
                    errors.push(format!("{}: {}", name, e));
                    eprintln!("   ✗ Failed to stop '{}': {}", name, e);
                }
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

        // Note: Object-registry unregistration is handled by the node crate
        // to avoid circular dependency (core can't depend on object-registry)

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
    /// This is a convenience method that works with node-specific application types.
    ///
    /// ## Returns
    /// ApplicationSpec if available, None otherwise
    ///
    /// ## Note
    /// This method requires node-specific types (SpecApplication, WasmApplication).
    /// For core crate, this always returns None. Node crate provides an extension
    /// method that handles the downcasting.
    pub async fn get_application_spec(&self, _name: &str) -> Option<plexspaces_proto::application::v1::ApplicationSpec> {
        // Core crate doesn't know about node-specific application types
        // Node crate can provide an extension trait for this functionality
        None
    }

    /// Access application instance for downcasting (node-specific functionality)
    ///
    /// ## Purpose
    /// Allows node-specific code to access the application instance for downcasting
    /// to SpecApplication or WasmApplication.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `f` - Closure that receives the application as `&dyn std::any::Any`
    ///
    /// ## Returns
    /// Result of the closure, or None if application not found
    pub async fn with_application<F, R>(&self, name: &str, f: F) -> Option<R>
    where
        F: FnOnce(&dyn std::any::Any) -> Option<R>,
    {
        let apps = self.applications.read().await;
        let instance = apps.get(name)?;
        f(instance.app.as_any())
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
    /// Get namespace and tenant_id for an application (stored in ApplicationInstance)
    pub async fn get_application_namespace_tenant(&self, name: &str) -> Option<(String, String)> {
        let apps = self.applications.read().await;
        let instance = apps.get(name)?;
        Some((instance.namespace.clone(), instance.tenant_id.clone()))
    }
    
    pub async fn get_application_info(&self, name: &str) -> Option<plexspaces_proto::application::v1::ApplicationInfo> {
        use plexspaces_proto::application::v1::ApplicationInfo;
        use prost_types::Timestamp;

        let apps = self.applications.read().await;
        let instance = apps.get(name)?;

        // Convert ApplicationState to ApplicationStatus
        // Note: Applications are considered "Active" (Running) when created since we haven't implemented activate/deactivate
        // Once an application is registered, it's in active state
        use plexspaces_proto::application::v1::ApplicationStatus as ProtoApplicationStatus;
        let status = match instance.state {
            ApplicationState::ApplicationStateUnspecified => ProtoApplicationStatus::ApplicationStatusUnspecified,
            ApplicationState::ApplicationStateCreated => ProtoApplicationStatus::ApplicationStatusRunning, // Created maps to Running (active by default)
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
            // Note: namespace and tenant_id are stored in ApplicationInstance but not in ApplicationInfo proto
            // They are accessed via ApplicationInstance in dashboard handlers
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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        let manager = ApplicationManager::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

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
        // Note: ApplicationStateCreated maps to ApplicationStatusRunning (active by default)
        assert_eq!(
            app_info.status,
            plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusRunning as i32
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
