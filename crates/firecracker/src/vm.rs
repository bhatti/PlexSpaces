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

//! # Firecracker VM Lifecycle Management
//!
//! ## Purpose
//! Provides high-level VM lifecycle management wrapping the Firecracker API client.
//!
//! ## Architecture Context
//! This module implements the VM abstraction for PlexSpaces, managing the full
//! lifecycle of Firecracker microVMs from creation through shutdown.
//!
//! ## Why This Exists
//! - Higher-level API than FirecrackerApiClient (config → running VM)
//! - Manages Firecracker process lifecycle (spawn, monitor, kill)
//! - Tracks VM state transitions (Created → Booting → Running → Stopped)
//! - Provides error recovery and cleanup
//!
//! ## Design Decisions
//! - Each `FirecrackerVm` instance manages ONE microVM
//! - VM ID uses ULID (CLAUDE.md Core Principle 0)
//! - Socket path auto-generated in /tmp (or user-specified)
//! - Firecracker process spawned as child (killed when dropped)
//!
//! ## Usage
//! ```rust,no_run
//! use plexspaces_firecracker::{FirecrackerVm, VmConfig, VmState};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create VM with default config
//! let config = VmConfig::default();
//! let mut vm = FirecrackerVm::create(config).await?;
//!
//! // Boot VM (< 200ms target)
//! vm.boot().await?;
//!
//! // Check state
//! assert_eq!(vm.state(), VmState::Running);
//!
//! // Pause/resume
//! vm.pause().await?;
//! vm.resume().await?;
//!
//! // Stop VM
//! vm.stop().await?;
//! # Ok(())
//! # }
//! ```

use crate::api_client::FirecrackerApiClient;
use crate::config::VmConfig;
use crate::error::{FirecrackerError, FirecrackerResult};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};
use tokio::time::{sleep, Duration};

/// VM state enumeration
///
/// ## Purpose
/// Tracks the lifecycle state of a Firecracker microVM.
///
/// ## State Transitions
/// ```text
/// Created → Booting → Running → [Paused → Running]
///                              → [Stopping → Stopped]
///                              → [Failed]
/// ```
///
/// ## Design Notes
/// - State machine enforced by FirecrackerVm methods
/// - Invalid transitions return VmOperationFailed error
/// - Failed state is terminal (must create new VM)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VmState {
    /// VM created, Firecracker process not started yet
    Created,

    /// Firecracker process started, API socket available
    Ready,

    /// Kernel booting (InstanceStart called)
    Booting,

    /// VM fully operational
    Running,

    /// Execution paused (can resume)
    Paused,

    /// Shutdown in progress
    Stopping,

    /// Fully stopped, resources released
    Stopped,

    /// Failed to boot or crashed
    Failed,
}

/// Firecracker VM instance
///
/// ## Purpose
/// High-level abstraction for managing a single Firecracker microVM.
///
/// ## Why This Exists
/// - Simplifies VM lifecycle management (no need to manually call API)
/// - Automatically spawns and monitors Firecracker process
/// - Ensures cleanup (kills process on drop)
/// - Provides state machine validation
///
/// ## Design Notes
/// - One instance = one microVM
/// - ULID-based VM IDs for sortability (CLAUDE.md Core Principle 0)
/// - Socket path auto-generated in /tmp unless specified
/// - Firecracker binary path configurable (default: /usr/bin/firecracker)
///
/// ## Example
/// ```rust,no_run
/// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = VmConfig {
///     vm_id: ulid::Ulid::new().to_string(),
///     vcpu_count: 2,
///     mem_size_mib: 512,
///     ..Default::default()
/// };
///
/// let mut vm = FirecrackerVm::create(config).await?;
/// vm.boot().await?;
/// # Ok(())
/// # }
/// ```
pub struct FirecrackerVm {
    /// VM configuration
    config: VmConfig,

    /// Current VM state
    state: VmState,

    /// Firecracker API client
    api_client: Option<FirecrackerApiClient>,

    /// Firecracker process handle
    process: Option<Child>,

    /// Path to Firecracker binary
    firecracker_binary: PathBuf,

    /// Whether to disable seccomp filtering (useful in Docker/emulated environments)
    no_seccomp: bool,
}

// Manual Debug implementation (Child doesn't implement Debug)
impl std::fmt::Debug for FirecrackerVm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FirecrackerVm")
            .field("vm_id", &self.config.vm_id)
            .field("state", &self.state)
            .field("vcpu_count", &self.config.vcpu_count)
            .field("mem_size_mib", &self.config.mem_size_mib)
            .field("socket_path", &self.config.socket_path)
            .field("process_running", &self.process.is_some())
            .finish()
    }
}

