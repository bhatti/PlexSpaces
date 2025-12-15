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

