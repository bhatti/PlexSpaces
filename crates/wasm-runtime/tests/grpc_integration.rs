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

//! Integration tests for WASM gRPC service
//!
//! ## Purpose
//! Tests the complete gRPC deployment workflow including:
//! - gRPC server startup and shutdown
//! - Remote module deployment
//! - Remote actor instantiation
//! - Error handling and validation
//! - Concurrent request handling

use plexspaces_proto::wasm::v1::{
    wasm_runtime_service_client::WasmRuntimeServiceClient,
    wasm_runtime_service_server::WasmRuntimeServiceServer, DeployWasmModuleRequest,
    InstantiateActorRequest, WasmConfig as ProtoWasmConfig, WasmModule as ProtoWasmModule,
};
use plexspaces_wasm_runtime::{WasmRuntime, WasmRuntimeServiceImpl};
use std::sync::Arc;
use tonic::transport::Server;

/// Minimal valid WASM module (exports "test" function returning i32)
const SIMPLE_WASM: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
    0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, 0x0a,
    0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
];

/// Start gRPC server on random port, return port and server handle
async fn start_grpc_server() -> (u16, tokio::task::JoinHandle<()>) {
    // Create runtime and service
    let runtime = WasmRuntime::new().await.unwrap();
    let service = WasmRuntimeServiceImpl::new(Arc::new(runtime));

    // Bind to localhost on random port
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    let port = addr.port();

    // Start server in background task
    let server = Server::builder()
        .add_service(WasmRuntimeServiceServer::new(service))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));

    let handle = tokio::spawn(async move {
        server.await.unwrap();
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (port, handle)
}

#[tokio::test]
async fn test_grpc_deploy_module_integration() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Create deploy request
    let module = ProtoWasmModule {
        name: "test-module".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
        version_number: 1,
    };

    let request = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });

    // Deploy module via gRPC
    let response = client.deploy_module(request).await.unwrap().into_inner();

    // Verify response
    assert!(response.success, "Deployment should succeed");
    assert_eq!(response.module_hash.len(), 64, "SHA-256 hash is 64 chars");
    assert_eq!(response.nodes_pre_warmed, 1);
    assert!(response.error.is_none(), "No error should be returned");
}

#[tokio::test]
async fn test_grpc_deploy_invalid_module() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Create deploy request with invalid WASM
    let module = ProtoWasmModule {
        name: "invalid-module".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: vec![0x00, 0x01, 0x02, 0x03], // Invalid WASM
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: 4,
        version_number: 1,
    };

    let request = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });

    // Deploy invalid module
    let response = client.deploy_module(request).await.unwrap().into_inner();

    // Verify error response
    assert!(!response.success, "Deployment should fail");
    assert_eq!(response.module_hash, "");
    assert!(response.error.is_some(), "Error should be returned");

    let error = response.error.unwrap();
    assert!(
        error.message.contains("compilation") || error.message.contains("Failed"),
        "Error message should indicate compilation failure: {}",
        error.message
    );
}

#[tokio::test]
async fn test_grpc_deploy_missing_module() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Create request with missing module field
    let request = tonic::Request::new(DeployWasmModuleRequest {
        module: None,
        pre_warm: 0,
        target_node_tags: vec![],
    });

    // Attempt deployment
    let result = client.deploy_module(request).await;

    // Verify error
    assert!(result.is_err(), "Should return error for missing module");
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("module field is required"));
}

#[tokio::test]
async fn test_grpc_deploy_empty_name() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Create module with empty name
    let module = ProtoWasmModule {
        name: String::new(), // Empty name
        version: "1.0.0".to_string(),
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
        version_number: 1,
    };

    let request = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });

    // Attempt deployment
    let result = client.deploy_module(request).await;

    // Verify error
    assert!(result.is_err(), "Should return error for empty name");
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("name cannot be empty"));
}

