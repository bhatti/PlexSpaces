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

use crate::application_impl::SpecApplication;
use crate::application_manager::ApplicationManager;
use crate::wasm_application::WasmApplication;
use crate::Node;
use std::sync::Arc;
use plexspaces_core::application::ApplicationState as CoreApplicationState;
use plexspaces_proto::application::v1::{
    application_service_server::ApplicationService, ApplicationSpec,
    DeployApplicationRequest, DeployApplicationResponse, GetApplicationStatusRequest,
    GetApplicationStatusResponse, ListApplicationsRequest, ListApplicationsResponse,
    UndeployApplicationRequest, UndeployApplicationResponse, ApplicationInfo, ApplicationMetrics,
    ApplicationStatus, ApplicationRuntimeState, StartApplicationRequest, StartApplicationResponse,
    StopApplicationRequest, StopApplicationResponse, Application,
};
use plexspaces_proto::node::v1::ReleaseSpec;
use plexspaces_wasm_runtime::WasmDeploymentService;
use tonic::{Request, Response, Status};

/// Application service implementation
pub struct ApplicationServiceImpl {
    node: Arc<Node>,
    application_manager: Arc<ApplicationManager>,
}

impl ApplicationServiceImpl {
    /// Create new application service
    pub fn new(node: Arc<Node>, application_manager: Arc<ApplicationManager>) -> Self {
        Self {
            node,
            application_manager,
        }
    }
}

#[tonic::async_trait]
impl ApplicationService for ApplicationServiceImpl {
    async fn deploy_application(
        &self,
        request: Request<DeployApplicationRequest>,
    ) -> Result<Response<DeployApplicationResponse>, Status> {
        let req = request.into_inner();
        
        // Record metrics
        metrics::counter!("plexspaces_node_application_deploy_attempts_total",
            "application_name" => req.name.clone()
        ).increment(1);
        tracing::info!(
            application_id = %req.application_id,
            application_name = %req.name,
            version = %req.version,
            has_wasm_module = req.wasm_module.is_some(),
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
            // Get WASM runtime from Node
            let wasm_runtime = self.node.wasm_runtime().await.ok_or_else(|| {
                Status::failed_precondition("WASM runtime not initialized - node may not be started")
            })?;

            // Deploy WASM module
            let deployment_service = WasmDeploymentService::new(wasm_runtime.clone());
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
            
            let wasm_app = WasmApplication::new(
                app_name.clone(),
                app_version,
                module_hash,
                wasm_runtime,
                Some(merged_config),
            );
            let app: Box<dyn plexspaces_core::application::Application> = Box::new(wasm_app);

            // Register with ApplicationManager
            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                "Registering WASM application with ApplicationManager"
            );
            self.node
                .register_application(app)
                .await
                .map_err(|e| {
                    tracing::error!(
                        application_id = %req.application_id,
                        application_name = %app_name,
                        error = %e,
                        "Failed to register WASM application"
                    );
                    Status::internal(format!("Failed to register application: {}", e))
                })?;

            // Start application
            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
                "Starting WASM application"
            );
            self.node
                .start_application(&app_name)
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

            tracing::info!(
                application_id = %req.application_id,
                application_name = %app_name,
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
        // 1. Extract NodeConfig from release_config.node and set it on Node (if not already set)
        // 2. Merge release config with application config
        let mut merged_config = config.clone();
        if let Some(ref release_config) = req.release_config {
            // If Node doesn't have release_spec set yet, set it from the deployment request
            // This allows gRPC deployments to configure the node
            {
                let node_release_spec = self.node.get_release_spec().await;
                if node_release_spec.is_none() {
                    // Set release config on node (this will update NodeConfig in ServiceLocator)
                    self.node.set_release_spec(release_config.clone()).await;
                    tracing::info!("Set release config on node from deployment request");
                } else {
                    // Node already has release config - log but don't override
                    tracing::debug!("Node already has release config, not overriding from deployment request");
                }
            }
            
            // Merge release config with application config
            merge_release_config(&mut merged_config, release_config);
        }

        // Create Application instance from merged config
        let app_name = req.name.clone();
        let spec_app = SpecApplication::new(merged_config);
        let app: Box<dyn plexspaces_core::application::Application> = Box::new(spec_app);

        // Register with ApplicationManager
        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Registering native application with ApplicationManager"
        );
        self.node
            .register_application(app)
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

        // Start application
        tracing::info!(
            application_id = %req.application_id,
            application_name = %app_name,
            "Starting native application"
        );
        self.node
            .start_application(&app_name)
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
        
        // Record metrics
        metrics::counter!("plexspaces_node_application_undeploy_attempts_total",
            "application_id" => req.application_id.clone()
        ).increment(1);
        tracing::info!(application_id = %req.application_id, "Undeploying application");

        if req.application_id.is_empty() {
            return Err(Status::invalid_argument("application_id is required"));
        }

        // Stop application gracefully
        let timeout = std::time::Duration::from_secs(30); // Default timeout
        self.node
            .stop_application(&req.application_id, timeout)
            .await
            .map_err(|e| Status::internal(format!("Failed to stop application: {}", e)))?;

        // Unregister application after successful stop
        self.application_manager
            .unregister(&req.application_id)
            .await
            .map_err(|e| Status::internal(format!("Failed to unregister application: {}", e)))?;

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
        tracing::debug!("Listing applications");
        // Get list of applications from ApplicationManager
        let app_names = self.application_manager.list_applications().await;
        
        // Get full ApplicationInfo for each application (collect futures first, then await)
        let mut applications = Vec::new();
        for name in app_names {
            if let Some(info) = self.application_manager.get_application_info(&name).await {
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

        // Get application status from ApplicationManager
        let state = self.application_manager.get_state(&req.application_id).await;
        
        match state {
            Some(app_state) => {
                // Get full ApplicationInfo from ApplicationManager
                let application = self
                    .application_manager
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
                let env = self.application_manager
                    .get_application_spec(&req.application_id)
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

    async fn start_application(
        &self,
        _request: Request<StartApplicationRequest>,
    ) -> Result<Response<StartApplicationResponse>, Status> {
        // Legacy method - use DeployApplication instead
        Err(Status::unimplemented("Use DeployApplication instead"))
    }

    async fn stop_application(
        &self,
        _request: Request<StopApplicationRequest>,
    ) -> Result<Response<StopApplicationResponse>, Status> {
        // Legacy method - use UndeployApplication instead
        Err(Status::unimplemented("Use UndeployApplication instead"))
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

