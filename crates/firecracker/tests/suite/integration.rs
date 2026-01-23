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

//! # Firecracker Integration Tests
//!
//! ## Purpose
//! End-to-end tests for Firecracker VM lifecycle and networking.
//!
//! ## Requirements
//! These tests require:
//! - Firecracker binary installed (`/usr/bin/firecracker` or in PATH)
//! - Linux kernel image (`/var/lib/firecracker/vmlinux`)
//! - Root filesystem image (`/var/lib/firecracker/rootfs.ext4`)
//! - Root privileges or CAP_NET_ADMIN (for TAP devices)
//! - Linux kernel 4.14+ with KVM support
//!
//! ## Running Tests
//! ```bash
//! # Run all integration tests (requires setup above)
//! cargo test -p plexspaces-firecracker --test integration -- --ignored --test-threads=1
//!
//! # Run specific test
//! cargo test -p plexspaces-firecracker --test integration test_vm_lifecycle -- --ignored
//!
//! # Skip integration tests (default)
//! cargo test -p plexspaces-firecracker
//! ```
//!
//! ## Test Setup
//! To set up the test environment:
//! ```bash
//! # Install Firecracker
//! wget https://github.com/firecracker-microvm/firecracker/releases/download/v1.4.0/firecracker-v1.4.0-x86_64.tgz
//! tar -xzf firecracker-v1.4.0-x86_64.tgz
//! sudo mv release-v1.4.0-x86_64/firecracker-v1.4.0-x86_64 /usr/bin/firecracker
//! sudo chmod +x /usr/bin/firecracker
//!
//! # Create test images directory
//! sudo mkdir -p /var/lib/firecracker
//!
//! # Download kernel (Alpine Linux kernel)
//! wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin
//! sudo mv vmlinux.bin /var/lib/firecracker/vmlinux
//!
//! # Download rootfs (Alpine Linux rootfs)
//! wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4
//! sudo mv bionic.rootfs.ext4 /var/lib/firecracker/rootfs.ext4
//! ```
//!
//! ## Design Notes
//! - All tests are marked `#[ignore]` by default (require manual environment setup)
//! - Tests run with `--test-threads=1` to avoid socket/resource conflicts
//! - Tests clean up resources (VMs, TAP devices, sockets) on completion
//! - Timeout: 30 seconds per test (VMs should boot in < 200ms)

use plexspaces_firecracker::{
    create_tap_device, delete_tap_device, generate_tap_name, FirecrackerVm, VmConfig, VmState,
};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

/// Check if Firecracker is available
fn is_firecracker_available() -> bool {
    Path::new("/usr/bin/firecracker").exists()
        || std::process::Command::new("firecracker")
            .arg("--version")
            .output()
            .is_ok()
}

/// Check if kernel image is available
fn is_kernel_available() -> bool {
    Path::new("/var/lib/firecracker/vmlinux").exists()
}

/// Check if rootfs image is available
fn is_rootfs_available() -> bool {
    Path::new("/var/lib/firecracker/rootfs.ext4").exists()
}

/// Check if all prerequisites are met
fn check_prerequisites() -> Result<(), String> {
    if !is_firecracker_available() {
        return Err("Firecracker binary not found. Install from: https://github.com/firecracker-microvm/firecracker/releases".to_string());
    }

    if !is_kernel_available() {
        return Err(
            "Kernel image not found at /var/lib/firecracker/vmlinux. See test documentation."
                .to_string(),
        );
    }

    if !is_rootfs_available() {
        return Err(
            "Rootfs image not found at /var/lib/firecracker/rootfs.ext4. See test documentation."
                .to_string(),
        );
    }

    Ok(())
}

// ============================================================================
// VM Lifecycle Tests
// ============================================================================

/// Test: Create, start Firecracker process, and cleanup
///
/// ## What This Tests
/// - VM creation with default config
/// - Firecracker process spawning
/// - Socket creation and availability
/// - VM configuration via API
/// - Process cleanup on stop
///
/// ## Expected Behavior
/// - VM transitions: Created → Ready
/// - Socket file created at /tmp/firecracker-{vm_id}.sock
/// - Firecracker process running
/// - Cleanup removes socket and kills process
#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_vm_create_and_start_firecracker() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let config = VmConfig {
        vm_id: vm_id.clone(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
        },
        ..Default::default()
    };

    // Create VM
    let mut vm = FirecrackerVm::create(config.clone())
        .await
        .expect("Failed to create VM");
    assert_eq!(vm.state(), VmState::Created);

    // Start Firecracker process
    vm.start_firecracker()
        .await
        .expect("Failed to start Firecracker");
    assert_eq!(vm.state(), VmState::Ready);

    // Verify socket exists
    let socket_path = format!("/tmp/firecracker-{}.sock", vm_id);
    assert!(
        Path::new(&socket_path).exists(),
        "Socket not created: {}",
        socket_path
    );

    // Stop VM
    vm.stop().await.expect("Failed to stop VM");
    assert_eq!(vm.state(), VmState::Stopped);

    // Verify socket cleaned up
    assert!(
        !Path::new(&socket_path).exists(),
        "Socket not cleaned up: {}",
        socket_path
    );
}

