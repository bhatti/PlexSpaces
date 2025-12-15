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

//! Firecracker VM configuration types

use serde::{Deserialize, Serialize};

/// Firecracker VM configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VmConfig {
    /// VM identifier (ULID)
    pub vm_id: String,

    /// Number of vCPUs (1-32)
    pub vcpu_count: u8,

    /// Memory size in MB (128-32768)
    pub mem_size_mib: u32,

    /// Kernel image path
    pub kernel_image_path: String,

    /// Kernel boot arguments
    pub boot_args: String,

    /// Rootfs drive configuration
    pub rootfs: DriveConfig,

    /// Network interfaces
    pub network_interfaces: Vec<NetworkInterfaceConfig>,

    /// Socket path for Firecracker API
    pub socket_path: String,
}

impl Default for VmConfig {
    fn default() -> Self {
        Self {
            vm_id: ulid::Ulid::new().to_string(),
            vcpu_count: 2,
            mem_size_mib: 512,
            kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
            boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
            rootfs: DriveConfig::default(),
            network_interfaces: vec![],
            socket_path: String::new(), // Will be set dynamically
        }
    }
}

/// Drive configuration (rootfs, data drives)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriveConfig {
    /// Drive identifier
    pub drive_id: String,

    /// Is this the root filesystem?
    pub is_root_device: bool,

    /// Path to drive image
    pub path_on_host: String,

    /// Is drive read-only?
    pub is_read_only: bool,
}

impl Default for DriveConfig {
    fn default() -> Self {
        Self {
            drive_id: "rootfs".to_string(),
            is_root_device: true,
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_read_only: false,
        }
    }
}

/// Network interface configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterfaceConfig {
    /// Interface identifier
    pub iface_id: String,

    /// TAP device name on host
    pub host_dev_name: String,

    /// Guest MAC address
    pub guest_mac: String,
}

impl NetworkInterfaceConfig {
    /// Create default network interface
    pub fn default_with_tap(tap_name: String) -> Self {
        Self {
            iface_id: "eth0".to_string(),
            host_dev_name: tap_name,
            guest_mac: "AA:FC:00:00:00:01".to_string(),
        }
    }
}

/// Machine configuration (vCPU and memory)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MachineConfig {
    /// Number of vCPUs
    pub vcpu_count: u8,

    /// Memory size in MB
    pub mem_size_mib: u32,

    /// Enable SMT (Simultaneous Multithreading)
    #[serde(default)]
    pub smt: bool,

    /// Track dirty pages for live migration
    #[serde(default)]
    pub track_dirty_pages: bool,
}

/// Boot source configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BootSource {
    /// Path to kernel image
    pub kernel_image_path: String,

    /// Kernel boot arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub boot_args: Option<String>,

    /// Path to initrd image
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initrd_path: Option<String>,
}

/// Drive configuration for Firecracker API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Drive {
    /// Drive identifier
    pub drive_id: String,

    /// Path to drive image on host
    pub path_on_host: String,

    /// Is this the root device?
    pub is_root_device: bool,

    /// Is drive read-only?
    pub is_read_only: bool,

    /// Rate limiter configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limiter: Option<RateLimiter>,
}

/// Network interface for Firecracker API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkInterface {
    /// Interface identifier
    pub iface_id: String,

    /// TAP device name on host
    pub host_dev_name: String,

    /// Guest MAC address
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guest_mac: Option<String>,

    /// Rate limiter configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rx_rate_limiter: Option<RateLimiter>,

    /// Rate limiter configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tx_rate_limiter: Option<RateLimiter>,
}

/// Rate limiter configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimiter {
    /// Bandwidth limit (bytes/second)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bandwidth: Option<TokenBucket>,

    /// Operations limit (ops/second)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ops: Option<TokenBucket>,
}

/// Token bucket for rate limiting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBucket {
    /// Bucket size (maximum burst)
    pub size: u64,

    /// Refill rate (tokens per millisecond)
    pub refill_time: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_vm_config() {
        let config = VmConfig::default();
        assert_eq!(config.vcpu_count, 2);
        assert_eq!(config.mem_size_mib, 512);
        assert!(!config.vm_id.is_empty());
    }

    #[test]
    fn test_default_drive_config() {
        let drive = DriveConfig::default();
        assert_eq!(drive.drive_id, "rootfs");
        assert!(drive.is_root_device);
    }

    #[test]
    fn test_network_interface_with_tap() {
        let iface = NetworkInterfaceConfig::default_with_tap("tap0".to_string());
        assert_eq!(iface.iface_id, "eth0");
        assert_eq!(iface.host_dev_name, "tap0");
    }
}
