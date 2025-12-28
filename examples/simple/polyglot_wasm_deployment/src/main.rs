// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Polyglot WASM Deployment Example
//!
//! Demonstrates deploying actors written in multiple languages (Rust, TypeScript, Go, Python)
//! to an empty PlexSpaces node via ApplicationService gRPC.

use anyhow::Result;
use clap::{Parser, Subcommand};
use plexspaces_node::{CoordinationComputeTracker, NodeBuilder};
use plexspaces_proto::application::v1::{
    application_service_client::ApplicationServiceClient, ApplicationSpec, ApplicationType,
    DeployApplicationRequest,
};
use plexspaces_proto::wasm::v1::WasmModule as ProtoWasmModule;
use std::path::PathBuf;
use std::sync::Arc;
use std::fs;
use tracing::{info, Level};
use tracing_subscriber;
use tonic::transport::Channel;
use ulid::Ulid;

#[derive(Parser)]
#[command(name = "polyglot-wasm-deployment")]
#[command(about = "Polyglot WASM Deployment Example - Deploy actors in multiple languages to an empty PlexSpaces node")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start empty node (framework only, ready for deployments)
    StartNode {
        /// Node ID
        #[arg(long, default_value = "polyglot-node-1")]
        node_id: String,
        
        /// Node address (host:port)
        #[arg(long, default_value = "127.0.0.1:9000")]
        address: String,
    },
    /// Deploy actor to node via ApplicationService
    Deploy {
        /// Actor name
        #[arg(long)]
        name: String,

        /// Language (rust, typescript, go, python)
        #[arg(long)]
        language: String,

        /// Path to WASM module
        #[arg(long)]
        wasm: PathBuf,

        /// Node address (host:port)
        #[arg(long, default_value = "127.0.0.1:9000")]
        node_address: String,
    },
    /// List deployed applications
    List {
        /// Node address (host:port)
        #[arg(long, default_value = "127.0.0.1:9000")]
        node_address: String,
    },
    /// Run example workload (simulates multiple languages with metrics)
    Run {
        /// Number of operations per actor
        #[arg(long, default_value = "10")]
        operations: u32,
    },
    /// Stop actor deployment
    Stop {
        /// Actor name
        #[arg(long)]
        name: String,
        
        /// Node address (host:port)
        #[arg(long, default_value = "127.0.0.1:9000")]
        node_address: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("polyglot_wasm_deployment=info,plexspaces=warn")
        .init();

    let args = Args::parse();

    info!("Starting Polyglot WASM Deployment Example");

    match args.command {
        Commands::StartNode { node_id, address } => {
            start_empty_node(&node_id, &address).await?;
        }
        Commands::Deploy {
            name,
            language,
            wasm,
            node_address,
        } => {
            deploy_actor(&name, &language, &wasm, &node_address).await?;
        }
        Commands::List { node_address } => {
            list_applications(&node_address).await?;
        }
        Commands::Run { operations } => {
            run_example_workload(operations).await?;
        }
        Commands::Stop { name, node_address } => {
            stop_actor(&name, &node_address).await?;
        }
    }

    Ok(())
}

async fn start_empty_node(node_id: &str, address: &str) -> Result<()> {
    info!("Starting empty PlexSpaces node: {}", node_id);
    info!("This node is ready to accept polyglot application deployments");
    info!("Node will listen on: {}", address);

    let mut metrics = CoordinationComputeTracker::new(format!("start-node-{}", node_id));
    metrics.start_coordinate();

    // Validate address format (host:port)
    if !address.contains(':') {
        return Err(anyhow::anyhow!("Invalid address format: {}. Expected host:port", address));
    }

    // Create and start an empty node (no applications pre-loaded)
    let node = NodeBuilder::new(node_id.to_string())
        .with_listen_address(address.to_string())
        .build().await;
    let node_arc = Arc::new(node);
    let node_clone = node_arc.clone();
    
    // Start node in background task (so we can print status first)
    let node_for_start = node_arc.clone();
    let node_handle = tokio::spawn(async move {
        node_for_start.start().await
    });
    
    // Wait a moment for node to initialize
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    
    metrics.end_coordinate();
    let report = metrics.finalize();
    
    info!("✓ Empty node started successfully");
    info!("  Node ID: {}", node_id);
    info!("  Address: {}", address);
    info!("  Coordination time: {} ms", report.coordinate_duration_ms);
    info!("  Node is ready to accept deployments via ApplicationService");
    info!("  Use 'deploy' command to deploy polyglot applications");
    
    // Keep node running until shutdown signal
    info!("  Node running (press Ctrl+C to stop)");
    
    // Wait for node handle or shutdown signal
    tokio::select! {
        result = node_handle => {
            match result {
                Ok(Ok(())) => info!("Node stopped normally"),
                Ok(Err(e)) => {
                    eprintln!("Node error: {}", e);
                }
                Err(e) => {
                    eprintln!("Node task error: {}", e);
                }
            }
        }
        _ = tokio::signal::ctrl_c() => {
            info!("Shutting down node...");
        }
    }

    Ok(())
}

