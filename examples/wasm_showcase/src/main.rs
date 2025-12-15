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

//! # PlexSpaces WASM Runtime - Comprehensive Showcase
//!
//! ## Purpose
//! Demonstrates all implemented WASM runtime capabilities:
//! - Module deployment with content-addressable caching
//! - Actor instantiation with resource limits
//! - Capability-based security (WASI restrictions)
//! - gRPC deployment service
//! - Concurrent operations
//! - Error handling
//!
//! ## Usage
//! ```bash
//! # Run all demonstrations
//! cargo run --example wasm-showcase
//!
//! # Run specific demonstration
//! cargo run --example wasm-showcase -- --demo deployment
//! cargo run --example wasm-showcase -- --demo grpc
//! cargo run --example wasm-showcase -- --demo security
//! cargo run --example wasm-showcase -- --demo concurrency
//! ```

use anyhow::Result;
use clap::{Parser, ValueEnum};
use plexspaces_node::{Node, NodeBuilder};
use plexspaces_proto::application::v1::{
    application_service_client::ApplicationServiceClient, DeployApplicationRequest,
    ApplicationSpec, ApplicationType,
};
use plexspaces_proto::wasm::v1::{
    wasm_runtime_service_client::WasmRuntimeServiceClient,
    wasm_runtime_service_server::WasmRuntimeServiceServer, DeployWasmModuleRequest,
    InstantiateActorRequest, WasmModule as ProtoWasmModule,
};
use plexspaces_proto::prost_types;
use plexspaces_wasm_runtime::{
    ResourceLimits, WasmCapabilities, WasmConfig, WasmDeploymentService, WasmRuntime,
    WasmRuntimeServiceImpl,
};
use plexspaces_core::ActorRef;
use plexspaces_mailbox::Message;
use std::sync::Arc;
use std::path::PathBuf;
use tonic::transport::{Channel, Server};
use tracing::{error, info, warn};

/// Create a functional WASM module for demonstration
/// This module implements a simple counter actor that processes messages and returns results
fn create_demo_wasm() -> Vec<u8> {
    let wat = r#"
(module
    (import "plexspaces" "log" (func $log (param i32 i32)))
    (memory (export "memory") 1)
    (global $counter (mut i32) (i32.const 0))
    
    (func (export "init") (param i32 i32) (result i32)
        (i32.const 0)
    )
    
    (func (export "handle_message") 
          (param $from_ptr i32) (param $from_len i32)
          (param $msg_type_ptr i32) (param $msg_type_len i32)
          (param $payload_ptr i32) (param $payload_len i32)
          (result i32)
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 105))
            (then
                (global.set $counter (i32.add (global.get $counter) (i32.const 1)))
                (call $log (i32.const 100) (i32.const 18))
                (i32.store (i32.const 200) (global.get $counter))
                (return (i32.const 200))
            )
        )
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 103))
            (then
                (call $log (i32.const 120) (i32.const 15))
                (i32.store (i32.const 200) (global.get $counter))
                (return (i32.const 200))
            )
        )
        (if (i32.eq (i32.load8_u (local.get $msg_type_ptr)) (i32.const 114))
            (then
                (global.set $counter (i32.const 0))
                (call $log (i32.const 140) (i32.const 13))
                (i32.store (i32.const 200) (i32.const 0))
                (return (i32.const 200))
            )
        )
        (i32.const 0)
    )
    
    (func (export "snapshot_state") (result i32 i32)
        (i32.store (i32.const 300) (global.get $counter))
        (i32.const 300)
        (i32.const 4)
    )
    
    (data (i32.const 100) "Counter incremented")
    (data (i32.const 120) "Getting count")
    (data (i32.const 140) "Counter reset")
)
"#;
    wat::parse_str(wat).expect("Failed to parse WAT")
}

