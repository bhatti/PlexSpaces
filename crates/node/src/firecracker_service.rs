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

//! Firecracker VM Service gRPC implementation
//!
//! ## Purpose
//! Provides gRPC service for managing Firecracker VMs and deploying applications to them.
//! This enables application-level isolation using Firecracker microVMs.

#[cfg(feature = "firecracker")]
use crate::Node;

#[cfg(feature = "firecracker")]
use plexspaces_firecracker::{
    ApplicationDeployment, FirecrackerVm, VmConfig, VmState,
};
#[cfg(feature = "firecracker")]
use plexspaces_proto::firecracker::v1::{
    firecracker_vm_service_server::FirecrackerVmService,
    CreateVmRequest, CreateVmResponse, BootVmRequest, BootVmResponse,
    PauseVmRequest, PauseVmResponse, ResumeVmRequest, ResumeVmResponse,
    StopVmRequest, StopVmResponse, GetVmStateRequest, GetVmStateResponse,
    ListVmsRequest, ListVmsResponse, DeployApplicationRequest, DeployApplicationResponse,
    UndeployApplicationRequest, UndeployApplicationResponse,
    VmInstance, FirecrackerError as ProtoFirecrackerError,
};

#[cfg(feature = "firecracker")]
use std::collections::HashMap;

#[cfg(feature = "firecracker")]
use std::sync::Arc;

#[cfg(feature = "firecracker")]
use tokio::sync::RwLock;

#[cfg(feature = "firecracker")]
use tonic::{Request, Response, Status};

/// Firecracker VM Service implementation
#[cfg(feature = "firecracker")]
pub struct FirecrackerVmServiceImpl {
    node: Arc<Node>,
    /// Active VMs by VM ID
    vms: Arc<RwLock<HashMap<String, Arc<RwLock<FirecrackerVm>>>>>,
}

