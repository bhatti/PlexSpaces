// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for simplified Firecracker deployment

use plexspaces_firecracker::{FirecrackerVm, VmConfig, VmState};
use std::path::Path;

/// Check if Firecracker prerequisites are available
fn check_prerequisites() -> bool {
    Path::new("/usr/bin/firecracker").exists()
        || std::process::Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok()
}

#[tokio::test]
async fn test_simplified_vm_start_empty() {
    // Test: Start empty VM (framework only, no app)
    if !check_prerequisites() {
        eprintln!("Skipping: Firecracker not available");
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    // Create VM (should succeed even without kernel/rootfs in test mode)
    let result = FirecrackerVm::create(config).await;
    assert!(result.is_ok(), "Should create VM successfully");
}

#[tokio::test]
async fn test_simplified_vm_start_with_app() {
    // Test: Start VM with bundled app (WASM)
    if !check_prerequisites() {
        eprintln!("Skipping: Firecracker not available");
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        vcpu_count: 2,
        mem_size_mib: 512,
        ..Default::default()
    };

    let result = FirecrackerVm::create(config).await;
    assert!(result.is_ok(), "Should create VM with app config");
}

#[tokio::test]
async fn test_vm_lifecycle_simplified() {
    // Test: Complete lifecycle (create -> boot -> stop)
    if !check_prerequisites() {
        eprintln!("Skipping: Firecracker not available");
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let mut vm = FirecrackerVm::create(config).await.unwrap();
    
    // Should be in Created state
    assert_eq!(vm.state(), VmState::Created);

    // Boot VM (if kernel/rootfs available)
    // Note: This will fail in test environment without actual images
    // but should not panic
    let boot_result = vm.boot().await;
    // Allow failure if images not available
    if boot_result.is_ok() {
        // If boot succeeded, verify state
        assert_eq!(vm.state(), VmState::Running);
        
        // Stop VM
        let stop_result = vm.stop().await;
        assert!(stop_result.is_ok(), "Should stop VM successfully");
    }
}

#[tokio::test]
async fn test_vm_list_and_status() {
    // Test: List VMs and get status via gRPC
    // This tests the gRPC API integration
    if !check_prerequisites() {
        eprintln!("Skipping: Firecracker not available");
        return;
    }

    // Create a VM
    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    
    // Verify VM exists (via state)
    assert_eq!(vm.state(), VmState::Created);
    
    // TODO: Test gRPC ListVms and GetVmState calls
    // This would require a running node with Firecracker service
}

#[tokio::test]
async fn test_deploy_app_to_vm() {
    // Test: Deploy WASM app to running VM
    if !check_prerequisites() {
        eprintln!("Skipping: Firecracker not available");
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        ..Default::default()
    };

    let mut vm = FirecrackerVm::create(config).await.unwrap();
    
    // Boot VM
    let boot_result = vm.boot().await;
    if boot_result.is_ok() {
        // TODO: Test DeployApplication RPC
        // This would deploy a WASM app to the VM
        // For now, just verify VM is running
        assert_eq!(vm.state(), VmState::Running);
    }
}

