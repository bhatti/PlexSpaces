// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for VM Object Registry Integration

use plexspaces_firecracker::{VmRegistry, VmRegistryEntry, VmState};
use plexspaces_keyvalue::InMemoryKVStore;
use plexspaces_object_registry::ObjectRegistry;
use plexspaces_proto::object_registry::v1::ObjectType;
use std::sync::Arc;

#[tokio::test]
async fn test_register_vm_in_object_registry() {
    // Test: Register VM in object registry
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry = ObjectRegistry::new(kv);

    // Create a VM entry (simulated - not actually running)
    let vm_entry = VmRegistryEntry {
        vm_id: "vm-test-001".to_string(),
        socket_path: "/tmp/firecracker-vm-test-001.sock".to_string(),
        state: VmState::Running,
        config: None,
    };

    // Register in object registry
    VmRegistry::register_in_object_registry(&object_registry, &vm_entry, None, None)
        .await
        .unwrap();

    // Lookup VM
    let found = object_registry
        .lookup("default", "default", ObjectType::ObjectTypeVm, "vm-test-001")
        .await
        .unwrap();

    assert!(found.is_some());
    let registration = found.unwrap();
    assert_eq!(registration.object_id, "vm-test-001");
    assert_eq!(registration.object_type, ObjectType::ObjectTypeVm as i32);
    assert_eq!(registration.object_category, "firecracker");
    assert_eq!(registration.grpc_address, "/tmp/firecracker-vm-test-001.sock");
}

#[tokio::test]
async fn test_discover_vms_from_object_registry() {
    // Test: Discover VMs using object registry
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry = ObjectRegistry::new(kv);

    // Register multiple VMs
    let vm1 = VmRegistryEntry {
        vm_id: "vm-001".to_string(),
        socket_path: "/tmp/firecracker-vm-001.sock".to_string(),
        state: VmState::Running,
        config: None,
    };

    let vm2 = VmRegistryEntry {
        vm_id: "vm-002".to_string(),
        socket_path: "/tmp/firecracker-vm-002.sock".to_string(),
        state: VmState::Running,
        config: None,
    };

    VmRegistry::register_in_object_registry(&object_registry, &vm1, None, None)
        .await
        .unwrap();
    VmRegistry::register_in_object_registry(&object_registry, &vm2, None, None)
        .await
        .unwrap();

    // Discover all VMs
    let vms = object_registry
        .discover(Some(ObjectType::ObjectTypeVm), None, None, None, None, 100)
        .await
        .unwrap();

    assert_eq!(vms.len(), 2);
    assert!(vms.iter().any(|v| v.object_id == "vm-001"));
    assert!(vms.iter().any(|v| v.object_id == "vm-002"));
}

#[tokio::test]
async fn test_vm_health_status_mapping() {
    // Test: VM state correctly maps to health status
    let kv = Arc::new(InMemoryKVStore::new());
    let object_registry = ObjectRegistry::new(kv);

    // Test Running -> Healthy
    let vm_running = VmRegistryEntry {
        vm_id: "vm-running".to_string(),
        socket_path: "/tmp/firecracker-vm-running.sock".to_string(),
        state: VmState::Running,
        config: None,
    };

    VmRegistry::register_in_object_registry(&object_registry, &vm_running, None, None)
        .await
        .unwrap();

    let found = object_registry
        .lookup("default", "default", ObjectType::ObjectTypeVm, "vm-running")
        .await
        .unwrap()
        .unwrap();

    use plexspaces_proto::object_registry::v1::HealthStatus;
    assert_eq!(found.health_status, HealthStatus::HealthStatusHealthy as i32);
}

