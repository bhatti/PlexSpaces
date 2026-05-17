// SPDX-License-Identifier: AGPL-3.0-or-later
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

use crate::application_trait::ApplicationNode;
use crate::{Application, ApplicationError, SpecApplication, WasmApplication};
use async_trait::async_trait;
use plexspaces_actor::{
    object_registry_helpers, ApplicationManager as ApplicationManagerTrait, Service,
};
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::application::v1::ApplicationSpec;
use plexspaces_proto::supervision::v1::SupervisorSpec;
use plexspaces_proto::v1::application::{ApplicationState, HealthStatus};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{timeout, Duration};

/// Application instance wrapper
struct ApplicationInstance {
    /// Application implementation
    app: Box<dyn Application>,
    /// Current state
    state: ApplicationState,
    /// When the application was deployed (registered)
    deployed_at: std::time::SystemTime,
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
    /// Tenant ID from ApplicationSpec (if available)
    tenant_id: String,
}

/// Application manager implementation for node
#[derive(Clone)]
pub struct ApplicationManagerImpl {
    /// Registered applications
    applications: Arc<RwLock<HashMap<String, ApplicationInstance>>>,
    /// Shutdown signal
    shutdown_requested: Arc<RwLock<bool>>,
    /// Reference to the node context for applications (protected by RwLock for Arc compatibility)
    node_context: Arc<RwLock<Option<Arc<dyn ApplicationNode>>>>,
}

impl Service for ApplicationManagerImpl {
    fn service_name(&self) -> String {
        // ApplicationManager is NOT registered in ServiceLocator
        // It is managed directly by Node and accessed through Node::application_manager()
        "ApplicationManager".to_string()
    }
}

impl ApplicationManagerImpl {
    /// Mark the application manager as shutting down.
    ///
    /// This is used by node-level shutdown orchestration paths that stop applications
    /// individually instead of going through `stop_all()`.
    pub async fn request_shutdown(&self) {
        *self.shutdown_requested.write().await = true;
    }

    fn count_supervisors_in_tree(supervisor_spec: &SupervisorSpec) -> u32 {
        supervisor_spec
            .children
            .iter()
            .map(|child| {
                let is_supervisor = child.role == "supervisor";
                let nested_count = child
                    .supervisor
                    .as_ref()
                    .map(Self::count_supervisors_in_tree)
                    .unwrap_or(0);

                u32::from(is_supervisor) + nested_count
            })
            .sum()
    }

    fn count_supervisors_in_spec(spec: &ApplicationSpec) -> u32 {
        spec.supervisor
            .as_ref()
            .map(|supervisor_spec| Self::supervisor_count_from_tree_spec(supervisor_spec))
            .unwrap_or(0)
    }

    /// Root supervisor plus nested supervisor children (matches a loaded `SupervisorSpec` root).
    pub fn supervisor_count_from_tree_spec(supervisor_spec: &SupervisorSpec) -> u32 {
        1 + Self::count_supervisors_in_tree(supervisor_spec)
    }

    fn tracked_supervisor_count_for_app(app: &dyn Application) -> u32 {
        if let Some(spec_app) = app.as_any().downcast_ref::<SpecApplication>() {
            return Self::count_supervisors_in_spec(spec_app.spec());
        }

        if let Some(wasm_app) = app.as_any().downcast_ref::<WasmApplication>() {
            return wasm_app
                .spec()
                .map(Self::count_supervisors_in_spec)
                .unwrap_or(0);
        }

        0
    }

    fn default_application_metrics(
        tracked_actor_count: u32,
        tracked_supervisor_count: u32,
        uptime_seconds: u64,
    ) -> plexspaces_proto::application::v1::ApplicationMetrics {
        let mut actor_counts = HashMap::new();
        if tracked_actor_count > 0 {
            actor_counts.insert("total".to_string(), tracked_actor_count as u64);
        }

        plexspaces_proto::application::v1::ApplicationMetrics {
            actor_counts,
            supervisor_count: tracked_supervisor_count,
            uptime_seconds,
            message_count: 0,
            error_count: 0,
            counter_metrics: HashMap::new(),
            latency_totals_ms: HashMap::new(),
            latency_max_ms: HashMap::new(),
            latency_samples: HashMap::new(),
        }
    }

    fn sync_tracked_counts_into_metrics(
        metrics: &mut plexspaces_proto::application::v1::ApplicationMetrics,
        tracked_actor_count: u32,
        tracked_supervisor_count: u32,
    ) {
        metrics.supervisor_count = tracked_supervisor_count;
        if tracked_actor_count > 0 {
            metrics
                .actor_counts
                .insert("total".to_string(), tracked_actor_count as u64);
        } else {
            metrics.actor_counts.remove("total");
        }
    }

    fn merge_u64_maps(
        target: &mut HashMap<String, u64>,
        delta: &HashMap<String, u64>,
        merge: impl Fn(u64, u64) -> u64,
    ) {
        for (key, value) in delta {
            let entry = target.entry(key.clone()).or_insert(0);
            *entry = merge(*entry, *value);
        }
    }

    fn merge_application_metrics_in_place(
        target: &mut plexspaces_proto::application::v1::ApplicationMetrics,
        delta: &plexspaces_proto::application::v1::ApplicationMetrics,
    ) {
        Self::merge_u64_maps(&mut target.actor_counts, &delta.actor_counts, |a, b| a + b);
        target.supervisor_count = target.supervisor_count.max(delta.supervisor_count);
        target.uptime_seconds = target.uptime_seconds.max(delta.uptime_seconds);
        target.message_count += delta.message_count;
        target.error_count += delta.error_count;
        Self::merge_u64_maps(
            &mut target.counter_metrics,
            &delta.counter_metrics,
            |a, b| a + b,
        );
        Self::merge_u64_maps(
            &mut target.latency_totals_ms,
            &delta.latency_totals_ms,
            |a, b| a + b,
        );
        Self::merge_u64_maps(&mut target.latency_max_ms, &delta.latency_max_ms, |a, b| {
            a.max(b)
        });
        Self::merge_u64_maps(
            &mut target.latency_samples,
            &delta.latency_samples,
            |a, b| a + b,
        );
    }

