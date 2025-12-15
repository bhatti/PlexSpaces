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

//! # PlexSpaces Firecracker Integration
//!
//! ## Purpose
//! Provides Firecracker microVM integration for PlexSpaces actors, enabling
//! strong isolation with < 200ms boot time (target: 125ms).
//!
//! ## Architecture Context
//! This crate implements **Walking Skeleton Phase 7** (Week 13-14): Firecracker
//! as the microVM isolation layer for PlexSpaces actors.
//!
//! ### Why Firecracker?
//! - **Fast boot**: < 125ms from cold start
//! - **Lightweight**: ~10MB memory overhead per VM
//! - **Secure**: Strong isolation via KVM virtualization
//! - **Minimal**: Purpose-built for serverless/container workloads
//! - **Battle-tested**: Powers AWS Lambda and Fargate
//!
//! ## Key Components
//! - [`config`]: VM configuration types
//! - [`error`]: Firecracker-specific errors
//! - [`api_client`]: HTTP client for Firecracker REST API
//! - [`vm`]: VM lifecycle management (create, boot, pause, stop)
//! - [`network`]: TAP device setup and VM networking
//!
//! ## Design Philosophy
//! This crate focuses **only on VM lifecycle management**. It does NOT handle:
//! - Actor deployment (belongs in application deployment layer)
//! - WASM execution (belongs in `crates/wasm-runtime/`)
//! - Application management (belongs in node/application layer)
//!
//! ## Implementation Status
//! See [IMPLEMENTATION_STATUS.md](../IMPLEMENTATION_STATUS.md) for detailed progress.
//!
//! ## Usage (Planned)
//! ```rust,no_run
//! use plexspaces_firecracker::{FirecrackerVm, VmConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create VM configuration
//! let config = VmConfig::default();
//!
//! // Create and boot VM
//! let mut vm = FirecrackerVm::create(config).await?;
//! vm.boot().await?;
//!
//! // VM is now ready for application deployment
//! // (Application deployment is handled by node/application layer)
//!
//! // Stop VM
//! vm.stop().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Requirements
//! - Linux kernel 4.14+
//! - Firecracker binary (`/usr/bin/firecracker`)
//! - Kernel image (`/var/lib/firecracker/vmlinux`)
//! - Rootfs image (`/var/lib/firecracker/rootfs.ext4`)
//! - Root or `CAP_NET_ADMIN` for TAP devices
//!
//! ## Performance Targets
//! - Boot time: < 200ms (target: 125ms)
//! - Memory overhead: < 10MB per VM
//! - Network latency: < 1ms VM-to-VM
//! - Actor instantiation: < 50ms in running VM

#![warn(missing_docs)]
#![warn(clippy::all)]

pub mod api_client;
pub mod application_deployment;
pub mod config;
pub mod error;
pub mod health;
pub mod network;
pub mod supervisor;
pub mod vm;
pub mod vm_registry;

pub use application_deployment::ApplicationDeployment;

// Re-exports
pub use api_client::{FirecrackerApiClient, InstanceInfo};
pub use health::{HealthStatus, VmHealthMonitor};
pub use supervisor::{BackoffConfig, RestartPolicy, SupervisionStrategy, VmSupervisor};
pub use vm_registry::{VmRegistry, VmRegistryEntry};
pub use config::{
    BootSource, Drive, DriveConfig, MachineConfig, NetworkInterface, NetworkInterfaceConfig,
    RateLimiter, TokenBucket, VmConfig,
};
pub use error::{FirecrackerError, FirecrackerResult};
pub use network::{create_tap_device, delete_tap_device, generate_tap_name};
pub use vm::{FirecrackerVm, VmState};
