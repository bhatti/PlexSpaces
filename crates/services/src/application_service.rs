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

//! Application Service gRPC implementation
//!
//! ## Purpose
//! gRPC service for deploying, undeploying, and managing applications on a node.
//! Enables AWS Lambda-like deployment model where applications (not individual actors)
//! are the unit of deployment.
//!
//! ## Design Philosophy
//! - Application as unit: Deploy entire applications (supervisors + actors + config)
//! - Multi-tenant: Multiple applications per node
//! - Graceful shutdown: Undeploy performs graceful shutdown
//! - WASM support: Applications can be deployed as WASM modules
//! - Release config support: Can pass release-level configuration
//!
//! ## Architecture
//! Uses ServiceLocator for dependency injection instead of direct Node references.
//! This enables clean separation and avoids circular dependencies.

use plexspaces_actor::{
    wasm_worker_actor_type_from_application_name, ApplicationManager as ApplicationManagerTrait,
    RequestContextExt, ServiceLocator,
};
use plexspaces_application::ApplicationError as AppError;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationRuntimeState, ApplicationSpec,
    ApplicationStatus, ApplicationType, DeployApplicationRequest, DeployApplicationResponse,
    GetApplicationStatusRequest, GetApplicationStatusResponse, ListApplicationsRequest,
    ListApplicationsResponse, ShutdownStrategy, UndeployApplicationRequest,
    UndeployApplicationResponse,
};
use plexspaces_proto::common::v1::ActorIdentity;
use plexspaces_proto::supervision::v1::{
    ChildSpec, RestartPolicy, SupervisionStrategy, SupervisorSpec,
};
use plexspaces_proto::v1::application::ApplicationState as CoreApplicationState;
use plexspaces_wasm_runtime::WasmDeploymentService;
use std::sync::Arc;
use std::time::Duration;
use tonic::{Request, Response, Status};

/// Create a default ApplicationSpec with a single worker actor supervisor tree.
///
/// This ensures consistent behavior between HTTP and gRPC deployment paths.
/// When no ApplicationSpec is provided, we create a minimal spec with:
/// - Application type: ACTIVE
/// - One-for-one supervision strategy
/// - Single worker actor with the application name as ID
///
/// ## Arguments
/// * `name` - Application/actor name
/// * `version` - Application version
/// * `behavior_kind` - Optional OTP-style behavior for logging (e.g. "GenEvent" for event-handler actors)
///
/// ## Returns
/// ApplicationSpec with default supervisor tree
pub fn create_default_application_spec(
    name: &str,
    version: &str,
    behavior_kind: Option<&str>,
) -> ApplicationSpec {
    let default_supervisor = SupervisorSpec {
        strategy: SupervisionStrategy::SupervisionStrategyOneForOne.into(),
        max_restarts: 5,
        max_restart_window: None,
        children: vec![ChildSpec {
            actor_identity: Some(ActorIdentity {
                name: name.to_string(),
                actor_type: wasm_worker_actor_type_from_application_name(name),
            }),
            role: "worker".to_string(),
            restart: RestartPolicy::RestartPolicyPermanent.into(),
            behavior_kind: behavior_kind.map(String::from),
            ..Default::default()
        }],
        ..Default::default()
    };

    ApplicationSpec {
        name: name.to_string(),
        tenant_id: String::new(), // Set by deployment code from JWT
        version: version.to_string(),
        description: format!("WASM application: {}", name),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: std::collections::HashMap::new(),
        supervisor: Some(default_supervisor),
        // Deployment configuration (merged from ApplicationConfig)
        enabled: true,
        auto_start: true,
        shutdown_timeout: Some(prost_types::Duration {
            seconds: 60,
            nanos: 0,
        }),
        shutdown_strategy: ShutdownStrategy::ShutdownStrategyGraceful.into(),
        metadata: None,
        seed_nodes: vec![],
        required_service_links: vec![],
    }
}

/// Application service implementation
#[derive(Clone)]
pub struct ApplicationServiceImpl {
    service_locator: Arc<dyn ServiceLocator>,
    /// Injected NodeConnectivity for connecting to ApplicationSpec.seed_nodes on deploy (set by node).
    node_connectivity: Option<Arc<dyn plexspaces_actor::NodeConnectivity>>,
}