#[derive(Debug, Clone, ValueEnum)]
enum DemoMode {
    /// Show all demonstrations
    All,
    /// Module deployment and caching
    Deployment,
    /// Resource limits and security
    Security,
    /// gRPC deployment service
    Grpc,
    /// Concurrent operations
    Concurrency,
    /// Python actor support
    Python,
    /// Different behaviors (GenServer, GenEvent, GenFSM)
    Behaviors,
    /// Service access (ChannelService, TupleSpace, ActorService)
    Services,
}

#[derive(Parser, Debug)]
#[command(name = "wasm-showcase")]
#[command(about = "PlexSpaces WASM Runtime Comprehensive Showcase", long_about = None)]
struct Args {
    /// Which demonstration to run
    #[arg(short, long, value_enum, default_value = "all")]
    demo: DemoMode,

    /// Verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let args = Args::parse();

    // Setup tracing
    let log_level = if args.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(format!("wasm_showcase={},plexspaces_wasm_runtime={}", log_level, log_level))
        .init();

    info!("🚀 PlexSpaces WASM Runtime Showcase");
    info!("====================================\n");

    // Run selected demonstrations
    match args.demo {
        DemoMode::All => {
            run_deployment_demo().await?;
            run_security_demo().await?;
            run_grpc_demo().await?;
            run_concurrency_demo().await?;
            run_python_demo().await?;
            run_behaviors_demo().await?;
            run_services_demo().await?;
        }
        DemoMode::Deployment => run_deployment_demo().await?,
        DemoMode::Security => run_security_demo().await?,
        DemoMode::Grpc => run_grpc_demo().await?,
        DemoMode::Concurrency => run_concurrency_demo().await?,
        DemoMode::Python => run_python_demo().await?,
        DemoMode::Behaviors => run_behaviors_demo().await?,
        DemoMode::Services => run_services_demo().await?,
    }

    info!("\n✅ All demonstrations completed successfully!");
    Ok(())
}

