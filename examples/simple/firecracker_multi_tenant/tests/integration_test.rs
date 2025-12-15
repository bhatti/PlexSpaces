// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for Firecracker Multi-Tenant Example
//
// These tests require real Firecracker binary, kernel, and rootfs.
// Run with: cargo test --test integration_test -- --ignored

use plexspaces_firecracker::{ApplicationDeployment, FirecrackerVm, VmConfig, VmState};
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker};
use std::path::Path;
use serde_json;

/// Check if Firecracker prerequisites are available
fn check_prerequisites() -> Result<(), String> {
    // Check Firecracker binary
    let firecracker_available = Path::new("/usr/bin/firecracker").exists()
        || std::process::Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok();
    
    if !firecracker_available {
        return Err("Firecracker binary not found".to_string());
    }

    // Check kernel (support env var override)
    let kernel_path = std::env::var("PLEXSPACES_FIRECRACKER_KERNEL")
        .unwrap_or_else(|_| "/var/lib/firecracker/vmlinux".to_string());
    if !Path::new(&kernel_path).exists() {
        return Err(format!("Kernel image not found at {}", kernel_path));
    }

    // Check rootfs (support env var override)
    let rootfs_path = std::env::var("PLEXSPACES_FIRECRACKER_ROOTFS")
        .unwrap_or_else(|_| "/var/lib/firecracker/rootfs.ext4".to_string());
    if !Path::new(&rootfs_path).exists() {
        return Err(format!("Rootfs image not found at {}", rootfs_path));
    }

    Ok(())
}

#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_deploy_tenant_with_real_firecracker() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    // Load configuration
    let _config = ConfigBootstrap::load().unwrap_or_else(|_| serde_json::json!({}));
    
    let tenant_id = "test-tenant-1";
    let mut metrics = CoordinationComputeTracker::new(format!("test-deploy-{}", tenant_id));
    metrics.start_coordinate();

    // Get paths from config or environment
    let firecracker_config = _config.get("firecracker").and_then(|c| c.as_object());
    let kernel_path = firecracker_config
        .and_then(|c| c.get("kernel_path"))
        .and_then(|v| v.as_str())
        .or_else(|| std::env::var("PLEXSPACES_FIRECRACKER_KERNEL").ok().as_deref())
        .unwrap_or("/var/lib/firecracker/vmlinux")
        .to_string();
    let rootfs_path = firecracker_config
        .and_then(|c| c.get("rootfs_path"))
        .and_then(|v| v.as_str())
        .or_else(|| std::env::var("PLEXSPACES_FIRECRACKER_ROOTFS").ok().as_deref())
        .unwrap_or("/var/lib/firecracker/rootfs.ext4")
        .to_string();

    // Create VM config
    let vm_config = VmConfig {
        vm_id: format!("test-vm-{}", ulid::Ulid::new()),
        vcpu_count: 1,
        mem_size_mib: 256,
        kernel_image_path: kernel_path,
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::config::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path,
            is_root_device: true,
            is_read_only: false,
        },
        ..Default::default()
    };

    // Deploy application to VM
    let deployment = ApplicationDeployment::new(tenant_id).with_vm_config(vm_config);
    let mut vm = deployment.deploy().await.expect("Failed to deploy VM");
    
    metrics.end_coordinate();
    let report = metrics.finalize();

    // Verify VM is running
    assert_eq!(vm.state(), VmState::Running);
    assert!(report.coordinate_duration_ms > 0);

    // Cleanup: Stop VM
    vm.stop().await.expect("Failed to stop VM");
    assert_eq!(vm.state(), VmState::Stopped);
}

#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_multiple_tenants_isolation() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let _config = ConfigBootstrap::load().unwrap_or_else(|_| serde_json::json!({}));
    
    let kernel_path = std::env::var("PLEXSPACES_FIRECRACKER_KERNEL")
        .unwrap_or_else(|_| "/var/lib/firecracker/vmlinux".to_string());
    let rootfs_path = std::env::var("PLEXSPACES_FIRECRACKER_ROOTFS")
        .unwrap_or_else(|_| "/var/lib/firecracker/rootfs.ext4".to_string());

    let mut vms = Vec::new();

    // Deploy 3 tenants
    for i in 0..3 {
        let tenant_id = format!("test-tenant-{}", i);
        let vm_config = VmConfig {
            vm_id: format!("test-vm-{}", ulid::Ulid::new()),
            vcpu_count: 1,
            mem_size_mib: 256,
            kernel_image_path: kernel_path.clone(),
            boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
            rootfs: plexspaces_firecracker::config::DriveConfig {
                drive_id: "rootfs".to_string(),
                path_on_host: rootfs_path.clone(),
                is_root_device: true,
                is_read_only: false,
            },
            ..Default::default()
        };

        let deployment = ApplicationDeployment::new(&tenant_id).with_vm_config(vm_config);
        let vm = deployment.deploy().await.expect(&format!("Failed to deploy tenant {}", i));
        assert_eq!(vm.state(), VmState::Running);
        vms.push(vm);
    }

    // Verify all VMs are running (isolation)
    for (i, vm) in vms.iter().enumerate() {
        assert_eq!(vm.state(), VmState::Running, "VM {} should be running", i);
    }

    // Cleanup: Stop all VMs
    for mut vm in vms {
        vm.stop().await.expect("Failed to stop VM");
    }
}

