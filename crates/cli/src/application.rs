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

//! Application deployment commands (like AWS Lambda)
//!
//! ## Purpose
//! Simple application deployment workflow:
//! - `deploy` - Deploy an application (handles WASM module + supervisor tree + config)
//! - `undeploy` - Gracefully shutdown application
//! - `list` - List deployed applications

use anyhow::{Context, Result};
use plexspaces_proto::application::v1::{
    application_service_client::ApplicationServiceClient,
    DeployApplicationRequest, UndeployApplicationRequest, ListApplicationsRequest,
    ApplicationSpec, ApplicationType,
};
use plexspaces_proto::wasm::v1::WasmModule;
use std::fs;
use tonic::transport::Channel;
use reqwest::multipart;

/// Deploy an application (like AWS Lambda deploy)
///
/// Handles:
/// 1. Deploy WASM module (if WASM application)
/// 2. Parse application config
/// 3. Initialize supervisor tree
/// 4. Register and start application
pub async fn deploy(
    node_addr: &str,
    app_id: &str,
    name: &str,
    version: &str,
    wasm_file: Option<&str>,
    config_file: Option<&str>,
    release_config_file: Option<&str>,
) -> Result<()> {
    // Set max message size to 5MB for gRPC (matches server setting)
    // Note: For large WASM files (>5MB), use HTTP multipart endpoint instead
    const GRPC_MAX_MESSAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB
    
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel)
        .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
        .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE);

    // OBSERVABILITY: Log deployment attempt
    tracing::info!(
        application_id = %app_id,
        application_name = %name,
        version = %version,
        node_addr = %node_addr,
        has_wasm_file = wasm_file.is_some(),
        has_config_file = config_file.is_some(),
        has_release_config_file = release_config_file.is_some(),
        "Deploying application via CLI"
    );
    println!("📦 Deploying application: {}", name);

    // Load WASM module if provided
    let wasm_module = if let Some(wasm_path) = wasm_file {
        let wasm_bytes = fs::read(wasm_path)
            .with_context(|| format!("Failed to read WASM file: {}", wasm_path))?;
        
        let file_size = wasm_bytes.len();
        const GRPC_MAX_SIZE: usize = 5 * 1024 * 1024; // 5MB
        const HTTP_MAX_SIZE: usize = 100 * 1024 * 1024; // 100MB
        
        // Check if file exceeds gRPC limit
        if file_size > GRPC_MAX_SIZE {
            if file_size > HTTP_MAX_SIZE {
                anyhow::bail!(
                    "WASM file size {} bytes exceeds maximum {} bytes. \
                    Please optimize the file with wasm-opt or split into smaller modules.",
                    file_size, HTTP_MAX_SIZE
                );
            }
            
            // File is >5MB but <=100MB - use HTTP multipart upload
            println!("⚠️  WASM file size ({:.2}MB) exceeds gRPC limit (5MB), using HTTP multipart upload", 
                file_size as f64 / (1024.0 * 1024.0));
            
            return deploy_via_http_multipart(
                node_addr,
                app_id,
                name,
                version,
                wasm_path,
                config_file,
                release_config_file,
            ).await;
        }

        Some(WasmModule {
            name: name.to_string(),
            version: version.to_string(),
            module_bytes: wasm_bytes,
            module_hash: String::new(), // Will be computed by server
            ..Default::default()
        })
    } else {
        None
    };

    // Load application config if provided
    let app_config = if let Some(config_path) = config_file {
        let config_str = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path))?;
        
        // TODO: Parse TOML/JSON to ApplicationSpec
        // For now, create minimal config
        Some(ApplicationSpec {
            name: name.to_string(),
            version: version.to_string(),
            description: format!("Application {}", name),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: None,
        })
    } else if wasm_module.is_none() {
        // Config required if not WASM
        anyhow::bail!("Either wasm_file or config_file must be provided");
    } else {
        // For WASM apps, create minimal config
        Some(ApplicationSpec {
            name: name.to_string(),
            version: version.to_string(),
            description: format!("WASM application {}", name),
            r#type: ApplicationType::ApplicationTypeActive.into(),
            dependencies: vec![],
            env: std::collections::HashMap::new(),
            supervisor: None,
        })
    };

    // Load release config if provided
    let release_config = if let Some(release_path) = release_config_file {
        use plexspaces_node::config_loader::ConfigLoader;
        
        // Determine file type from extension
        let path = std::path::Path::new(release_path);
        let is_yaml = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext == "yaml" || ext == "yml")
            .unwrap_or(false);
        
        if is_yaml {
            // Load YAML release config
            let loader = ConfigLoader::new(); // Enable security validation
              let spec = loader.load_release_spec_with_env_precedence(release_path).await
                .map_err(|e| anyhow::anyhow!("Failed to load release config from {}: {}", release_path, e))?;
            Some(spec)
        } else {
            // Try TOML (for future support)
            anyhow::bail!("TOML release config parsing not yet implemented, use YAML format");
        }
    } else {
        None
    };

    let request = DeployApplicationRequest {
        application_id: app_id.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        wasm_module,
        config: app_config,
        release_config,
        initial_state: vec![],
    };

    let response = client
        .deploy_application(tonic::Request::new(request))
        .await
        .context("Failed to deploy application")?
        .into_inner();

    if !response.success {
        anyhow::bail!("Deployment failed: {:?}", response.error);
    }

    println!("✅ Application deployed: {}", name);
    println!("   Application ID: {}", response.application_id);
    println!("   Status: {:?}", response.status);

    Ok(())
}

