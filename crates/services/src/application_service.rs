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

use plexspaces_core::{ServiceLocator, ApplicationManager as ApplicationManagerTrait, object_registry_helpers};
use plexspaces_proto::v1::application::ApplicationState as CoreApplicationState;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec,
    DeployApplicationRequest, DeployApplicationResponse, GetApplicationStatusRequest,
    GetApplicationStatusResponse, ListApplicationsRequest, ListApplicationsResponse,
    UndeployApplicationRequest, UndeployApplicationResponse, ApplicationInfo, ApplicationMetrics,
    ApplicationStatus, ApplicationRuntimeState, Application,
};
use plexspaces_wasm_runtime::{WasmRuntime, WasmDeploymentService};
use std::sync::Arc;
use std::time::Duration;
use tonic::{Request, Response, Status};

/// Application service implementation
#[derive(Clone)]
pub struct ApplicationServiceImpl {
    service_locator: Arc<dyn ServiceLocator>,
}

impl ApplicationServiceImpl {
    /// Create new application service
    ///
    /// ## Arguments
    /// * `service_locator` - ServiceLocator for accessing services (ApplicationManager, WASM runtime, etc.)
    pub fn new(
        service_locator: Arc<dyn ServiceLocator>,
    ) -> Self {
        Self {
            service_locator,
        }
    }
    
    /// Get ApplicationManager from ServiceLocator (as concrete type)
    async fn get_application_manager(&self) -> Result<Arc<plexspaces_application::ApplicationManagerImpl>, Status> {
        let manager_trait = self.service_locator.application_manager().await
            .ok_or_else(|| Status::failed_precondition("ApplicationManager not registered in ServiceLocator"))?;
        // Downcast to concrete type to access methods like register, start, stop
        // Note: as_any takes self: Arc<Self>, so we pass the cloned Arc
        let manager_any = plexspaces_core::ApplicationManager::as_any(manager_trait.clone());
        manager_any
            .downcast::<plexspaces_application::ApplicationManagerImpl>()
            .map_err(|_| Status::internal("Failed to downcast ApplicationManager to concrete type"))
    }
    
    /// Get WASM runtime from ServiceLocator
    async fn get_wasm_runtime(&self) -> Result<Arc<dyn plexspaces_core::WasmRuntimeTrait>, Status> {
        self.service_locator.get_wasm_runtime().await
            .ok_or_else(|| Status::failed_precondition("WASM runtime not initialized - node may not be started"))
    }
}

#[tonic::async_trait]
impl ApplicationService for ApplicationServiceImpl {
    async fn deploy_application(
        &self,
        request: Request<DeployApplicationRequest>,
    ) -> Result<Response<DeployApplicationResponse>, Status> {
        let req = request.into_inner();
        
        // OBSERVABILITY: Record metrics and log deployment attempt
        metrics::counter!("plexspaces_node_application_deploy_attempts_total",
            "application_name" => req.name.clone()
        ).increment(1);
        tracing::info!(
            application_id = %req.application_id,
            application_name = %req.name,
            version = %req.version,
            has_wasm_module = req.wasm_module.is_some(),
            has_config = req.config.is_some(),
            has_release_config = req.release_config.is_some(),
            "Deploying application"
        );

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
            // Get WASM runtime from ServiceLocator
            let wasm_runtime = self.get_wasm_runtime().await?;
            
            // Deploy WASM module
            let deployment_service = WasmDeploymentService::new(wasm_runtime);
            let module_hash = deployment_service
                .deploy_module(
                    &wasm_module.name,
                    &wasm_module.version,
                    &wasm_module.module_bytes,
                )
                .await
                .map_err(|e| Status::internal(format!("Failed to deploy WASM module: {}", e)))?;

            // Create WASM application
            let app_name = req.name.clone();
            let app_version = req.version.clone();
            
            // Merge release config if provided
            let mut merged_config = req.config.clone().unwrap_or_default();
            if let Some(ref release_config) = req.release_config {
                merge_release_config(&mut merged_config, release_config);
            }
            
            // Get namespace and tenant_id from NodeConfig defaults
            // This ensures proper tenant isolation instead of using RequestContext::internal()
            let (namespace, tenant_id) = {
                if let Some(node_config) = self.service_locator.get_node_config().await {
                    (
                        if node_config.default_namespace.is_empty() { "default".to_string() } else { node_config.default_namespace },
                        if node_config.default_tenant_id.is_empty() { "default".to_string() } else { node_config.default_tenant_id },
                    )
                } else {
                    // Fallback for tests or when NodeConfig is not set
                    ("default".to_string(), "default".to_string())
                }
            };
            
            // Clone values for observability logging before moving them
            let module_hash_for_log = module_hash.clone();
            let namespace_for_log = namespace.clone();
            let tenant_id_for_log = tenant_id.clone();
            
            // Create WasmApplication from application crate
            use plexspaces_application::wasm_application::WasmApplication;
            // Get WASM runtime again for WasmApplication
            let wasm_runtime_for_app = self.get_wasm_runtime().await?;
            
            let wasm_app = WasmApplication::new(
                app_name.clone(),
                app_version,
                module_hash,
                wasm_runtime_for_app,
                Some(merged_config),
            );
            let app: Box<dyn plexspaces_application::Application> = Box::new(wasm_app);

            // Register with ApplicationManager
            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                "Registering WASM application with ApplicationManager"
            );
            