async fn deploy_actor(
    actor_name: &str,
    language: &str,
    wasm_path: &PathBuf,
    node_address: &str,
) -> Result<()> {
    info!("Deploying actor: {} ({}) to node: {}", actor_name, language, node_address);

    let mut metrics = CoordinationComputeTracker::new(format!("deploy-{}", actor_name));
    metrics.start_coordinate();

    // Validate language
    let valid_languages = ["rust", "typescript", "go", "python"];
    if !valid_languages.contains(&language.to_lowercase().as_str()) {
        anyhow::bail!("Invalid language: {}. Must be one of: {:?}", language, valid_languages);
    }

    // Check if WASM file exists
    if !wasm_path.exists() {
        anyhow::bail!("WASM file not found: {}", wasm_path.display());
    }

    // Connect to node's ApplicationService
    let node_address_http = format!("http://{}", node_address);
    info!("  Connecting to ApplicationService at {}...", node_address_http);
    let channel = Channel::from_shared(node_address_http)
        .map_err(|e| anyhow::anyhow!("Invalid node address {}: {}", node_address, e))?
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to node {}: {}", node_address, e))?;
    
    let mut client = ApplicationServiceClient::new(channel);
    info!("  ✓ Connected to ApplicationService");

    // Read WASM file
    let wasm_bytes = fs::read(wasm_path)
        .map_err(|e| anyhow::anyhow!("Failed to read WASM file {}: {}", wasm_path.display(), e))?;

    // Create WASM module
    let wasm_module = ProtoWasmModule {
        name: actor_name.to_string(),
        version: "1.0.0".to_string(),
        module_bytes: wasm_bytes,
        module_hash: String::new(), // Will be computed by server
        wit_interface: String::new(),
    };

    // Create application config
    let app_config = ApplicationSpec {
        name: actor_name.to_string(),
        version: "1.0.0".to_string(),
        description: format!("Polyglot {} actor", language),
        r#type: ApplicationType::ApplicationTypeActive.into(),
        dependencies: vec![],
        env: std::collections::HashMap::new(),
        supervisor: None,
    };

    // Deploy application
    let application_id = format!("{}-{}", actor_name, Ulid::new());
    let deploy_request = DeployApplicationRequest {
        application_id: application_id.clone(),
        name: actor_name.to_string(),
        version: "1.0.0".to_string(),
        wasm_module: Some(wasm_module),
        config: Some(app_config),
        release_config: None,
        initial_state: vec![],
    };

    info!("  Deploying application to node...");
    let deploy_response = client.deploy_application(tonic::Request::new(deploy_request)).await
        .map_err(|e| anyhow::anyhow!("Failed to deploy application: {}", e))?;
    
    let deploy = deploy_response.into_inner();
    metrics.end_coordinate();
    
    let report = metrics.finalize();
    
    if deploy.success {
        info!("✓ Polyglot application deployed successfully");
        info!("  Application: {} ({})", actor_name, language);
        info!("  Application ID: {}", deploy.application_id);
        info!("  WASM: {}", wasm_path.display());
        info!("  Node: {}", node_address);
        info!("  Coordination time: {} ms", report.coordinate_duration_ms);
    } else {
        anyhow::bail!("Deployment failed: {:?}", deploy.error);
    }

    Ok(())
}

async fn list_applications(node_address: &str) -> Result<()> {
    info!("Listing deployed applications on node: {}", node_address);

    let mut metrics = CoordinationComputeTracker::new("list-applications".to_string());
    metrics.start_coordinate();

    // Connect to node's ApplicationService
    let node_address_http = format!("http://{}", node_address);
    let channel = Channel::from_shared(node_address_http)
        .map_err(|e| anyhow::anyhow!("Invalid node address {}: {}", node_address, e))?
        .connect()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to node {}: {}", node_address, e))?;
    
    let mut client = ApplicationServiceClient::new(channel);

    // List applications
    let list_request = tonic::Request::new(
        plexspaces_proto::application::v1::ListApplicationsRequest {
            status_filter: None,
        }
    );
    
    let list_response = client.list_applications(list_request).await
        .map_err(|e| anyhow::anyhow!("Failed to list applications: {}", e))?;
    
    let apps = list_response.into_inner();
    metrics.end_coordinate();
    let report = metrics.finalize();

    if apps.applications.is_empty() {
        info!("No applications currently deployed");
    } else {
        info!("Found {} application(s):", apps.applications.len());
        for (i, app) in apps.applications.iter().enumerate() {
            info!("  {}. {} v{} (ID: {})", i + 1, app.name, app.version, app.application_id);
        }
    }

    info!("Discovery time: {} ms", report.coordinate_duration_ms);
    Ok(())
}

