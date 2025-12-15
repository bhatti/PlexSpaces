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

//! N-Body WASM Example - Application-Level Deployment
//!
//! Demonstrates deploying a TypeScript-based N-Body simulation as a WASM application.
//! The entire application (all actors + coordinator) is packaged as a single WASM module.

use clap::{Parser, Subcommand};
use plexspaces_proto::application::v1::application_service_client::ApplicationServiceClient;
use plexspaces_proto::application::v1::DeployApplicationRequest;
use plexspaces_proto::wasm::v1::WasmModule;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "nbody-wasm")]
#[command(about = "N-Body simulation with TypeScript WASM actors")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy WASM application module
    Deploy {
        /// Path to WASM module
        #[arg(short, long)]
        wasm: PathBuf,
        /// Node address (gRPC)
        #[arg(short, long, default_value = "http://localhost:9001")]
        node: String,
        /// Application name
        #[arg(short, long, default_value = "nbody-simulation")]
        name: String,
        /// Application version
        #[arg(short, long, default_value = "0.1.0")]
        version: String,
    },
    /// Build TypeScript actors to WASM
    Build {
        /// TypeScript source directory
        #[arg(short, long, default_value = "ts-actors")]
        source: PathBuf,
        /// Output WASM file
        #[arg(short, long, default_value = "wasm-modules/nbody-application.wasm")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Deploy { wasm, node, name, version } => {
            println!("╔════════════════════════════════════════════════════════════════╗");
            println!("║     N-Body WASM - Deploy Application                         ║");
            println!("╚════════════════════════════════════════════════════════════════╝");
            println!();

            // Read WASM module
            println!("Reading WASM module: {:?}", wasm);
            let wasm_bytes = std::fs::read(&wasm)?;
            println!("✅ Loaded WASM module: {} bytes ({:.2} KB)", 
                wasm_bytes.len(), 
                wasm_bytes.len() as f64 / 1024.0);
            println!();

            // Connect to node
            println!("Connecting to node: {}", node);
            let mut client = ApplicationServiceClient::connect(node.clone()).await?;
            println!("✅ Connected to node");
            println!();

            // Deploy application
            println!("Deploying application:");
            println!("  Name: {}", name);
            println!("  Version: {}", version);
            println!("  WASM size: {} bytes", wasm_bytes.len());
            println!();

            let request = DeployApplicationRequest {
                application_id: format!("{}-{}", name, version),
                name: name.clone(),
                version: version.clone(),
                wasm_module: Some(WasmModule {
                    name: name.clone(),
                    version: version.clone(),
                    module_bytes: wasm_bytes.clone(),
                    module_hash: String::new(), // Will be computed by server
                    wit_interface: String::new(), // Optional, empty for now
                    source_languages: vec!["typescript".to_string()],
                    metadata: None,
                    created_at: None,
                    size_bytes: wasm_bytes.len() as u64,
                    version_number: 1,
                }),
                config: None,
                release_config: None,
                initial_state: Vec::new(),
            };

            let response = client
                .deploy_application(request)
                .await?;

            let deploy_response = response.into_inner();
            if deploy_response.success {
                println!("✅ Application deployed successfully!");
                println!("   Application ID: {}", deploy_response.application_id);
                println!("   Status: {:?}", deploy_response.status);
            } else {
                eprintln!("❌ Deployment failed: {:?}", deploy_response.error);
                return Err(format!("Deployment failed: {:?}", deploy_response.error).into());
            }
        }
        Commands::Build { source, output } => {
            println!("╔════════════════════════════════════════════════════════════════╗");
            println!("║     N-Body WASM - Build TypeScript to WASM                   ║");
            println!("╚════════════════════════════════════════════════════════════════╝");
            println!();

            println!("Building TypeScript actors to WASM...");
            println!("  Source: {:?}", source);
            println!("  Output: {:?}", output);
            println!();

            // Ensure output directory exists
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }

            // Build TypeScript to JavaScript
            println!("Step 1: Compiling TypeScript to JavaScript...");
            let build_status = std::process::Command::new("npm")
                .arg("run")
                .arg("build")
                .current_dir(&source)
                .status()?;

            if !build_status.success() {
                return Err("TypeScript compilation failed".into());
            }
            println!("✅ TypeScript compiled");
            println!();

            // Build JavaScript to WASM using Javy
            println!("Step 2: Compiling JavaScript to WASM (Javy)...");
            let js_file = source.join("dist").join("body.js");
            if !js_file.exists() {
                return Err(format!("JavaScript file not found: {:?}", js_file).into());
            }

            let wasm_status = std::process::Command::new("javy")
                .arg("build")
                .arg(&js_file)
                .arg("-o")
                .arg(&output)
                .status()?;

            if !wasm_status.success() {
                return Err("WASM compilation failed. Make sure Javy is installed and in PATH: ~/.local/bin/javy".into());
            }
            println!("✅ WASM module created: {:?}", output);
            println!();

            // Show file size
            let wasm_bytes = std::fs::read(&output)?;
            println!("WASM Module Info:");
            println!("  Size: {} bytes ({:.2} KB)", 
                wasm_bytes.len(), 
                wasm_bytes.len() as f64 / 1024.0);
            println!("  Path: {:?}", output);
            println!();
            println!("✅ Build complete!");
        }
    }

    Ok(())
}

