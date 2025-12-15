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

//! # Firecracker API Client
//!
//! ## Purpose
//! Provides HTTP client for Firecracker's REST API over Unix socket.
//!
//! ## Architecture Context
//! This module implements the low-level HTTP client for communicating with
//! Firecracker microVMs via their Unix socket API. It wraps Firecracker's
//! REST API endpoints with type-safe Rust functions.
//!
//! ## Why This Exists
//! - Firecracker exposes a REST API over Unix socket (not TCP)
//! - Need to convert PlexSpaces types to Firecracker JSON format
//! - Need to handle HTTP errors and map them to FirecrackerError
//! - Enable VM lifecycle management (create, boot, pause, stop)
//!
//! ## Firecracker API Overview
//! Firecracker API is documented at:
//! https://github.com/firecracker-microvm/firecracker/blob/main/src/api_server/swagger/firecracker.yaml
//!
//! Key endpoints:
//! - PUT /machine-config - Set vCPU and memory
//! - PUT /boot-source - Set kernel and boot args
//! - PUT /drives/{drive_id} - Attach disk drives
//! - PUT /network-interfaces/{iface_id} - Attach network interfaces
//! - PUT /actions - Control VM (InstanceStart, SendCtrlAltDel, etc.)
//! - GET / - Get instance info
//! - PATCH /machine-config - Update VM config
//!
//! ## Design Notes
//! - Uses hyper + hyperlocal for Unix socket HTTP
//! - All requests are synchronous (Firecracker API is fast < 10ms)
//! - Errors mapped from HTTP status codes to FirecrackerError
//! - Socket path is required for all operations

use crate::config::{BootSource, Drive, MachineConfig, NetworkInterface};
use crate::error::{FirecrackerError, FirecrackerResult};
use hyper::{Body, Client, Method, Request, StatusCode};
use hyperlocal::{UnixClientExt, Uri as UnixUri};
use serde::{Deserialize, Serialize};
use std::path::Path;

/// Firecracker API client for managing microVMs via Unix socket
///
/// ## Purpose
/// Provides type-safe wrapper around Firecracker's HTTP REST API.
///
/// ## Usage
/// ```rust,no_run
/// use plexspaces_firecracker::{FirecrackerApiClient, MachineConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
///
/// // Configure VM
/// let machine_config = MachineConfig {
///     vcpu_count: 2,
///     mem_size_mib: 512,
///     smt: false,
///     track_dirty_pages: false,
/// };
/// client.set_machine_config(machine_config).await?;
///
/// // Start VM
/// client.start_instance().await?;
/// # Ok(())
/// # }
/// ```
///
/// ## Design Notes
/// - Socket path must exist before creating client
/// - All API calls are async (use tokio runtime)
/// - Firecracker must be running with `--api-sock <path>`
#[derive(Debug, Clone)]
pub struct FirecrackerApiClient {
    /// Unix socket path for Firecracker API
    socket_path: String,

    /// Hyper HTTP client with Unix socket connector
    client: Client<hyperlocal::UnixConnector>,
}

impl FirecrackerApiClient {
    /// Create new API client connected to Firecracker Unix socket
    ///
    /// ## Arguments
    /// * `socket_path` - Path to Firecracker API socket (e.g., "/tmp/firecracker.sock")
    ///
    /// ## Returns
    /// New API client ready for use
    ///
    /// ## Design Notes
    /// Does NOT check if socket exists - Firecracker may not be started yet.
    /// Socket check happens on first API call.
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_firecracker::FirecrackerApiClient;
    ///
    /// let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// ```
    pub fn new<P: AsRef<Path>>(socket_path: P) -> Self {
        Self {
            socket_path: socket_path.as_ref().to_string_lossy().to_string(),
            client: Client::unix(),
        }
    }