/// Deploy application via HTTP multipart upload (for large files >5MB, up to 100MB)
async fn deploy_via_http_multipart(
    node_addr: &str,
    app_id: &str,
    name: &str,
    version: &str,
    wasm_path: &str,
    config_file: Option<&str>,
    _release_config_file: Option<&str>,
) -> Result<()> {
    // HTTP gateway runs on gRPC port + 1
    // Parse port from node_addr (format: host:port)
    let http_port = if let Some(colon_pos) = node_addr.rfind(':') {
        let port_str = &node_addr[colon_pos + 1..];
        if let Ok(port) = port_str.parse::<u16>() {
            port + 1
        } else {
            anyhow::bail!("Invalid port in node address: {}", node_addr);
        }
    } else {
        anyhow::bail!("Node address must include port: {}", node_addr);
    };
    
    let host = if let Some(colon_pos) = node_addr.rfind(':') {
        &node_addr[..colon_pos]
    } else {
        node_addr
    };
    
    let http_url = format!("http://{}:{}", host, http_port);
    
    // OBSERVABILITY: Log HTTP multipart deployment
    tracing::info!(
        application_id = %app_id,
        application_name = %name,
        version = %version,
        node_addr = %node_addr,
        http_url = %http_url,
        wasm_path = %wasm_path,
        wasm_size_bytes = fs::metadata(wasm_path).ok().map(|m| m.len()).unwrap_or(0),
        "Deploying application via HTTP multipart"
    );
    println!("📤 Uploading via HTTP multipart (supports files up to 100MB)");
    
    // Read WASM file
    let wasm_bytes = fs::read(wasm_path)
        .with_context(|| format!("Failed to read WASM file: {}", wasm_path))?;
    
    // Verify file size is within 100MB limit
    const HTTP_MAX_SIZE: usize = 100 * 1024 * 1024; // 100MB
    if wasm_bytes.len() > HTTP_MAX_SIZE {
        anyhow::bail!(
            "WASM file size {} bytes exceeds maximum {} bytes. \
            Please optimize the file with wasm-opt or split into smaller modules.",
            wasm_bytes.len(), HTTP_MAX_SIZE
        );
    }
    
    // Create multipart form
    let mut form = multipart::Form::new()
        .text("application_id", app_id.to_string())
        .text("name", name.to_string())
        .text("version", version.to_string())
        .part("wasm_file",
            multipart::Part::bytes(wasm_bytes)
                .file_name(std::path::Path::new(wasm_path).file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("actor.wasm")
                    .to_string())
                .mime_str("application/wasm")
                .context("Failed to set MIME type")?
        );
    
    // Add config file if provided
    if let Some(config_path) = config_file {
        let config_str = fs::read_to_string(config_path)
            .with_context(|| format!("Failed to read config file: {}", config_path))?;
        form = form.text("config", config_str);
    }
    
    // Send HTTP request
    let client = reqwest::Client::new();
    let response = client
        .post(&format!("{}/api/v1/applications/deploy", http_url))
        .multipart(form)
        .send()
        .await
        .context("Failed to send HTTP request")?;
    
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
        anyhow::bail!("HTTP deployment failed ({}): {}", status, error_text);
    }
    
    let json: serde_json::Value = response.json().await
        .context("Failed to parse JSON response")?;
    
    if json.get("success").and_then(|v| v.as_bool()).unwrap_or(false) {
        // OBSERVABILITY: Log successful HTTP deployment
        tracing::info!(
            application_id = %app_id,
            application_name = %name,
            version = %version,
            "Application deployed successfully via HTTP multipart"
        );
        println!("✅ Application deployed: {}", name);
        if let Some(app_id) = json.get("application_id").and_then(|v| v.as_str()) {
            println!("   Application ID: {}", app_id);
        }
        if let Some(status) = json.get("status").and_then(|v| v.as_str()) {
            println!("   Status: {}", status);
        }
        Ok(())
    } else {
        let error = json.get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown error");
        // OBSERVABILITY: Log failed HTTP deployment
        tracing::error!(
            application_id = %app_id,
            application_name = %name,
            error = %error,
            "Application deployment failed via HTTP multipart"
        );
        anyhow::bail!("Deployment failed: {}", error);
    }
}

