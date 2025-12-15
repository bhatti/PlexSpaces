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
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel);

    println!("📦 Deploying application: {}", name);

    // Load WASM module if provided
    let wasm_module = if let Some(wasm_path) = wasm_file {
        let wasm_bytes = fs::read(wasm_path)
            .with_context(|| format!("Failed to read WASM file: {}", wasm_path))?;

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
        let release_str = fs::read_to_string(release_path)
            .with_context(|| format!("Failed to read release config file: {}", release_path))?;
        
        // TODO: Parse TOML to ReleaseSpec
        // For now, return error
        anyhow::bail!("Release config parsing not yet implemented");
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

/// Undeploy an application (graceful shutdown)
pub async fn undeploy(node_addr: &str, app_id: &str) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel);

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
        anyhow::bail!("Undeployment failed: {:?}", response.error);
    }

    println!("✅ Application undeployed: {}", app_id);

    Ok(())
}

/// List deployed applications
pub async fn list(node_addr: &str) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut client = ApplicationServiceClient::new(channel);

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