impl ApplicationServiceImpl {
    /// Creates a new application service.
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for ApplicationManager, WASM runtime, etc.
    /// * `node_connectivity` - Optional; when set (e.g. by node), deploy will connect to spec.seed_nodes.
    pub fn new(
        service_locator: Arc<dyn ServiceLocator>,
        node_connectivity: Option<Arc<dyn plexspaces_actor::NodeConnectivity>>,
    ) -> Self {
        Self {
            service_locator,
            node_connectivity,
        }
    }

    /// Get ApplicationManager from ServiceLocator (as concrete type)
    async fn get_application_manager(
        &self,
    ) -> Result<Arc<plexspaces_application::ApplicationManagerImpl>, Status> {
        let manager_trait = self
            .service_locator
            .application_manager()
            .await
            .ok_or_else(|| {
                Status::failed_precondition("ApplicationManager not registered in ServiceLocator")
            })?;
        // Downcast to concrete type to access methods like register, start, stop
        // Note: as_any takes self: Arc<Self>, so we pass the cloned Arc
        let manager_any = plexspaces_actor::ApplicationManager::as_any(manager_trait.clone());
        manager_any
            .downcast::<plexspaces_application::ApplicationManagerImpl>()
            .map_err(|_| Status::internal("Failed to downcast ApplicationManager to concrete type"))
    }

    /// Get WASM runtime from ServiceLocator
    async fn get_wasm_runtime(
        &self,
    ) -> Result<Arc<dyn plexspaces_actor::WasmRuntimeTrait>, Status> {
        self.service_locator
            .get_wasm_runtime()
            .await
            .ok_or_else(|| {
                Status::failed_precondition(
                    "WASM runtime not initialized - node may not be started",
                )
            })
    }

    async fn local_status_endpoint(&self) -> Result<(String, String), Status> {
        let node_id = self
            .service_locator
            .get_node_config()
            .await
            .filter(|c| !c.id.is_empty())
            .map(|c| c.id)
            .unwrap_or_default();

        let node_address = if node_id.is_empty() {
            String::new()
        } else {
            let ctx = self
                .service_locator
                .request_context_for_system_operations()
                .await;
            match self.service_locator.get_node_registry().await {
                Some(node_registry) => node_registry
                    .lookup_node(&ctx, &node_id)
                    .await
                    .ok()
                    .flatten()
                    .map(|r| r.node_address)
                    .unwrap_or_default(),
                None => String::new(),
            }
        };
        Ok((node_id, node_address))
    }

    /// Remove the on-disk WASM app directory for `application_id` if it exists.
    /// Non-fatal: logs a warning on failure so undeploy still succeeds.
    async fn remove_wasm_app_directory(&self, application_id: &str) {
        let wasm_apps_dir = self
            .service_locator
            .get_runtime_config()
            .await
            .map(|rc| rc.wasm_apps_directory)
            .unwrap_or_default();

        if wasm_apps_dir.is_empty() {
            return;
        }

        let app_dir = std::path::Path::new(&wasm_apps_dir).join(application_id);
        if !app_dir.exists() {
            return;
        }

        match std::fs::remove_dir_all(&app_dir) {
            Ok(()) => tracing::info!(
                application_id = %application_id,
                path = %app_dir.display(),
                "Removed WASM app directory on undeploy"
            ),
            Err(e) => tracing::warn!(
                application_id = %application_id,
                path = %app_dir.display(),
                error = %e,
                "Failed to remove WASM app directory on undeploy"
            ),
        }
    }

    async fn cleanup_namespace_for_undeploy(
        &self,
        tenant_id: &str,
        application_id: &str,
    ) -> Result<(), Status> {
        let namespace = application_id.to_string();
        let ctx = plexspaces_actor::RequestContext::new_without_auth(
            tenant_id.to_string(),
            namespace.clone(),
        );

        let virtual_cleanup =
            if let Some(manager) = self.service_locator.virtual_actor_manager().await {
                manager.unregister_namespace(&namespace).await
            } else {
                plexspaces_actor::virtual_actor_manager::VirtualActorNamespaceCleanup::default()
            };

        let mut purged_records = 0_u64;
        if let Some(journal_storage) = self.service_locator.get_journal_storage().await {
            for actor_id in &virtual_cleanup.actor_ids {
                purged_records += journal_storage
                    .purge_actor(actor_id)
                    .await
                    .map_err(|e| Status::internal(format!("Failed to purge actor state: {}", e)))?;
            }
            purged_records += journal_storage
                .purge_namespace(&namespace)
                .await
                .map_err(|e| Status::internal(format!("Failed to purge namespace state: {}", e)))?;
        }

        let removed_registrations =
            if let Some(object_registry) = self.service_locator.get_object_registry().await {
                object_registry.unregister_all(&ctx).await.map_err(|e| {
                    Status::internal(format!(
                        "Failed to purge object registrations during undeploy: {}",
                        e
                    ))
                })?
            } else {
                0
            };

        tracing::info!(
            application_id = %application_id,
            namespace = %namespace,
            removed_virtual_types = virtual_cleanup.actor_types.len(),
            removed_virtual_instances = virtual_cleanup.actor_ids.len(),
            purged_records = purged_records,
            removed_registrations = removed_registrations,
            "Stateless undeploy cleanup completed"
        );

        Ok(())
    }
}