    /// Set machine configuration (vCPU count, memory size)
    ///
    /// ## Arguments
    /// * `config` - Machine configuration
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::ConfigurationError`]: Invalid config (e.g., vcpu_count = 0)
    /// - [`FirecrackerError::ApiError`]: Firecracker rejected config
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /machine-config
    /// Content-Type: application/json
    ///
    /// {
    ///   "vcpu_count": 2,
    ///   "mem_size_mib": 512,
    ///   "smt": false,
    ///   "track_dirty_pages": false
    /// }
    /// ```
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerApiClient, MachineConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// let config = MachineConfig {
    ///     vcpu_count: 2,
    ///     mem_size_mib: 512,
    ///     smt: false,
    ///     track_dirty_pages: false,
    /// };
    /// client.set_machine_config(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_machine_config(&self, config: MachineConfig) -> FirecrackerResult<()> {
        let body = serde_json::to_string(&config)?;
        self.put("/machine-config", body).await
    }

    /// Set boot source (kernel image and boot arguments)
    ///
    /// ## Arguments
    /// * `boot_source` - Kernel image path and boot arguments
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::ConfigurationError`]: Kernel image not found
    /// - [`FirecrackerError::ApiError`]: Firecracker rejected boot source
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /boot-source
    /// Content-Type: application/json
    ///
    /// {
    ///   "kernel_image_path": "/var/lib/firecracker/vmlinux",
    ///   "boot_args": "console=ttyS0 reboot=k panic=1 pci=off",
    ///   "initrd_path": null
    /// }
    /// ```
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerApiClient, BootSource};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// let boot_source = BootSource {
    ///     kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
    ///     boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
    ///     initrd_path: None,
    /// };
    /// client.set_boot_source(boot_source).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn set_boot_source(&self, boot_source: BootSource) -> FirecrackerResult<()> {
        let body = serde_json::to_string(&boot_source)?;
        self.put("/boot-source", body).await
    }

    /// Attach drive (rootfs or data drive) to VM
    ///
    /// ## Arguments
    /// * `drive` - Drive configuration
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::ConfigurationError`]: Drive image not found
    /// - [`FirecrackerError::ApiError`]: Firecracker rejected drive config
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /drives/{drive_id}
    /// Content-Type: application/json
    ///
    /// {
    ///   "drive_id": "rootfs",
    ///   "path_on_host": "/var/lib/firecracker/rootfs.ext4",
    ///   "is_root_device": true,
    ///   "is_read_only": false
    /// }
    /// ```
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerApiClient, Drive};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// let drive = Drive {
    ///     drive_id: "rootfs".to_string(),
    ///     path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
    ///     is_root_device: true,
    ///     is_read_only: false,
    ///     rate_limiter: None,
    /// };
    /// client.attach_drive(drive).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn attach_drive(&self, drive: Drive) -> FirecrackerResult<()> {
        let path = format!("/drives/{}", drive.drive_id);
        let body = serde_json::to_string(&drive)?;
        self.put(&path, body).await
    }

    /// Attach network interface (TAP device) to VM
    ///
    /// ## Arguments
    /// * `iface` - Network interface configuration
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::NetworkSetupFailed`]: TAP device doesn't exist
    /// - [`FirecrackerError::ApiError`]: Firecracker rejected network config
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /network-interfaces/{iface_id}
    /// Content-Type: application/json
    ///
    /// {
    ///   "iface_id": "eth0",
    ///   "host_dev_name": "tap-vm001",
    ///   "guest_mac": "AA:FC:00:00:00:01"
    /// }
    /// ```
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerApiClient, NetworkInterface};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// let iface = NetworkInterface {
    ///     iface_id: "eth0".to_string(),
    ///     host_dev_name: "tap-vm001".to_string(),
    ///     guest_mac: "AA:FC:00:00:00:01".to_string(),
    ///     rx_rate_limiter: None,
    ///     tx_rate_limiter: None,
    /// };
    /// client.attach_network_interface(iface).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn attach_network_interface(
        &self,
        iface: NetworkInterface,
    ) -> FirecrackerResult<()> {
        let path = format!("/network-interfaces/{}", iface.iface_id);
        let body = serde_json::to_string(&iface)?;
        self.put(&path, body).await
    }

    /// Start VM instance (boot kernel)
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmBootFailed`]: VM failed to boot (kernel panic, missing rootfs, etc.)
    /// - [`FirecrackerError::ApiError`]: Firecracker rejected start action
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /actions
    /// Content-Type: application/json
    ///
    /// {
    ///   "action_type": "InstanceStart"
    /// }
    /// ```
    ///
    /// ## Design Notes
    /// This is async but returns quickly (< 10ms). Actual boot happens in background.
    /// Use `get_instance_info()` to check if boot succeeded.
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::FirecrackerApiClient;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
    /// client.start_instance().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start_instance(&self) -> FirecrackerResult<()> {
        let action = ActionRequest {
            action_type: "InstanceStart".to_string(),
        };
        let body = serde_json::to_string(&action)?;
        self.put("/actions", body).await
    }

    /// Pause VM execution
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmOperationFailed`]: VM not in running state
    ///
    /// ## Firecracker API
    /// ```text
    /// PATCH /vm
    /// Content-Type: application/json
    ///
    /// {
    ///   "state": "Paused"
    /// }
    /// ```
    pub async fn pause_instance(&self) -> FirecrackerResult<()> {
        let state = VmStateRequest {
            state: "Paused".to_string(),
        };
        let body = serde_json::to_string(&state)?;
        self.patch("/vm", body).await
    }

    /// Resume paused VM execution
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmOperationFailed`]: VM not in paused state
    ///
    /// ## Firecracker API
    /// ```text
    /// PATCH /vm
    /// Content-Type: application/json
    ///
    /// {
    ///   "state": "Resumed"
    /// }
    /// ```
    pub async fn resume_instance(&self) -> FirecrackerResult<()> {
        let state = VmStateRequest {
            state: "Resumed".to_string(),
        };
        let body = serde_json::to_string(&state)?;
        self.patch("/vm", body).await
    }

    /// Send Ctrl+Alt+Del to VM (graceful shutdown)
    ///
    /// ## Returns
    /// `Ok(())` on success
    ///
    /// ## Firecracker API
    /// ```text
    /// PUT /actions
    /// Content-Type: application/json
    ///
    /// {
    ///   "action_type": "SendCtrlAltDel"
    /// }
    /// ```
    pub async fn send_ctrl_alt_del(&self) -> FirecrackerResult<()> {
        let action = ActionRequest {
            action_type: "SendCtrlAltDel".to_string(),
        };
        let body = serde_json::to_string(&action)?;
        self.put("/actions", body).await
    }

    /// Get instance information (state, ID, etc.)
    ///
    /// ## Returns
    /// Instance information JSON string
    ///
    /// ## Firecracker API
    /// ```text
    /// GET /
    /// ```
    ///
    /// Response:
    /// ```json
    /// {
    ///   "id": "anonymous-instance",
    ///   "state": "Running",
    ///   "vmm_version": "1.4.0",
    ///   "app_name": "Firecracker"
    /// }
    /// ```
    pub async fn get_instance_info(&self) -> FirecrackerResult<InstanceInfo> {
        let response = self.get("/").await?;
        let info: InstanceInfo = serde_json::from_str(&response)?;
        Ok(info)
    }

    // ========================================================================
    // Low-level HTTP methods
    // ========================================================================

    /// Send PUT request to Firecracker API
    async fn put(&self, path: &str, body: String) -> FirecrackerResult<()> {
        let uri = UnixUri::new(&self.socket_path, path);
        let request = Request::builder()
            .method(Method::PUT)
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .map_err(|e| FirecrackerError::ApiError(format!("Failed to build request: {}", e)))?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| FirecrackerError::ApiError(format!("HTTP request failed: {}", e)))?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT | StatusCode::CREATED => Ok(()),
            StatusCode::BAD_REQUEST => Err(FirecrackerError::ConfigurationError(
                "Invalid configuration".to_string(),
            )),
            StatusCode::NOT_FOUND => Err(FirecrackerError::VmNotFound(path.to_string())),
            StatusCode::INTERNAL_SERVER_ERROR => Err(FirecrackerError::VmOperationFailed(
                "Firecracker internal error".to_string(),
            )),
            code => Err(FirecrackerError::ApiError(format!(
                "Unexpected status code: {}",
                code
            ))),
        }
    }

    /// Send PATCH request to Firecracker API
    async fn patch(&self, path: &str, body: String) -> FirecrackerResult<()> {
        let uri = UnixUri::new(&self.socket_path, path);
        let request = Request::builder()
            .method(Method::PATCH)
            .uri(uri)
            .header("Content-Type", "application/json")
            .body(Body::from(body))
            .map_err(|e| FirecrackerError::ApiError(format!("Failed to build request: {}", e)))?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| FirecrackerError::ApiError(format!("HTTP request failed: {}", e)))?;

        match response.status() {
            StatusCode::OK | StatusCode::NO_CONTENT => Ok(()),
            StatusCode::BAD_REQUEST => Err(FirecrackerError::ConfigurationError(
                "Invalid configuration".to_string(),
            )),
            StatusCode::NOT_FOUND => Err(FirecrackerError::VmNotFound(path.to_string())),
            code => Err(FirecrackerError::ApiError(format!(
                "Unexpected status code: {}",
                code
            ))),
        }
    }

    /// Send GET request to Firecracker API
    async fn get(&self, path: &str) -> FirecrackerResult<String> {
        let uri = UnixUri::new(&self.socket_path, path);
        let request = Request::builder()
            .method(Method::GET)
            .uri(uri)
            .body(Body::empty())
            .map_err(|e| FirecrackerError::ApiError(format!("Failed to build request: {}", e)))?;

        let response = self
            .client
            .request(request)
            .await
            .map_err(|e| FirecrackerError::ApiError(format!("HTTP request failed: {}", e)))?;

        let status = response.status();
        let body_bytes = hyper::body::to_bytes(response.into_body())
            .await
            .map_err(|e| FirecrackerError::ApiError(format!("Failed to read response: {}", e)))?;

        let body_str = String::from_utf8(body_bytes.to_vec())
            .map_err(|e| FirecrackerError::ApiError(format!("Invalid UTF-8 response: {}", e)))?;

        match status {
            StatusCode::OK => Ok(body_str),
            StatusCode::NOT_FOUND => Err(FirecrackerError::VmNotFound(path.to_string())),
            code => Err(FirecrackerError::ApiError(format!(
                "Unexpected status code: {} - {}",
                code, body_str
            ))),
        }
    }
}