#[tokio::test]
async fn test_grpc_instantiate_actor_integration() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // First, deploy a module
    let module = ProtoWasmModule {
        name: "actor-module".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
        version_number: 1,
    };

    let deploy_req = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });

    let deploy_resp = client.deploy_module(deploy_req).await.unwrap().into_inner();
    assert!(deploy_resp.success);
    let module_hash = deploy_resp.module_hash;

    // Now instantiate actor using the module hash
    let instantiate_req = tonic::Request::new(InstantiateActorRequest {
        module_ref: module_hash,
        actor_id: "actor-001".to_string(),
        initial_state: vec![],
        config: Some(ProtoWasmConfig::default()),
        target_node_id: String::new(),
    });

    let instantiate_resp = client
        .instantiate_actor(instantiate_req)
        .await
        .unwrap()
        .into_inner();

    // Verify response
    assert!(
        instantiate_resp.success,
        "Instantiation should succeed: {:?}",
        instantiate_resp.error
    );
    assert_eq!(instantiate_resp.actor_id, "actor-001");
    assert_eq!(instantiate_resp.node_id, "local");
    assert!(instantiate_resp.created_at.is_some());
    assert!(instantiate_resp.error.is_none());
}

#[tokio::test]
async fn test_grpc_instantiate_actor_missing_module() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Try to instantiate with non-existent module
    let request = tonic::Request::new(InstantiateActorRequest {
        module_ref: "nonexistent-hash".to_string(),
        actor_id: "actor-002".to_string(),
        initial_state: vec![],
        config: None,
        target_node_id: String::new(),
    });

    let response = client.instantiate_actor(request).await.unwrap().into_inner();

    // Verify error response
    assert!(!response.success, "Should fail for missing module");
    assert!(response.error.is_some());

    let error = response.error.unwrap();
    assert!(
        error.message.contains("not found") || error.message.contains("Module"),
        "Error should indicate module not found: {}",
        error.message
    );
}

#[tokio::test]
async fn test_grpc_instantiate_empty_module_ref() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Try to instantiate with empty module_ref
    let request = tonic::Request::new(InstantiateActorRequest {
        module_ref: String::new(),
        actor_id: "actor-003".to_string(),
        initial_state: vec![],
        config: None,
        target_node_id: String::new(),
    });

    let result = client.instantiate_actor(request).await;

    // Verify error
    assert!(result.is_err(), "Should return error for empty module_ref");
    let err = result.unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
    assert!(err.message().contains("module_ref cannot be empty"));
}

#[tokio::test]
async fn test_grpc_concurrent_deployments() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create multiple clients for concurrent requests
    let addr = format!("http://127.0.0.1:{}", port);

    // Spawn 5 concurrent deployment requests
    let mut handles = vec![];
    for i in 0..5 {
        let addr = addr.clone();
        let handle = tokio::spawn(async move {
            let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

            let module = ProtoWasmModule {
                name: format!("concurrent-module-{}", i),
                version: "1.0.0".to_string(),
                module_bytes: SIMPLE_WASM.to_vec(),
                module_hash: String::new(),
                wit_interface: String::new(),
                source_languages: vec![],
                metadata: None,
                created_at: None,
                size_bytes: SIMPLE_WASM.len() as u64,
                version_number: 1,
            };

            let request = tonic::Request::new(DeployWasmModuleRequest {
                module: Some(module),
                pre_warm: 0,
                target_node_tags: vec![],
            });

            client.deploy_module(request).await.unwrap().into_inner()
        });

        handles.push(handle);
    }

    // Wait for all deployments to complete
    let results = futures::future::join_all(handles).await;

    // Verify all succeeded
    for (i, result) in results.into_iter().enumerate() {
        let response = result.unwrap();
        assert!(
            response.success,
            "Deployment {} should succeed: {:?}",
            i,
            response.error
        );
        assert_eq!(response.module_hash.len(), 64);
    }
}

#[tokio::test]
async fn test_grpc_idempotent_deployment() {
    // Start gRPC server
    let (port, _server) = start_grpc_server().await;

    // Create gRPC client
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await.unwrap();

    // Deploy same module twice
    let module = ProtoWasmModule {
        name: "idempotent-module".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: SIMPLE_WASM.to_vec(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec![],
        metadata: None,
        created_at: None,
        size_bytes: SIMPLE_WASM.len() as u64,
        version_number: 1,
    };

    // First deployment
    let req1 = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module.clone()),
        pre_warm: 0,
        target_node_tags: vec![],
    });
    let resp1 = client.deploy_module(req1).await.unwrap().into_inner();
    assert!(resp1.success);
    let hash1 = resp1.module_hash.clone();

    // Second deployment (should be idempotent)
    let req2 = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });
    let resp2 = client.deploy_module(req2).await.unwrap().into_inner();
    assert!(resp2.success);
    let hash2 = resp2.module_hash;

    // Hashes should be identical (content-addressable)
    assert_eq!(hash1, hash2, "Idempotent deployment should return same hash");
}