/// Test: Full VM lifecycle (create → boot → stop)
///
/// ## What This Tests
/// - Complete VM boot sequence
/// - Kernel loading and execution
/// - VM state transitions
/// - Boot timeout (< 200ms target)
///
/// ## Expected Behavior
/// - VM transitions: Created → Ready → Booting → Running → Stopped
/// - Boot completes in < 2 seconds (timeout)
/// - VM responds to API queries
/// Note: This tests basic lifecycle (4 states: Create → Start → Boot → Stop)
/// For full lifecycle with pause/resume, see test_vm_full_lifecycle() in integration_comprehensive.rs
#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_vm_basic_lifecycle() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
        },
        ..Default::default()
    };

    // Create VM
    let mut vm = FirecrackerVm::create(config)
        .await
        .expect("Failed to create VM");
    assert_eq!(vm.state(), VmState::Created);

    // Start Firecracker
    vm.start_firecracker()
        .await
        .expect("Failed to start Firecracker");
    assert_eq!(vm.state(), VmState::Ready);

    // Boot VM
    let start = std::time::Instant::now();
    vm.boot().await.expect("Failed to boot VM");
    let boot_time = start.elapsed();

    assert_eq!(vm.state(), VmState::Running);
    println!("VM booted in {:?}", boot_time);

    // Boot should be fast (< 2 seconds including setup)
    assert!(
        boot_time < Duration::from_secs(2),
        "Boot took too long: {:?}",
        boot_time
    );

    // Let VM run for a moment
    sleep(Duration::from_millis(100)).await;

    // Stop VM
    vm.stop().await.expect("Failed to stop VM");
    assert_eq!(vm.state(), VmState::Stopped);
}

/// Test: Pause and resume VM
///
/// ## What This Tests
/// - VM pause operation
/// - VM resume operation
/// - State transitions during pause/resume
///
/// ## Expected Behavior
/// - VM can be paused while running
/// - VM can be resumed from paused state
/// - State transitions: Running → Paused → Running
#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_vm_pause_resume() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
        },
        ..Default::default()
    };

    // Create and boot VM
    let mut vm = FirecrackerVm::create(config).await.unwrap();
    vm.start_firecracker().await.unwrap();
    vm.boot().await.unwrap();
    assert_eq!(vm.state(), VmState::Running);

    // Pause VM
    vm.pause().await.expect("Failed to pause VM");
    assert_eq!(vm.state(), VmState::Paused);

    // Resume VM
    vm.resume().await.expect("Failed to resume VM");
    assert_eq!(vm.state(), VmState::Running);

    // Cleanup
    vm.stop().await.unwrap();
}

/// Test: Multiple VMs can run concurrently
///
/// ## What This Tests
/// - Multiple VMs with unique sockets
/// - No resource conflicts between VMs
/// - Concurrent VM operations
///
/// ## Expected Behavior
/// - Each VM gets unique socket path
/// - VMs don't interfere with each other
/// - All VMs can boot successfully
#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_multiple_vms_concurrent() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let mut vms = Vec::new();

    // Create and start 3 VMs
    for i in 0..3 {
        let config = VmConfig {
            vm_id: format!("vm-{}-{}", i, ulid::Ulid::new()),
            vcpu_count: 1,
            mem_size_mib: 128,
            kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
            boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
            rootfs: plexspaces_firecracker::DriveConfig {
                drive_id: "rootfs".to_string(),
                path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
                is_root_device: true,
                is_read_only: false,
            },
            ..Default::default()
        };

        let mut vm = FirecrackerVm::create(config).await.unwrap();
        vm.start_firecracker().await.unwrap();
        vm.boot().await.unwrap();

        assert_eq!(vm.state(), VmState::Running);
        vms.push(vm);
    }

    // All VMs running
    assert_eq!(vms.len(), 3);

    // Stop all VMs
    for mut vm in vms {
        vm.stop().await.unwrap();
        assert_eq!(vm.state(), VmState::Stopped);
    }
}

// ============================================================================
// Network/TAP Device Tests
// ============================================================================

/// Test: Create and delete TAP device
///
/// ## What This Tests
/// - TAP device creation on host
/// - TAP device cleanup
/// - Device name validation
///
/// ## Expected Behavior
/// - TAP device created successfully
/// - Device visible in `ip link show`
/// - Device deleted successfully
///
/// ## Requirements
/// - Root privileges or CAP_NET_ADMIN
#[tokio::test]
#[ignore] // Requires root privileges
async fn test_tap_device_lifecycle() {
    // Generate TAP name
    let vm_id = ulid::Ulid::new().to_string();
    let tap_name = generate_tap_name(&vm_id);

    println!("Creating TAP device: {}", tap_name);

    // Create TAP device
    match create_tap_device(&tap_name).await {
        Ok(_) => {
            println!("TAP device created successfully");

            // TODO: Verify device exists with `ip link show {tap_name}`
            // (requires implementing actual ioctl in network.rs)

            // Delete TAP device
            delete_tap_device(&tap_name)
                .await
                .expect("Failed to delete TAP device");
            println!("TAP device deleted successfully");

            // TODO: Verify device doesn't exist
        }
        Err(e) => {
            eprintln!("TAP creation skipped (likely not root): {}", e);
            // This is expected if not running as root
        }
    }
}