    fn emit_application_metrics_snapshot(
        application: &str,
        metrics: &plexspaces_proto::application::v1::ApplicationMetrics,
    ) {
        let actor_count = metrics
            .actor_counts
            .get("total")
            .copied()
            .unwrap_or_else(|| metrics.actor_counts.values().copied().sum::<u64>());

        metrics::gauge!(
            "plexspaces_application_tracked_actors",
            "application" => application.to_string()
        )
        .set(actor_count as f64);
        metrics::gauge!(
            "plexspaces_application_tracked_supervisors",
            "application" => application.to_string()
        )
        .set(metrics.supervisor_count as f64);
        metrics::gauge!(
            "plexspaces_application_message_count_snapshot",
            "application" => application.to_string()
        )
        .set(metrics.message_count as f64);
        metrics::gauge!(
            "plexspaces_application_error_count_snapshot",
            "application" => application.to_string()
        )
        .set(metrics.error_count as f64);
        metrics::gauge!(
            "plexspaces_application_uptime_seconds_snapshot",
            "application" => application.to_string()
        )
        .set(metrics.uptime_seconds as f64);
    }

    fn emit_tracked_counts(
        application: &str,
        tracked_actor_count: u32,
        tracked_supervisor_count: u32,
        uptime_seconds: u64,
    ) {
        let snapshot = Self::default_application_metrics(
            tracked_actor_count,
            tracked_supervisor_count,
            uptime_seconds,
        );
        Self::emit_application_metrics_snapshot(application, &snapshot);
    }

    /// Create new application manager
    pub fn new() -> Self {
        Self {
            applications: Arc::new(RwLock::new(HashMap::new())),
            shutdown_requested: Arc::new(RwLock::new(false)),
            node_context: Arc::new(RwLock::new(None)),
        }
    }

    /// Resolve lookup: try exact key (app name), then fall back to searching by application_id field.
    async fn resolve_id(&self, id: &str) -> String {
        let apps = self.applications.read().await;
        if apps.contains_key(id) {
            return id.to_string();
        }
        // Fall back: find by application_id (for WasmApplication)
        for (key, inst) in apps.iter() {
            if let Some(wasm_app) = inst
                .app
                .as_any()
                .downcast_ref::<crate::wasm_application::WasmApplication>()
            {
                if wasm_app.application_id() == id {
                    return key.clone();
                }
            }
        }
        id.to_string()
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

    /// Get node context (for behavior re-registration during reactivation)
    ///
    /// ## Purpose
    /// Returns the ApplicationNode stored in node_context, which is needed
    /// to call register_behaviors_from_supervisor_tree during actor reactivation.
    ///
    /// ## Returns
    /// Some(ApplicationNode) if set, None otherwise
    pub async fn get_node_context(&self) -> Option<Arc<dyn ApplicationNode>> {
        let ctx = self.node_context.read().await;
        ctx.clone()
    }

    /// Register an application
    ///
    /// Records metrics and logs for observability.
    /// ## Purpose
    /// Add an application to the manager without starting it.
    ///
    /// ## Arguments
    /// * `ctx` - Request context; tenant comes from auth and namespace is normalized to the application ID
    /// * `app` - Application implementation
    ///
    /// ## Returns
    /// * `Ok(())` - Application registered successfully
    /// * `Err(ApplicationError)` - Application with same name already registered
    pub async fn register(
        &self,
        ctx: &RequestContext,
        app: Box<dyn Application>,
    ) -> Result<(), ApplicationError> {
        let name = app.name().to_string();
        let version = app.version().to_string();
        let tracked_supervisor_count = Self::tracked_supervisor_count_for_app(app.as_ref());
        // Namespace is always the application name.
        let namespace = name.clone();
        let registration_ctx = ctx.clone().with_namespace(namespace.clone());
        let tenant_id = registration_ctx.tenant_id().to_string();

        if let Some(wasm_app) = app
            .as_any()
            .downcast_ref::<crate::wasm_application::WasmApplication>()
        {
            wasm_app
                .set_tenant_namespace(tenant_id.clone(), namespace.clone())
                .await;
        }

        if self.applications.read().await.contains_key(&name) {
            return Err(ApplicationError::Other(format!(
                "Application '{}' already registered",
                name
            )));
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                application_id = %name,
                application_name = %name,
                application_type = if app.as_any().is::<WasmApplication>() {
                    "wasm"
                } else if app.as_any().is::<SpecApplication>() {
                    "native"
                } else {
                    "unknown"
                },
                "Registering application"
            );
        }

        if let Some(node_context) = self.node_context.read().await.as_ref() {
            if let Some(service_locator) = node_context.service_locator() {
                if let Some(object_registry) = service_locator.get_object_registry().await {
                    if let Err(err) = object_registry_helpers::register_application(
                        &object_registry,
                        &registration_ctx,
                        &name,
                        &version,
                        node_context.id(),
                        &format!("http://{}", node_context.listen_addr()),
                    )
                    .await
                    {
                        return Err(ApplicationError::Other(format!(
                            "Failed to register application '{}' in object registry: {}",
                            name, err
                        )));
                    }
                }
            }
        }

        let mut apps = self.applications.write().await;
        if apps.contains_key(&name) {
            return Err(ApplicationError::Other(format!(
                "Application '{}' already registered",
                name
            )));
        }

        apps.insert(
            name.clone(),
            ApplicationInstance {
                app,
                state: ApplicationState::ApplicationStateCreated,
                deployed_at: std::time::SystemTime::now(),
                started_at: None,
                stopped_at: None,
                metrics: None,
                tracked_actor_count: 0,
                tracked_supervisor_count,
                tenant_id: tenant_id.clone(),
            },
        );

        Self::emit_tracked_counts(&name, 0, tracked_supervisor_count, 0);

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
        let key = self.resolve_id(name).await;
        let name = key.as_str();
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
        )
        .increment(1);