            // Clone namespace/tenant_id for object-registry registration
            let namespace_for_registry = namespace.clone();
            let tenant_id_for_registry = tenant_id.clone();
            
            // Get ApplicationManager from ServiceLocator
            let application_manager = self.get_application_manager().await?;
            
            // Register application with namespace/tenant metadata
            application_manager.register_with_metadata(app, namespace, tenant_id).await
                .map_err(|e| {
                    tracing::error!(
                        application_id = %req.application_id,
                        application_name = %app_name,
                        error = %e,
                        "Failed to register WASM application"
                    );
                    Status::internal(format!("Failed to register application: {}", e))
                })?;
            
            // Register application with object-registry using proper tenant/namespace
            if let Some(object_registry) = self.service_locator.get_object_registry().await {
                use plexspaces_core::RequestContext;
                
                // Get node_id and listen_addr from NodeConfig
                let (node_id, listen_addr) = {
                    if let Some(node_config) = self.service_locator.get_node_config().await {
                        (
                            node_config.id.clone(),
                            node_config.listen_addr.clone(),
                        )
                    } else {
                        return Err(Status::failed_precondition("NodeConfig not available"));
                    }
                };
                
                let ctx = RequestContext::new_without_auth(tenant_id_for_registry.clone(), namespace_for_registry.clone());
                let grpc_address = format!("http://{}", listen_addr);
                if let Err(e) = object_registry_helpers::register_application(&object_registry, &ctx, &app_name, &req.version, &node_id, &grpc_address).await {
                    tracing::warn!(application = %app_name, error = %e, "Failed to register application with object-registry");
                } else {
                    tracing::info!(application = %app_name, node_id = %node_id, tenant_id = %tenant_id_for_registry, namespace = %namespace_for_registry, "Registered application with object-registry");
                }
            }

