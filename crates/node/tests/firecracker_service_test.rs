// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Unit and integration tests for Firecracker VM Service

#[cfg(feature = "firecracker")]
mod firecracker_service_tests {
    use plexspaces_node::{firecracker_service::FirecrackerVmServiceImpl, Node, NodeId, default_node_config};
    use plexspaces_proto::firecracker::v1::{
        firecracker_vm_service_server::FirecrackerVmService,
        CreateVmRequest, BootVmRequest, GetVmStateRequest, ListVmsRequest,
        DeployApplicationRequest, UndeployApplicationRequest, StopVmRequest,
        VmConfig as ProtoVmConfig, VmState as ProtoVmState,
    };
    use std::sync::Arc;
    use tonic::Request;

    /// Helper to create a test node
    async fn create_test_node() -> Arc<Node> {
        Arc::new(Node::new(NodeId::new("test-node"), default_node_config()))
    }

    /// Helper to create Firecracker service
    fn create_service(node: Arc<Node>) -> FirecrackerVmServiceImpl {
        FirecrackerVmServiceImpl::new(node)
    }

    /// Test: Create VM with valid configuration
    #[tokio::test]
    async fn test_create_vm_success() {
        let node = create_test_node().await;
        let service = create_service(node);

        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-1".to_string(),
            vcpu_count: 2,
            mem_size_mib: 512,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let response = FirecrackerVmService::create_vm(&service, request).await;

        assert!(response.is_ok(), "Should create VM successfully");
        let response = response.unwrap().into_inner();
        assert!(response.success, "Response should indicate success");
        assert_eq!(response.vm_id, "test-vm-1");
        assert!(response.error.is_none(), "Should have no error");
    }

    /// Test: Create VM without configuration should fail
    #[tokio::test]
    async fn test_create_vm_missing_config() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(CreateVmRequest {
            config: None,
        });

        let response = FirecrackerVmService::create_vm(&service, request).await;

        assert!(response.is_err(), "Should fail for missing config");
        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    /// Test: Boot VM that doesn't exist should fail
    #[tokio::test]
    async fn test_boot_vm_not_found() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(BootVmRequest {
            vm_id: "nonexistent-vm".to_string(),
        });

        let response = FirecrackerVmService::boot_vm(&service, request).await;

        assert!(response.is_err(), "Should fail for nonexistent VM");
        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    /// Test: Boot VM after creation
    #[tokio::test]
    async fn test_boot_vm_after_creation() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create VM first
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-2".to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let create_request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // Now try to boot it
        // Note: This will fail in unit tests because FirecrackerVm::boot() requires
        // actual Firecracker binary, but we can test the service logic
        let boot_request = Request::new(BootVmRequest {
            vm_id: "test-vm-2".to_string(),
        });