/// Demo 1: Application Deployment via ApplicationService
async fn run_deployment_demo() -> Result<()> {
    info!("📦 Demo 1: Application Deployment & Content-Addressable Caching");
    info!("-----------------------------------------------------------");

    // Start a node
    info!("\n1. Starting PlexSpaces node...");
    let node = NodeBuilder::new("wasm-showcase-node".to_string())
        .build();
    let node_arc = Arc::new(node);
    let node_clone = node_arc.clone();
    
    // Start node in background
    let node_handle = tokio::spawn(async move {
        node_clone.start().await.unwrap();
    });
    
    // Wait for node to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    info!("✓ Node started");

    // Connect to ApplicationService
    info!("\n2. Connecting to ApplicationService...");
    let channel = Channel::from_shared("http://127.0.0.1:9000".to_string())?
        .connect()
        .await?;
    let mut client = ApplicationServiceClient::new(channel);
    info!("✓ Connected to ApplicationService");

    // Deploy first application
    info!("\n3. Deploying 'counter-app' application (first time)...");
    let demo_wasm = create_demo_wasm();
    let wasm_module1 = ProtoWasmModule {
        name: "counter".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: demo_wasm.clone(),
        module_hash: String::new(), // Will be computed by server
        wit_interface: String::new(),
        ..Default::default()
    };

    let request1 = DeployApplicationRequest {
        application_id: "counter-app-1".to_string(),
        name: "counter-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module1),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let response1 = client.deploy_application(tonic::Request::new(request1)).await?;
    let deploy1 = response1.into_inner();
    if deploy1.success {
        info!("✓ Application deployed successfully");
        info!("  Application ID: {}", deploy1.application_id);
    } else {
        return Err(anyhow::anyhow!("Deployment failed: {:?}", deploy1.error));
    }

    // Deploy same application again with different ID (should use cached module)
    info!("\n4. Deploying 'counter-app' again with different ID (should use cached module)...");
    let wasm_module2 = ProtoWasmModule {
        name: "counter".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: demo_wasm.clone(),
        module_hash: String::new(),
        wit_interface: String::new(),
        ..Default::default()
    };

    let request2 = DeployApplicationRequest {
        application_id: "counter-app-2".to_string(),
        name: "counter-app-2".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module2),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let response2 = client.deploy_application(tonic::Request::new(request2)).await?;
    let deploy2 = response2.into_inner();
    if deploy2.success {
        info!("✓ Application deployed (module cached on server)");
        info!("  Application ID: {}", deploy2.application_id);
        info!("  Note: Same WASM bytes = same hash = cached module");
    }

    // List applications
    info!("\n5. Listing deployed applications...");
    let list_request = tonic::Request::new(plexspaces_proto::application::v1::ListApplicationsRequest {
        status_filter: None, // List all applications
    });
    let list_response = client.list_applications(list_request).await?;
    let apps = list_response.into_inner();
    info!("✓ Total applications deployed: {}", apps.applications.len());
    for (i, app) in apps.applications.iter().enumerate() {
        info!("  {}. {} v{} (ID: {})", i + 1, app.name, app.version, app.application_id);
    }

    // Demonstrate actual actor functionality
    info!("\n6. Demonstrating actor functionality...");
    let runtime = Arc::new(WasmRuntime::new().await?);
    let module = runtime.load_module("counter-demo", "1.0.0", &demo_wasm).await?;
    
    // Instantiate actor
    let instance = runtime.instantiate(
        module,
        "counter-actor-1".to_string(),
        &[],
        WasmConfig::default(),
        None, // No channel service for this demo
    ).await?;
    info!("✓ Actor instantiated: {}", instance.actor_id());
    
    // Send increment message
    info!("\n7. Sending messages to actor and showing results...");
    let response1 = instance.handle_message("demo-client", "increment", vec![]).await?;
    let count1 = if response1.len() >= 4 {
        i32::from_le_bytes([response1[0], response1[1], response1[2], response1[3]])
    } else {
        0
    };
    info!("  → Message: 'increment' → Response: Counter = {}", count1);
    
    // Send another increment
    let response2 = instance.handle_message("demo-client", "increment", vec![]).await?;
    let count2 = if response2.len() >= 4 {
        i32::from_le_bytes([response2[0], response2[1], response2[2], response2[3]])
    } else {
        0
    };
    info!("  → Message: 'increment' → Response: Counter = {}", count2);
    
    // Get count
    let response3 = instance.handle_message("demo-client", "get_count", vec![]).await?;
    let count3 = if response3.len() >= 4 {
        i32::from_le_bytes([response3[0], response3[1], response3[2], response3[3]])
    } else {
        0
    };
    info!("  → Message: 'get_count' → Response: Counter = {}", count3);
    
    // Reset counter
    let response4 = instance.handle_message("demo-client", "reset", vec![]).await?;
    let count4 = if response4.len() >= 4 {
        i32::from_le_bytes([response4[0], response4[1], response4[2], response4[3]])
    } else {
        0
    };
    info!("  → Message: 'reset' → Response: Counter = {}", count4);
    
    info!("\n📊 Results Summary:");
    info!("  ✓ Initial count: 0");
    info!("  ✓ After 2 increments: {} (expected: 2)", count2);
    info!("  ✓ Final count (before reset): {} (expected: 2)", count3);
    info!("  ✓ After reset: {} (expected: 0)", count4);
    
    if count2 == 2 && count3 == 2 && count4 == 0 {
        info!("  ✅ All actor operations verified successfully!");
    } else {
        warn!("  ⚠ Some operations may not have returned expected values");
    }

    // Shutdown node
    node_arc.shutdown(tokio::time::Duration::from_secs(5)).await?;
    node_handle.abort();

    info!("\n✅ Deployment demo complete!\n");
    Ok(())
}

/// Demo 2: Resource Limits and Capability-Based Security
async fn run_security_demo() -> Result<()> {
    info!("🔒 Demo 2: Resource Limits & Capability-Based Security");
    info!("-------------------------------------------------------");

    // Start a node
    info!("\n1. Starting PlexSpaces node...");
    let node = NodeBuilder::new("wasm-security-node".to_string())
        .build();
    let node_arc = Arc::new(node);
    let node_clone = node_arc.clone();
    
    let node_handle = tokio::spawn(async move {
        node_clone.start().await.unwrap();
    });
    
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    info!("✓ Node started");

    // Connect to ApplicationService
    info!("\n2. Connecting to ApplicationService...");
    let channel = Channel::from_shared("http://127.0.0.1:9000".to_string())?
        .connect()
        .await?;
    let mut client = ApplicationServiceClient::new(channel);
    info!("✓ Connected to ApplicationService");

    // Deploy application with WASM module
    info!("\n3. Deploying application for security testing...");
    let demo_wasm = create_demo_wasm();
    let wasm_module = ProtoWasmModule {
        name: "secure-actor".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: demo_wasm.clone(),
        module_hash: String::new(),
        wit_interface: String::new(),
        ..Default::default()
    };

    let deploy_request = DeployApplicationRequest {
        application_id: "secure-app-1".to_string(),
        name: "secure-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: None,
        release_config: None,
        initial_state: vec![],
    };

    let deploy_response = client.deploy_application(tonic::Request::new(deploy_request)).await?;
    let deploy = deploy_response.into_inner();
    if !deploy.success {
        return Err(anyhow::anyhow!("Deployment failed: {:?}", deploy.error));
    }
    info!("✓ Application deployed via ApplicationService");

    // For demonstration, we'll use the runtime directly to show different configs
    let runtime = Arc::new(WasmRuntime::new().await?);
    let deployment_service = Arc::new(WasmDeploymentService::new(runtime.clone()));
    
    // Deploy module for testing different configurations
    let demo_wasm = create_demo_wasm();
    let module_hash = deployment_service
        .deploy_module("secure-actor", "1.0.0", &demo_wasm)
        .await?;

    // Configuration 1: Strict resource limits
    info!("\n2. Testing STRICT resource limits...");
    let strict_config = WasmConfig {
        limits: ResourceLimits {
            max_memory_bytes: 1 * 1024 * 1024, // 1MB
            max_fuel: 1_000_000,                 // Low fuel limit
            max_execution_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 100_000_000, // 100ms
            }),
            max_stack_bytes: 64 * 1024, // 64KB
            max_pooled_instances: 10,
            max_table_elements: 10000,
        },
        capabilities: WasmCapabilities {
            allow_filesystem: false,
            allow_network: false,
            allow_spawn_actors: false,
            allow_send_messages: false,
            allow_tuplespace: false,
            allow_logging: true, // Only logging allowed
            allow_env: false,
            allow_random: false,
            allow_clocks: false,
            filesystem_root: String::new(),
        },
        profile_name: "strict".to_string(),
        enable_pooling: false,
        enable_aot: false,
    };

    let _actor1 = deployment_service
        .instantiate_actor(&module_hash, "strict-actor", &[], Some(strict_config))
        .await?;
    info!("✓ Actor instantiated with STRICT limits:");
    info!("  - Memory: 1MB max");
    info!("  - Fuel: 1,000,000 units");
    info!("  - Timeout: 100ms");
    info!("  - Capabilities: logging only");

    // Configuration 2: Permissive (trusted actor)
    info!("\n3. Testing PERMISSIVE configuration (trusted actor)...");
    let permissive_config = WasmConfig {
        limits: ResourceLimits {
            max_memory_bytes: 256 * 1024 * 1024, // 256MB
            max_fuel: 100_000_000_000,             // High fuel limit
            max_execution_time: Some(prost_types::Duration {
                seconds: 60,
                nanos: 0,
            }),
            max_stack_bytes: 1024 * 1024, // 1MB
            max_pooled_instances: 100,
            max_table_elements: 100000,
        },
        capabilities: WasmCapabilities {
            allow_filesystem: true,
            allow_network: true,
            allow_spawn_actors: true,
            allow_send_messages: true,
            allow_tuplespace: true,
            allow_logging: true,
            allow_env: true,
            allow_random: true,
            allow_clocks: true,
            filesystem_root: "/tmp/wasm".to_string(),
        },
        profile_name: "trusted".to_string(),
        enable_pooling: true,
        enable_aot: false,
    };

    let _actor2 = deployment_service
        .instantiate_actor(&module_hash, "trusted-actor", &[], Some(permissive_config))
        .await?;
    info!("✓ Actor instantiated with PERMISSIVE limits:");
    info!("  - Memory: 256MB max");
    info!("  - Fuel: 100 billion units");
    info!("  - Timeout: 60s");
    info!("  - Capabilities: ALL enabled");

    // Configuration 3: Balanced (production default)
    info!("\n4. Testing BALANCED configuration (production default)...");
    let balanced_config = WasmConfig::default();
    let _actor3 = deployment_service
        .instantiate_actor(&module_hash, "balanced-actor", &[], Some(balanced_config.clone()))
        .await?;
    info!("✓ Actor instantiated with BALANCED limits:");
    info!("  - Memory: {}MB", balanced_config.limits.max_memory_bytes / (1024 * 1024));
    info!("  - Profile: {}", balanced_config.profile_name);

    // Shutdown node
    node_arc.shutdown(tokio::time::Duration::from_secs(5)).await?;
    node_handle.abort();

    info!("\n✅ Security demo complete!\n");
    Ok(())
}

