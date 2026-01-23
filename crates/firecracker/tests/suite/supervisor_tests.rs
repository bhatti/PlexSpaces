// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for VM Supervisor

use plexspaces_firecracker::supervisor::{BackoffConfig, RestartPolicy, SupervisionStrategy, VmSupervisor};
use plexspaces_firecracker::{FirecrackerVm, VmConfig};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_supervisor_creation() {
    // Test: Create supervisor
    let supervisor = VmSupervisor::new();
    assert!(supervisor.is_empty().await);
}

#[tokio::test]
async fn test_supervisor_add_vm() {
    // Test: Add VM to supervisor
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::new();

    let policy = RestartPolicy::Always;
    supervisor.supervise(vm_arc, policy).await.unwrap();

    assert_eq!(supervisor.supervised_vm_count().await, 1);
}

#[tokio::test]
async fn test_supervisor_restart_policy_always() {
    // Test: Supervisor with Always policy restarts VM on failure
    // This would require simulating a failure and verifying restart
}

#[tokio::test]
async fn test_supervisor_restart_policy_with_backoff() {
    // Test: Supervisor with backoff policy waits before restarting
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::new();

    let policy = RestartPolicy::WithBackoff {
        initial_delay: Duration::from_secs(1),
        max_delay: Duration::from_secs(60),
        factor: 2.0,
    };

    supervisor.supervise(vm_arc, policy).await.unwrap();
    assert_eq!(supervisor.supervised_vm_count().await, 1);
}

#[tokio::test]
async fn test_supervisor_restart_policy_max_retries() {
    // Test: Supervisor with max retries stops after N failures
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::new();

    let policy = RestartPolicy::MaxRetries {
        max_retries: 3,
        backoff: BackoffConfig::default(),
    };

    supervisor.supervise(vm_arc, policy).await.unwrap();
    assert_eq!(supervisor.supervised_vm_count().await, 1);
}

#[tokio::test]
async fn test_supervisor_restart_policy_never() {
    // Test: Supervisor with Never policy doesn't restart
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::new();

    let policy = RestartPolicy::Never;
    supervisor.supervise(vm_arc, policy).await.unwrap();
    assert_eq!(supervisor.supervised_vm_count().await, 1);
}

#[tokio::test]
async fn test_supervisor_handle_failure() {
    // Test: Supervisor handles VM failure and applies restart policy
}

#[tokio::test]
async fn test_supervisor_stop_supervision() {
    // Test: Stop supervising a VM
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_id = vm.vm_id().to_string();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::new();
    supervisor
        .supervise(vm_arc, RestartPolicy::Always)
        .await
        .unwrap();
    assert_eq!(supervisor.supervised_vm_count().await, 1);

    supervisor.stop_supervision(&vm_id).await.unwrap();
    assert_eq!(supervisor.supervised_vm_count().await, 0);
}

#[tokio::test]
async fn test_supervisor_multiple_vms() {
    // Test: Supervisor can manage multiple VMs with different policies
    let mut supervisor = VmSupervisor::new();

    // Add first VM
    let config1 = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };
    let vm1 = FirecrackerVm::create(config1).await.unwrap();
    let vm1_arc = Arc::new(RwLock::new(vm1));
    supervisor
        .supervise(vm1_arc, RestartPolicy::Always)
        .await
        .unwrap();

    // Add second VM with different policy
    let config2 = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };
    let vm2 = FirecrackerVm::create(config2).await.unwrap();
    let vm2_arc = Arc::new(RwLock::new(vm2));
    supervisor
        .supervise(vm2_arc, RestartPolicy::Never)
        .await
        .unwrap();

    assert_eq!(supervisor.supervised_vm_count().await, 2);
}

#[tokio::test]
async fn test_supervision_strategy_one_for_one() {
    // Test: OneForOne strategy only restarts failed VM
    let config1 = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };
    let config2 = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm1 = FirecrackerVm::create(config1).await.unwrap();
    let vm2 = FirecrackerVm::create(config2).await.unwrap();
    let vm1_arc = Arc::new(RwLock::new(vm1));
    let vm2_arc = Arc::new(RwLock::new(vm2));

    let mut supervisor = VmSupervisor::with_strategy(SupervisionStrategy::OneForOne);
    supervisor
        .supervise(vm1_arc.clone(), RestartPolicy::Always)
        .await
        .unwrap();
    supervisor
        .supervise(vm2_arc.clone(), RestartPolicy::Always)
        .await
        .unwrap();

    assert_eq!(supervisor.supervised_vm_count().await, 2);
    // OneForOne is the default, so this should work
}

#[tokio::test]
async fn test_supervision_strategy_one_for_all() {
    // Test: OneForAll strategy can be set
    let config = VmConfig {
        vm_id: ulid::Ulid::new().to_string(),
        vcpu_count: 1,
        mem_size_mib: 256,
        ..Default::default()
    };

    let vm = FirecrackerVm::create(config).await.unwrap();
    let vm_arc = Arc::new(RwLock::new(vm));

    let mut supervisor = VmSupervisor::with_strategy(SupervisionStrategy::OneForAll);
    supervisor
        .supervise(vm_arc, RestartPolicy::Always)
        .await
        .unwrap();

    assert_eq!(supervisor.supervised_vm_count().await, 1);
}

