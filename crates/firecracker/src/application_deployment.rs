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

//! # Application Deployment Builder
//!
//! ## Purpose
//! Provides a fluent, builder-style API for deploying entire applications to Firecracker VMs.
//! This simplifies the developer experience for deploying applications with strong isolation.
//!
//! ## Design Principles
//! - **Simplicity**: Sensible defaults, minimal required configuration
//! - **Application-Level**: Deploys entire applications (framework + actors), not individual actors
//! - **Firecracker Integration**: Uses Firecracker VMs for strong isolation
//!
//! ## Examples
//!
//! ### Simple Application Deployment
//! ```rust,no_run
//! use plexspaces_firecracker::{ApplicationDeployment, VmConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let deployment = ApplicationDeployment::new("my-app")
//!     .with_vm_config(VmConfig::default());
//! // Deploy to VM
//! let vm = deployment.deploy().await?;
//! # Ok(())
//! # }
//! ```

use crate::{FirecrackerVm, VmConfig};
use crate::error::{FirecrackerError, FirecrackerResult};
use ulid::Ulid;

/// Builder for deploying applications to Firecracker VMs
///
/// ## Purpose
/// Simplifies deployment of entire applications to Firecracker microVMs.
/// An application includes the PlexSpaces framework runtime and all actors.
///
/// ## Design Notes
/// - Application-level isolation (not actor-level)
/// - VM contains entire application, not individual actors
/// - Uses Firecracker for strong isolation with low overhead
pub struct ApplicationDeployment {
    application_id: String,
    vm_config: Option<VmConfig>,
}

impl ApplicationDeployment {
    /// Create a new application deployment builder
    ///
    /// ## Arguments
    /// * `application_id` - Unique identifier for the application
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_firecracker::ApplicationDeployment;
    /// let deployment = ApplicationDeployment::new("my-app");
    /// ```
    pub fn new(application_id: impl Into<String>) -> Self {
        Self {
            application_id: application_id.into(),
            vm_config: None,
        }
    }

    /// Set the VM configuration for this application
    ///
    /// ## Arguments
    /// * `config` - Firecracker VM configuration
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_firecracker::{ApplicationDeployment, VmConfig};
    /// let deployment = ApplicationDeployment::new("my-app")
    ///     .with_vm_config(VmConfig {
    ///         vcpu_count: 2,
    ///         mem_size_mib: 512,
    ///         ..Default::default()
    ///     });
    /// ```
    pub fn with_vm_config(mut self, config: VmConfig) -> Self {
        self.vm_config = Some(config);
        self
    }

    /// Deploy the application to a Firecracker VM
    ///
    /// ## Returns
    /// `Ok(FirecrackerVm)` - The deployed VM instance
    ///
    /// ## Errors
    /// - [`FirecrackerError::ConfigurationError`]: Invalid VM configuration
    /// - [`FirecrackerError::VmCreationFailed`]: Failed to create VM
    ///
    /// ## Example
    /// ```rust,no_run
    /// use plexspaces_firecracker::{ApplicationDeployment, VmConfig};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let vm = ApplicationDeployment::new("my-app")
    ///     .with_vm_config(VmConfig::default())
    ///     .deploy()
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn deploy(self) -> FirecrackerResult<FirecrackerVm> {
        let vm_config = self.vm_config.ok_or_else(|| {
            FirecrackerError::ConfigurationError(
                "VM configuration is required for application deployment".to_string(),
            )
        })?;

        // Create VM with application ID
        let mut vm = FirecrackerVm::create(vm_config).await?;

        // Start Firecracker process
        vm.start_firecracker().await?;

        // Boot VM
        vm.boot().await?;

        Ok(vm)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_application_deployment_creation() {
        let deployment = ApplicationDeployment::new("test-app");
        assert_eq!(deployment.application_id, "test-app");
    }

    #[tokio::test]
    async fn test_application_deployment_with_vm_config() {
        let config = VmConfig {
            vm_id: Ulid::new().to_string(),
            vcpu_count: 1,
            mem_size_mib: 128,
            ..Default::default()
        };

        let deployment = ApplicationDeployment::new("test-app")
            .with_vm_config(config);

        assert!(deployment.vm_config.is_some());
    }

    #[tokio::test]
    async fn test_application_deployment_requires_vm_config() {
        let deployment = ApplicationDeployment::new("test-app");
        let result = deployment.deploy().await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FirecrackerError::ConfigurationError(_)
        ));
    }
}

