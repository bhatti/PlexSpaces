// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for VM registry

use plexspaces_firecracker::VmRegistry;

#[tokio::test]
async fn test_vm_registry_discover_vms_empty() {
    // Test: Discover VMs when none are running
    let vms = VmRegistry::discover_vms().await.unwrap();
    // Should not panic, may return empty list
    assert!(vms.len() >= 0);
}

#[tokio::test]
async fn test_vm_registry_find_vm_not_found() {
    // Test: Find non-existent VM
    let result = VmRegistry::find_vm("non-existent-vm-id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn test_vm_registry_get_socket_path_not_found() {
    // Test: Get socket path for non-existent VM
    let result = VmRegistry::get_vm_socket_path("non-existent-vm-id").await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
#[ignore] // Requires running VM
async fn test_vm_registry_discover_running_vm() {
    use plexspaces_firecracker::{FirecrackerVm, VmConfig, VmState};
    use std::time::Duration;
    use tokio::time::sleep;
    use ulid::Ulid;

    // Check prerequisites
    if std::path::Path::new("/var/lib/firecracker/vmlinux").exists() &&
       std::path::Path::new("/var/lib/firecracker/rootfs.ext4").exists() {
        let vm_id = Ulid::new().to_string();
        let config = VmConfig {
            vm_id: vm_id.clone(),
            vcpu_count: 1,
            mem_size_mib: 256,
            kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
            rootfs: plexspaces_firecracker::config::DriveConfig {
                path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
                ..Default::default()
            },
            ..Default::default()
        };

        let mut vm = FirecrackerVm::create(config).await.unwrap();
        vm.start_firecracker().await.unwrap();
        vm.boot().await.unwrap();

        sleep(Duration::from_millis(500)).await;

        let vms = VmRegistry::discover_vms().await.unwrap();
        let found = vms.iter().find(|v| v.vm_id == vm_id);
        assert!(found.is_some(), "Should discover running VM");

        vm.stop().await.unwrap();
    } else {
        eprintln!("Skipping: Firecracker prerequisites not available");
    }
}

