// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
//! # VM Registry - Discover and Track Running VMs
//!
//! ## Purpose
//! Discovers running Firecracker VMs by scanning socket files and provides
//! a simple registry for VM tracking. Can optionally integrate with ObjectRegistry
//! for unified service discovery.
//!
//! ## Design
//! - Scans `/tmp` for Firecracker socket files (pattern: `firecracker-*.sock`)
//! - Extracts VM ID from socket filename
//! - Connects to VM API to get state
//! - Provides simple registry interface
//! - Can register VMs in ObjectRegistry for unified discovery

use crate::api_client::FirecrackerApiClient;
use crate::error::{FirecrackerError, FirecrackerResult};
use crate::vm::VmState;
use std::fs;
use std::path::{Path, PathBuf};

/// VM registry entry
#[derive(Debug, Clone)]
pub struct VmRegistryEntry {
    /// VM ID
    pub vm_id: String,

    /// Socket path
    pub socket_path: String,

    /// Current VM state
    pub state: VmState,

    /// VM configuration (if available)
    pub config: Option<crate::config::VmConfig>,
}

/// VM Registry for discovering and tracking VMs
pub struct VmRegistry;

impl VmRegistry {
    /// Discover all running VMs by scanning socket files
    ///
    /// ## Returns
    /// List of discovered VM entries
    ///
    /// ## Design Notes
    /// - Scans `/tmp` for `firecracker-*.sock` files
    /// - Extracts VM ID from filename
    /// - Connects to each VM to get current state
    pub async fn discover_vms() -> FirecrackerResult<Vec<VmRegistryEntry>> {
        let tmp_dir = Path::new("/tmp");
        let mut vms = Vec::new();

        // Scan for socket files
        let entries = fs::read_dir(tmp_dir).map_err(|e| {
            FirecrackerError::VmOperationFailed(format!("Failed to read /tmp: {}", e))
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                FirecrackerError::VmOperationFailed(format!("Failed to read directory entry: {}", e))
            })?;

            let path = entry.path();
            if let Some(file_name) = path.file_name() {
                let name = file_name.to_string_lossy();
                if name.starts_with("firecracker-") && name.ends_with(".sock") {
                    // Extract VM ID from filename: firecracker-{vm_id}.sock
                    let vm_id = name
                        .strip_prefix("firecracker-")
                        .and_then(|s| s.strip_suffix(".sock"))
                        .unwrap_or("unknown")
                        .to_string();

                    // Try to connect and get state
                    let socket_path = path.to_string_lossy().to_string();
                    if let Ok(state) = Self::get_vm_state(&socket_path).await {
                        vms.push(VmRegistryEntry {
                            vm_id,
                            socket_path,
                            state,
                            config: None,
                        });
                    }
                }
            }
        }

        Ok(vms)
    }

    /// Get VM state by connecting to socket
    async fn get_vm_state(socket_path: &str) -> FirecrackerResult<VmState> {
        let client = FirecrackerApiClient::new(socket_path);
        match client.get_instance_info().await {
            Ok(info) => {
                // Map Firecracker state string to VmState
                match info.state.as_str() {
                    "Running" => Ok(VmState::Running),
                    "Paused" => Ok(VmState::Paused),
                    _ => Ok(VmState::Ready), // Default to Ready for other states
                }
            }
            Err(_) => Ok(VmState::Stopped), // Socket exists but VM not responding
        }
    }

    /// Find VM by ID
    ///
    /// ## Arguments
    /// * `vm_id` - VM identifier
    ///
    /// ## Returns
    /// VM registry entry if found
    pub async fn find_vm(vm_id: &str) -> FirecrackerResult<Option<VmRegistryEntry>> {
        let vms = Self::discover_vms().await?;
        Ok(vms.into_iter().find(|vm| vm.vm_id == vm_id))
    }

    /// Get VM socket path from VM ID
    ///
    /// ## Arguments
    /// * `vm_id` - VM identifier
    ///
    /// ## Returns
    /// Socket path if VM is running
    pub async fn get_vm_socket_path(vm_id: &str) -> FirecrackerResult<Option<String>> {
        if let Some(entry) = Self::find_vm(vm_id).await? {
            Ok(Some(entry.socket_path))
        } else {
            Ok(None)
        }
    }

    /// Register VM in ObjectRegistry
    ///
    /// ## Arguments
    /// * `registry` - ObjectRegistry instance
    /// * `vm_entry` - VM registry entry to register
    /// * `tenant_id` - Tenant ID (default: "default")
    /// * `namespace` - Namespace (default: "default")
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Design Notes
    /// Registers VM with ObjectTypeVm in the unified object registry.
    /// This enables VM discovery alongside actors, tuplespaces, and services.
    pub async fn register_in_object_registry(
        registry: &plexspaces_object_registry::ObjectRegistry,
        vm_entry: &VmRegistryEntry,
        tenant_id: Option<&str>,
        namespace: Option<&str>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use plexspaces_proto::object_registry::v1::{ObjectRegistration, ObjectType, HealthStatus};

        // Map VM state to health status
        let health_status = match vm_entry.state {
            crate::vm::VmState::Running => HealthStatus::HealthStatusHealthy,
            crate::vm::VmState::Paused => HealthStatus::HealthStatusDegraded,
            crate::vm::VmState::Booting | crate::vm::VmState::Ready => HealthStatus::HealthStatusStarting,
            crate::vm::VmState::Stopping => HealthStatus::HealthStatusStopping,
            crate::vm::VmState::Stopped | crate::vm::VmState::Failed => HealthStatus::HealthStatusUnhealthy,
            crate::vm::VmState::Created => HealthStatus::HealthStatusStarting,
        };

        let registration = ObjectRegistration {
            object_id: vm_entry.vm_id.clone(),
            object_type: ObjectType::ObjectTypeVm as i32,
            object_category: "firecracker".to_string(),
            grpc_address: vm_entry.socket_path.clone(),
            tenant_id: tenant_id.unwrap_or("default").to_string(),
            namespace: namespace.unwrap_or("default").to_string(),
            health_status: health_status as i32,
            labels: vec!["firecracker".to_string(), "microvm".to_string()],
            ..Default::default()
        };

        registry.register(registration).await?;
        Ok(())
    }
}

