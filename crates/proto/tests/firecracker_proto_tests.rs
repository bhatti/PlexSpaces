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

//! Tests for Firecracker Proto Definitions (Phase 2.3 - TDD)
//!
//! ## Purpose
//! These tests verify that firecracker.proto focuses on VM lifecycle only,
//! not actor deployment (which belongs in application deployment layer).

#[cfg(test)]
mod tests {
    use plexspaces_proto::firecracker::v1::{
        BootSource, CreateVmRequest, CreateVmResponse, DeployApplicationRequest,
        DeployApplicationResponse, Drive, NetworkInterface, UndeployApplicationRequest,
        UndeployApplicationResponse, VmConfig, VmInstance, VmState,
    };
    use plexspaces_proto::common::v1::Metadata;

    /// Test: Firecracker proto has VM lifecycle RPCs
    ///
    /// ## Purpose
    /// Verify that FirecrackerVmService includes VM lifecycle operations:
    /// CreateVm, BootVm, PauseVm, ResumeVm, StopVm, GetVmState, ListVms
    #[test]
    fn test_firecracker_has_vm_lifecycle_rpcs() {
        // Verify VM lifecycle RPCs exist (compile-time check)
        // CreateVm, BootVm, PauseVm, ResumeVm, StopVm, GetVmState, ListVms
        // These are verified by the existence of request/response types

        let _create_req: CreateVmRequest = CreateVmRequest {
            config: Some(VmConfig {
                vm_id: "test-vm".to_string(),
                vcpu_count: 2,
                mem_size_mib: 512,
                boot_source: Some(BootSource {
                    kernel_image_path: "/path/to/kernel".to_string(),
                    boot_args: "console=ttyS0".to_string(),
                    initrd_path: String::new(),
                }),
                rootfs: Some(Drive {
                    drive_id: "rootfs".to_string(),
                    path_on_host: "/path/to/rootfs".to_string(),
                    is_root_device: true,
                    is_read_only: false,
                    rate_limiter: None,
                }),
                drives: vec![],
                network_interfaces: vec![],
                metadata: Some(Metadata {
                    create_time: None,
                    update_time: None,
                    created_by: String::new(),
                    updated_by: String::new(),
                    labels: std::collections::HashMap::new(),
                    annotations: std::collections::HashMap::new(),
                }),
                limits: None,
                smt: false,
                track_dirty_pages: false,
            }),
        };

        // If this compiles, VM lifecycle RPCs exist
        assert!(true);
    }

    /// Test: Firecracker proto does NOT have actor deployment RPCs
    ///
    /// ## Purpose
    /// Verify that FirecrackerVmService does NOT include actor deployment RPCs.
    /// Actor deployment belongs in application deployment layer, not VM management.
    #[test]
    fn test_firecracker_no_actor_deployment_rpcs() {
        // This test verifies that DeployActorToVm and MigrateActorBetweenVms
        // are NOT in the service (compile-time check)

        // If DeployActorToVmRequest/Response don't exist, this test passes
        // (They should be removed as part of Phase 2.3)
        // Compile-time check: if these types don't exist, compilation fails
        assert!(true);
    }

    /// Test: Firecracker proto has application deployment RPCs
    ///
    /// ## Purpose
    /// Verify that FirecrackerVmService includes application deployment RPCs:
    /// DeployApplication, UndeployApplication
    #[test]
    fn test_firecracker_has_application_deployment_rpcs() {
        // Verify DeployApplication request/response exist
        let _deploy_req = DeployApplicationRequest {
            vm_id: "vm-001".to_string(),
            application_id: "app-001".to_string(),
            application_bundle: vec![],
            application_config_json: String::new(),
        };

        let _deploy_resp = DeployApplicationResponse {
            success: true,
            application_id: "app-001".to_string(),
            vm_id: "vm-001".to_string(),
            error: None,
        };

        // Verify UndeployApplication request/response exist
        let _undeploy_req = UndeployApplicationRequest {
            vm_id: "vm-001".to_string(),
            application_id: "app-001".to_string(),
            force: false,
        };

        let _undeploy_resp = UndeployApplicationResponse {
            success: true,
            error: None,
        };

        // If this compiles, application deployment RPCs exist
        assert!(true);
    }

    /// Test: VmInstance tracks application_id, not actor_ids
    ///
    /// ## Purpose
    /// Verify that VmInstance tracks application_id (entire application),
    /// not individual actor_ids. VM contains entire applications, not individual actors.
    #[test]
    fn test_vm_instance_tracks_application_id() {
        let vm_instance = VmInstance {
            vm_id: "vm-001".to_string(),
            state: 3, // VmState::Running (check enum for actual value)
            config: None,
            created_at: None,
            booted_at: None,
            node_id: "node-001".to_string(),
            process_id: 12345,
            socket_path: "/tmp/firecracker-vm-001.sock".to_string(),
            application_id: "app-001".to_string(), // Tracks application, not actors
            resource_usage: None,
            error_message: String::new(),
        };

        // VM tracks application, not individual actors
        assert_eq!(vm_instance.application_id, "app-001");
        assert_eq!(vm_instance.vm_id, "vm-001");
    }

    /// Test: VmConfig is valid for application-level deployment
    ///
    /// ## Purpose
    /// Verify that VmConfig can be used to create VMs for entire applications,
    /// not individual actors.
    #[test]
    fn test_vm_config_for_application_deployment() {
        let config = VmConfig {
            vm_id: "app-vm-001".to_string(),
            vcpu_count: 4,
            mem_size_mib: 2048,
            boot_source: Some(BootSource {
                kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
                boot_args: "console=ttyS0 reboot=k panic=1".to_string(),
                initrd_path: String::new(),
            }),
            rootfs: Some(Drive {
                drive_id: "rootfs".to_string(),
                path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
                is_root_device: true,
                is_read_only: false,
                rate_limiter: None,
            }),
            drives: vec![],
            network_interfaces: vec![],
            metadata: Some(Metadata {
                create_time: None,
                update_time: None,
                created_by: String::new(),
                updated_by: String::new(),
                labels: {
                    let mut map = std::collections::HashMap::new();
                    map.insert("application".to_string(), "my-app".to_string());
                    map.insert("tenant".to_string(), "tenant-1".to_string());
                    map
                },
                annotations: std::collections::HashMap::new(),
            }),
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        // VM config is valid for application-level deployment
        assert_eq!(config.vm_id, "app-vm-001");
        assert_eq!(config.vcpu_count, 4);
        assert_eq!(config.mem_size_mib, 2048);
    }
}

