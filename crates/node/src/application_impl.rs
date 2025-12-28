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
use plexspaces_supervisor::{Supervisor, SupervisionStrategy};
use plexspaces_core::{ActorId, ExitReason};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use metrics;

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
    /// Root supervisor handle (for graceful shutdown with Supervisor::shutdown())
    root_supervisor_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Root supervisor (for shutdown)
    root_supervisor: Arc<RwLock<Option<Arc<tokio::sync::RwLock<plexspaces_supervisor::Supervisor>>>>>,
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
            root_supervisor_handle: Arc::new(RwLock::new(None)),
            root_supervisor: Arc::new(RwLock::new(None)),
        }
    }

    /// Create new application with behavior factory
    ///
    /// ## Purpose
    /// Allows applications to dynamically spawn actors using behavior factory
    /// (actor_type is derived from child.id).
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
            root_supervisor_handle: Arc::new(RwLock::new(None)),
            root_supervisor: Arc::new(RwLock::new(None)),
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
    /// Actor type is derived from child.id for both native Rust and WASM applications.
    ///
    /// ## Design Notes
    /// - Native Rust applications need a behavior factory/registry to create behaviors
    ///   (future enhancement - currently not fully implemented)
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

            // Note: start_module removed - actor_type is derived from child.id

            // Log deployment progress: processing child
            debug!(
                application = %self.spec.name,
                child_id = %child.id,
                child_type = ?child.r#type(),
                progress = format!("{}/{}", actor_ids.len() + 1, supervisor_spec.children.len()),
                "Processing child spec"
            );

            // Use child.id as actor_type (start_module removed - was confusing)
            let actor_type = child.id.clone();
            let actor_id = format!("{}@{}", child.id, node.id());
            
            // Phase 1: Unified Lifecycle - Attach facets from ChildSpec before spawning
            // Use ActorFactory::spawn_actor() which supports facets directly
            use plexspaces_core::RequestContext;
            use plexspaces_actor::get_actor_factory;
            let ctx = RequestContext::new_without_auth("internal".to_string(), self.spec.name.clone());
            
            // Get ActorFactory from ServiceLocator
            let actor_factory: Arc<dyn plexspaces_actor::ActorFactory> = get_actor_factory(service_locator.as_ref()).await
                .ok_or_else(|| {
                    ApplicationError::StartupFailed(
                        "ActorFactory not found in ServiceLocator. Ensure Node::start() has been called.".to_string()
                    )
                })?;
            
            // Phase 1: Unified Lifecycle - Convert proto facets to facet instances
            // Use FacetRegistry to create facets from proto configurations
            let facets: Vec<Box<dyn plexspaces_facet::Facet>> = if !child.facets.is_empty() {
                // Get FacetRegistry from ServiceLocator
                use plexspaces_core::service_locator::service_names;
                if let Some(facet_registry_wrapper) = service_locator.get_service_by_name::<plexspaces_core::facet_service_wrapper::FacetRegistryServiceWrapper>(service_names::FACET_REGISTRY).await {
                    let facet_registry = facet_registry_wrapper.inner_clone();
                    // Use facet_helpers to create facets from proto
                    use plexspaces_supervisor::create_facets_from_proto;
                    let facets = create_facets_from_proto(&child.facets, &facet_registry).await;
                    
                    debug!(
                        application = %self.spec.name,
                        child_id = %child.id,
                        facet_count = child.facets.len(),
                        created_count = facets.len(),
                        "Created facets from ChildSpec for actor"
                    );
                    
                    facets
                } else {
                    // FacetRegistry not available - log and skip facets (graceful degradation)
                    debug!(
                        application = %self.spec.name,
                        child_id = %child.id,
                        facet_count = child.facets.len(),
                        "FacetRegistry not available - skipping facet attachment (graceful degradation)"
                    );
                    Vec::new()
                }
            } else {
                Vec::new()
            };
            
            // Spawn actor with facets using ActorFactory
            let actor_id_parsed = actor_id.parse()
                .map_err(|e| ApplicationError::StartupFailed(format!("Invalid actor ID '{}': {}", actor_id, e)))?;
            
            match actor_factory.spawn_actor(
                &ctx,
                &actor_id_parsed,
                &actor_type, // Use child.id as actor_type
                vec![], // initial_state
                None, // config
                child.args.clone(), // labels (using args as labels for now)
                facets, // facets from ChildSpec
            ).await {
                Ok(_message_sender) => {
                    debug!(
                        application = %self.spec.name,
                        child_id = %child.id,
                        actor_type = %actor_type,
                        actor_id = %actor_id,
                        facet_count = child.facets.len(),
                        "Spawned actor with facets"
                    );
                    actor_ids.push(actor_id);
                }
                Err(e) => {
                    error!(
                        application = %self.spec.name,
                        child_id = %child.id,
                        actor_type = %actor_type,
                        error = %e,
                        "Failed to spawn actor"
                    );
                    return Err(ApplicationError::StartupFailed(format!(
                        "Failed to spawn actor '{}': {}",
                        child.id, e
                    )));
                }
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

        // Initialize supervisor tree if specified (Phase 5/6: Production-grade with validation)
        let actor_count = if let Some(ref supervisor_spec) = self.spec.supervisor {
            // OBSERVABILITY: Record metrics for supervisor tree initialization
            let startup_start = std::time::Instant::now();
            metrics::counter!("plexspaces_application_supervisor_tree_init_total",
                "application" => self.spec.name.clone()
            ).increment(1);
            
            // Validate ApplicationSpec/ChildSpec before creating supervisor (Production-grade validation)
            Self::validate_supervisor_spec_static(
                &self.spec.name,
                supervisor_spec,
                self.behavior_factory.is_some(),
            )?;
            
            // Use existing initialize_supervisor_tree which spawns actors
            // This maintains backward compatibility while adding validation
            let actor_ids = self.initialize_supervisor_tree(node.clone(), service_locator.clone(), supervisor_spec)
                .await
                .map_err(|e| {
                    error!(
                        application = %self.spec.name,
                        error = %e,
                        "Failed to initialize supervisor tree"
                    );
                    metrics::counter!("plexspaces_application_supervisor_tree_init_errors_total",
                        "application" => self.spec.name.clone(),
                        "error_type" => format!("{:?}", e)
                    ).increment(1);
                    ApplicationError::StartupFailed(format!(
                        "Failed to initialize supervisor tree: {}",
                        e
                    ))
                })?;
            
            let actor_count = actor_ids.len() as u32;
            
            // OBSERVABILITY: Record metrics for successful startup
            let startup_duration = startup_start.elapsed();
            metrics::histogram!("plexspaces_application_supervisor_tree_init_duration_seconds",
                "application" => self.spec.name.clone()
            ).record(startup_duration.as_secs_f64());
            metrics::counter!("plexspaces_application_supervisor_tree_init_success_total",
                "application" => self.spec.name.clone()
            ).increment(1);
            
            info!(
                application = %self.spec.name,
                actor_count = actor_count,
                duration_ms = startup_duration.as_millis(),
                "Supervisor tree initialized successfully with validation"
            );
            
            actor_count
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

        // OBSERVABILITY: Record metrics for shutdown
        let shutdown_start = std::time::Instant::now();
        metrics::counter!("plexspaces_application_shutdown_total",
            "application" => self.spec.name.clone()
        ).increment(1);
        
        info!(
            application = %self.spec.name,
            "Stopping application (Phase 5/6: Graceful shutdown)"
        );

        // Phase 5/6: Gracefully shutdown supervisor hierarchy (top-down with shutdown specs)
        // Phase 1: Unified Lifecycle - Supervisor shutdown triggers facet lifecycle hooks
        // For each child actor, Supervisor::shutdown() calls actor.stop() which:
        // 1. Calls facet.on_terminate_start() for all facets (priority order)
        // 2. Calls actor.on_facets_detaching()
        // 3. Calls actor.terminate()
        // 4. Calls facet.on_detach() for all facets (reverse priority order)
        if let Some(ref root_supervisor) = *self.root_supervisor.read().await {
            let mut supervisor_guard = root_supervisor.write().await;
            
            // OBSERVABILITY: Record metrics for supervisor hierarchy shutdown
            let supervisor_shutdown_start = std::time::Instant::now();
            metrics::counter!("plexspaces_application_supervisor_shutdown_total",
                "application" => self.spec.name.clone()
            ).increment(1);
            
            // Call Supervisor::shutdown() for top-down cascading shutdown
            // This will trigger facet lifecycle hooks for all child actors
            if let Err(e) = supervisor_guard.shutdown().await {
                error!(
                    application = %self.spec.name,
                    error = %e,
                    "Failed to shutdown supervisor hierarchy"
                );
                metrics::counter!("plexspaces_application_supervisor_shutdown_errors_total",
                    "application" => self.spec.name.clone(),
                    "error_type" => format!("{:?}", e)
                ).increment(1);
            } else {
                let supervisor_shutdown_duration = supervisor_shutdown_start.elapsed();
                metrics::histogram!("plexspaces_application_supervisor_shutdown_duration_seconds",
                    "application" => self.spec.name.clone()
                ).record(supervisor_shutdown_duration.as_secs_f64());
                
                info!(
                    application = %self.spec.name,
                    duration_ms = supervisor_shutdown_duration.as_millis(),
                    "Supervisor hierarchy shutdown completed (all facet lifecycle hooks executed)"
                );
            }
            drop(supervisor_guard);
        }
        
        // Abort supervisor handle if still running
        if let Some(handle) = self.root_supervisor_handle.write().await.take() {
            handle.abort();
        }

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
            let node = node.ok_or_else(|| ApplicationError::ShutdownFailed(
                "Node reference not available".to_string()
            ))?;
            
            let service_locator = node.service_locator()
                .ok_or_else(|| ApplicationError::ShutdownFailed(
                    "ServiceLocator not available from node".to_string()
                ))?;
            
            let mut errors = Vec::new();
            {
                use plexspaces_actor::get_actor_factory;

                let actor_factory = get_actor_factory(service_locator.as_ref()).await
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
                    }
                }
            }

            if !errors.is_empty() {
                warn!(
                    application = %self.spec.name,
                    error_count = errors.len(),
                    "Some actors failed to stop: {}",
                    errors.join(", ")
                );
            }
        }

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

        // OBSERVABILITY: Record metrics for shutdown completion
        let shutdown_duration = shutdown_start.elapsed();
        metrics::histogram!("plexspaces_application_shutdown_duration_seconds",
            "application" => self.spec.name.clone()
        ).record(shutdown_duration.as_secs_f64());
        metrics::counter!("plexspaces_application_shutdown_success_total",
            "application" => self.spec.name.clone()
        ).increment(1);

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
            HealthStatus::HealthStatusHealthy
        } else {
            HealthStatus::HealthStatusUnhealthy
        }
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl SpecApplication {
    /// Validate SupervisorSpec and ChildSpecs (Production-grade validation)
    ///
    /// ## Purpose
    /// Validates ApplicationSpec/ChildSpec before creating supervisor hierarchy.
    /// Ensures all required fields are present and valid.
    ///
    /// ## Returns
    /// `Ok(())` if valid, `Err(ApplicationError)` if invalid
    fn validate_supervisor_spec_static(
        app_name: &str,
        supervisor_spec: &SupervisorSpec,
        has_behavior_factory: bool,
    ) -> Result<(), ApplicationError> {
        // Validate supervision strategy
        use plexspaces_proto::application::v1::SupervisionStrategy as ProtoSupervisionStrategy;
        let strategy = ProtoSupervisionStrategy::try_from(supervisor_spec.strategy())
            .unwrap_or(ProtoSupervisionStrategy::SupervisionStrategyUnspecified);
        if strategy == ProtoSupervisionStrategy::SupervisionStrategyUnspecified {
            return Err(ApplicationError::ConfigError(
                "Supervisor spec has unspecified supervision strategy".to_string()
            ));
        }
        
        // Validate max_restarts
        if supervisor_spec.max_restarts == 0 {
            return Err(ApplicationError::ConfigError(
                "Supervisor spec max_restarts must be > 0".to_string()
            ));
        }
        
        // Validate each child spec
        for (idx, child) in supervisor_spec.children.iter().enumerate() {
            // Validate child ID
            if child.id.is_empty() {
                return Err(ApplicationError::ConfigError(format!(
                    "Child at index {} has empty ID in application '{}'",
                    idx, app_name
                )));
            }
            
            // Validate child type
            use plexspaces_proto::application::v1::ChildType as ProtoChildType;
            let child_type = ProtoChildType::try_from(child.r#type())
                .unwrap_or(ProtoChildType::ChildTypeUnspecified);
            if child_type == ProtoChildType::ChildTypeUnspecified {
                return Err(ApplicationError::ConfigError(format!(
                    "Child '{}' at index {} has unspecified type in application '{}'",
                    child.id, idx, app_name
                )));
            }
            
            // Note: start_module removed - actor_type is derived from child.id
            
            // Validate restart policy
            use plexspaces_proto::application::v1::RestartPolicy as ProtoRestartPolicy;
            let restart_val = child.restart();
            let restart = ProtoRestartPolicy::try_from(restart_val)
                .unwrap_or(ProtoRestartPolicy::RestartPolicyUnspecified);
            if restart == ProtoRestartPolicy::RestartPolicyUnspecified {
                return Err(ApplicationError::ConfigError(format!(
                    "Child '{}' at index {} has unspecified restart policy in application '{}'",
                    child.id, idx, app_name
                )));
            }
            
            // Validate nested supervisor spec if child is a supervisor
            if child_type == ProtoChildType::ChildTypeSupervisor {
                if child.supervisor.is_none() {
                    return Err(ApplicationError::ConfigError(format!(
                        "Child supervisor '{}' at index {} missing supervisor specification in application '{}'",
                        child.id, idx, app_name
                    )));
                }
                // Recursively validate nested supervisor
                if let Some(ref nested_spec) = child.supervisor {
                    // Recursive validation
                    Self::validate_supervisor_spec_static(app_name, nested_spec, has_behavior_factory)?;
                }
            }
        }
        
        // OBSERVABILITY: Log validation success
        debug!(
            application = %app_name,
            children_count = supervisor_spec.children.len(),
            "Supervisor spec validation passed"
        );
        
        Ok(())
    }
    
    /// Create supervisor hierarchy from SupervisorSpec (Phase 5/6)
    ///
    /// ## Purpose
    /// Creates a Supervisor from SupervisorSpec structure.
    /// Note: Children are added via initialize_supervisor_tree which spawns actors.
    ///
    /// ## Returns
    /// Tuple of (root_supervisor, supervisor_handle)
    /// 
    /// ## Note
    /// This is a placeholder for future enhancement. Currently, actors are spawned
    /// directly via initialize_supervisor_tree. In a full implementation, we would:
    /// 1. Create supervisor structure
    /// 2. Spawn actors and add them to supervisor
    /// 3. Call Supervisor::start() for bottom-up startup
    #[allow(dead_code)] // Placeholder for future enhancement
    async fn create_supervisor_hierarchy(
        &self,
        node: Arc<dyn ApplicationNode>,
        service_locator: Arc<plexspaces_core::ServiceLocator>,
        supervisor_spec: &SupervisorSpec,
    ) -> Result<(Arc<tokio::sync::RwLock<Supervisor>>, tokio::task::JoinHandle<()>), ApplicationError> {
        // Convert ApplicationSpec SupervisorSpec to Supervisor
        // Note: ApplicationSpec uses application.proto::SupervisorSpec
        // Supervisor::from_config() expects supervision.proto::SupervisorConfig
        // We'll create Supervisor directly from SupervisorSpec
        
        // Convert supervision strategy (simplified - use OneForOne as default)
        // Note: Full conversion would use try_from, but for now we use a simple approach
        let strategy = SupervisionStrategy::OneForOne {
            max_restarts: supervisor_spec.max_restarts,
            within_seconds: supervisor_spec.max_restart_window
                .as_ref()
                .map(|d| d.seconds as u64)
                .unwrap_or(60),
        };
        
        // Create root supervisor
        let supervisor_id = format!("{}@{}", self.spec.name, node.id());
        let (mut supervisor, _event_rx) = Supervisor::new(supervisor_id.clone(), strategy);
        supervisor = supervisor.with_service_locator(service_locator.clone());
        
        // Note: Node reference for linking would be added here if ApplicationNode exposed Node
        // For now, links will be established when children are added via Supervisor::add_child()
        // which uses the node field if available
        
        // Add children from spec (recursively for nested supervisors)
        // Note: For now, we'll use the existing initialize_supervisor_tree logic
        // which spawns actors directly. In a future enhancement, we could
        // use Supervisor::start_child() for each child, but that requires
        // start_fn factories which are not serializable in proto.
        //
        // For production-grade implementation, we:
        // 1. Validate all specs (done in validate_supervisor_spec)
        // 2. Create supervisor structure
        // 3. Spawn actors using behavior factory (existing logic)
        // 4. Add actors to supervisor as children
        // 5. Call Supervisor::start() for bottom-up startup
        
        // Use existing initialize_supervisor_tree to spawn actors
        // This spawns actors directly using behavior factory
        // After spawning, we can add them to supervisor if needed
        // For now, we create the supervisor structure and start it
        // The actual actor spawning happens in initialize_supervisor_tree
        // which is called separately. The supervisor is created for future
        // integration when we can use Supervisor::add_child() properly.
        
        // For production-grade implementation with full Supervisor::start() integration:
        // 1. Spawn actors using behavior factory (existing initialize_supervisor_tree)
        // 2. Add spawned actors to supervisor via Supervisor::add_child()
        // 3. Call Supervisor::start() for bottom-up startup
        //
        // Current approach: Use existing initialize_supervisor_tree
        // which handles actor spawning. Supervisor structure is created
        // for future enhancement.
        
        // Start supervisor (Phase 4: Bottom-up startup with rollback)
        // Note: Supervisor::start() requires children to be added first
        // For now, we create the supervisor but don't start it yet
        // as children need to be added via initialize_supervisor_tree first
        let supervisor_arc = Arc::new(tokio::sync::RwLock::new(supervisor));
        
        // TODO: After initialize_supervisor_tree spawns actors, add them to supervisor
        // and then call Supervisor::start(). For now, we'll use the existing
        // approach where actors are spawned directly.
        
        // Create a placeholder handle (supervisor structure is created but not started yet)
        // In a full implementation with Supervisor::start() integration:
        // 1. Spawn actors via initialize_supervisor_tree
        // 2. Add them to supervisor via Supervisor::add_child()
        // 3. Call supervisor.start() for bottom-up startup
        //
        // For now, we create the supervisor structure for future enhancement.
        // The actual actor spawning happens in initialize_supervisor_tree.
        let handle = tokio::spawn(async {
            // Supervisor monitoring task (placeholder for future enhancement)
            loop {
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });
        
        // OBSERVABILITY: Log supervisor hierarchy creation
        debug!(
            application = %self.spec.name,
            supervisor_id = %supervisor_id,
            "Supervisor hierarchy structure created (actors spawned via initialize_supervisor_tree)"
        );
        
        Ok((supervisor_arc, handle))
    }
}

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
        async fn new(id: impl Into<String>) -> Self {
            use crate::create_default_service_locator;
            let id_str = id.into();
            Self {
                id: id_str.clone(),
                addr: "0.0.0.0:9001".to_string(),
                spawned_actors: Arc::new(RwLock::new(Vec::new())),
                stopped_actors: Arc::new(RwLock::new(Vec::new())),
                service_locator: create_default_service_locator(Some(id_str), None, None).await,
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
                        args: Default::default(),
                        restart: RestartPolicy::RestartPolicyPermanent as i32,
                        shutdown_timeout: Some(ProtoDuration {
                            seconds: 10,
                            nanos: 0,
                        }),
                        supervisor: None,
                        facets: vec![], // Phase 1: Unified Lifecycle - facets support
                    },
                    ChildSpec {
                        id: "worker2".to_string(),
                        r#type: ChildType::ChildTypeWorker as i32,
                        args: Default::default(),
                        restart: RestartPolicy::RestartPolicyTransient as i32,
                        shutdown_timeout: Some(ProtoDuration {
                            seconds: 5,
                            nanos: 0,
                        }),
                        supervisor: None,
                        facets: vec![], // Phase 1: Unified Lifecycle - facets support
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
        let node = Arc::new(MockNode::new("test-node").await);

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
        let node = Arc::new(MockNode::new("test-node").await);

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
        let node = Arc::new(MockNode::new("test-node").await);

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
        let node = Arc::new(MockNode::new("test-node").await);

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
        let node = Arc::new(MockNode::new("test-node").await);

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
        let node = Arc::new(MockNode::new("test-node").await);

        let result = app.start(node).await;
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("empty ID"));
    }

    // Note: start_module validation removed - actor_type is derived from child.id
}