impl FirecrackerVmServiceImpl {
    /// Create new Firecracker VM Service
    pub fn new(node: Arc<Node>) -> Self {
        Self {
            node,
            vms: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[cfg(feature = "firecracker")]
#[tonic::async_trait]
impl FirecrackerVmService for FirecrackerVmServiceImpl {
    async fn create_vm(
        &self,
        request: Request<CreateVmRequest>,
    ) -> Result<Response<CreateVmResponse>, Status> {
        let req = request.into_inner();
        
        // Convert proto VmConfig to Firecracker VmConfig
        let vm_config = req.config.ok_or_else(|| {
            Status::invalid_argument("VM configuration is required")
        })?;

        let vm_id = vm_config.vm_id.clone();
        tracing::info!(vm_id = %vm_id, "Creating Firecracker VM");

        let config = VmConfig {
            vm_id: vm_id.clone(),
            vcpu_count: vm_config.vcpu_count as u8,
            mem_size_mib: vm_config.mem_size_mib as u32,
            ..Default::default()
        };

        // Create VM
        let vm = FirecrackerVm::create(config).await.map_err(|e| {
            tracing::error!(vm_id = %vm_id, error = %e, "Failed to create VM");
            Status::internal(format!("Failed to create VM: {}", e))
        })?;

        // Get socket path from VM (if available)
        let socket_path = String::new(); // TODO: Get actual socket path from VM

        // Store VM
        let vm_arc = Arc::new(RwLock::new(vm));
        {
            let mut vms = self.vms.write().await;
            vms.insert(vm_id.clone(), vm_arc.clone());
        }

        tracing::info!(vm_id = %vm_id, "VM created successfully");

        Ok(Response::new(CreateVmResponse {
            success: true,
            vm_id,
            socket_path,
            error: None,
        }))
    }

    async fn boot_vm(
        &self,
        request: Request<BootVmRequest>,
    ) -> Result<Response<BootVmResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(vm_id = %req.vm_id, "Booting Firecracker VM");

        let vm = {
            let vms = self.vms.read().await;
            vms.get(&req.vm_id)
                .ok_or_else(|| Status::not_found(format!("VM {} not found", req.vm_id)))?
                .clone()
        };

        let mut vm_guard = vm.write().await;
        vm_guard.boot().await.map_err(|e| {
            tracing::error!(vm_id = %req.vm_id, error = %e, "Failed to boot VM");
            Status::internal(format!("Failed to boot VM: {}", e))
        })?;

        let state = vm_guard.state();

        tracing::info!(vm_id = %req.vm_id, state = ?state, "VM booted successfully");

        Ok(Response::new(BootVmResponse {
            success: true,
            state: map_vm_state_to_proto(state),
            error: None,
        }))
    }

    async fn pause_vm(
        &self,
        request: Request<PauseVmRequest>,
    ) -> Result<Response<PauseVmResponse>, Status> {
        let req = request.into_inner();
        
        let vm = {
            let vms = self.vms.read().await;
            vms.get(&req.vm_id)
                .ok_or_else(|| Status::not_found(format!("VM {} not found", req.vm_id)))?
                .clone()
        };

        let mut vm_guard = vm.write().await;
        vm_guard.pause().await.map_err(|e| {
            Status::internal(format!("Failed to pause VM: {}", e))
        })?;

        let state = vm_guard.state();

        Ok(Response::new(PauseVmResponse {
            success: true,
            state: map_vm_state_to_proto(state),
            error: None,
        }))
    }

    async fn resume_vm(
        &self,
        request: Request<ResumeVmRequest>,
    ) -> Result<Response<ResumeVmResponse>, Status> {
        let req = request.into_inner();
        
        let vm = {
            let vms = self.vms.read().await;
            vms.get(&req.vm_id)
                .ok_or_else(|| Status::not_found(format!("VM {} not found", req.vm_id)))?
                .clone()
        };

        let mut vm_guard = vm.write().await;
        vm_guard.resume().await.map_err(|e| {
            Status::internal(format!("Failed to resume VM: {}", e))
        })?;

        let state = vm_guard.state();

        Ok(Response::new(ResumeVmResponse {
            success: true,
            state: map_vm_state_to_proto(state),
            error: None,
        }))
    }

    async fn stop_vm(
        &self,
        request: Request<StopVmRequest>,
    ) -> Result<Response<StopVmResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(vm_id = %req.vm_id, force = req.force, "Stopping Firecracker VM");

        let vm = {
            let mut vms = self.vms.write().await;
            vms.remove(&req.vm_id)
                .ok_or_else(|| Status::not_found(format!("VM {} not found", req.vm_id)))?
        };

        let mut vm_guard = vm.write().await;
        vm_guard.stop().await.map_err(|e| {
            tracing::error!(vm_id = %req.vm_id, error = %e, "Failed to stop VM");
            Status::internal(format!("Failed to stop VM: {}", e))
        })?;

        tracing::info!(vm_id = %req.vm_id, "VM stopped successfully");

        Ok(Response::new(StopVmResponse {
            success: true,
            error: None,
        }))
    }

    async fn get_vm_state(
        &self,
        request: Request<GetVmStateRequest>,
    ) -> Result<Response<GetVmStateResponse>, Status> {
        let req = request.into_inner();
        
        let vm = {
            let vms = self.vms.read().await;
            vms.get(&req.vm_id)
                .ok_or_else(|| Status::not_found(format!("VM {} not found", req.vm_id)))?
                .clone()
        };

        let vm_guard = vm.read().await;
        let state = vm_guard.state();

        let vm_instance = VmInstance {
            vm_id: req.vm_id.clone(),
            state: map_vm_state_to_proto(state),
            config: None, // TODO: Store and return config
            created_at: None,
            ..Default::default()
        };

        Ok(Response::new(GetVmStateResponse {
            success: true,
            vm: Some(vm_instance),
            error: None,
        }))
    }

    async fn list_vms(
        &self,
        request: Request<ListVmsRequest>,
    ) -> Result<Response<ListVmsResponse>, Status> {
        let req = request.into_inner();
        
        let vms = self.vms.read().await;
        let mut vm_instances = Vec::new();

        for (vm_id, vm) in vms.iter() {
            let vm_guard = vm.read().await;
            let state = vm_guard.state();

            // Filter by state if requested
            if !req.states.is_empty() {
                let proto_state = map_vm_state_to_proto(state);
                if !req.states.contains(&proto_state) {
                    continue;
                }
            }

            vm_instances.push(VmInstance {
                vm_id: vm_id.clone(),
                state: map_vm_state_to_proto(state),
                config: None,
                created_at: None,
                ..Default::default()
            });
        }

        let total_count = vm_instances.len() as u32;
        Ok(Response::new(ListVmsResponse {
            vms: vm_instances,
            next_page_token: String::new(),
            total_count,
        }))
    }

    async fn deploy_application(
        &self,
        request: Request<DeployApplicationRequest>,
    ) -> Result<Response<DeployApplicationResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(
            vm_id = %req.vm_id,
            application_id = %req.application_id,
            "Deploying application to Firecracker VM"
        );

        // Get or create VM
        let vm = {
            let vms = self.vms.read().await;
            if let Some(vm) = vms.get(&req.vm_id) {
                vm.clone()
            } else {
                // VM doesn't exist, create it with default config
                drop(vms);
                let config = VmConfig {
                    vm_id: req.vm_id.clone(),
                    ..Default::default()
                };
                let new_vm = FirecrackerVm::create(config).await.map_err(|e| {
                    Status::internal(format!("Failed to create VM: {}", e))
                })?;
                let vm_arc = Arc::new(RwLock::new(new_vm));
                let mut vms = self.vms.write().await;
                vms.insert(req.vm_id.clone(), vm_arc.clone());
                vm_arc
            }
        };

        // Ensure VM is booted
        {
            let mut vm_guard = vm.write().await;
            if vm_guard.state() == VmState::Created {
                vm_guard.boot().await.map_err(|e| {
                    Status::internal(format!("Failed to boot VM: {}", e))
                })?;
            }
        }

        // Deploy application to VM
        // Strategy: Use ApplicationService to deploy to the node, then associate with VM
        // In a full implementation, we would:
        // 1. Copy application bundle to VM filesystem
        // 2. Start PlexSpaces node inside VM
        // 3. Deploy application to that node via gRPC
        // 4. Configure networking between host and VM
        
        // For now, we use ApplicationDeployment to create the deployment configuration
        // and store the association between VM and application
        let deployment = ApplicationDeployment::new(&req.application_id)
            .with_vm_config(VmConfig {
                vm_id: req.vm_id.clone(),
                ..Default::default()
            });
        
        // Store deployment association (VM ID -> Application ID)
        // In production, this would be persisted and used for VM lifecycle management
        tracing::info!(
            vm_id = %req.vm_id,
            application_id = %req.application_id,
            "Application deployment to VM configured"
        );

        // Note: Actual application deployment to VM requires:
        // - VM filesystem access (to copy application bundle)
        // - Network configuration (to connect to VM's PlexSpaces node)
        // - Application startup inside VM
        // This is a placeholder for the full implementation
        
        Ok(Response::new(DeployApplicationResponse {
            success: true,
            application_id: req.application_id,
            vm_id: req.vm_id,
            error: None,
        }))
    }

