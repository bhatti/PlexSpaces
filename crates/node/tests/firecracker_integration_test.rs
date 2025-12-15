// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for Firecracker VM Service via gRPC
// These tests require Docker to run the node with Firecracker support

#[cfg(feature = "firecracker")]
mod firecracker_integration_tests {
    use plexspaces_proto::firecracker::v1::{
        firecracker_vm_service_client::FirecrackerVmServiceClient,
        CreateVmRequest, BootVmRequest, GetVmStateRequest, ListVmsRequest,
        DeployApplicationRequest, StopVmRequest,
        VmConfig as ProtoVmConfig, VmState as ProtoVmState,
    };
    use std::process::Command;
    use std::time::Duration;
    use tokio::time::sleep;
    use tonic::transport::Channel;
    use tonic::Request;

    const DOCKER_IMAGE: &str = "plexspaces-firecracker-test:latest";
    const CONTAINER_NAME: &str = "plexspaces-firecracker-test";
    const GRPC_PORT: u16 = 9001;
    const GRPC_ADDR: &str = "http://localhost:9001";

    /// Helper to check if Docker is available
    fn docker_available() -> bool {
        Command::new("docker")
            .arg("--version")
            .output()
            .is_ok()
    }

    /// Helper to detect docker-compose command
    fn docker_compose_cmd() -> String {
        // Try docker-compose first (older versions)
        if Command::new("docker-compose")
            .arg("--version")
            .output()
            .is_ok()
        {
            return "docker-compose".to_string();
        }
        // Try docker compose (newer versions)
        if Command::new("docker")
            .args(&["compose", "version"])
            .output()
            .is_ok()
        {
            return "docker compose".to_string();
        }
        "docker-compose".to_string() // fallback
    }

    /// Helper to get project root directory (where docker-compose file is located)
    fn get_project_root() -> std::path::PathBuf {
        // Tests run from workspace root, so we can use env!("CARGO_MANIFEST_DIR")
        // and go up to workspace root
        let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // crates/node -> workspace root
        manifest_dir.parent().unwrap().parent().unwrap().to_path_buf()
    }

    /// Helper to build Docker image (using docker-compose)
    fn build_docker_image() -> Result<(), Box<dyn std::error::Error>> {
        if !docker_available() {
            return Err("Docker is not available".into());
        }

        let project_root = get_project_root();
        let compose_cmd = docker_compose_cmd();
        println!("Building Docker image via {}: {}", compose_cmd, DOCKER_IMAGE);
        
        let mut cmd = if compose_cmd == "docker compose" {
            let mut c = Command::new("docker");
            c.args(&["compose", "-f", "docker-compose.firecracker-test.yml", "build"]);
            c.current_dir(&project_root);
            c
        } else {
            let mut c = Command::new("docker-compose");
            c.args(&["-f", "docker-compose.firecracker-test.yml", "build"]);
            c.current_dir(&project_root);
            c
        };
        
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Docker build failed: {}", stderr).into());
        }