impl FirecrackerVm {
    /// Create new VM instance (but don't start Firecracker yet)
    ///
    /// ## Arguments
    /// * `config` - VM configuration
    ///
    /// ## Returns
    /// New VM in Created state
    ///
    /// ## Design Notes
    /// This is async but returns immediately. No I/O happens until `boot()`.
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = VmConfig::default();
    /// let vm = FirecrackerVm::create(config).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn create(config: VmConfig) -> FirecrackerResult<Self> {
        // Validate config
        if config.vcpu_count == 0 {
            return Err(FirecrackerError::ConfigurationError(
                "vcpu_count must be > 0".to_string(),
            ));
        }
        if config.mem_size_mib < 128 {
            return Err(FirecrackerError::ConfigurationError(
                "mem_size_mib must be >= 128".to_string(),
            ));
        }

        Ok(Self {
            config,
            state: VmState::Created,
            api_client: None,
            process: None,
            firecracker_binary: PathBuf::from("/usr/bin/firecracker"),
            // Check environment variable for seccomp disable (useful in Docker)
            no_seccomp: std::env::var("FIRECRACKER_NO_SECCOMP")
                .map(|v| v == "1" || v.to_lowercase() == "true")
                .unwrap_or(false),
        })
    }

    /// Start Firecracker process and configure VM
    ///
    /// ## Returns
    /// `Ok(())` on success, VM state transitions to Ready
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmCreationFailed`]: Firecracker binary not found or failed to start
    /// - [`FirecrackerError::ConfigurationError`]: Invalid kernel/rootfs paths
    ///
    /// ## Side Effects
    /// - Spawns Firecracker process
    /// - Creates API socket at config.socket_path
    /// - Configures machine, boot source, drives, network interfaces
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut vm = FirecrackerVm::create(VmConfig::default()).await?;
    /// vm.start_firecracker().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn start_firecracker(&mut self) -> FirecrackerResult<()> {
        if self.state != VmState::Created {
            return Err(FirecrackerError::VmOperationFailed(format!(
                "Cannot start Firecracker in state {:?}",
                self.state
            )));
        }

        // Generate socket path if not specified
        if self.config.socket_path.is_empty() {
            self.config.socket_path =
                format!("/tmp/firecracker-{}.sock", self.config.vm_id);
        }

        // Remove old socket if it exists
        let _ = std::fs::remove_file(&self.config.socket_path);

        // Spawn Firecracker process
        let mut cmd = Command::new(&self.firecracker_binary);
        cmd.arg("--api-sock")
            .arg(&self.config.socket_path);
        
        // Add --no-seccomp flag if requested (needed in Docker/emulated environments)
        if self.no_seccomp {
            cmd.arg("--no-seccomp");
        }
        
        let mut child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                FirecrackerError::VmCreationFailed(format!(
                    "Failed to spawn Firecracker: {}",
                    e
                ))
            })?;

        // Wait for socket to be created (up to 1 second)
        let socket_path = PathBuf::from(&self.config.socket_path);
        for _ in 0..20 {
            if socket_path.exists() {
                break;
            }
            sleep(Duration::from_millis(50)).await;

            // Check if process crashed
            if let Ok(Some(status)) = child.try_wait() {
                // Read stderr to get error details
                let mut stderr_output = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = stderr.read_to_string(&mut stderr_output).await;
                }
                
                let error_msg = if !stderr_output.trim().is_empty() {
                    format!(
                        "Firecracker process exited with status: {}. Stderr: {}",
                        status, stderr_output.trim()
                    )
                } else {
                    format!("Firecracker process exited with status: {}", status)
                };
                
                return Err(FirecrackerError::VmCreationFailed(error_msg));
            }
        }

        if !socket_path.exists() {
            // Check if process crashed while waiting
            if let Ok(Some(status)) = child.try_wait() {
                // Read stderr to get error details
                let mut stderr_output = String::new();
                if let Some(mut stderr) = child.stderr.take() {
                    use tokio::io::AsyncReadExt;
                    let _ = stderr.read_to_string(&mut stderr_output).await;
                }
                
                let error_msg = if !stderr_output.trim().is_empty() {
                    format!(
                        "Firecracker process exited with status: {} before socket was created. Stderr: {}",
                        status, stderr_output.trim()
                    )
                } else {
                    format!(
                        "Firecracker process exited with status: {} before socket was created",
                        status
                    )
                };
                
                return Err(FirecrackerError::VmCreationFailed(error_msg));
            }
            
            return Err(FirecrackerError::VmCreationFailed(
                "Firecracker socket not created within timeout".to_string(),
            ));
        }

        // Create API client
        let api_client = FirecrackerApiClient::new(&self.config.socket_path);

        // Configure machine
        api_client
            .set_machine_config(crate::config::MachineConfig {
                vcpu_count: self.config.vcpu_count,
                mem_size_mib: self.config.mem_size_mib,
                smt: false,
                track_dirty_pages: false,
            })
            .await?;

        // Configure boot source
        api_client
            .set_boot_source(crate::config::BootSource {
                kernel_image_path: self.config.kernel_image_path.clone(),
                boot_args: Some(self.config.boot_args.clone()),
                initrd_path: None,
            })
            .await?;

        // Attach rootfs
        api_client
            .attach_drive(crate::config::Drive {
                drive_id: self.config.rootfs.drive_id.clone(),
                path_on_host: self.config.rootfs.path_on_host.clone(),
                is_root_device: true,
                is_read_only: self.config.rootfs.is_read_only,
                rate_limiter: None,
            })
            .await?;

        // Attach network interfaces
        for iface_config in &self.config.network_interfaces {
            api_client
                .attach_network_interface(crate::config::NetworkInterface {
                    iface_id: iface_config.iface_id.clone(),
                    host_dev_name: iface_config.host_dev_name.clone(),
                    guest_mac: Some(iface_config.guest_mac.clone()),
                    rx_rate_limiter: None,
                    tx_rate_limiter: None,
                })
                .await?;
        }

        // Update state
        self.state = VmState::Ready;
        self.api_client = Some(api_client);
        self.process = Some(child);

        Ok(())
    }

    /// Boot VM (start kernel execution)
    ///
    /// ## Returns
    /// `Ok(())` on success, VM state transitions to Running
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmBootFailed`]: Kernel failed to boot
    /// - [`FirecrackerError::VmOperationFailed`]: VM not in Ready state
    ///
    /// ## Performance
    /// Target: < 200ms (goal: < 125ms)
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let mut vm = FirecrackerVm::create(VmConfig::default()).await?;
    /// # vm.start_firecracker().await?;
    /// vm.boot().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn boot(&mut self) -> FirecrackerResult<()> {
        if self.state != VmState::Ready {
            return Err(FirecrackerError::VmOperationFailed(format!(
                "Cannot boot VM in state {:?}",
                self.state
            )));
        }

        let api_client = self.api_client.as_ref().ok_or_else(|| {
            FirecrackerError::VmOperationFailed("API client not initialized".to_string())
        })?;

        // Transition to Booting
        self.state = VmState::Booting;

        // Start instance
        api_client.start_instance().await.map_err(|e| {
            self.state = VmState::Failed;
            FirecrackerError::VmBootFailed(format!("Failed to start instance: {}", e))
        })?;

        // Wait for boot (check instance info)
        for _ in 0..40 {
            // 2 seconds max
            sleep(Duration::from_millis(50)).await;

            match api_client.get_instance_info().await {
                Ok(info) => {
                    if info.state == "Running" {
                        self.state = VmState::Running;
                        return Ok(());
                    }
                }
                Err(_) => continue,
            }
        }

        // Boot timeout
        self.state = VmState::Failed;
        Err(FirecrackerError::VmBootFailed(
            "Boot timeout (2 seconds)".to_string(),
        ))
    }

    /// Pause VM execution
    ///
    /// ## Returns
    /// `Ok(())` on success, VM state transitions to Paused
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmOperationFailed`]: VM not in Running state
    pub async fn pause(&mut self) -> FirecrackerResult<()> {
        if self.state != VmState::Running {
            return Err(FirecrackerError::VmOperationFailed(format!(
                "Cannot pause VM in state {:?}",
                self.state
            )));
        }

        let api_client = self.api_client.as_ref().ok_or_else(|| {
            FirecrackerError::VmOperationFailed("API client not initialized".to_string())
        })?;

        api_client.pause_instance().await?;
        self.state = VmState::Paused;
        Ok(())
    }

    /// Resume paused VM execution
    ///
    /// ## Returns
    /// `Ok(())` on success, VM state transitions to Running
    ///
    /// ## Errors
    /// - [`FirecrackerError::VmOperationFailed`]: VM not in Paused state
    pub async fn resume(&mut self) -> FirecrackerResult<()> {
        if self.state != VmState::Paused {
            return Err(FirecrackerError::VmOperationFailed(format!(
                "Cannot resume VM in state {:?}",
                self.state
            )));
        }

        let api_client = self.api_client.as_ref().ok_or_else(|| {
            FirecrackerError::VmOperationFailed("API client not initialized".to_string())
        })?;

        api_client.resume_instance().await?;
        self.state = VmState::Running;
        Ok(())
    }

    /// Stop VM (graceful shutdown)
    ///
    /// ## Returns
    /// `Ok(())` on success, VM state transitions to Stopped
    ///
    /// ## Side Effects
    /// - Sends Ctrl+Alt+Del to VM
    /// - Waits up to 5 seconds for graceful shutdown
    /// - Force kills Firecracker process if graceful shutdown fails
    /// - Removes socket file
    pub async fn stop(&mut self) -> FirecrackerResult<()> {
        if self.state == VmState::Stopped {
            return Ok(());
        }

        self.state = VmState::Stopping;

        // Try graceful shutdown
        if let Some(api_client) = &self.api_client {
            let _ = api_client.send_ctrl_alt_del().await;

            // Wait for process to exit (up to 5 seconds)
            if let Some(child) = &mut self.process {
                for _ in 0..100 {
                    match child.try_wait() {
                        Ok(Some(_)) => break,
                        Ok(None) => sleep(Duration::from_millis(50)).await,
                        Err(_) => break,
                    }
                }
            }
        }

        // Force kill if still running
        if let Some(mut child) = self.process.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }

        // Cleanup socket
        let _ = std::fs::remove_file(&self.config.socket_path);

        self.state = VmState::Stopped;
        self.api_client = None;

        Ok(())
    }

    /// Get current VM state
    pub fn state(&self) -> VmState {
        self.state
    }

    /// Get VM ID
    pub fn vm_id(&self) -> &str {
        &self.config.vm_id
    }

    /// Get VM configuration
    pub fn config(&self) -> &VmConfig {
        &self.config
    }

    /// Set Firecracker binary path (default: /usr/bin/firecracker)
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut vm = FirecrackerVm::create(VmConfig::default()).await?;
    /// vm.set_firecracker_binary("/usr/local/bin/firecracker");
    /// vm.start_firecracker().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_firecracker_binary<P: Into<PathBuf>>(&mut self, path: P) {
        self.firecracker_binary = path.into();
    }

    /// Disable seccomp filtering (useful in Docker/emulated environments)
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_firecracker::{FirecrackerVm, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut vm = FirecrackerVm::create(VmConfig::default()).await?;
    /// vm.disable_seccomp();
    /// vm.start_firecracker().await?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn disable_seccomp(&mut self) {
        self.no_seccomp = true;
    }

    /// Enable seccomp filtering (default, recommended for production)
    pub fn enable_seccomp(&mut self) {
        self.no_seccomp = false;
    }
}

// Cleanup on drop
impl Drop for FirecrackerVm {
    fn drop(&mut self) {
        // Best-effort cleanup (can't be async in Drop)
        if let Some(mut child) = self.process.take() {
            let _ = child.start_kill();
        }
        let _ = std::fs::remove_file(&self.config.socket_path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_vm_creation() {
        let config = VmConfig {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 2,
            mem_size_mib: 512,
            ..Default::default()
        };

        let vm = FirecrackerVm::create(config).await.unwrap();
        assert_eq!(vm.state(), VmState::Created);
        assert_eq!(vm.vm_id().len(), 26); // ULID length
    }

    #[tokio::test]
    async fn test_vm_invalid_config() {
        let config = VmConfig {
            vcpu_count: 0, // Invalid
            ..Default::default()
        };

        let result = FirecrackerVm::create(config).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FirecrackerError::ConfigurationError(_)
        ));
    }

    #[tokio::test]
    async fn test_vm_state_transitions() {
        let config = VmConfig::default();
        let mut vm = FirecrackerVm::create(config).await.unwrap();

        assert_eq!(vm.state(), VmState::Created);

        // Cannot boot before starting Firecracker
        let result = vm.boot().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_vm_set_firecracker_binary() {
        let config = VmConfig::default();
        let mut vm = FirecrackerVm::create(config).await.unwrap();

        vm.set_firecracker_binary("/custom/path/firecracker");
        assert_eq!(vm.firecracker_binary, PathBuf::from("/custom/path/firecracker"));
    }
}