/// Demo 3: gRPC Deployment Service
async fn run_grpc_demo() -> Result<()> {
    info!("🌐 Demo 3: gRPC Deployment Service");
    info!("-----------------------------------");

    // Start gRPC server
    info!("\n1. Starting gRPC server...");
    let (port, _server) = start_grpc_server().await?;
    info!("✓ gRPC server listening on port {}", port);

    // Create gRPC client
    info!("\n2. Connecting gRPC client...");
    let addr = format!("http://127.0.0.1:{}", port);
    let mut client = WasmRuntimeServiceClient::connect(addr).await?;
    info!("✓ Connected to gRPC service");

    // Deploy module via gRPC
    info!("\n3. Deploying module via gRPC...");
    let demo_wasm = create_demo_wasm();
    let module = ProtoWasmModule {
        name: "grpc-actor".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: demo_wasm.clone(),
        module_hash: String::new(),
        wit_interface: String::new(),
        source_languages: vec!["rust".to_string()],
        metadata: None,
        created_at: None,
        size_bytes: demo_wasm.len() as u64,
        version_number: 1,
    };

    let deploy_req = tonic::Request::new(DeployWasmModuleRequest {
        module: Some(module),
        pre_warm: 0,
        target_node_tags: vec![],
    });

    let deploy_resp = client.deploy_module(deploy_req).await?.into_inner();
    if deploy_resp.success {
        info!("✓ Module deployed successfully via gRPC");
        info!("  Hash: {}", deploy_resp.module_hash);
        info!("  Nodes pre-warmed: {}", deploy_resp.nodes_pre_warmed);
    } else {
        error!("✗ Deployment failed: {:?}", deploy_resp.error);
        return Err(anyhow::anyhow!("gRPC deployment failed"));
    }

    // Instantiate actor via gRPC
    info!("\n4. Instantiating actor via gRPC...");
    let instantiate_req = tonic::Request::new(InstantiateActorRequest {
        module_ref: deploy_resp.module_hash,
        actor_id: "grpc-actor-001".to_string(),
        initial_state: vec![],
        config: None,
        target_node_id: String::new(),
    });

    let instantiate_resp = client.instantiate_actor(instantiate_req).await?.into_inner();
    if instantiate_resp.success {
        info!("✓ Actor instantiated successfully via gRPC");
        info!("  Actor ID: {}", instantiate_resp.actor_id);
        info!("  Node ID: {}", instantiate_resp.node_id);
        if let Some(created_at) = instantiate_resp.created_at {
            info!("  Created at: {}", created_at.seconds);
        }
    } else {
        warn!("⚠ Instantiation failed (expected - module may not be Component Model): {:?}", instantiate_resp.error);
    }

    info!("\n✅ gRPC demo complete!\n");
    Ok(())
}