#[tonic::async_trait]
impl ApplicationService for ApplicationServiceImpl {
    async fn deploy_application(
        &self,
        request: Request<DeployApplicationRequest>,
    ) -> Result<Response<DeployApplicationResponse>, Status> {
        // Extract RequestContext from gRPC request - tenant_id/namespace come from API request
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let metadata = request.metadata().clone();
        let ctx = crate::request_context_from_grpc_request(
            &metadata,
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;

        // Extract tenant_id and namespace from RequestContext (from API request)
        let tenant_id = ctx.tenant_id().to_string();
        let namespace = ctx.namespace().to_string();

        // Check if this is an API deployment (has proper metadata) vs internal auto-deploy
        // API deployments will have metadata headers (authorization, x-tenant-id, etc.)
        // Internal auto-deploy calls won't have these headers
        let is_api_deployment = metadata.contains_key("authorization")
            || metadata.contains_key("x-tenant-id")
            || metadata.contains_key("x-namespace");

        let req = request.into_inner();

        // OBSERVABILITY: Record metrics and log deployment attempt
        metrics::counter!("plexspaces_node_application_deploy_attempts_total",
            "application_name" => req.name.clone()
        )
        .increment(1);
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                application_id = %req.application_id,
                application_name = %req.name,
                tenant_id = %tenant_id,
                namespace = %namespace,
                version = %req.version,
                has_wasm_module = req.wasm_module.is_some(),
                has_config = req.config.is_some(),
                "Deploying application"
            );
        }

        // Validate request
        if req.application_id.is_empty() {
            return Err(Status::invalid_argument("application_id is required"));
        }
        if req.name.is_empty() {
            return Err(Status::invalid_argument("name is required"));
        }
        if req.version.is_empty() {
            return Err(Status::invalid_argument("version is required"));
        }

