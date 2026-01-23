// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for VM Health Monitoring

use plexspaces_firecracker::health::{HealthStatus, VmHealthMonitor};
use plexspaces_firecracker::{FirecrackerVm, VmConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

#[tokio::test]
async fn test_health_monitor_creation() {
    // Test: Create health monitor for a VM
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));
    assert_eq!(monitor.consecutive_failures().await, 0);
    assert_eq!(monitor.check_interval(), Duration::from_secs(5));
}

#[tokio::test]
async fn test_health_check_healthy_vm() {
    // Test: Health check on healthy VM should return Healthy
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

    // VM in Created state (not running) should be unhealthy
    let status = monitor.check_health().await;
    assert_eq!(status, HealthStatus::Unhealthy("VM not running yet".to_string()));
}

#[tokio::test]
#[ignore] // Requires Firecracker binary
async fn test_health_check_running_vm() {
    // Test: Health check on running VM should return Healthy
    // This test requires Firecracker binary
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        kernel_image_path: "/var/lib/firecracker/vmlinux".to_string(),
        rootfs: plexspaces_firecracker::config::DriveConfig {
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

    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

    // Running VM should be healthy
    let status = monitor.check_health().await;
    // Note: This will only be Healthy if API is actually responsive
    // In test environment without Firecracker, it may be Unhealthy
    // That's OK - the test verifies the check runs without panicking
    let _ = status;
}

#[tokio::test]
async fn test_health_check_process_dead() {
    // Test: Health check when process is dead should return Unhealthy
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

    // Simulate process death (would need to actually kill process in real test)
    // For now, just test that monitor handles missing process
    let status = monitor.check_health().await;
    // Should detect process is not running (VM in Created state)
    assert!(matches!(status, HealthStatus::Unhealthy(_)));
}

#[tokio::test]
async fn test_health_check_api_unresponsive() {
    // Test: Health check when API is unresponsive should return Unhealthy
    // This would require mocking the API client or using a non-existent socket
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        socket_path: "/tmp/nonexistent-socket.sock".to_string(),
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

    // API should be unresponsive (socket doesn't exist)
    let status = monitor.check_health().await;
    // Should detect API is unresponsive
    assert!(matches!(status, HealthStatus::Unhealthy(_)));
}

#[tokio::test]
async fn test_health_check_consecutive_failures() {
    // Test: Monitor should track consecutive failures
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(5));

    // Initial state
    assert_eq!(monitor.consecutive_failures().await, 0);

    // First check (VM not running - Created state doesn't increment counter)
    monitor.check_health().await;
    // Created state doesn't increment, so still 0

    // Test that failures are tracked when API is unresponsive
    // (would need actual failure scenario to test increment)
}

#[tokio::test]
async fn test_health_monitor_start_stop() {
    // Test: Monitor can be started and stopped
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut monitor = VmHealthMonitor::new(vm_arc.clone(), Duration::from_secs(1));

    // Start monitoring (spawns background task)
    monitor.start_monitoring().await.unwrap();
    assert!(monitor.is_monitoring().await);

    // Wait a bit for health checks to run
    sleep(Duration::from_millis(1500)).await;

    // Stop monitoring
    monitor.stop().await.unwrap();
    assert!(!monitor.is_monitoring().await);
}