        println!("Docker image built successfully");
        Ok(())
    }

    /// Helper to start Docker container (using docker-compose)
    fn start_docker_container() -> Result<(), Box<dyn std::error::Error>> {
        let project_root = get_project_root();
        let compose_cmd = docker_compose_cmd();
        
        // Stop and remove existing containers
        if compose_cmd == "docker compose" {
            let _ = Command::new("docker")
                .args(&["compose", "-f", "docker-compose.firecracker-test.yml", "down"])
                .current_dir(&project_root)
                .output();
        } else {
            let _ = Command::new("docker-compose")
                .args(&["-f", "docker-compose.firecracker-test.yml", "down"])
                .current_dir(&project_root)
                .output();
        }

        println!("Starting Docker container via {}: {}", compose_cmd, CONTAINER_NAME);
        
        let mut cmd = if compose_cmd == "docker compose" {
            let mut c = Command::new("docker");
            c.args(&["compose", "-f", "docker-compose.firecracker-test.yml", "up", "-d"]);
            c.current_dir(&project_root);
            c
        } else {
            let mut c = Command::new("docker-compose");
            c.args(&["-f", "docker-compose.firecracker-test.yml", "up", "-d"]);
            c.current_dir(&project_root);
            c
        };
        
        let output = cmd.output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("Failed to start container: {}", stderr).into());
        }

        println!("Container started, waiting for node to be ready...");
        Ok(())
    }

    /// Helper to stop Docker container
    fn stop_docker_container() {
        let compose_cmd = docker_compose_cmd();
        
        if compose_cmd == "docker compose" {
            let _ = Command::new("docker")
                .args(&["compose", "-f", "docker-compose.firecracker-test.yml", "down"])
                .output();
        } else {
            let _ = Command::new("docker-compose")
                .args(&["-f", "docker-compose.firecracker-test.yml", "down"])
                .output();
        }
        println!("Container stopped and removed");
    }

    /// Helper to wait for gRPC service to be ready
    async fn wait_for_service(max_wait: Duration) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        let mut last_error: Option<String> = None;
        let mut attempt = 0;

        while start.elapsed() < max_wait {
            attempt += 1;
            if attempt % 10 == 0 {
                println!("Waiting for gRPC service... (attempt {})", attempt);
            }
            
            match Channel::from_shared(GRPC_ADDR.to_string()) {
                Ok(channel) => {
                    match channel.connect().await {
                        Ok(connected) => {
                            // Try a simple health check by attempting to create a client
                            println!("gRPC service is ready (connected after {} attempts)", attempt);
                            return Ok(connected);
                        }
                        Err(e) => {
                            last_error = Some(format!("Connection error: {:?}", e));
                            sleep(Duration::from_millis(1000)).await; // Wait 1 second between attempts
                        }
                    }
                }
                Err(e) => {
                    last_error = Some(format!("Channel creation error: {:?}", e));
                    sleep(Duration::from_millis(1000)).await;
                }
            }
        }

        Err(format!(
            "Service not ready after {:?} ({} attempts): {:?}",
            max_wait,
            attempt,
            last_error
        )
        .into())
    }

    /// Helper to create gRPC client
    async fn create_client() -> Result<FirecrackerVmServiceClient<Channel>, Box<dyn std::error::Error + Send + Sync>> {
        // Wait up to 60 seconds for service to be ready (Docker container startup time)
        let channel = wait_for_service(Duration::from_secs(60)).await?;
        Ok(FirecrackerVmServiceClient::new(channel))
    }

    /// Test: Create VM via gRPC
    ///
    /// This test:
    /// 1. Builds Docker image with Firecracker support
    /// 2. Starts container with KVM access
    /// 3. Waits for gRPC service to be ready
    /// 4. Calls CreateVm API via gRPC
    /// 5. Verifies VM was created successfully
    #[tokio::test]
    #[ignore] // Requires Docker, KVM, and Firecracker setup
    async fn test_create_vm_via_grpc() {
        // Skip test if Docker is not available
        if !docker_available() {
            println!("Skipping test: Docker is not available");
            return;
        }

        // Build and start Docker container
        build_docker_image().expect("Failed to build Docker image");
        start_docker_container().expect("Failed to start container");

        // Ensure cleanup on test end
        let _guard = TestGuard;

        // Wait for service and create client
        let mut client = create_client().await.expect("Failed to create gRPC client");

        // Create VM request
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-grpc-1".to_string(),
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

        let request = Request::new(CreateVmRequest {
            config: Some(vm_config),
        });

        // Call gRPC API
        let response = client.create_vm(request).await;

        assert!(response.is_ok(), "Should create VM via gRPC");
        let response = response.unwrap().into_inner();
        assert!(response.success, "Response should indicate success");
        assert_eq!(response.vm_id, "test-vm-grpc-1");
    }

    /// Test: List VMs via gRPC
    ///
    /// This test:
    /// 1. Creates a VM via gRPC
    /// 2. Lists all VMs via gRPC
    /// 3. Verifies the created VM appears in the list
    #[tokio::test]
    #[ignore] // Requires Docker, KVM, and Firecracker setup
    async fn test_list_vms_via_grpc() {
        build_docker_image().expect("Failed to build Docker image");
        start_docker_container().expect("Failed to start container");
        let _guard = TestGuard;

        let mut client = create_client().await.expect("Failed to create gRPC client");

        // Create a VM first
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-list-1".to_string(),
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

        let create_response = client.create_vm(create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // List VMs
        let list_request = Request::new(ListVmsRequest {
            states: vec![],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let list_response = client.list_vms(list_request).await;
        assert!(list_response.is_ok(), "Should list VMs");
        let list_response = list_response.unwrap().into_inner();
        assert!(list_response.vms.len() >= 1, "Should have at least one VM");
    }

    /// Test: Get VM state via gRPC
    ///
    /// This test:
    /// 1. Creates a VM via gRPC
    /// 2. Retrieves VM state via gRPC
    /// 3. Verifies state is correct
    #[tokio::test]
    #[ignore] // Requires Docker, KVM, and Firecracker setup
    async fn test_get_vm_state_via_grpc() {
        if !docker_available() {
            println!("Skipping test: Docker is not available");
            return;
        }

        build_docker_image().expect("Failed to build Docker image");
        start_docker_container().expect("Failed to start container");
        let _guard = TestGuard;

        let mut client = create_client().await.expect("Failed to create gRPC client");

        // Create a VM first
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-state-1".to_string(),
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

        let create_response = client.create_vm(create_request).await;
        assert!(create_response.is_ok(), "Should create VM");

        // Get VM state
        let state_request = Request::new(GetVmStateRequest {
            vm_id: "test-vm-state-1".to_string(),
        });

        let state_response = client.get_vm_state(state_request).await;
        assert!(state_response.is_ok(), "Should get VM state");
        let state_response = state_response.unwrap().into_inner();
        assert!(state_response.success);
        assert!(state_response.vm.is_some());
        assert_eq!(state_response.vm.as_ref().unwrap().vm_id, "test-vm-state-1");
    }

    /// Test: Deploy application via gRPC
    ///
    /// This test:
    /// 1. Deploys an application to a VM via gRPC
    /// 2. Verifies deployment succeeds (VM is created if needed)
    #[tokio::test]
    #[ignore] // Requires Docker, KVM, and Firecracker setup
    async fn test_deploy_application_via_grpc() {
        if !docker_available() {
            println!("Skipping test: Docker is not available");
            return;
        }

        build_docker_image().expect("Failed to build Docker image");
        start_docker_container().expect("Failed to start container");
        let _guard = TestGuard;

        let mut client = create_client().await.expect("Failed to create gRPC client");

        // Deploy application (will create VM if needed)
        let deploy_request = Request::new(DeployApplicationRequest {
            vm_id: "test-vm-deploy-1".to_string(),
            application_id: "test-app-1".to_string(),
            application_bundle: vec![],
            application_config_json: String::new(),
        });

        let deploy_response = client.deploy_application(deploy_request).await;
        assert!(deploy_response.is_ok(), "Should deploy application");
        let deploy_response = deploy_response.unwrap().into_inner();
        assert!(deploy_response.success);
        assert_eq!(deploy_response.vm_id, "test-vm-deploy-1");
        assert_eq!(deploy_response.application_id, "test-app-1");
    }

    /// Test: Full VM lifecycle via gRPC
    ///
    /// This test exercises the complete VM lifecycle:
    /// 1. Create VM
    /// 2. Get VM state
    /// 3. List VMs
    /// 4. Stop VM
    #[tokio::test]
    #[ignore] // Requires Docker, KVM, and Firecracker setup
    async fn test_vm_lifecycle_via_grpc() {
        if !docker_available() {
            println!("Skipping test: Docker is not available");
            return;
        }

        build_docker_image().expect("Failed to build Docker image");
        start_docker_container().expect("Failed to start container");
        let _guard = TestGuard;

        let mut client = create_client().await.expect("Failed to create gRPC client");

        // 1. Create VM
        let vm_config = ProtoVmConfig {
            vm_id: "test-vm-lifecycle-1".to_string(),
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

        let create_response = client.create_vm(create_request).await;
        assert!(create_response.is_ok(), "Should create VM");
        let create_response = create_response.unwrap().into_inner();
        assert!(create_response.success);

        // 2. Get VM state
        let state_request = Request::new(GetVmStateRequest {
            vm_id: "test-vm-lifecycle-1".to_string(),
        });

        let state_response = client.get_vm_state(state_request).await;
        assert!(state_response.is_ok(), "Should get VM state");

        // 3. List VMs (should include our VM)
        let list_request = Request::new(ListVmsRequest {
            states: vec![],
            node_id: String::new(),
            page_size: 10,
            page_token: String::new(),
        });

        let list_response = client.list_vms(list_request).await;
        assert!(list_response.is_ok(), "Should list VMs");
        let list_response = list_response.unwrap().into_inner();
        assert!(list_response.vms.iter().any(|vm| vm.vm_id == "test-vm-lifecycle-1"));

        // 4. Stop VM
        let stop_request = Request::new(StopVmRequest {
            vm_id: "test-vm-lifecycle-1".to_string(),
            force: false,
        });

        let stop_response = client.stop_vm(stop_request).await;
        // May fail if Firecracker is not fully available, but should not be NotFound
        if stop_response.is_err() {
            let status = stop_response.unwrap_err();
            assert_ne!(status.code(), tonic::Code::NotFound, "VM should be found");
        } else {
            let stop_response = stop_response.unwrap().into_inner();
            assert!(stop_response.success);
        }
    }

    /// Guard to ensure container cleanup on test failure
    struct TestGuard;

    impl Drop for TestGuard {
        fn drop(&mut self) {
            stop_docker_container();
        }
    }
}