            // Start application using ApplicationManager directly
            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                "Starting WASM application"
            );
            application_manager
                .start(&app_name)
                .await
                .map_err(|e| {
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
            ).increment(1);
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

        // Handle release config if provided
        let mut merged_config = config.clone();
        if let Some(ref release_config) = req.release_config {
            // TODO: Handle release_spec storage in ServiceLocator or ApplicationManager
            // For now, we'll just merge the config
            merge_release_config(&mut merged_config, release_config);
        }

        // Create Application instance from merged config
        let app_name = req.name.clone();
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
        application_manager
            .register(app)
            .await
            .map_err(|e| {
                tracing::error!(
                    application_id = %req.application_id,
                    application_name = %app_name,
                    error = %e,
                    "Failed to register native application"
                );
                Status::internal(format!("Failed to register application: {}", e))
            })?;
        
        // Register application with object-registry using proper tenant/namespace
        if let Some(object_registry) = self.service_locator.get_object_registry().await {
            use plexspaces_core::RequestContext;
            // Get namespace and tenant_id from NodeConfig defaults
            let (namespace, tenant_id) = {
                if let Some(node_config) = self.service_locator.get_node_config().await {
                    (
                        if node_config.default_namespace.is_empty() { "default".to_string() } else { node_config.default_namespace },
                        if node_config.default_tenant_id.is_empty() { "default".to_string() } else { node_config.default_tenant_id },
                    )
                } else {
                    // Fallback for tests or when NodeConfig is not set
                    ("default".to_string(), "default".to_string())
                }
            };
            
            // Get node_id and listen_addr from NodeConfig
            let (node_id, listen_addr) = {
                if let Some(node_config) = self.service_locator.get_node_config().await {
                    (
                        node_config.id.clone(),
                        node_config.listen_addr.clone(),
                    )
                } else {
                    return Err(Status::failed_precondition("NodeConfig not available"));
                }
            };
            
            let ctx = RequestContext::new_without_auth(tenant_id.clone(), namespace.clone());
            let grpc_address = format!("http://{}", listen_addr);
            if let Err(e) = object_registry_helpers::register_application(&object_registry, &ctx, &app_name, &req.version, &node_id, &grpc_address).await {
                tracing::warn!(application = %app_name, error = %e, "Failed to register application with object-registry");
            } else {
                tracing::info!(application = %app_name, node_id = %node_id, tenant_id = %tenant_id, namespace = %namespace, "Registered application with object-registry");
            }
        }

        // Start application using ApplicationManager directly
        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Starting native application"
        );
        application_manager
            .start(&app_name)
            .await
            .map_err(|e| {
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
        let req = request.into_inner();
        
        // OBSERVABILITY: Record metrics and log undeployment attempt
        metrics::counter!("plexspaces_node_application_undeploy_attempts_total",
            "application_id" => req.application_id.clone()
        ).increment(1);
        let timeout_seconds = req.timeout.as_ref().map(|d| d.seconds).unwrap_or(30);
        tracing::info!(
            application_id = %req.application_id,
            timeout_seconds = timeout_seconds,
            "Undeploying application"
        );

        if req.application_id.is_empty() {
            return Err(Status::invalid_argument("application_id is required"));
        }

        // Get ApplicationManager from ServiceLocator
        let application_manager = self.get_application_manager().await?;
        
        // Stop application gracefully using ApplicationManager directly
        let timeout = Duration::from_secs(req.timeout.as_ref().map(|d| d.seconds as u64).unwrap_or(30));
        application_manager
            .stop(&req.application_id, timeout)
            .await
            .map_err(|e| Status::internal(format!("Failed to stop application: {}", e)))?;

        // Unregister application after successful stop
        application_manager
            .unregister(&req.application_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to unregister application: {}", e)))?;

        // OBSERVABILITY: Log successful undeployment
        metrics::counter!("plexspaces_node_application_undeploy_success_total",
            "application_id" => req.application_id.clone()
        ).increment(1);
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

        Ok(Response::new(ListApplicationsResponse {
            applications,
        }))
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
                let application = app_manager
                    .get_application_info(&req.application_id)
                    .await;

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
                let env = ApplicationManagerExt::get_application_spec(&manager, &req.application_id)
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
                            .map(|ts| (ts.seconds * 1000) as i64)
                            .unwrap_or(0),
                        supervisor_pid: None, // Supervisor PID not tracked yet
                        env,
                    }
                });

                Ok(Response::new(GetApplicationStatusResponse {
                    application,
                    state,
                    error: None,
                }))
            }
            None => Ok(Response::new(GetApplicationStatusResponse {
                application: None,
                state: None,
                error: Some("Application not found".to_string()),
            })),
        }
    }
}

/// Merge release configuration into application configuration
///
/// ## Purpose
/// Merges release-level settings (environment variables, runtime config, etc.)
/// into application configuration. Release config takes precedence.
///
/// ## Merge Strategy
/// - Environment variables: Release env overrides application env
/// - Other settings: Application config takes precedence (release config is node-level)
fn merge_release_config(
    app_config: &mut plexspaces_proto::application::v1::ApplicationSpec,
    release_config: &plexspaces_proto::node::v1::ReleaseSpec,
) {
    // Merge environment variables
    // Release env overrides application env
    for (key, value) in &release_config.env {
        app_config.env.insert(key.clone(), value.clone());
    }

    // Note: Other release config fields (node, runtime, shutdown) are node-level
    // and don't need to be merged into application config. They are applied
    // at the node level when the node starts.
}