        // Handle WASM module deployment if provided
        if let Some(wasm_module) = req.wasm_module {
            // Clone WASM bytes for potential file saving (before deployment)
            let wasm_bytes_for_save = wasm_module.module_bytes.clone();

            // Get WASM runtime from ServiceLocator
            let wasm_runtime = self.get_wasm_runtime().await?;

            // Deploy WASM module
            let deployment_service = WasmDeploymentService::new(wasm_runtime);
            let module_hash = deployment_service
                .deploy_module_for_application(
                    &wasm_module.name,
                    &wasm_module.version,
                    &wasm_module.module_bytes,
                    &tenant_id,
                    &namespace,
                    req.config.is_some(),
                )
                .await
                .map_err(|e| {
                    tracing::error!(
                        tenant_id = %tenant_id,
                        application_id = %req.application_id,
                        error = %e,
                        "Deploy failed: WASM module deployment"
                    );
                    Status::internal(format!("Failed to deploy WASM module: {}", e))
                })?;

            // Store in ApplicationManager by application_id; use name (or application_id) as display name
            let app_name = req.application_id.clone();
            let app_display_name = if req.name.is_empty() {
                req.application_id.clone()
            } else {
                req.name.clone()
            };
            let app_version = req.version.clone();

            // Get or create ApplicationSpec with default supervisor tree
            // This ensures consistent behavior with HTTP deployment path
            let mut merged_config = req.config.clone().unwrap_or_else(|| {
                // No config provided - create default spec with supervisor tree
                // This ensures at least one actor is spawned for every deployed application
                tracing::info!(
                    application_id = %req.application_id,
                    application_name = %req.name,
                    "No ApplicationSpec provided - creating default with supervisor tree"
                );
                create_default_application_spec(&req.name, &req.version, None)
            });

            // If config was provided but has no supervisor, add default supervisor
            // This handles the case where client passes minimal config without supervisor
            if merged_config.supervisor.is_none() {
                tracing::info!(
                    application_id = %req.application_id,
                    application_name = %req.name,
                    "ApplicationSpec has no supervisor - adding default supervisor tree"
                );
                let default_spec = create_default_application_spec(&req.name, &req.version, None);
                merged_config.supervisor = default_spec.supervisor;
            }

            // Resolve tenant_id for this deployment:
            // - API deployments (auth enabled): JWT → RequestContext.tenant_id takes precedence.
            // - Embedded/file-copy deploys (no JWT, no metadata): RequestContext.tenant_id is
            //   empty; fall back to the value already in the ApplicationSpec (read from TOML by
            //   parse_app_config_toml).  This preserves the operator-supplied tenant_id for
            //   file-copy WASM apps without weakening the API/JWT path.
            let final_tenant_id = if !tenant_id.is_empty() {
                tenant_id.clone()
            } else if !merged_config.tenant_id.is_empty() {
                tracing::debug!(
                    application_id = %req.application_id,
                    toml_tenant_id = %merged_config.tenant_id,
                    "Using tenant_id from ApplicationSpec (no JWT in request)"
                );
                merged_config.tenant_id.clone()
            } else {
                tracing::warn!(
                    application_id = %req.application_id,
                    "Deploying WASM application with empty tenant_id; set tenant_id in app-config.toml for multi-tenant isolation"
                );
                String::new()
            };
            merged_config.tenant_id = final_tenant_id.clone();
            merged_config.name = app_display_name.clone();
            // Namespace is always derived from the application name (spec.name).
            let final_namespace = app_name.clone();

            if !merged_config.required_service_links.is_empty() {
                let rt = self
                    .service_locator
                    .get_runtime_config()
                    .await
                    .ok_or_else(|| {
                        Status::failed_precondition(
                            "ApplicationSpec.required_service_links set but RuntimeConfig not available",
                        )
                    })?;
                plexspaces_http_client::validate_application_service_links(
                    &rt,
                    &merged_config.required_service_links,
                )
                .map_err(Status::failed_precondition)?;
            }

            // Clone values for observability logging before moving them
            let module_hash_for_log = module_hash.clone();
            let namespace_for_log = final_namespace.clone();
            let tenant_id_for_log = final_tenant_id.clone();

            // Clone merged_config for file saving (before it's moved into WasmApplication)
            let merged_config_for_save = merged_config.clone();

            // Create WasmApplication from application crate
            use plexspaces_application::wasm_application::WasmApplication;
            // Get WASM runtime again for WasmApplication
            let wasm_runtime_for_app = self.get_wasm_runtime().await?;

            let wasm_app = WasmApplication::new(
                app_display_name.clone(),
                app_name.clone(),
                app_version,
                module_hash,
                wasm_runtime_for_app,
                Some(merged_config),
            );
            let app: Box<dyn plexspaces_application::Application> = Box::new(wasm_app);

            // Register with ApplicationManager
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(
                    application_id = %req.application_id,
                    application_name = %app_name,
                    deployment_type = "wasm",
                    "Registering application with ApplicationManager"
                );
            }

            // Get ApplicationManager from ServiceLocator
            let application_manager = self.get_application_manager().await?;

            application_manager.register(&ctx, app).await.map_err(|e| {
                tracing::error!(
                    application_id = %req.application_id,
                    application_name = %app_name,
                    error = %e,
                    "Failed to register WASM application"
                );
                Status::internal(format!("Failed to register application: {}", e))
            })?;

            // Save WASM file to disk atomically ONLY if:
            // 1. save_wasm_apps is enabled
            // 2. This is an API deployment (HTTP/gRPC) - NOT auto-deploy on startup
            // Auto-deploy should NOT save files (they're already on disk from previous API deployments)
            // We check is_api_deployment flag to distinguish API calls from internal auto-deploy calls
            if is_api_deployment {
                if let Some(runtime_config) = self.service_locator.get_runtime_config().await {
                    if runtime_config.save_wasm_apps
                        && !runtime_config.wasm_apps_directory.is_empty()
                    {
                        use crate::wasm_file_saver::save_wasm_app_atomically;
                        if let Err(e) = save_wasm_app_atomically(
                            &runtime_config.wasm_apps_directory,
                            &app_name,
                            &wasm_bytes_for_save,
                            &merged_config_for_save,
                            runtime_config.save_wasm_apps,
                        ) {
                            // Log error but don't fail deployment (non-fatal)
                            tracing::warn!(
                                app_name = %app_name,
                                error = %e,
                                "Failed to save WASM file and config to disk (deployment continues)"
                            );
                        }
                    }
                }
            } else {
                tracing::debug!(
                    app_name = %app_name,
                    "Skipping WASM file save - this is an auto-deploy (file already on disk)"
                );
            }