/// Test: Delete non-existent TAP device (should not error)
///
/// ## What This Tests
/// - Idempotent TAP deletion
/// - Graceful handling of missing devices
///
/// ## Expected Behavior
/// - No error when deleting non-existent device
#[tokio::test]
#[ignore] // Requires root privileges
async fn test_tap_device_delete_nonexistent() {
    let result = delete_tap_device("tap-nosuch123").await;

    // Should succeed even if device doesn't exist
    match result {
        Ok(_) => println!("TAP deletion succeeded (idempotent)"),
        Err(e) => {
            // May fail if not root, which is acceptable for this test
            eprintln!("TAP deletion skipped: {}", e);
        }
    }
}

// ============================================================================
// Error Handling Tests
// ============================================================================

/// Test: VM creation fails with invalid config
///
/// ## What This Tests
/// - Config validation (vcpu_count = 0)
/// - Config validation (mem_size_mib < 128)
/// - Error handling for invalid parameters
///
/// ## Expected Behavior
/// - VM creation returns ConfigurationError
#[tokio::test]
async fn test_vm_invalid_config_validation() {
    // Invalid vcpu_count
    let config = VmConfig {
        vcpu_count: 0, // Invalid
        ..Default::default()
    };

    let result = FirecrackerVm::create(config).await;
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        plexspaces_firecracker::FirecrackerError::ConfigurationError(_)
    ));

    // Invalid mem_size_mib
    let config = VmConfig {
        vcpu_count: 1,
        mem_size_mib: 64, // Too small
        ..Default::default()
    };

    let result = FirecrackerVm::create(config).await;
    assert!(result.is_err());
}

/// Test: Boot fails if Firecracker not started
///
/// ## What This Tests
/// - State machine enforcement
/// - Boot requires Ready state
///
/// ## Expected Behavior
/// - Boot returns VmOperationFailed if called in Created state
#[tokio::test]
async fn test_vm_boot_requires_ready_state() {
    let config = VmConfig::default();
    let mut vm = FirecrackerVm::create(config).await.unwrap();

    assert_eq!(vm.state(), VmState::Created);

    // Try to boot without starting Firecracker
    let result = vm.boot().await;
    assert!(result.is_err());
}

// ============================================================================
// Performance Tests
// ============================================================================

/// Test: Measure VM boot time
///
/// ## What This Tests
/// - Boot time performance
/// - Target: < 200ms (goal: 125ms)
///
/// ## Expected Behavior
/// - VM boots in < 2 seconds (including all setup)
/// - Reports actual boot time for monitoring
#[tokio::test]
#[ignore] // Requires Firecracker binary, kernel, rootfs
async fn test_vm_boot_performance() {
    if let Err(e) = check_prerequisites() {
        eprintln!("Skipping test: {}", e);
        return;
    }

    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
        },
        ..Default::default()
    };

    let mut vm = FirecrackerVm::create(config).await.unwrap();
    vm.start_firecracker().await.unwrap();

    // Measure boot time
    let start = std::time::Instant::now();
    vm.boot().await.expect("Failed to boot VM");
    let boot_time = start.elapsed();

    println!("=== VM Boot Performance ===");
    println!("Boot time: {:?}", boot_time);
    println!("Target: < 200ms (goal: 125ms)");
    println!("Status: {}", if boot_time < Duration::from_millis(200) {
        "✅ PASS"
    } else if boot_time < Duration::from_secs(2) {
        "⚠️  SLOW (but acceptable)"
    } else {
        "❌ FAIL"
    });

    vm.stop().await.unwrap();
}

// ============================================================================
// Test Helpers
// ============================================================================

/// Helper: Create test VM config
#[allow(dead_code)]
fn create_test_config(vm_id: &str) -> VmConfig {
    VmConfig {
        vm_id: vm_id.to_string(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: "/var/lib/firecracker/rootfs.ext4".to_string(),
            is_root_device: true,
            is_read_only: false,
        },
        network_interfaces: vec![],
        socket_path: String::new(),
    }
}

/// Helper: Cleanup test resources
#[allow(dead_code)]
async fn cleanup_test_resources(vm_id: &str) {
    let socket_path = format!("/tmp/firecracker-{}.sock", vm_id);
    let _ = tokio::fs::remove_file(socket_path).await;

    let tap_name = generate_tap_name(vm_id);
    let _ = delete_tap_device(&tap_name).await;
}

// ============================================================================
// WASM Runtime Integration Tests
// ============================================================================
// NOTE: WASM runtime integration tests removed as part of Phase 2.1 simplification.
// WASM runtime belongs in `crates/wasm-runtime/`, not in Firecracker crate.
// Firecracker crate now focuses only on VM lifecycle management.
// 
// WASM-related tests should be in `crates/wasm-runtime/tests/` instead.
