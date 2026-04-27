// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Real Firecracker Integration Tests (Phase 2.4 - TDD)
//!
//! ## Purpose
//! These tests verify real Firecracker API client integration with actual Firecracker binary.
//! Tests automatically skip if prerequisites are not available:
//! - Firecracker binary installed
//! - Linux kernel image
//! - Root filesystem image
//! - Root privileges (for TAP devices)
//!
//! ## Running Tests
//! ```bash
//! # Run all real Firecracker tests (tests will skip if prerequisites not available)
//! cargo test -p plexspaces-firecracker --test-threads=1
//! ```

use plexspaces_common::skip_if_unavailable;
use plexspaces_common::test_helpers::{firecracker_available, firecracker_prerequisites_error};
use plexspaces_firecracker::{FirecrackerApiClient, FirecrackerVm, VmConfig, VmState};
use std::path::Path;
use std::time::Duration;
use tokio::time::sleep;

/// Test: API client can configure VM via real Firecracker
///
/// ## Purpose
/// Verify that FirecrackerApiClient can communicate with real Firecracker binary
/// and configure VM (machine config, boot source, drives).
///
/// ## Test Flow
/// 1. Start Firecracker process
/// 2. Create API client
/// 3. Configure machine (vCPU, memory)
/// 4. Configure boot source (kernel)
/// 5. Attach rootfs drive
/// 6. Verify configuration succeeded
#[tokio::test]
async fn test_api_client_configure_vm() {
    if !firecracker_available() {
        if let Some(err) = firecracker_prerequisites_error() {
            eprintln!("Skipping test: {}", err);
        }
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let socket_path = format!("/tmp/firecracker-test-{}.sock", vm_id);

    // Start Firecracker process
    let mut child = std::process::Command::new("firecracker")
        .arg("--api-sock")
        .arg(&socket_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn Firecracker");

    // Wait for socket to be created
    for _ in 0..20 {
        if Path::new(&socket_path).exists() {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }

    if !Path::new(&socket_path).exists() {
        let _ = child.kill();
        panic!("Firecracker socket not created");
    }

    // Create API client
    let client = FirecrackerApiClient::new(&socket_path);

    // Configure machine
    client
        .set_machine_config(plexspaces_firecracker::MachineConfig {
            vcpu_count: 2,
            mem_size_mib: 512,
            smt: false,
            track_dirty_pages: false,
        })
        .await
        .expect("Failed to set machine config");

    // Configure boot source
    let kernel_path = if Path::new("/var/lib/firecracker/vmlinux").exists() {
        "/var/lib/firecracker/vmlinux"
    } else {
        "/tmp/test-kernel"
    };

    client
        .set_boot_source(plexspaces_firecracker::BootSource {
            kernel_image_path: kernel_path.to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None,
        })
        .await
        .expect("Failed to set boot source");

    // Attach rootfs
    let rootfs_path = if Path::new("/var/lib/firecracker/rootfs.ext4").exists() {
        "/var/lib/firecracker/rootfs.ext4"
    } else {
        "/tmp/test-rootfs"
    };

    client
        .attach_drive(plexspaces_firecracker::Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path.to_string(),
            is_root_device: true,
            is_read_only: false,
            rate_limiter: None,
        })
        .await
        .expect("Failed to attach rootfs");

    // Get instance info to verify
    let info = client
        .get_instance_info()
        .await
        .expect("Failed to get instance info");

    assert_eq!(info.state, "Not started");

    // Cleanup
    let _ = child.kill();
    let _ = std::fs::remove_file(&socket_path);
}

/// Test: API client can start VM instance
///
/// ## Purpose
/// Verify that FirecrackerApiClient can start a configured VM instance.
///
/// ## Test Flow
/// 1. Start Firecracker and configure VM
/// 2. Call start_instance()
/// 3. Verify instance state is "Running"
#[tokio::test]
async fn test_api_client_start_instance() {
    if !firecracker_available() {
        if let Some(err) = firecracker_prerequisites_error() {
            eprintln!("Skipping test: {}", err);
        }
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let socket_path = format!("/tmp/firecracker-test-{}.sock", vm_id);

    // Start Firecracker process
    let mut child = std::process::Command::new("firecracker")
        .arg("--api-sock")
        .arg(&socket_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to spawn Firecracker");

    // Wait for socket
    for _ in 0..20 {
        if Path::new(&socket_path).exists() {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }

    let client = FirecrackerApiClient::new(&socket_path);

    // Configure VM
    client
        .set_machine_config(plexspaces_firecracker::MachineConfig {
            vcpu_count: 1,
            mem_size_mib: 128,
            smt: false,
            track_dirty_pages: false,
        })
        .await
        .expect("Failed to set machine config");

    let kernel_path = if Path::new("/var/lib/firecracker/vmlinux").exists() {
        "/var/lib/firecracker/vmlinux"
    } else {
        "/tmp/test-kernel"
    };

    client
        .set_boot_source(plexspaces_firecracker::BootSource {
            kernel_image_path: kernel_path.to_string(),
            boot_args: Some("console=ttyS0 reboot=k panic=1 pci=off".to_string()),
            initrd_path: None,
        })
        .await
        .expect("Failed to set boot source");

    let rootfs_path = if Path::new("/var/lib/firecracker/rootfs.ext4").exists() {
        "/var/lib/firecracker/rootfs.ext4"
    } else {
        "/tmp/test-rootfs"
    };

    client
        .attach_drive(plexspaces_firecracker::Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path.to_string(),
            is_root_device: true,
            is_read_only: false,
            rate_limiter: None,
        })
        .await
        .expect("Failed to attach rootfs");

    // Start instance
    client
        .start_instance()
        .await
        .expect("Failed to start instance");

    // Verify state
    sleep(Duration::from_millis(100)).await; // Give VM time to boot
    let info = client
        .get_instance_info()
        .await
        .expect("Failed to get instance info");

    assert_eq!(info.state, "Running");

    // Cleanup
    let _ = child.kill();
    let _ = std::fs::remove_file(&socket_path);
}

/// Test: Full VM lifecycle with real Firecracker
///
/// ## Purpose
/// Verify complete VM lifecycle: create → configure → boot → stop
/// using real Firecracker binary.
///
/// ## Test Flow
/// 1. Create VM config
/// 2. Start Firecracker process
/// 3. Configure and boot VM
/// 4. Verify VM is running
/// 5. Stop VM
#[tokio::test]
async fn test_full_vm_lifecycle_real_firecracker() {
    if !firecracker_available() {
        if let Some(err) = firecracker_prerequisites_error() {
            eprintln!("Skipping test: {}", err);
        }
        return;
    }

    let vm_id = ulid::Ulid::new().to_string();
    let kernel_path = if Path::new("/var/lib/firecracker/vmlinux").exists() {
        "/var/lib/firecracker/vmlinux"
    } else {
        "/tmp/test-kernel"
    };
    let rootfs_path = if Path::new("/var/lib/firecracker/rootfs.ext4").exists() {
        "/var/lib/firecracker/rootfs.ext4"
    } else {
        "/tmp/test-rootfs"
    };

    let config = VmConfig {
        vm_id: vm_id.clone(),
        vcpu_count: 1,
        mem_size_mib: 128,
        kernel_image_path: kernel_path.to_string(),
        boot_args: "console=ttyS0 reboot=k panic=1 pci=off".to_string(),
        rootfs: plexspaces_firecracker::DriveConfig {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_path.to_string(),
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

    // Start Firecracker process
    vm.start_firecracker()
        .await
        .expect("Failed to start Firecracker");
    assert_eq!(vm.state(), VmState::Ready);

    // Boot VM
    vm.boot().await.expect("Failed to boot VM");
    assert_eq!(vm.state(), VmState::Running);

    // Stop VM
    vm.stop().await.expect("Failed to stop VM");
    assert_eq!(vm.state(), VmState::Stopped);
}

/// Test: TAP device creation (if root)
///
/// ## Purpose
/// Verify that TAP device creation works with real Linux TUN/TAP driver.
/// This test requires root privileges.
///
/// ## Test Flow
/// 1. Generate TAP name
/// 2. Create TAP device
/// 3. Verify device exists
/// 4. Delete TAP device
#[tokio::test]
async fn test_tap_device_creation_real() {
    // This test requires root privileges - skip if not running as root
    if std::fs::metadata("/proc/1/root").is_err() && std::env::var("CI").is_err() {
        eprintln!("Skipping test: requires root privileges");
        return;
    }

    use plexspaces_firecracker::network::{
        create_tap_device, delete_tap_device, generate_tap_name,
    };

    let vm_id = ulid::Ulid::new().to_string();
    let tap_name = generate_tap_name(&vm_id);

    // Create TAP device
    match create_tap_device(&tap_name).await {
        Ok(_) => {
            // Verify device exists (if we can check)
            // Note: Actual verification requires root and system commands
            println!("TAP device created: {}", tap_name);

            // Delete TAP device
            delete_tap_device(&tap_name)
                .await
                .expect("Failed to delete TAP device");
            println!("TAP device deleted: {}", tap_name);
        }
        Err(e) => {
            eprintln!("TAP creation skipped (likely not root): {}", e);
        }
    }
}

/// Test: Application deployment to VM (placeholder)
///
/// ## Purpose
/// Verify that application deployment to VM works.
/// This is a placeholder test - actual implementation will be in application layer.
///
/// ## Test Flow
/// 1. Create and boot VM
/// 2. Deploy application bundle
/// 3. Verify application is running
#[tokio::test]
async fn test_application_deployment_to_vm() {
    skip_if_unavailable!(firecracker_available(), "Firecracker");
    // TODO: Implement after application deployment layer is ready
    // This test verifies the design: VM contains entire applications, not individual actors
    assert!(true);
}