        // This will fail because boot() requires actual Firecracker, but we verify
        // the service correctly retrieves the VM
        let boot_response = FirecrackerVmService::boot_vm(&service, boot_request).await;
        // In unit tests without Firecracker, this will fail with internal error
        // but not with NotFound, which means the VM was found
        if boot_response.is_err() {
            let status = boot_response.unwrap_err();
            // Should be internal error (Firecracker not available), not NotFound
            assert_ne!(status.code(), tonic::Code::NotFound, "VM should be found");
        }
    }

    /// Test: Get VM state for nonexistent VM
    #[tokio::test]
    async fn test_get_vm_state_not_found() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(GetVmStateRequest {
            vm_id: "nonexistent-vm".to_string(),
        });

        let response = FirecrackerVmService::get_vm_state(&service, request).await;

        assert!(response.is_err(), "Should fail for nonexistent VM");
        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    /// Test: List VMs when empty
    #[tokio::test]
    async fn test_list_vms_empty() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(ListVmsRequest {
            states: vec![],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let response = FirecrackerVmService::list_vms(&service, request).await;

        assert!(response.is_ok(), "Should succeed even with no VMs");
        let response = response.unwrap().into_inner();
        assert_eq!(response.vms.len(), 0, "Should have no VMs");
        assert_eq!(response.total_count, 0);
    }

    /// Test: List VMs after creating one
    #[tokio::test]
    async fn test_list_vms_with_vm() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create a VM
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-3".to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let create_request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // List VMs
        let list_request = Request::new(ListVmsRequest {
            states: vec![],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let list_response = FirecrackerVmService::list_vms(&service, list_request).await;
        assert!(list_response.is_ok(), "Should list VMs");
        let list_response = list_response.unwrap().into_inner();
        assert_eq!(list_response.vms.len(), 1, "Should have one VM");
        assert_eq!(list_response.total_count, 1);
        assert_eq!(list_response.vms[0].vm_id, "test-vm-3");
    }

    /// Test: List VMs with state filter
    #[tokio::test]
    async fn test_list_vms_with_state_filter() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create a VM
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-4".to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let create_request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // List VMs with state filter (Created state)
        let list_request = Request::new(ListVmsRequest {
            states: vec![ProtoVmState::VmStateCreated as i32],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let list_response = FirecrackerVmService::list_vms(&service, list_request).await;
        assert!(list_response.is_ok(), "Should list VMs");
        let list_response = list_response.unwrap().into_inner();
        // Should find the VM in Created state
        assert!(list_response.vms.len() >= 1, "Should find VM in Created state");
    }

    /// Test: Deploy application to nonexistent VM (should create VM)
    #[tokio::test]
    async fn test_deploy_application_creates_vm() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(DeployApplicationRequest {
            vm_id: "new-vm-1".to_string(),
            application_id: "app-1".to_string(),
            application_bundle: vec![],
            application_config_json: String::new(),
        });

        let response = FirecrackerVmService::deploy_application(&service, request).await;

        assert!(response.is_ok(), "Should deploy application (creates VM)");
        let response = response.unwrap().into_inner();
        assert!(response.success, "Should succeed");
        assert_eq!(response.vm_id, "new-vm-1");
        assert_eq!(response.application_id, "app-1");
    }

    /// Test: Deploy application to existing VM
    #[tokio::test]
    async fn test_deploy_application_to_existing_vm() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create VM first
        let vm_config = ProtoVmConfig {
            vm_id: "existing-vm-1".to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let create_request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // Deploy application to existing VM
        let deploy_request = Request::new(DeployApplicationRequest {
            vm_id: "existing-vm-1".to_string(),
            application_id: "app-2".to_string(),
            application_bundle: vec![],
            application_config_json: String::new(),
        });

        let deploy_response = FirecrackerVmService::deploy_application(&service, deploy_request).await;
        assert!(deploy_response.is_ok(), "Should deploy to existing VM");
        let deploy_response = deploy_response.unwrap().into_inner();
        assert!(deploy_response.success);
        assert_eq!(deploy_response.vm_id, "existing-vm-1");
    }

    /// Test: Undeploy application
    #[tokio::test]
    async fn test_undeploy_application() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(UndeployApplicationRequest {
            vm_id: "vm-1".to_string(),
            application_id: "app-1".to_string(),
            force: false,
        });

        // Undeploy should succeed even if VM doesn't exist (idempotent)
        let response = FirecrackerVmService::undeploy_application(&service, request).await;
        assert!(response.is_ok(), "Should succeed");
        let response = response.unwrap().into_inner();
        assert!(response.success);
    }

    /// Test: Stop VM that doesn't exist
    #[tokio::test]
    async fn test_stop_vm_not_found() {
        let node = create_test_node().await;
        let service = create_service(node);

        let request = Request::new(StopVmRequest {
            vm_id: "nonexistent-vm".to_string(),
            force: false,
        });

        let response = FirecrackerVmService::stop_vm(&service, request).await;

        assert!(response.is_err(), "Should fail for nonexistent VM");
        let status = response.unwrap_err();
        assert_eq!(status.code(), tonic::Code::NotFound);
    }

    /// Test: Stop VM after creation
    #[tokio::test]
    async fn test_stop_vm_after_creation() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create VM first
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-stop".to_string(),
            vcpu_count: 1,
            mem_size_mib: 256,
            boot_source: None,
            rootfs: None,
            drives: vec![],
            network_interfaces: vec![],
            metadata: None,
            limits: None,
            smt: false,
            track_dirty_pages: false,
        };

        let create_request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // Stop VM
        let stop_request = Request::new(StopVmRequest {
            vm_id: "test-vm-stop".to_string(),
            force: false,
        });

        // This will fail in unit tests because FirecrackerVm::stop() requires
        // actual Firecracker binary, but we can test the service logic
        let stop_response = FirecrackerVmService::stop_vm(&service, stop_request).await;
        // In unit tests without Firecracker, this will fail with internal error
        // but not with NotFound, which means the VM was found
        if stop_response.is_err() {
            let status = stop_response.unwrap_err();
            // Should be internal error (Firecracker not available), not NotFound
            assert_ne!(status.code(), tonic::Code::NotFound, "VM should be found");
        } else {
            // If it succeeds, verify response
            let response = stop_response.unwrap().into_inner();
            assert!(response.success);
        }
    }

    /// Test: Multiple VMs can be created and listed
    #[tokio::test]
    async fn test_multiple_vms() {
        let node = create_test_node().await;
        let service = create_service(node);

        // Create multiple VMs
        for i in 1..=3 {
            let vm_config = ProtoVmConfig {
                vm_id: format!("multi-vm-{}", i),
                vcpu_count: 1,
                mem_size_mib: 256,
                boot_source: None,
                rootfs: None,
                drives: vec![],
                network_interfaces: vec![],
                metadata: None,
                limits: None,
                smt: false,
                track_dirty_pages: false,
            };

            let create_request = Request::new(CreateVmRequest {
                config: Some(vm_config),
            });

            let create_response = FirecrackerVmService::create_vm(&service, create_request).await;
            assert!(create_response.is_ok(), "Should create VM {}", i);
        }

        // List all VMs
        let list_request = Request::new(ListVmsRequest {
            states: vec![],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let list_response = FirecrackerVmService::list_vms(&service, list_request).await;
        assert!(list_response.is_ok(), "Should list VMs");
        let list_response = list_response.unwrap().into_inner();
        assert_eq!(list_response.vms.len(), 3, "Should have 3 VMs");
        assert_eq!(list_response.total_count, 3);
    }
}