    async fn undeploy_application(
        &self,
        request: Request<UndeployApplicationRequest>,
    ) -> Result<Response<UndeployApplicationResponse>, Status> {
        let req = request.into_inner();
        
        tracing::info!(
            vm_id = %req.vm_id,
            application_id = %req.application_id,
            "Undeploying application from Firecracker VM"
        );

        // TODO: Implement actual undeployment (stop application in VM)
        // For now, just return success

        Ok(Response::new(UndeployApplicationResponse {
            success: true,
            error: None,
        }))
    }
}

/// Map Firecracker VmState to proto VmState
fn map_vm_state_to_proto(state: VmState) -> i32 {
    use plexspaces_proto::firecracker::v1::VmState as ProtoVmState;
    match state {
        VmState::Created => ProtoVmState::VmStateCreated as i32,
        VmState::Ready => ProtoVmState::VmStateCreated as i32, // Ready maps to Created in proto
        VmState::Booting => ProtoVmState::VmStateBooting as i32,
        VmState::Running => ProtoVmState::VmStateRunning as i32,
        VmState::Paused => ProtoVmState::VmStatePaused as i32,
        VmState::Stopping => ProtoVmState::VmStateStopping as i32,
        VmState::Stopped => ProtoVmState::VmStateStopped as i32,
        VmState::Failed => ProtoVmState::VmStateFailed as i32,
    }
}