            let node_id_for_log = self
                .service_locator
                .get_node_config()
                .await
                .map(|c| c.id.clone())
                .unwrap_or_default();

            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                node_id = %node_id_for_log,
                tenant_id = %final_tenant_id,
                namespace = %final_namespace,
                "Starting WASM application"
            );
            // Connect to ApplicationSpec.seed_nodes BEFORE start() so seeds are always
            // registered even if start() blocks for a long time (e.g. WASM compilation).
            if !merged_config_for_save.seed_nodes.is_empty() {
                if let Some(conn) = self.node_connectivity.clone() {
                    let addrs = merged_config_for_save.seed_nodes.clone();
                    match conn.connect_to_node_addresses(addrs).await {
                        Ok(r) => tracing::info!(
                            application_id = %req.application_id,
                            connected = r.connected.len(),
                            failed = r.failed.len(),
                            "Connected to application seed_nodes"
                        ),
                        Err(e) => tracing::warn!(
                            application_id = %req.application_id,
                            error = %e,
                            "Failed to connect to application seed_nodes"
                        ),
                    }
                }
            }

            // No rollback on failure: we return the first error so the client always sees the
            // original deploy/start failure, not any subsequent cleanup error.
            application_manager.start(&app_name).await.map_err(|e| {
                tracing::error!(
                    application_id = %req.application_id,
                    application_name = %app_name,
                    error = %e,
                    "Failed to start WASM application"
                );
                Status::internal(format!("Failed to start application: {}", e))
            })?;

            // OBSERVABILITY: Log successful deployment with metrics
            metrics::counter!("plexspaces_node_application_deploy_success_total",
                "application_name" => app_name.clone()
            )
            .increment(1);
            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                version = %req.version,
                module_hash = %module_hash_for_log,
                namespace = %namespace_for_log,
                tenant_id = %tenant_id_for_log,
                "WASM application deployed and started successfully"
            );

            return Ok(Response::new(DeployApplicationResponse {
                success: true,
                application_id: req.application_id,
                status: ApplicationStatus::ApplicationStatusRunning.into(),
                error: None,
            }));
        }

        // Create Application from ApplicationSpec
        let config = req.config.ok_or_else(|| {
            Status::invalid_argument("config is required (WASM deployment not yet implemented)")
        })?;

        let mut merged_config = config.clone();
        merged_config.name = req.application_id.clone();
        // Prefer JWT tenant_id; fall back to TOML-supplied value for file-copy deploys.
        merged_config.tenant_id = if !tenant_id.is_empty() {
            tenant_id.clone()
        } else {
            merged_config.tenant_id.clone()
        };
        if !merged_config.required_service_links.is_empty() {
            let rt = self
                .service_locator
                .get_runtime_config()
                .await
                .ok_or_else(|| {
                    Status::failed_precondition(
                        "ApplicationSpec.required_service_links set but RuntimeConfig not available",
                    )
                })?;
            plexspaces_http_client::validate_application_service_links(
                &rt,
                &merged_config.required_service_links,
            )
            .map_err(Status::failed_precondition)?;
        }
        let seed_nodes = merged_config.seed_nodes.clone();

        // Create Application instance from config; store by application_id
        let app_name = req.application_id.clone();
        use plexspaces_application::application_impl::SpecApplication;
        let spec_app = SpecApplication::new(merged_config);
        let app: Box<dyn plexspaces_application::Application> = Box::new(spec_app);

        // Get ApplicationManager from ServiceLocator
        let application_manager = self.get_application_manager().await?;

        // Register with ApplicationManager
        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Registering native application with ApplicationManager"
        );
        application_manager.register(&ctx, app).await.map_err(|e| {
            tracing::error!(
                application_id = %req.application_id,
                application_name = %app_name,
                error = %e,
                "Failed to register native application"
            );
            Status::internal(format!("Failed to register application: {}", e))
        })?;

        // Start application using ApplicationManager directly
        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Starting native application"
        );
        application_manager.start(&app_name).await.map_err(|e| {
            tracing::error!(
                application_id = %req.application_id,
                application_name = %app_name,
                error = %e,
                "Failed to start native application"
            );
            Status::internal(format!("Failed to start application: {}", e))
        })?;

        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Native application deployed and started successfully"
        );

        // Connect to ApplicationSpec.seed_nodes if configured (idempotent; already-connected are skipped)
        if !seed_nodes.is_empty() {
            if let Some(conn) = self.node_connectivity.clone() {
                let addrs = seed_nodes;
                match conn.connect_to_node_addresses(addrs).await {
                    Ok(r) => tracing::info!(
                        application_id = %req.application_id,
                        connected = r.connected.len(),
                        failed = r.failed.len(),
                        "Connected to application seed_nodes"
                    ),
                    Err(e) => tracing::warn!(
                        application_id = %req.application_id,
                        error = %e,
                        "Failed to connect to application seed_nodes"
                    ),
                }
            }
        }

        Ok(Response::new(DeployApplicationResponse {
            success: true,
            application_id: req.application_id,
            status: ApplicationStatus::ApplicationStatusRunning.into(),
            error: None,
        }))
    }

    async fn undeploy_application(
        &self,
        request: Request<UndeployApplicationRequest>,
    ) -> Result<Response<UndeployApplicationResponse>, Status> {
        // Extract RequestContext for observability (tenant_id in logs)
        let service_locator_trait: Arc<dyn plexspaces_actor::ServiceLocator> =
            self.service_locator.clone();
        let ctx = crate::request_context_from_grpc_request(
            request.metadata(),
            &std::collections::HashMap::new(),
            &service_locator_trait,
        )
        .await
        .map_err(|e| Status::invalid_argument(format!("Invalid request context: {}", e)))?;
        let tenant_id = ctx.tenant_id().to_string();

        let req = request.into_inner();

        // OBSERVABILITY: Record metrics and log undeployment attempt
        metrics::counter!("plexspaces_node_application_undeploy_attempts_total",
            "application_id" => req.application_id.clone()
        )
        .increment(1);
        let timeout_seconds = req.timeout.as_ref().map(|d| d.seconds).unwrap_or(30);
        tracing::info!(
            application_id = %req.application_id,
            tenant_id = %tenant_id,
            timeout_seconds = timeout_seconds,
            "Undeploying application"
        );

        if req.application_id.is_empty() {
            return Err(Status::invalid_argument("application_id is required"));
        }

        // Get ApplicationManager from ServiceLocator
        let application_manager = self.get_application_manager().await?;
        // Stop application gracefully using ApplicationManager directly
        let timeout =
            Duration::from_secs(req.timeout.as_ref().map(|d| d.seconds as u64).unwrap_or(30));
        let stop_result = application_manager
            .stop(&req.application_id, timeout)
            .await
            .map_err(|e| {
                let msg = e.to_string();
                let is_not_found =
                    matches!(e, AppError::NotFound(_)) || msg.to_lowercase().contains("not found");
                if is_not_found {
                    Status::not_found(msg)
                } else {
                    tracing::error!(
                        tenant_id = %tenant_id,
                        application_id = %req.application_id,
                        error = %e,
                        "Undeploy failed: Failed to stop application"
                    );
                    Status::internal(format!("Failed to stop application: {}", e))
                }
            });

        if let Err(status) = stop_result {
            if status.code() == tonic::Code::NotFound {
                self.cleanup_namespace_for_undeploy(&tenant_id, &req.application_id)
                    .await?;
                self.remove_wasm_app_directory(&req.application_id).await;

                metrics::counter!("plexspaces_node_application_undeploy_success_total",
                    "application_id" => req.application_id.clone()
                )
                .increment(1);
                tracing::info!(
                    application_id = %req.application_id,
                    "Application undeployed successfully (stateless cleanup)"
                );

                return Ok(Response::new(UndeployApplicationResponse {
                    success: true,
                    error: None,
                }));
            }

            return Err(status);
        }

        // Unregister application after successful stop (returns module hash for WASM apps)
        let module_hash = application_manager
            .unregister(&req.application_id)
            .await
            .map_err(|e| {
                tracing::error!(
                    tenant_id = %tenant_id,
                    application_id = %req.application_id,
                    error = %e,
                    "Undeploy failed: Failed to unregister application"
                );
                Status::internal(format!("Failed to unregister application: {}", e))
            })?;

        // Evict WASM module from cache to avoid memory leaks
        if let Some(ref hash) = module_hash {
            if let Ok(wasm_runtime) = self.get_wasm_runtime().await {
                if wasm_runtime.evict_module(hash).await {
                    tracing::debug!(
                        application_id = %req.application_id,
                        module_hash = %hash,
                        "WASM module evicted from cache on undeploy"
                    );
                }
            }
        }

        self.remove_wasm_app_directory(&req.application_id).await;

        // OBSERVABILITY: Log successful undeployment
        metrics::counter!("plexspaces_node_application_undeploy_success_total",
            "application_id" => req.application_id.clone()
        )
        .increment(1);
        tracing::info!(
            application_id = %req.application_id,
            "Application undeployed successfully"
        );

        Ok(Response::new(UndeployApplicationResponse {
            success: true,
            error: None,
        }))
    }

    async fn list_applications(
        &self,
        _request: Request<ListApplicationsRequest>,
    ) -> Result<Response<ListApplicationsResponse>, Status> {
        // Record metrics
        metrics::counter!("plexspaces_node_application_list_requests_total").increment(1);
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!("Listing applications");
        }
        // Get ApplicationManager from ServiceLocator
        let application_manager = self.get_application_manager().await?;

        // Get list of applications from ApplicationManager
        let app_manager: &dyn ApplicationManagerTrait = application_manager.as_ref();
        let app_names = app_manager.list_applications().await;

        // Get full ApplicationInfo for each application
        let mut applications = Vec::new();
        for name in app_names {
            if let Some(info) = app_manager.get_application_info(&name).await {
                applications.push(info);
            }
        }

        Ok(Response::new(ListApplicationsResponse { applications }))
    }

    async fn get_application_status(
        &self,
        request: Request<GetApplicationStatusRequest>,
    ) -> Result<Response<GetApplicationStatusResponse>, Status> {
        let req = request.into_inner();

        if req.application_id.is_empty() {
            return Err(Status::invalid_argument("application_id is required"));
        }

        // Get ApplicationManager from ServiceLocator
        let application_manager = self.get_application_manager().await?;

        // Get application status from ApplicationManager
        let app_manager: &dyn ApplicationManagerTrait = application_manager.as_ref();
        let state = app_manager.get_state(&req.application_id).await;

        match state {
            Some(app_state) => {
                // Get full ApplicationInfo from ApplicationManager
                let application = app_manager.get_application_info(&req.application_id).await;

                let _proto_status = match app_state {
                    CoreApplicationState::ApplicationStateUnspecified => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusUnspecified
                    }
                    CoreApplicationState::ApplicationStateCreated => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusLoading
                    }
                    CoreApplicationState::ApplicationStateStarting => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusStarting
                    }
                    CoreApplicationState::ApplicationStateRunning => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusRunning
                    }
                    CoreApplicationState::ApplicationStateStopping => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusStopping
                    }
                    CoreApplicationState::ApplicationStateStopped => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusStopped
                    }
                    CoreApplicationState::ApplicationStateFailed => {
                        plexspaces_proto::application::v1::ApplicationStatus::ApplicationStatusFailed
                    }
                };

                // Get environment variables from ApplicationSpec if available
                use plexspaces_application::application_manager_ext::ApplicationManagerExt;
                use plexspaces_application::ApplicationManager;
                let manager: Arc<ApplicationManager> = application_manager.clone();
                let env =
                    ApplicationManagerExt::get_application_spec(&manager, &req.application_id)
                        .await
                        .map(|spec| spec.env)
                        .unwrap_or_default();

                // Build runtime state from application info if available
                let state = application.as_ref().map(|app_info| {
                    ApplicationRuntimeState {
                        name: app_info.name.clone(),
                        status: app_info.status,
                        start_timestamp_ms: app_info
                            .deployed_at
                            .as_ref()
                            .map(|ts| ts.seconds * 1000)
                            .unwrap_or(0),
                        supervisor_pid: None, // Supervisor PID not tracked yet
                        env,
                    }
                });
                let (node_id, node_address) = self.local_status_endpoint().await?;

                Ok(Response::new(GetApplicationStatusResponse {
                    application,
                    state,
                    error: None,
                    node_id,
                    node_address,
                }))
            }
            // Return self-identifying metadata even for not-found responses so callers probing
            // candidate endpoints can reconcile the responding node deterministically.
            None => {
                let (node_id, node_address) = self.local_status_endpoint().await?;
                Ok(Response::new(GetApplicationStatusResponse {
                    application: None,
                    state: None,
                    error: Some("Application not found".to_string()),
                    node_id,
                    node_address,
                }))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServiceLocatorImpl;
    use plexspaces_actor::NodeConnectivity;
    use plexspaces_application::ApplicationManagerImpl;
    use plexspaces_proto::node::v1::NodeConfig;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tonic::metadata::MetadataValue;

    struct TestNode {
        service_locator: Arc<dyn plexspaces_actor::ServiceLocator>,
    }
    impl plexspaces_application::ApplicationNode for TestNode {
        fn id(&self) -> &str {
            "test-node"
        }
        fn listen_addr(&self) -> &str {
            "127.0.0.1:0"
        }
        fn service_locator(&self) -> Option<Arc<dyn plexspaces_actor::ServiceLocator>> {
            Some(self.service_locator.clone())
        }
    }

    #[derive(Default)]
    struct RecordingNodeConnectivity {
        calls: Mutex<Vec<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl NodeConnectivity for RecordingNodeConnectivity {
        async fn connect_to_nodes(
            &self,
            node_addresses: Vec<String>,
            _timeout_secs: Option<u64>,
        ) -> Result<plexspaces_actor::ConnectNodesResult, String> {
            self.calls
                .lock()
                .expect("recording connectivity lock poisoned")
                .push(node_addresses.clone());
            Ok(plexspaces_actor::ConnectNodesResult {
                connected: node_addresses
                    .into_iter()
                    .map(|address| (address.clone(), address))
                    .collect(),
                failed: HashMap::new(),
            })
        }
    }

    #[tokio::test]
    async fn deploy_application_connects_seed_nodes_for_native_specs() {
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        service_locator
            .register_security_config(plexspaces_proto::node::v1::SecurityConfig {
                disable_auth: true,
                oidc: None,
                ..Default::default()
            })
            .await;
        service_locator
            .register_node_config(NodeConfig {
                id: "node-a".to_string(),
                listen_addr: "127.0.0.1:50051".to_string(),
                ..Default::default()
            })
            .await;

        let app_manager = Arc::new(ApplicationManagerImpl::new());
        app_manager
            .set_node_context(Arc::new(TestNode {
                service_locator: service_locator.clone(),
            }))
            .await;
        service_locator
            .register_application_manager(app_manager)
            .await;

        let connectivity = Arc::new(RecordingNodeConnectivity::default());
        let service = ApplicationServiceImpl::new(
            service_locator,
            Some(connectivity.clone() as Arc<dyn NodeConnectivity>),
        );

        // Use a minimal spec with no supervisor so start() doesn't try to spawn actors.
        let mut spec = ApplicationSpec {
            name: "seeded-app".to_string(),
            version: "1.0.0".to_string(),
            ..Default::default()
        };
        spec.seed_nodes = vec!["127.0.0.1:8091".to_string(), "127.0.0.1:8093".to_string()];

        let mut request = Request::new(DeployApplicationRequest {
            application_id: "seeded-app".to_string(),
            name: "seeded-app".to_string(),
            version: "1.0.0".to_string(),
            wasm_module: None,
            config: Some(spec),
            initial_state: vec![],
        });
        request
            .metadata_mut()
            .insert("x-tenant-id", MetadataValue::from_static(""));
        request
            .metadata_mut()
            .insert("x-namespace", MetadataValue::from_static("seeded-app"));

        let response = service
            .deploy_application(request)
            .await
            .unwrap()
            .into_inner();
        assert!(response.success);

        let calls = connectivity
            .calls
            .lock()
            .expect("recording connectivity lock poisoned");
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0],
            vec!["127.0.0.1:8091".to_string(), "127.0.0.1:8093".to_string()]
        );
    }

    #[test]
    fn create_default_application_spec_uses_app_name() {
        let spec = create_default_application_spec("heat-diffusion-rust", "1.0.0", None);
        assert_eq!(spec.name, "heat-diffusion-rust");
    }
}