        // Transition to Starting
        instance.state = ApplicationState::ApplicationStateStarting;

        // Get tenant_id and namespace (namespace is always the application name)
        let tenant_id = instance.tenant_id.clone();
        let namespace = instance.app.name().to_string();

        tracing::info!(
            application = %name,
            version = %instance.app.version(),
            state = ?instance.state,
            tenant_id = %if tenant_id.is_empty() { "<empty>" } else { &tenant_id },
            namespace = %if namespace.is_empty() { "<empty>" } else { &namespace },
            "Starting application"
        );

        // Get node context (must be set before calling start)
        let node_context = {
            let ctx = self.node_context.read().await;
            ctx.as_ref().ok_or_else(|| ApplicationError::Other(
                "Node context not set. Call set_node_context() before starting applications.".to_string()
            ))?.clone()
        };
        match instance.app.start(node_context).await {
            Ok(()) => {
                instance.state = ApplicationState::ApplicationStateRunning;
                instance.started_at = Some(std::time::Instant::now());
                instance.tracked_supervisor_count =
                    Self::tracked_supervisor_count_for_app(instance.app.as_ref());

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
                )
                .record(startup_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_startup_success_total",
                    "application" => name.to_string()
                )
                .increment(1);
                Self::emit_tracked_counts(name, actor_count, supervisor_count, 0);

                // Log metrics
                if actor_count > 0 || supervisor_count > 0 {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            application = %name,
                            actor_count = actor_count,
                            supervisor_count = supervisor_count,
                            duration_ms = startup_duration.as_millis(),
                            "Application metrics after startup"
                        );
                    }
                }

                Ok(())
            }
            Err(e) => {
                // OBSERVABILITY: Record metrics for startup failure (Phase 8)
                let startup_duration = startup_start.elapsed();
                metrics::histogram!("plexspaces_application_startup_duration_seconds",
                    "application" => name.to_string()
                )
                .record(startup_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_startup_errors_total",
                    "application" => name.to_string(),
                    "error_type" => format!("{:?}", e)
                )
                .increment(1);

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
        let key = self.resolve_id(name).await;
        let name = key.as_str();
        let mut apps = self.applications.write().await;

        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        if instance.state != ApplicationState::ApplicationStateRunning {
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!(
                    "Application '{}' is in state {:?}, expected Running",
                    name,
                    instance.state
                );
            }
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

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                application = %name,
                state = ?instance.state,
                actor_count = actor_count,
                supervisor_count = supervisor_count,
                "Application state transition: Running -> Stopping"
            );
        }

        // OBSERVABILITY: Record metrics for application shutdown (Phase 8)
        let shutdown_start = std::time::Instant::now();
        metrics::counter!("plexspaces_application_shutdown_total",
            "application" => name.to_string()
        )
        .increment(1);

        // Call application's stop() method with timeout
        match timeout(timeout_duration, instance.app.stop()).await {
            Ok(Ok(())) => {
                instance.state = ApplicationState::ApplicationStateStopped;
                instance.stopped_at = Some(std::time::Instant::now());

                // Phase 3: Application Facet Metrics - Record aggregated shutdown metrics
                let shutdown_duration = shutdown_start.elapsed();
                metrics::histogram!("plexspaces_application_shutdown_duration_seconds",
                    "application" => name.to_string()
                )
                .record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_success_total",
                    "application" => name.to_string()
                )
                .increment(1);

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
                )
                .record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_errors_total",
                    "application" => name.to_string(),
                    "error_type" => format!("{:?}", e)
                )
                .increment(1);

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
                )
                .record(shutdown_duration.as_secs_f64());
                metrics::counter!("plexspaces_application_shutdown_errors_total",
                    "application" => name.to_string(),
                    "error_type" => "timeout"
                )
                .increment(1);

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
                if tracing::enabled!(tracing::Level::INFO) {
                    tracing::info!(
                        "   → Stopping '{}' (actors: {}, supervisors: {})",
                        name,
                        actor_count,
                        supervisor_count
                    );
                }
                name.clone()
            })
            .collect();

        drop(apps); // Release read lock before stopping

        if app_names.is_empty() {
            if tracing::enabled!(tracing::Level::INFO) {
                tracing::info!("   (No running applications to stop)");
            }
            return Ok(());
        }

        // Stop in reverse order (last started, first stopped)
        let mut errors = Vec::new();
        let mut stopped_count = 0;
        for name in app_names.iter().rev() {
            match self.stop(name, timeout_duration).await {
                Ok(()) => {
                    stopped_count += 1;
                    if tracing::enabled!(tracing::Level::INFO) {
                        tracing::info!(
                            "   ✓ Stopped '{}' ({}/{})",
                            name,
                            stopped_count,
                            app_names.len()
                        );
                    }
                }
                Err(e) => {
                    errors.push(format!("{}: {}", name, e));
                    tracing::warn!("   ✗ Failed to stop '{}': {}", name, e);
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

    /// Unregister an application
    ///
    /// ## Purpose
    /// Remove an application from the manager. The application must be stopped first.
    /// For WASM applications, returns the module hash so the caller can evict the
    /// compiled module from cache (cleanup to avoid memory leaks).
    ///
    /// ## Arguments
    /// * `name` - Application name
    ///
    /// ## Returns
    /// * `Ok(Some(hash))` - WASM app unregistered; caller should evict module with this hash
    /// * `Ok(None)` - Non-WASM app unregistered
    /// * `Err(ApplicationError)` - Application not found or still running
    pub async fn unregister(&self, name: &str) -> Result<Option<String>, ApplicationError> {
        let key = self.resolve_id(name).await;
        let name = key.as_str();
        let (module_hash, tenant_id, namespace) = {
            let apps = self.applications.read().await;
            let instance = apps.get(name).ok_or_else(|| {
                ApplicationError::Other(format!("Application '{}' not found", name))
            })?;

            if instance.state == ApplicationState::ApplicationStateRunning {
                return Err(ApplicationError::Other(format!(
                    "Cannot unregister running application '{}'. Stop it first.",
                    name
                )));
            }

            (
                instance.app.module_hash_for_cleanup(),
                instance.tenant_id.clone(),
                instance.app.name().to_string(),
            )
        };

        if let Some(node_context) = self.node_context.read().await.as_ref() {
            if let Some(service_locator) = node_context.service_locator() {
                if let Some(object_registry) = service_locator.get_object_registry().await {
                    let ctx = RequestContext::new_without_auth(tenant_id, namespace);
                    object_registry_helpers::unregister_application(
                        &object_registry,
                        &ctx,
                        name,
                        node_context.id(),
                    )
                    .await
                    .map_err(|err| {
                        ApplicationError::Other(format!(
                            "Failed to unregister application '{}' from object registry: {}",
                            name, err
                        ))
                    })?;
                }
            }
        }

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

        let mut instance = apps
            .remove(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;
        drop(apps);

        instance.app.cleanup_for_undeploy().await?;

        if tracing::enabled!(tracing::Level::INFO) {
            tracing::info!("Unregistered application: {}", name);
        }

        Ok(module_hash)
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
        let old_supervisor_count = instance.tracked_supervisor_count;
        let old_actor_count = instance.tracked_actor_count;
        instance.tracked_actor_count = metrics
            .actor_counts
            .get("total")
            .copied()
            .unwrap_or_else(|| metrics.actor_counts.values().copied().sum::<u64>())
            as u32;
        instance.tracked_supervisor_count = metrics.supervisor_count;
        let mut stored_metrics = metrics.clone();
        Self::sync_tracked_counts_into_metrics(
            &mut stored_metrics,
            instance.tracked_actor_count,
            instance.tracked_supervisor_count,
        );
        instance.metrics = Some(stored_metrics.clone());
        Self::emit_application_metrics_snapshot(name, &stored_metrics);

        // Log metrics update
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                application = %name,
                actor_count = instance.tracked_actor_count,
                supervisor_count = stored_metrics.supervisor_count,
                uptime_seconds = stored_metrics.uptime_seconds,
                message_count = stored_metrics.message_count,
                error_count = stored_metrics.error_count,
                actor_count_changed = old_actor_count != instance.tracked_actor_count,
                supervisor_count_changed = old_supervisor_count != stored_metrics.supervisor_count,
                "Application metrics updated"
            );
        }

        Ok(())
    }

    /// Merge node-local application metrics into the stored snapshot.
    pub async fn merge_metrics(
        &self,
        name: &str,
        metrics: plexspaces_proto::application::v1::ApplicationMetrics,
    ) -> Result<plexspaces_proto::application::v1::ApplicationMetrics, ApplicationError> {
        let mut apps = self.applications.write().await;
        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        let uptime_seconds = if let Some(started_at) = instance.started_at {
            started_at.elapsed().as_secs()
        } else {
            instance.deployed_at.elapsed().unwrap_or_default().as_secs()
        };
        let mut stored_metrics = instance.metrics.clone().unwrap_or_else(|| {
            Self::default_application_metrics(
                instance.tracked_actor_count,
                instance.tracked_supervisor_count,
                uptime_seconds,
            )
        });
        Self::merge_application_metrics_in_place(&mut stored_metrics, &metrics);
        stored_metrics.uptime_seconds = uptime_seconds;
        instance.tracked_actor_count = stored_metrics
            .actor_counts
            .get("total")
            .copied()
            .unwrap_or_else(|| stored_metrics.actor_counts.values().copied().sum::<u64>())
            as u32;
        Self::sync_tracked_counts_into_metrics(
            &mut stored_metrics,
            instance.tracked_actor_count,
            instance.tracked_supervisor_count,
        );
        instance.metrics = Some(stored_metrics.clone());
        Self::emit_application_metrics_snapshot(name, &stored_metrics);
        Ok(stored_metrics)
    }

    /// Update tracked actor count for an application
    ///
    /// ## Purpose
    /// Updates the tracked actor count for metrics reporting.
    ///
    /// ## Arguments
    /// * `name` - Application name
    /// * `actor_count` - New actor count
    pub async fn update_actor_count(
        &self,
        name: &str,
        actor_count: u32,
    ) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;
        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        let old_count = instance.tracked_actor_count;
        instance.tracked_actor_count = actor_count;

        // Update metrics if they exist
        if let Some(ref mut metrics) = instance.metrics {
            Self::sync_tracked_counts_into_metrics(
                metrics,
                instance.tracked_actor_count,
                instance.tracked_supervisor_count,
            );
        }

        // Log metrics update
        if old_count != actor_count {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    application = %name,
                    old_count = old_count,
                    new_count = actor_count,
                    "Actor count updated"
                );
            }
        }

        let uptime_seconds = instance
            .metrics
            .as_ref()
            .map(|metrics| metrics.uptime_seconds)
            .unwrap_or(0);
        Self::emit_tracked_counts(
            name,
            instance.tracked_actor_count,
            instance.tracked_supervisor_count,
            uptime_seconds,
        );

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
    pub async fn update_supervisor_count(
        &self,
        name: &str,
        supervisor_count: u32,
    ) -> Result<(), ApplicationError> {
        let mut apps = self.applications.write().await;
        let instance = apps
            .get_mut(name)
            .ok_or_else(|| ApplicationError::Other(format!("Application '{}' not found", name)))?;

        let old_count = instance.tracked_supervisor_count;
        instance.tracked_supervisor_count = supervisor_count;

        // Update metrics if they exist
        if let Some(ref mut metrics) = instance.metrics {
            Self::sync_tracked_counts_into_metrics(
                metrics,
                instance.tracked_actor_count,
                instance.tracked_supervisor_count,
            );
        }

        // Log metrics update
        if old_count != supervisor_count {
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    application = %name,
                    old_count = old_count,
                    new_count = supervisor_count,
                    "Supervisor count updated"
                );
            }
        }

        let uptime_seconds = instance
            .metrics
            .as_ref()
            .map(|metrics| metrics.uptime_seconds)
            .unwrap_or(0);
        Self::emit_tracked_counts(
            name,
            instance.tracked_actor_count,
            instance.tracked_supervisor_count,
            uptime_seconds,
        );

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
    pub async fn get_application_spec(
        &self,
        _name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationSpec> {
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
        Some((instance.app.name().to_string(), instance.tenant_id.clone()))
    }

    /// Get application information (internal implementation)
    async fn get_application_info_impl(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationInfo> {
        use plexspaces_proto::application::v1::ApplicationInfo;
        use prost_types::Timestamp;

        let apps = self.applications.read().await;
        let instance = apps.get(name)?;

        // Convert ApplicationState to ApplicationStatus
        // Note: Applications are considered "Active" (Running) when created since we haven't implemented activate/deactivate
        // Once an application is registered, it's in active state
        use plexspaces_proto::application::v1::ApplicationStatus as ProtoApplicationStatus;
        let status = match instance.state {
            ApplicationState::ApplicationStateUnspecified => {
                ProtoApplicationStatus::ApplicationStatusUnspecified
            }
            ApplicationState::ApplicationStateCreated => {
                ProtoApplicationStatus::ApplicationStatusRunning
            } // Created maps to Running (active by default)
            ApplicationState::ApplicationStateStarting => {
                ProtoApplicationStatus::ApplicationStatusStarting
            }
            ApplicationState::ApplicationStateRunning => {
                ProtoApplicationStatus::ApplicationStatusRunning
            }
            ApplicationState::ApplicationStateStopping => {
                ProtoApplicationStatus::ApplicationStatusStopping
            }
            ApplicationState::ApplicationStateStopped => {
                ProtoApplicationStatus::ApplicationStatusStopped
            }
            ApplicationState::ApplicationStateFailed => {
                ProtoApplicationStatus::ApplicationStatusFailed
            }
        };

        // Calculate deployed_at timestamp from wall-clock time
        let deployed_at = instance
            .deployed_at
            .duration_since(std::time::UNIX_EPOCH)
            .ok()
            .map(|d| Timestamp {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            });

        // Calculate uptime if running
        let _uptime_seconds = if let Some(started_at) = instance.started_at {
            started_at.elapsed().as_secs()
        } else {
            0
        };

        // Build metrics — always recompute uptime_seconds from monotonic clock
        let current_uptime = if let Some(started_at) = instance.started_at {
            started_at.elapsed().as_secs()
        } else {
            instance.deployed_at.elapsed().unwrap_or_default().as_secs()
        };
        let metrics = instance.metrics.clone().map(|mut m| {
            m.uptime_seconds = current_uptime;
            m
        }).or_else(|| {
            Some(Self::default_application_metrics(
                instance.tracked_actor_count,
                instance.tracked_supervisor_count,
                current_uptime,
            ))
        });

        // Record metrics for application info retrieval
        let name_clone = name.to_string();
        metrics::counter!("plexspaces_node_application_info_requests_total",
            "application_name" => name_clone
        )
        .increment(1);

        let application_id = instance
            .app
            .as_any()
            .downcast_ref::<crate::wasm_application::WasmApplication>()
            .map(|w| w.application_id().to_string())
            .unwrap_or_else(|| name.to_string());

        Some(ApplicationInfo {
            application_id,
            name: name.to_string(),
            tenant_id: instance.tenant_id.clone(),
            version: instance.app.version().to_string(),
            status: status.into(),
            deployed_at,
            metrics,
        })
    }
}

// Implement ApplicationManager trait
#[async_trait]
impl ApplicationManagerTrait for ApplicationManagerImpl {
    async fn get_state(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::v1::application::ApplicationState> {
        let key = self.resolve_id(name).await;
        let apps = self.applications.read().await;
        apps.get(&key).map(|inst| inst.state.clone())
    }

    fn as_any(self: std::sync::Arc<Self>) -> std::sync::Arc<dyn std::any::Any + Send + Sync> {
        self
    }

    async fn list_applications(&self) -> Vec<String> {
        let apps = self.applications.read().await;
        apps.keys().cloned().collect()
    }

    async fn is_shutdown_requested(&self) -> bool {
        *self.shutdown_requested.read().await
    }

    async fn get_application_info(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationInfo> {
        let key = self.resolve_id(name).await;
        self.get_application_info_impl(&key).await
    }

    async fn get_application_metrics(
        &self,
        name: &str,
    ) -> Option<plexspaces_proto::application::v1::ApplicationMetrics> {
        let key = self.resolve_id(name).await;
        self.get_application_info_impl(&key)
            .await
            .and_then(|info| info.metrics)
    }

    async fn merge_application_metrics(
        &self,
        name: &str,
        metrics: plexspaces_proto::application::v1::ApplicationMetrics,
    ) -> Result<(), String> {
        let key = self.resolve_id(name).await;
        self.merge_metrics(&key, metrics)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    async fn get_node_context(
        &self,
    ) -> Option<std::sync::Arc<dyn plexspaces_actor::ApplicationNode>> {
        // ApplicationNode trait is defined in both application_trait and core
        // They are the same trait, but Rust treats them as different types
        // We need to return the core version for the trait method
        // The node_context stores application_trait::ApplicationNode, but we need core::ApplicationNode
        // Since they're the same trait, we can't cast directly - return None for now
        // The proper fix: use core::ApplicationNode everywhere
        None
    }
}

// Add helper method to ApplicationManagerImpl for re-registering behaviors
impl ApplicationManagerImpl {}

impl Default for ApplicationManagerImpl {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use plexspaces_proto::application::v1::{ApplicationSpec, ApplicationType, ShutdownStrategy};
    use plexspaces_proto::supervision::v1::{
        ChildSpec, RestartPolicy, SupervisionStrategy, SupervisorSpec,
    };
    use std::collections::HashMap;

    fn app_ctx(name: &str) -> RequestContext {
        RequestContext::new_without_auth(String::new(), name.to_string())
    }

    /// Declared **worker** instance name (must not equal a nested supervisor instance name).
    fn worker_child(instance_name: &str) -> ChildSpec {
        ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: instance_name.to_string(),
                actor_type: "test_behavior_class".to_string(),
            }),
            role: "worker".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            ..Default::default()
        }
    }

    /// Declared **supervisor** instance name (behavior class `test_supervisor_class`); distinct from worker names.
    fn supervisor_child(supervisor_instance_name: &str, nested: SupervisorSpec) -> ChildSpec {
        ChildSpec {
            actor_identity: Some(plexspaces_proto::common::v1::ActorIdentity {
                name: supervisor_instance_name.to_string(),
                actor_type: "test_supervisor_class".to_string(),
            }),
            role: "supervisor".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            supervisor: Some(nested),
            ..Default::default()
        }
    }

    fn supervisor_spec(children: Vec<ChildSpec>) -> SupervisorSpec {
        SupervisorSpec {
            strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
            max_restarts: 3,
            max_restart_window: None,
            children,
            ..Default::default()
        }
    }

    fn application_spec(name: &str, supervisor: Option<SupervisorSpec>) -> ApplicationSpec {
        ApplicationSpec {
            name: name.to_string(),
            tenant_id: String::new(),
            version: "0.1.0".to_string(),
            description: "test application".to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: HashMap::new(),
            supervisor,
            enabled: true,
            auto_start: false,
            shutdown_timeout: None,
            shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
            seed_nodes: vec![],
            required_service_links: vec![],
            metadata: None,
        }
    }

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
        cleanup_called: Arc<std::sync::atomic::AtomicBool>,
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

        async fn cleanup_for_undeploy(&mut self) -> Result<(), ApplicationError> {
            self.cleanup_called
                .store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    #[tokio::test]
    async fn test_register_application() {
        let manager = ApplicationManagerImpl::new();

        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();

        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateCreated)
        );
    }

    #[tokio::test]
    async fn test_register_duplicate_application() {
        let manager = ApplicationManagerImpl::new();

        let app1 = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        let app2 = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.2.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app1).await.unwrap();
        let result = manager.register(&app_ctx("test-app"), app2).await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("already registered"));
    }

    #[tokio::test]
    async fn test_start_application_success() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();

        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateRunning)
        );
    }

    #[tokio::test]
    async fn test_start_application_failure() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        let result = manager.start("test-app").await;

        assert!(result.is_err());
        assert_eq!(
            manager.get_state("test-app").await,
            Some(ApplicationState::ApplicationStateFailed)
        );
    }

    #[tokio::test]
    async fn test_stop_application_success() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
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
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
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
        let manager = ApplicationManagerImpl::new();
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
                cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });

            manager
                .register(&app_ctx(&format!("test-app-{}", i)), app)
                .await
                .unwrap();
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
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();

        let health = manager.health_check("test-app").await.unwrap();
        assert_eq!(health, HealthStatus::HealthStatusHealthy);
    }

    #[tokio::test]
    async fn test_list_applications() {
        let manager = ApplicationManagerImpl::new();

        for i in 1..=3 {
            let app = Box::new(MockApplication {
                name: format!("test-app-{}", i),
                version: "0.1.0".to_string(),
                should_fail_start: false,
                should_fail_stop: false,
                stop_delay: Duration::from_secs(0),
                cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            });

            manager
                .register(&app_ctx(&format!("test-app-{}", i)), app)
                .await
                .unwrap();
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
        let manager = ApplicationManagerImpl::new();
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });

        manager.set_node_context(node).await;

        let cleanup_called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let app = Box::new(MockApplication {
            name: "test-app".to_string(),
            version: "0.1.0".to_string(),
            should_fail_start: false,
            should_fail_stop: false,
            stop_delay: Duration::from_secs(0),
            cleanup_called: cleanup_called.clone(),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();
        manager
            .stop("test-app", Duration::from_secs(5))
            .await
            .unwrap();

        // Unregister should succeed for stopped application
        manager.unregister("test-app").await.unwrap();

        // Application should no longer exist
        assert_eq!(manager.get_state("test-app").await, None);
        assert!(manager.list_applications().await.is_empty());
        assert!(cleanup_called.load(std::sync::atomic::Ordering::SeqCst));
    }

    /// Test: Unregister running application fails
    #[tokio::test]
    async fn test_unregister_running_application_fails() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
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
        let manager = ApplicationManagerImpl::new();

        let result = manager.unregister("nonexistent").await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not found"));
    }

    /// Test: Update application metrics
    #[tokio::test]
    async fn test_update_metrics() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();

        // Update metrics
        let metrics = plexspaces_proto::application::v1::ApplicationMetrics {
            actor_counts: HashMap::from([("leader".to_string(), 1), ("worker".to_string(), 4)]),
            supervisor_count: 2,
            uptime_seconds: 100,
            message_count: 42,
            error_count: 3,
            counter_metrics: HashMap::from([("tuple_operations".to_string(), 77)]),
            latency_totals_ms: HashMap::from([("compute".to_string(), 123)]),
            latency_max_ms: HashMap::from([("compute".to_string(), 17)]),
            latency_samples: HashMap::from([("compute".to_string(), 9)]),
        };

        manager
            .update_metrics("test-app", metrics.clone())
            .await
            .unwrap();

        // Verify metrics are stored
        let app_info = manager.get_application_info("test-app").await.unwrap();
        assert!(app_info.metrics.is_some());
        let stored_metrics = app_info.metrics.unwrap();
        assert_eq!(stored_metrics.uptime_seconds, 100);
        assert_eq!(stored_metrics.actor_counts.get("leader"), Some(&1));
        assert_eq!(stored_metrics.actor_counts.get("worker"), Some(&4));
        assert_eq!(stored_metrics.actor_counts.get("total"), Some(&5));
        assert_eq!(stored_metrics.supervisor_count, 2);
        assert_eq!(stored_metrics.message_count, 42);
        assert_eq!(stored_metrics.error_count, 3);
        assert_eq!(
            stored_metrics.counter_metrics.get("tuple_operations"),
            Some(&77)
        );
    }

    #[test]
    fn test_count_supervisors_in_spec_counts_nested_supervisors() {
        let nested = supervisor_spec(vec![worker_child("leaf-worker")]);
        let middle = supervisor_spec(vec![
            supervisor_child("leaf-supervisor", nested),
            worker_child("middle-worker"),
        ]);
        let root = supervisor_spec(vec![
            worker_child("root-worker"),
            supervisor_child("middle-supervisor", middle),
        ]);

        let spec = application_spec("nested-supervisors", Some(root));

        assert_eq!(ApplicationManagerImpl::count_supervisors_in_spec(&spec), 3);
    }

    #[tokio::test]
    async fn test_register_seeds_supervisor_count_from_application_spec() {
        let manager = ApplicationManagerImpl::new();
        let nested = supervisor_spec(vec![worker_child("leaf-worker")]);
        let root = supervisor_spec(vec![
            worker_child("root-worker"),
            supervisor_child("sub-supervisor", nested),
        ]);
        let app = Box::new(SpecApplication::new(application_spec(
            "spec-app",
            Some(root),
        )));

        manager.register(&app_ctx("spec-app"), app).await.unwrap();

        let info = manager
            .get_application_info("spec-app")
            .await
            .expect("application info should exist after registration");
        let metrics = info
            .metrics
            .expect("application info should include synthesized metrics");

        assert_eq!(metrics.supervisor_count, 2);
    }

    /// Test: Merge application metrics accumulates counters and preserves maxima.
    #[tokio::test]
    async fn test_merge_metrics_accumulates_extensible_metrics() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();

        let merged = manager
            .merge_metrics(
                "test-app",
                plexspaces_proto::application::v1::ApplicationMetrics {
                    actor_counts: HashMap::from([
                        ("leader".to_string(), 1),
                        ("worker".to_string(), 8),
                    ]),
                    supervisor_count: 1,
                    uptime_seconds: 5,
                    message_count: 12,
                    error_count: 1,
                    counter_metrics: HashMap::from([
                        ("scatter_gather_rounds".to_string(), 3),
                        ("tuple_operations".to_string(), 18),
                    ]),
                    latency_totals_ms: HashMap::from([
                        ("worker".to_string(), 90),
                        ("worker.compute".to_string(), 30),
                    ]),
                    latency_max_ms: HashMap::from([
                        ("worker".to_string(), 20),
                        ("worker.compute".to_string(), 9),
                    ]),
                    latency_samples: HashMap::from([
                        ("worker".to_string(), 6),
                        ("worker.compute".to_string(), 6),
                    ]),
                },
            )
            .await
            .unwrap();

        assert_eq!(merged.actor_counts.get("leader"), Some(&1));
        assert_eq!(merged.actor_counts.get("worker"), Some(&8));
        assert_eq!(merged.actor_counts.get("total"), Some(&9));
        assert_eq!(merged.message_count, 12);
        assert_eq!(merged.error_count, 1);
        assert_eq!(
            merged.counter_metrics.get("scatter_gather_rounds"),
            Some(&3)
        );
        assert_eq!(merged.counter_metrics.get("tuple_operations"), Some(&18));
        assert_eq!(merged.latency_totals_ms.get("worker"), Some(&90));
        assert_eq!(merged.latency_max_ms.get("worker"), Some(&20));
        assert_eq!(merged.latency_samples.get("worker"), Some(&6));

        let merged = manager
            .merge_metrics(
                "test-app",
                plexspaces_proto::application::v1::ApplicationMetrics {
                    actor_counts: HashMap::new(),
                    supervisor_count: 0,
                    uptime_seconds: 7,
                    message_count: 5,
                    error_count: 2,
                    counter_metrics: HashMap::from([
                        ("scatter_gather_rounds".to_string(), 2),
                        ("tuple_operations".to_string(), 4),
                    ]),
                    latency_totals_ms: HashMap::from([
                        ("worker".to_string(), 40),
                        ("worker.compute".to_string(), 11),
                    ]),
                    latency_max_ms: HashMap::from([
                        ("worker".to_string(), 24),
                        ("worker.compute".to_string(), 6),
                    ]),
                    latency_samples: HashMap::from([
                        ("worker".to_string(), 2),
                        ("worker.compute".to_string(), 2),
                    ]),
                },
            )
            .await
            .unwrap();

        assert_eq!(merged.actor_counts.get("leader"), Some(&1));
        assert_eq!(merged.actor_counts.get("worker"), Some(&8));
        assert_eq!(merged.actor_counts.get("total"), Some(&9));
        assert_eq!(merged.message_count, 17);
        assert_eq!(merged.error_count, 3);
        assert_eq!(
            merged.counter_metrics.get("scatter_gather_rounds"),
            Some(&5)
        );
        assert_eq!(merged.counter_metrics.get("tuple_operations"), Some(&22));
        assert_eq!(merged.latency_totals_ms.get("worker"), Some(&130));
        assert_eq!(merged.latency_totals_ms.get("worker.compute"), Some(&41));
        assert_eq!(merged.latency_max_ms.get("worker"), Some(&24));
        assert_eq!(merged.latency_max_ms.get("worker.compute"), Some(&9));
        assert_eq!(merged.latency_samples.get("worker"), Some(&8));
        assert_eq!(merged.latency_samples.get("worker.compute"), Some(&8));

        let stored = manager.get_application_metrics("test-app").await.unwrap();
        assert_eq!(stored.message_count, 17);
        assert_eq!(
            stored.counter_metrics.get("scatter_gather_rounds"),
            Some(&5)
        );
    }

    /// Test: Get application info with full details
    #[tokio::test]
    async fn test_get_application_info() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();

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
                                              // tracked_actor_count is 0, so "total" key is absent (only inserted when count > 0)
        assert_eq!(metrics.actor_counts.get("total"), None);
    }

    /// Test: Get application info for non-existent application
    #[tokio::test]
    async fn test_get_application_info_nonexistent() {
        let manager = ApplicationManagerImpl::new();

        let app_info = manager.get_application_info("nonexistent").await;
        assert!(app_info.is_none());
    }

    /// Regression test: ApplicationManagerImpl::start() holds applications.write() while
    /// calling app.start(). Previously, WasmApplication::initialize_supervisor_tree() called
    /// update_supervisor_count() from inside app.start(), which tried to re-acquire
    /// applications.write() → self-deadlock. Fix: removed the callback from
    /// initialize_supervisor_tree; start() sets tracked_supervisor_count after app.start()
    /// returns.
    ///
    /// This test verifies that:
    /// 1. update_supervisor_count() works after start() returns (lock is released)
    /// 2. supervisor count set via update_supervisor_count() is reflected in app info
    #[tokio::test]
    async fn test_supervisor_count_can_be_updated_after_start_no_deadlock() {
        let manager = ApplicationManagerImpl::new();
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
            cleanup_called: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        });

        manager.register(&app_ctx("test-app"), app).await.unwrap();
        manager.start("test-app").await.unwrap();

        // After start() returns, the write lock must be released.
        // update_supervisor_count() acquires applications.write() — if start() still held
        // the lock, this would deadlock. The fix ensures start() releases the lock first.
        manager
            .update_supervisor_count("test-app", 3)
            .await
            .unwrap();

        let info = manager.get_application_info("test-app").await.unwrap();
        let metrics = info.metrics.expect("metrics should be present");
        assert_eq!(metrics.supervisor_count, 3);
    }

    /// Regression test: WasmApplication::initialize_supervisor_tree() must NOT call
    /// update_supervisor_count() because start() holds applications.write() while
    /// calling app.start(). Any re-entry into ApplicationManager from app.start() that
    /// acquires applications.write() (or read) will deadlock.
    ///
    /// This test simulates the exact deadlock pattern: a mock app that calls
    /// update_supervisor_count() from within its start() method. It must NOT deadlock.
    ///
    /// With the old buggy code in initialize_supervisor_tree, this test would hang.
    /// After the fix (removing the callback), real WASM apps no longer call back in.
    /// This test documents the pattern to prevent regression.
    #[tokio::test(flavor = "multi_thread")]
    async fn test_app_start_that_reenters_manager_deadlocks_documenting_the_pattern() {
        use std::sync::Arc;

        // A mock app that simulates the old buggy behavior: calling update_supervisor_count
        // from inside start() while start() holds applications.write().
        struct MockAppWithReentrantCall {
            name: String,
            manager: Arc<ApplicationManagerImpl>,
        }

        #[async_trait]
        impl Application for MockAppWithReentrantCall {
            fn name(&self) -> &str {
                &self.name
            }
            fn version(&self) -> &str {
                "1.0"
            }
            async fn start(
                &mut self,
                _node: Arc<dyn ApplicationNode>,
            ) -> Result<(), ApplicationError> {
                // This is the EXACT pattern that caused the deadlock:
                // start() → app.start() → update_supervisor_count() → applications.write()
                // With the old code this would deadlock. The fix is: don't do this.
                // We prove it deadlocks by using a timeout.
                let name = self.name.clone();
                let manager = self.manager.clone();
                // Try to call update_supervisor_count — this acquires write lock.
                // Applications.write() is already held by the outer start() call.
                // Use a non-blocking try to detect the deadlock without hanging the test.
                let result = tokio::time::timeout(
                    Duration::from_millis(100),
                    manager.update_supervisor_count(&name, 2),
                )
                .await;
                // Timeout proves the re-entrant call would deadlock.
                // If this returns Ok, it means the lock was NOT held (which would be wrong).
                assert!(
                    result.is_err(),
                    "update_supervisor_count from inside app.start() should deadlock (timeout expected)"
                );
                Ok(())
            }
            async fn stop(&mut self) -> Result<(), ApplicationError> {
                Ok(())
            }
            async fn health_check(&self) -> HealthStatus {
                HealthStatus::HealthStatusHealthy
            }
            async fn cleanup_for_undeploy(&mut self) -> Result<(), ApplicationError> {
                Ok(())
            }
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }

        let manager = Arc::new(ApplicationManagerImpl::new());
        let node = Arc::new(MockNode {
            id: "test-node".to_string(),
            addr: "0.0.0.0:9000".to_string(),
        });
        manager.set_node_context(node).await;

        let app = Box::new(MockAppWithReentrantCall {
            name: "test-app".to_string(),
            manager: manager.clone(),
        });
        manager.register(&app_ctx("test-app"), app).await.unwrap();

        // start() must complete — the mock app internally tries update_supervisor_count
        // with a 100ms timeout, confirms it deadlocks (timeout), then returns Ok.
        // The outer start() must then also complete.
        let result = tokio::time::timeout(Duration::from_secs(5), manager.start("test-app")).await;
        assert!(
            result.is_ok(),
            "start() itself should not hang — the mock app handles the inner timeout"
        );
        assert!(result.unwrap().is_ok());

        // After start() returns, the lock is released and update_supervisor_count works.
        manager
            .update_supervisor_count("test-app", 5)
            .await
            .unwrap();
    }
}
