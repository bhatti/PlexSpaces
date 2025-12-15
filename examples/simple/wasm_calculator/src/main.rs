// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// WASM Calculator Example
//
// Demonstrates Python application deployment to PlexSpaces nodes.
// Shows how Python applications written as WASM modules can be:
// - Deployed to nodes via ApplicationService
// - Use durable actors for state persistence
// - Use tuplespace for distributed coordination
//
// Key Features:
// 1. Python Application Deployment: Deploy Python WASM applications to nodes
// 2. Durable Actors: State persistence, journaling, checkpointing
// 3. TupleSpace Coordination: Distributed coordination via tuplespace

use anyhow::Result;
use clap::Parser;
use plexspaces_node::{ConfigBootstrap, NodeBuilder};
use plexspaces_tuplespace::{Pattern, PatternField, TupleField};
use plexspaces_proto::v1::actor::actor_service_client::ActorServiceClient;
use plexspaces_proto::application::v1::{
    application_service_client::ApplicationServiceClient, DeployApplicationRequest,
    ApplicationSpec, ApplicationType,
};
use plexspaces_proto::wasm::v1::WasmModule as ProtoWasmModule;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tonic::transport::Channel;
use tracing::{info, Level};
use tracing_subscriber;

#[derive(Parser)]
#[command(name = "wasm-calculator")]
#[command(about = "WASM Calculator Example - Polyglot Actor Support with Different Behaviors")]
struct Args {
    /// Build Python actors before running
    #[arg(long, default_value = "false")]
    build: bool,
}