/// Demo 4: Concurrent Operations
async fn run_concurrency_demo() -> Result<()> {
    info!("⚡ Demo 4: Concurrent Operations");
    info!("--------------------------------");

    let runtime = Arc::new(WasmRuntime::new().await?);
    let deployment_service = Arc::new(WasmDeploymentService::new(runtime.clone()));

    // Deploy base module
    info!("\n1. Deploying base module...");
    let module_hash = deployment_service
        .deploy_module("concurrent-actor", "1.0.0", &create_demo_wasm())
        .await?;
    info!("✓ Base module deployed: {}", &module_hash[..16]);

    // Spawn 10 actors concurrently
    info!("\n2. Spawning 10 actors concurrently...");
    let start = std::time::Instant::now();

    let mut handles = vec![];
    for i in 0..10 {
        let service = deployment_service.clone();
        let hash = module_hash.clone();
        let handle = tokio::spawn(async move {
            let actor_id = format!("concurrent-actor-{:03}", i);
            service
                .instantiate_actor(&hash, &actor_id, &[], None)
                .await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let results = futures::future::join_all(handles).await;
    let duration = start.elapsed();

    let success_count = results.iter().filter(|r| r.is_ok() && r.as_ref().unwrap().is_ok()).count();
    info!("✓ Concurrent instantiation complete:");
    info!("  - Total: 10 actors");
    info!("  - Successful: {}", success_count);
    info!("  - Duration: {:?}", duration);
    info!("  - Throughput: {:.1} actors/sec", 10.0 / duration.as_secs_f64());

    info!("\n✅ Concurrency demo complete!\n");
    Ok(())
}

/// Helper: Start gRPC server on random port
async fn start_grpc_server() -> Result<(u16, tokio::task::JoinHandle<()>)> {
    let runtime = WasmRuntime::new().await?;
    let service = WasmRuntimeServiceImpl::new(Arc::new(runtime));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let port = addr.port();

    let server = Server::builder()
        .add_service(WasmRuntimeServiceServer::new(service))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener));

    let handle = tokio::spawn(async move {
        if let Err(e) = server.await {
            error!("gRPC server error: {}", e);
        }
    });

    // Give server time to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    Ok((port, handle))
}

/// Demo 5: Python Actor Support
async fn run_python_demo() -> Result<()> {
    info!("🐍 Demo 5: Python Actor Support");
    info!("-------------------------------");

    let runtime = Arc::new(WasmRuntime::new().await?);
    let deployment_service = WasmDeploymentService::new(runtime.clone());

    info!("\n1. Python actors can be compiled to WASM using componentize-py");
    info!("   Example actors:");
    info!("   - counter_actor.py: GenServer behavior (request-reply)");
    info!("   - event_actor.py: GenEvent behavior (fire-and-forget)");
    info!("   - fsm_actor.py: GenFSM behavior (state machine)");
    info!("   - service_demo_actor.py: Service access demonstration");

    info!("\n2. Building Python actors...");
    info!("   Run: ./scripts/build_python_actors.sh");
    info!("   This compiles Python to WASM using componentize-py");

    info!("\n3. Python actor capabilities:");
    info!("   ✓ Same WIT interface as Rust/TypeScript actors");
    info!("   ✓ Access to all host functions (send_message, spawn_actor, tuplespace)");
    info!("   ✓ Capability-based security (same as other languages)");
    info!("   ✓ Resource limits (memory, fuel, CPU time)");

    // Note: Actual Python WASM compilation requires componentize-py setup
    // For now, we demonstrate the concept
    info!("\n4. Python actor example structure:");
    info!("   ```python");
    info!("   def init(initial_state: bytes) -> tuple[None, str | None]:");
    info!("       # Initialize actor state");
    info!("       return None, None");
    info!("   ");
    info!("   def handle_message(from_actor: str, message_type: str, payload: bytes) -> tuple[bytes, str | None]:");
    info!("       # Handle messages (GenServer/GenEvent/GenFSM patterns)");
    info!("       return response_bytes, None");
    info!("   ```");

    info!("\n✅ Python demo complete!\n");
    Ok(())
}

/// Helper to start a node
async fn start_node(node_id: &str) -> Result<(Arc<Node>, tokio::task::JoinHandle<()>)> {
    let node = NodeBuilder::new(node_id.to_string()).build();
    let node_arc = Arc::new(node);
    let node_clone = node_arc.clone();
    
    let node_handle = tokio::spawn(async move {
        if let Err(e) = node_clone.start().await {
            eprintln!("Node error: {}", e);
        }
    });
    
    // Wait for node to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    Ok((node_arc, node_handle))
}

/// Demo 6: Different Behaviors (GenServer, GenEvent, GenFSM)
async fn run_behaviors_demo() -> Result<()> {
    info!("🎭 Demo 6: Actor Behaviors (GenServer, GenEvent, GenFSM)");
    info!("--------------------------------------------------------");

    let runtime = Arc::new(WasmRuntime::new().await?);
    let deployment_service = WasmDeploymentService::new(runtime.clone());

    info!("\n1. GenServer Behavior (Request-Reply Pattern)");
    info!("   - Synchronous request-reply communication");
    info!("   - Uses handle_request() export (MUST return response)");
    info!("   - Message type 'call' routes to handle_request()");
    info!("   - Example: Counter actor (get_count returns current value)");
    info!("   - Use case: Stateful services, CRUD operations");
    info!("   - Python example: counter_actor.py exports handle_request()");

    info!("\n2. GenEvent Behavior (Fire-and-Forget Pattern)");
    info!("   - Asynchronous event notifications");
    info!("   - Uses handle_event() export (no response needed)");
    info!("   - Message types 'cast' or 'info' route to handle_event()");
    info!("   - Example: Event logger, metrics collector");
    info!("   - Use case: Event streaming, pub/sub patterns");
    info!("   - Python example: event_actor.py exports handle_event()");

    info!("\n3. GenFSM Behavior (Finite State Machine)");
    info!("   - State transitions based on messages");
    info!("   - Uses handle_transition() export (returns new state name)");
    info!("   - Any message type can route to handle_transition()");
    info!("   - Example: Workflow actor (Idle -> Processing -> Done)");
    info!("   - Use case: Workflows, protocol state machines");
    info!("   - Python example: fsm_actor.py exports handle_transition()");

    info!("\n4. Behavior Routing in WASM Runtime:");
    info!("   - Runtime automatically routes messages to behavior-specific handlers");
    info!("   - Falls back to handle_message() for backward compatibility");
    info!("   - Behavior pattern is determined by exported functions");
    info!("   - WASM actors can implement any behavior pattern");

    // Deploy a simple module to demonstrate behavior concept
    info!("\n5. Deploying example actor module...");
    let module_hash = deployment_service
        .deploy_module("behavior-demo", "1.0.0", &create_demo_wasm())
        .await?;
    info!("✓ Module deployed: {}", &module_hash[..16]);
    info!("   This module can implement any behavior pattern");

    info!("\n✅ Behaviors demo complete!\n");
    Ok(())
}

/// Demo 7: Service Access (ChannelService, TupleSpace, ActorService)
async fn run_services_demo() -> Result<()> {
    info!("🔌 Demo 7: Service Access from WASM Actors");
    info!("-------------------------------------------");

    let runtime = Arc::new(WasmRuntime::new().await?);
    let deployment_service = WasmDeploymentService::new(runtime.clone());

    info!("\n1. ActorService Access (via host functions)");
    info!("   - send_message(to, message_type, payload)");
    info!("     → Send message to another actor (local or remote)");
    info!("   - spawn_actor(module_ref, initial_state)");
    info!("     → Spawn child actor from WASM module");
    info!("   - Requires: allow_send_messages, allow_spawn_actors capabilities");

    info!("\n2. TupleSpace Access (via host functions)");
    info!("   - tuplespace_write(tuple)");
    info!("     → Write tuple to coordination space");
    info!("   - tuplespace_read(pattern)");
    info!("     → Read tuple matching pattern (non-destructive)");
    info!("   - tuplespace_take(pattern)");
    info!("     → Take tuple matching pattern (destructive)");
    info!("   - tuplespace_count(pattern)");
    info!("     → Count matching tuples");
    info!("   - Requires: allow_tuplespace capability");

    info!("\n3. ChannelService Access (via host functions)");
    info!("   - send_to_queue(queue_name, message_type, payload)");
    info!("     → Send message to queue (load-balanced to one consumer)");
    info!("     → Returns message ID on success");
    info!("   - publish_to_topic(topic_name, message_type, payload)");
    info!("     → Publish message to topic (all subscribers receive)");
    info!("     → Returns message ID on success");
    info!("   - receive_from_queue(queue_name, timeout_ms)");
    info!("     → Receive message from queue (blocking, with timeout)");
    info!("     → Returns (message_type, payload) tuple or None if timeout");
    info!("   - Requires: ChannelService configured in WASM instance");
    info!("   - ChannelService is automatically passed from node when creating instances");
    info!("   - Python example: service_demo_actor.py demonstrates channel usage");
    info!("   - Note: receive_from_queue requires async host functions (placeholder for now)");

    info!("\n4. Capability-Based Security:");
    info!("   - Actors must be granted capabilities to access services");
    info!("   - Untrusted actors: No service access (logging only)");
    info!("   - Trusted actors: Full service access");
    info!("   - Fine-grained control per service type");

    info!("\n5. Example: Service-Aware Actor");
    info!("   ```python");
    info!("   def handle_message(from_actor, message_type, payload):");
    info!("       if message_type == 'spawn_worker':");
    info!("           # Spawn child actor (requires allow_spawn_actors)");
    info!("           child_id = host.spawn_actor('worker@1.0.0', b'')");
    info!("           ");
    info!("       elif message_type == 'send_task':");
    info!("           # Send message to worker (requires allow_send_messages)");
    info!("           host.send_message('worker-1', 'process', payload)");
    info!("           ");
    info!("       elif message_type == 'coordinate':");
    info!("           # Use TupleSpace (requires allow_tuplespace)");
    info!("           tuple = [('task', 'pending', payload)]");
    info!("           host.tuplespace_write(tuple)");
    info!("   ```");

    // Deploy an application with service access capabilities
    info!("\n6. Deploying service-aware application...");
    let (node_arc, node_handle) = start_node("wasm-services-node").await?;
    
    // Wait for node to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    let channel = Channel::from_shared("http://127.0.0.1:9000".to_string())?
        .connect()
        .await?;
    let mut client = ApplicationServiceClient::new(channel);

    let demo_wasm = create_demo_wasm();
    let wasm_module = ProtoWasmModule {
        name: "service-demo".to_string(),
        version: "1.0.0".to_string(),
        module_bytes: demo_wasm,
        module_hash: String::new(),
        wit_interface: String::new(),
        ..Default::default()
    };

    // Create application config with service capabilities
    let app_config = ApplicationSpec {
        name: "service-demo-app".to_string(),
        version: "1.0.0".to_string(),
        description: "Service-aware application with full capabilities".to_string(),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: std::collections::HashMap::new(),
        supervisor: None,
    };

    let deploy_request = DeployApplicationRequest {
        application_id: "service-demo-app-1".to_string(),
        name: "service-demo-app".to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_config),
        release_config: None,
        initial_state: vec![],
    };

    let deploy_response = client.deploy_application(tonic::Request::new(deploy_request)).await?;
    let deploy = deploy_response.into_inner();
    if deploy.success {
        info!("✓ Service-aware application deployed");
        info!("  Application ID: {}", deploy.application_id);
        info!("  Note: Capabilities configured via ApplicationSpec");
    }

    // Shutdown node
    node_arc.shutdown(tokio::time::Duration::from_secs(5)).await?;
    node_handle.abort();

    info!("\n✅ Services demo complete!\n");
    Ok(())
}