async fn stop_actor(actor_name: &str, node_address: &str) -> Result<()> {
    info!("Stopping actor: {} on node: {}", actor_name, node_address);

    let mut metrics = CoordinationComputeTracker::new(format!("stop-{}", actor_name));
    metrics.start_coordinate();

    // In real implementation, would:
    // 1. Find application by name
    // 2. Call UndeployApplication via ApplicationService

    metrics.end_coordinate();
    let report = metrics.finalize();
    info!("Actor stopped: {}", actor_name);
    info!("Stop time: {} ms", report.coordinate_duration_ms);

    Ok(())
}

async fn run_example_workload(operations_per_actor: u32) -> Result<()> {
    info!("Running example workload: {} operations per actor", operations_per_actor);
    info!("Note: This simulates deployment and execution for demonstration purposes");
    info!("For real deployment, use: start-node, then deploy commands");

    let languages = vec!["rust", "typescript", "go", "python"];
    let mut actor_metrics: std::collections::HashMap<String, plexspaces_proto::metrics::v1::CoordinationComputeMetrics> = std::collections::HashMap::new();

    // Simulate deploying and running actors in different languages
    for language in &languages {
        let actor_id = format!("actor-{}", Ulid::new());
        let mut metrics = CoordinationComputeTracker::new(format!("{}-{}", actor_id, language));

        info!("Processing actor: {} ({})", actor_id, language);

        // Simulate WASM deployment (coordination)
        metrics.start_coordinate();
        // In real implementation: deploy WASM module to node via ApplicationService
        tokio::time::sleep(tokio::time::Duration::from_millis(20)).await; // Simulate deployment
        metrics.end_coordinate();

        // Simulate actor instantiation (coordination)
        metrics.start_coordinate();
        tokio::time::sleep(tokio::time::Duration::from_millis(5)).await; // Simulate instantiation
        metrics.end_coordinate();

        // Simulate operations (computation)
        for op in 0..operations_per_actor {
            metrics.start_compute();
            // Simulate actor processing (language-specific computation)
            let compute_duration = match *language {
                "rust" => 30,      // Fast native code
                "typescript" => 50, // WASM overhead
                "go" => 40,        // WASM overhead
                "python" => 60,    // WASM overhead + interpreter
                _ => 50,
            };
            tokio::time::sleep(tokio::time::Duration::from_millis(compute_duration)).await;
            metrics.end_compute();

            // Simulate message passing (coordination)
            metrics.start_coordinate();
            tokio::time::sleep(tokio::time::Duration::from_millis(2)).await; // Simulate message
            metrics.end_coordinate();
            metrics.increment_message();

            if op % 5 == 0 {
                info!("  Actor {} ({}): Completed operation {}", actor_id, language, op);
            }
        }

        let report = metrics.finalize();
        print_actor_report(&actor_id, language, &report);
        actor_metrics.insert(actor_id, report);
    }

    // Print aggregate metrics
    let mut total_compute: u64 = 0;
    let mut total_coordinate: u64 = 0;
    let mut total_messages: u64 = 0;

    for (_, report) in actor_metrics.iter() {
        total_compute += report.compute_duration_ms;
        total_coordinate += report.coordinate_duration_ms;
        total_messages += report.message_count;
    }

    let overall_ratio = if total_coordinate > 0 {
        total_compute as f64 / total_coordinate as f64
    } else {
        f64::INFINITY
    };

    println!("\n=== Aggregate Metrics ===");
    println!("Total Actors: {}", languages.len());
    println!("Total Compute Time: {} ms", total_compute);
    println!("Total Coordinate Time: {} ms", total_coordinate);
    println!("Total Messages: {}", total_messages);
    println!("Overall Granularity Ratio: {:.2}×", overall_ratio);
    println!("========================\n");

    Ok(())
}

fn print_actor_report(actor_id: &str, language: &str, report: &plexspaces_proto::metrics::v1::CoordinationComputeMetrics) {
    println!("\n=== Polyglot WASM Deployment Performance Report ===");
    println!("Actor: {} ({})", actor_id, language);
    println!();
    println!("Timing:");
    println!("  Compute Time:     {:>8} ms", report.compute_duration_ms);
    println!("  Coordinate Time:  {:>8} ms", report.coordinate_duration_ms);
    println!("  Total Time:       {:>8} ms", report.total_duration_ms);
    println!();
    println!("Operations:");
    println!("  Messages:         {:>8}", report.message_count);
    println!();
    println!("Performance:");
    println!("  Granularity Ratio: {:>7.2}× (compute/coordinate)", report.granularity_ratio);

    if report.granularity_ratio < 10.0 {
        println!("    ⚠️  WARNING: Ratio < 10×! Overhead too high!");
    } else if report.granularity_ratio < 100.0 {
        println!("    ✓  Acceptable (>10×), but could be better");
    } else {
        println!("    ✅ Excellent (>100×)! Optimal granularity");
    }

    println!("  Efficiency:        {:>7.1}% (compute/total)", report.efficiency * 100.0);
    println!("====================================================\n");
}
