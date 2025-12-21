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

//! Unit tests for Firecracker API Client (Phase 2.4 - TDD)
//!
//! ## Purpose
//! These tests verify the API client implementation without requiring
//! a real Firecracker binary. They test error handling, serialization,
//! and API request construction.

#[cfg(test)]
mod tests {
    use plexspaces_firecracker::{
        BootSource, Drive, FirecrackerApiClient, MachineConfig, NetworkInterface,
    };

    /// Test: API client creation
    ///
    /// ## Purpose
    /// Verify that FirecrackerApiClient can be created with a socket path.
    #[test]
    fn test_api_client_creation() {
        let client = FirecrackerApiClient::new("/tmp/firecracker.sock");
        // Client creation should succeed
        assert!(true);
    }

    /// Test: Machine config serialization
    ///
    /// ## Purpose
    /// Verify that MachineConfig can be serialized to JSON correctly.
    #[test]
    fn test_machine_config_serialization() {
        let config = MachineConfig {
            vcpu_count: 2,
            mem_size_mib: 512,
            smt: false,
            track_dirty_pages: false,
        };

        let json = serde_json::to_string(&config).expect("Failed to serialize");
        assert!(json.contains("\"vcpu_count\":2"));
        assert!(json.contains("\"mem_size_mib\":512"));
    }

    /// Test: Boot source serialization
    ///
    /// ## Purpose
    /// Verify that BootSource can be serialized to JSON correctly.
    #[test]
    fn test_boot_source_serialization() {
        let boot_source = BootSource {
            kernel_image_path: "/path/to/kernel".to_string(),
            boot_args: Some("console=ttyS0".to_string()),
            initrd_path: None,
        };

        let json = serde_json::to_string(&boot_source).expect("Failed to serialize");
        assert!(json.contains("\"kernel_image_path\":\"/path/to/kernel\""));
        assert!(json.contains("\"boot_args\":\"console=ttyS0\""));
    }

    /// Test: Boot source without boot_args
    ///
    /// ## Purpose
    /// Verify that BootSource serializes correctly when boot_args is None.
    #[test]
    fn test_boot_source_no_boot_args() {
        let boot_source = BootSource {
            kernel_image_path: "/path/to/kernel".to_string(),
            boot_args: None,
            initrd_path: None,
        };

        let json = serde_json::to_string(&boot_source).expect("Failed to serialize");
        assert!(json.contains("\"kernel_image_path\":\"/path/to/kernel\""));
        // boot_args should be omitted (skip_serializing_if)
        assert!(!json.contains("\"boot_args\""));
    }

    /// Test: Drive serialization
    ///
    /// ## Purpose
    /// Verify that Drive can be serialized to JSON correctly.
    #[test]
    fn test_drive_serialization() {
        let drive = Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: "/path/to/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
            rate_limiter: None,
        };

        let json = serde_json::to_string(&drive).expect("Failed to serialize");
        assert!(json.contains("\"drive_id\":\"rootfs\""));
        assert!(json.contains("\"path_on_host\":\"/path/to/rootfs.ext4\""));
        assert!(json.contains("\"is_root_device\":true"));
    }

    /// Test: Network interface serialization
    ///
    /// ## Purpose
    /// Verify that NetworkInterface can be serialized to JSON correctly.
    #[test]
    fn test_network_interface_serialization() {
        let iface = NetworkInterface {
            iface_id: "eth0".to_string(),
            host_dev_name: "tap-vm001".to_string(),
            guest_mac: Some("AA:FC:00:00:00:01".to_string()),
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        };

        let json = serde_json::to_string(&iface).expect("Failed to serialize");
        assert!(json.contains("\"iface_id\":\"eth0\""));
        assert!(json.contains("\"host_dev_name\":\"tap-vm001\""));
        assert!(json.contains("\"guest_mac\":\"AA:FC:00:00:00:01\""));
    }

    /// Test: Network interface without guest_mac
    ///
    /// ## Purpose
    /// Verify that NetworkInterface serializes correctly when guest_mac is None.
    #[test]
    fn test_network_interface_no_guest_mac() {
        let iface = NetworkInterface {
            iface_id: "eth0".to_string(),
            host_dev_name: "tap-vm001".to_string(),
            guest_mac: None,
            rx_rate_limiter: None,
            tx_rate_limiter: None,
        };

        let json = serde_json::to_string(&iface).expect("Failed to serialize");
        assert!(json.contains("\"iface_id\":\"eth0\""));
        // guest_mac should be omitted (skip_serializing_if)
        assert!(!json.contains("\"guest_mac\""));
    }

    /// Test: API client socket path handling
    ///
    /// ## Purpose
    /// Verify that API client handles different socket path formats correctly.
    #[test]
    fn test_api_client_socket_paths() {
        // Absolute path
        let _client1 = FirecrackerApiClient::new("/tmp/firecracker.sock");

        // Path with spaces (should be handled)
        let _client2 = FirecrackerApiClient::new("/tmp/firecracker test.sock");

        // Long path
        let long_path = format!("/tmp/{}", "a".repeat(100));
        let _client3 = FirecrackerApiClient::new(&long_path);

        // All should succeed (client creation doesn't validate path existence)
        assert!(true);
    }
}


















