/// Undeploy an application (graceful shutdown)
pub async fn undeploy(node_addr: &str, app_id: &str) -> Result<()> {
    // Set max message size to 5MB for gRPC (matches server setting)
    // Note: For large WASM files (>5MB), use HTTP multipart endpoint instead
    const GRPC_MAX_MESSAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB
    
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel)
        .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
        .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE);

    // OBSERVABILITY: Log undeployment attempt
    tracing::info!(
        application_id = %app_id,
        node_addr = %node_addr,
        "Undeploying application via CLI"
    );
    println!("🛑 Undeploying application: {}", app_id);

    let request = UndeployApplicationRequest {
        application_id: app_id.to_string(),
        timeout: None, // Use default timeout
    };

    let response = client
        .undeploy_application(tonic::Request::new(request))
        .await
        .context("Failed to undeploy application")?
        .into_inner();

    if !response.success {
        // OBSERVABILITY: Log failed undeployment
        tracing::error!(
            application_id = %app_id,
            error = ?response.error,
            "Application undeployment failed via CLI"
        );
        anyhow::bail!("Undeployment failed: {:?}", response.error);
    }

    // OBSERVABILITY: Log successful undeployment
    tracing::info!(
        application_id = %app_id,
        "Application undeployed successfully via CLI"
    );
    println!("✅ Application undeployed: {}", app_id);

    Ok(())
}

/// List deployed applications
pub async fn list(node_addr: &str) -> Result<()> {
    // Set max message size to 5MB for gRPC (matches server setting)
    // Note: For large WASM files (>5MB), use HTTP multipart endpoint instead
    const GRPC_MAX_MESSAGE_SIZE: usize = 5 * 1024 * 1024; // 5MB
    
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel)
        .max_decoding_message_size(GRPC_MAX_MESSAGE_SIZE)
        .max_encoding_message_size(GRPC_MAX_MESSAGE_SIZE);

    println!("📋 Listing applications on node: {}", node_addr);

    let request = ListApplicationsRequest {
        status_filter: None,
    };

    let response = client
        .list_applications(tonic::Request::new(request))
        .await
        .context("Failed to list applications")?
        .into_inner();

    if response.applications.is_empty() {
        println!("   No applications deployed");
    } else {
        for app in response.applications {
            println!("   - {} (v{}) - Status: {:?}", app.name, app.version, app.status);
        }
    }

    Ok(())
}