// ============================================================================
// Firecracker API Request/Response Types
// ============================================================================

/// Action request for VM control
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ActionRequest {
    action_type: String,
}

/// VM state change request
#[derive(Debug, Clone, Serialize, Deserialize)]
struct VmStateRequest {
    state: String,
}

/// Instance information response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceInfo {
    /// Instance ID
    pub id: String,

    /// Current state (Running, Paused, etc.)
    pub state: String,

    /// Firecracker version
    pub vmm_version: String,

    /// Application name ("Firecracker")
    pub app_name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_client_creation() {
        let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
        assert_eq!(client.socket_path, "/tmp/firecracker.sock");
    }

    #[test]
    fn test_action_request_serialization() {
        let action = ActionRequest {
            action_type: "InstanceStart".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        assert!(json.contains("InstanceStart"));
    }

    #[test]
    fn test_vm_state_request_serialization() {
        let state = VmStateRequest {
            state: "Paused".to_string(),
        };
        let json = serde_json::to_string(&state).unwrap();
        assert!(json.contains("Paused"));
    }

    #[test]
    fn test_instance_info_deserialization() {
        let json = r#"{"id":"test","state":"Running","vmm_version":"1.4.0","app_name":"Firecracker"}"#;
        let info: InstanceInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.id, "test");
        assert_eq!(info.state, "Running");
        assert_eq!(info.vmm_version, "1.4.0");
        assert_eq!(info.app_name, "Firecracker");
    }
}