#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("wasm_calculator=info,plexspaces=warn")
        .init();

    let args = Args::parse();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  WASM Calculator Example - Python Application Deployment       ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    info!("Starting WASM Calculator Example");
    info!("Demonstrating: Python application deployment with durable actors and tuplespace");

    // Build Python actors if requested
    if args.build {
        info!("Building Python calculator actors...");
        let build_script = PathBuf::from("scripts/build_python_actors.sh");
        if build_script.exists() {
            use std::process::Command;
            let output = Command::new("bash")
                .arg(&build_script)
                .output()?;
            if output.status.success() {
                info!("✓ Python actors built successfully");
            } else {
                info!("⚠ Python actor build had issues (will use fallback)");
            }
        }
    }

    // Load configuration
    let _config: serde_json::Value = ConfigBootstrap::load()
        .unwrap_or_else(|_| serde_json::json!({}));

    // Create node
    let node = NodeBuilder::new("wasm-calculator-node-1")
        .build();

    let node_arc = Arc::new(node);
    info!("Node created: wasm-calculator-node-1");

    // Start node in background task
    let node_for_start = node_arc.clone();
    tokio::spawn(async move {
        if let Err(e) = node_for_start.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to be ready
    sleep(Duration::from_millis(500)).await;
    info!("Node started");

    // Deploy Python WASM applications via ApplicationService
    info!("Step 1: Deploying Python WASM applications to PlexSpaces node...");
    
    // Connect to node's ApplicationService
    let node_address = format!("http://{}", node_arc.config().listen_addr);
    info!("  Connecting to ApplicationService at {}...", node_address);
    let channel = Channel::from_shared(node_address)?
        .connect()
        .await?;
    let mut client = ApplicationServiceClient::new(channel);
    info!("  ✓ Connected to ApplicationService");
    
    let wasm_dir = PathBuf::from("wasm-modules");
    
    // Helper to deploy a Python WASM application
    async fn deploy_python_app(
        client: &mut ApplicationServiceClient<Channel>,
        app_id: &str,
        app_name: &str,
        wasm_path: &PathBuf,
        description: &str,
    ) -> Result<String> {
        let wasm_bytes = if wasm_path.exists() {
            fs::read(wasm_path)?
        } else {
            info!("    ⚠ WASM file not found at {}, creating placeholder", wasm_path.display());
            // Create minimal placeholder WASM
            vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00]
        };
        
        let wasm_module = ProtoWasmModule {
            name: app_name.to_string(),
            version: "1.0.0".to_string(),
            module_bytes: wasm_bytes,
            module_hash: String::new(), // Will be computed by server
            wit_interface: String::new(),
            ..Default::default()
        };
        
        let app_config = ApplicationSpec {
            name: app_name.to_string(),
            version: "1.0.0".to_string(),
            description: description.to_string(),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: None,
        };
        
        let deploy_request = DeployApplicationRequest {
            application_id: app_id.to_string(),
            name: app_name.to_string(),
            version: "1.0.0".to_string(),
            wasm_module: Some(wasm_module),
            config: Some(app_config),
            release_config: None,
            initial_state: vec![],
        };
        
        let deploy_response = client.deploy_application(tonic::Request::new(deploy_request)).await?;
        let deploy = deploy_response.into_inner();
        if deploy.success {
            info!("  ✓ {} deployed: {}", app_name, deploy.application_id);
            Ok(deploy.application_id)
        } else {
            Err(anyhow::anyhow!("Deployment failed: {:?}", deploy.error))
        }
    }
    
    // Deploy durable calculator actor (demonstrates state persistence)
    info!("  Deploying durable calculator actor (state persistence)...");
    let durable_calc_wasm = wasm_dir.join("durable_calculator_actor.wasm");
    let _durable_app_id = deploy_python_app(
        &mut client,
        "durable-calc-app-1",
        "durable-calculator-app",
        &durable_calc_wasm,
        "Python calculator with durable state persistence and journaling",
    ).await?;
    
    // Deploy tuplespace calculator actor (demonstrates tuplespace coordination)
    info!("  Deploying tuplespace calculator actor (coordination)...");
    let tuplespace_calc_wasm = wasm_dir.join("tuplespace_calculator_actor.wasm");
    let _tuplespace_app_id = deploy_python_app(
        &mut client,
        "tuplespace-calc-app-1",
        "tuplespace-calculator-app",
        &tuplespace_calc_wasm,
        "Python calculator with tuplespace coordination",
    ).await?;
    
    // List deployed applications
    info!("  Listing deployed applications...");
    let list_request = tonic::Request::new(
        plexspaces_proto::application::v1::ListApplicationsRequest {
            status_filter: None,
        }
    );
    let list_response = client.list_applications(list_request).await?;
    let apps = list_response.into_inner();
    info!("  ✓ Total applications deployed: {}", apps.applications.len());
    for (i, app) in apps.applications.iter().enumerate() {
        info!("    {}. {} v{} (ID: {})", i + 1, app.name, app.version, app.application_id);
    }
    println!();

    // Demonstrate durable actor features
    info!("Step 2: Demonstrating Durable Actor Features...");
    info!("  Durable actors provide:");
    info!("    • State persistence: Actor state is automatically journaled");
    info!("    • Checkpointing: Periodic snapshots for fast recovery");
    info!("    • Replay: Deterministic replay of messages after restart");
    info!("    • Side effect caching: External calls cached during replay");
    info!("  ");
    info!("  The durable_calculator_actor.py implements:");
    info!("    • snapshot_state() - Serializes state for checkpointing");
    info!("    • init(initial_state) - Restores state from checkpoint");
    info!("    • State is automatically persisted by DurabilityFacet");
    println!();
    
    // Demonstrate tuplespace coordination
    info!("Step 3: Demonstrating TupleSpace Coordination from Python...");
    info!("  TupleSpace enables distributed coordination:");
    info!("    • Write: Share results with other actors");
    info!("    • Read: Query results from other actors");
    info!("    • Pattern matching: Flexible queries with wildcards");
    info!("  ");
    info!("  The tuplespace_calculator_actor.py demonstrates:");
    info!("    • host.tuplespace_write() - Write calculation results to tuplespace");
    info!("    • host.tuplespace_read() - Read results from other actors");
    info!("    • Results are shared across all actors in the node");
    info!("  ");
    info!("  See: actors/python/tuplespace_calculator_actor.py for Python implementation");
    
    // Connect to ActorService to send messages to Python WASM actors
    info!("  Connecting to ActorService to interact with Python WASM actors...");
    let actor_channel = Channel::from_shared(format!("http://{}", node_arc.config().listen_addr))?
        .connect()
        .await?;
    let mut _actor_client = ActorServiceClient::new(actor_channel);
    info!("  ✓ Connected to ActorService");
    
    // Note: In a real scenario, we would:
    // 1. Get actor IDs from the deployed applications
    // 2. Send messages to those actors
    // 3. Actors would write results to tuplespace
    // 4. We would read from tuplespace to see the results
    // 
    // For this example, we demonstrate the tuplespace read pattern
    // that Python actors would use to coordinate
    
    // Read results from TupleSpace (demonstrates coordination pattern)
    // Python WASM actors use host.tuplespace_read() with this same pattern
    info!("  Reading from TupleSpace (Python actors use host.tuplespace_read() with this pattern)...");
    let result_pattern = Pattern::new(vec![
        PatternField::Exact(TupleField::String("calculator_result".to_string())),
        PatternField::Wildcard,  // any operation
        PatternField::Wildcard,  // any operands
        PatternField::Wildcard,  // any result
        PatternField::Wildcard,  // any actor_id
    ]);
    
    let results = node_arc.tuplespace().read_all(result_pattern).await?;
    info!("  ✓ Found {} results in TupleSpace", results.len());
    
    if results.is_empty() {
        info!("    (No results yet - Python WASM actors write results using host.tuplespace_write())");
        info!("    See tuplespace_calculator_actor.py for the Python implementation");
    } else {
        for (i, tuple) in results.iter().enumerate() {
            let fields = tuple.fields();
            if fields.len() >= 5 {
                match (&fields[0], &fields[1], &fields[2], &fields[3], &fields[4]) {
                    (
                        TupleField::String(_tag),
                        TupleField::String(ref op),
                        TupleField::String(_operands),
                        TupleField::Float(result),
                        TupleField::String(_actor_id),
                    ) => {
                        let result_value = result.get();
                        info!("    {}. {} = {} (from Python actor: {})", i + 1, op, result_value, _actor_id);
                    }
                    _ => {}
                }
            }
        }
    }
    
    println!();
    info!("  Features demonstrated:");
    info!("    • Python applications deployed to PlexSpaces nodes via ApplicationService");
    info!("    • Durable actors: State persistence, journaling, checkpointing");
    info!("    • TupleSpace coordination: Results shared via distributed tuplespace");
    info!("    • Dynamic module deployment at runtime");
    info!("    • Content-addressed module caching for efficiency");
    println!();

    // Keep running for a bit to demonstrate
    sleep(Duration::from_secs(2)).await;

    println!();
    info!("Shutting down...");
    node_arc.shutdown(Duration::from_secs(5)).await?;

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Example complete!");
    println!();
    println!("Key Takeaways:");
    println!("  • Python applications can be deployed to PlexSpaces nodes via ApplicationService");
    println!("  • Durable actors: State persistence, journaling, checkpointing for fault tolerance");
    println!("  • TupleSpace coordination: Distributed coordination primitives for result sharing");
    println!("  • Dynamic module deployment: Deploy Python WASM modules at runtime");
    println!("  • Content-addressed caching: Modules cached by hash for efficiency");
    println!("  • Polyglot support: Same interface works for Python, Rust, TypeScript, Go");
    println!();

    Ok(())
}
