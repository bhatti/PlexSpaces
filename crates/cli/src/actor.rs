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

//! Actor deployment and management commands
//!
//! ## Purpose
//! Simple actor deployment workflow (like AWS Lambda):
//! - `deploy` - Deploy an actor (handles WASM module deployment + actor instantiation)
//! - `invoke` - Send message to actor
//! - `list` - List deployed actors

use anyhow::{Context, Result};
use plexspaces_proto::wasm::v1::{
    wasm_runtime_service_client::WasmRuntimeServiceClient,
    DeployWasmModuleRequest, InstantiateActorRequest, WasmModule,
};
use plexspaces_proto::v1::actor::actor_service_client::ActorServiceClient;
use std::fs;
use tonic::transport::Channel;

/// Deploy an actor (like AWS Lambda deploy)
///
/// Handles:
/// 1. Deploy WASM module (if WASM actor)
/// 2. Instantiate actor
/// 3. Register actor with node
pub async fn deploy(
    node_addr: &str,
    name: &str,
    wasm_file: Option<&str>,
    actor_type: &str,
    initial_state: Option<&str>,
) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut wasm_client = WasmRuntimeServiceClient::new(channel.clone());

    // For WASM actors, deploy module first
    let module_ref = if actor_type == "wasm" || actor_type == "rust" || actor_type == "js" || actor_type == "go" || actor_type == "python" {
        let wasm_path = wasm_file.context("WASM file required for WASM actors")?;
        let wasm_bytes = fs::read(wasm_path)
            .with_context(|| format!("Failed to read WASM file: {}", wasm_path))?;

        println!("📦 Deploying WASM module: {}", name);

        // Deploy module
        let module = WasmModule {
            name: name.to_string(),
            version: "1.0.0".to_string(),
            module_bytes: wasm_bytes,
            module_hash: String::new(), // Will be computed by server
            ..Default::default()
        };

        let request = DeployWasmModuleRequest {
            module: Some(module),
            pre_warm: 0, // NONE
            target_node_tags: vec![],
        };

        let response = wasm_client
            .deploy_module(tonic::Request::new(request))
            .await
            .context("Failed to deploy WASM module")?
            .into_inner();

        if !response.success {
            anyhow::bail!("Deployment failed: {:?}", response.error);
        }

        println!("✅ Module deployed: {}", response.module_hash);
        format!("{}@1.0.0", name)
    } else {
        // For non-WASM actors, use name directly
        name.to_string()
    };

    // Instantiate actor
    println!("🎭 Instantiating actor: {}", name);

    let initial_state_bytes = initial_state
        .map(|s| s.as_bytes().to_vec())
        .unwrap_or_default();

    let request = InstantiateActorRequest {
        module_ref: module_ref.clone(),
        actor_id: name.to_string(),
        initial_state: initial_state_bytes,
        config: None,
        target_node_id: String::new(), // Optional - empty means current node
    };

    let response = wasm_client
        .instantiate_actor(tonic::Request::new(request))
        .await
        .context("Failed to instantiate actor")?
        .into_inner();

    if !response.success {
        anyhow::bail!("Instantiation failed: {:?}", response.error);
    }

    println!("✅ Actor deployed: {}", name);
    println!("   Module: {}", module_ref);
    println!("   Actor ID: {}", response.actor_id);

    Ok(())
}

/// Invoke an actor (like AWS Lambda invoke)
pub async fn invoke(node_addr: &str, actor_id: &str, payload: &str) -> Result<()> {
    let channel = Channel::from_shared(format!("http://{}", node_addr))
        .context("Invalid node address")?
        .connect()
        .await
        .context("Failed to connect to node")?;

    let mut actor_client = ActorServiceClient::new(channel);

    println!("📨 Sending message to actor: {}", actor_id);

    use plexspaces_proto::v1::actor::{SendMessageRequest, Message};
    let mut msg = Message::default();
    msg.receiver_id = actor_id.to_string();
    msg.message_type = "application/json".to_string();
    msg.payload = payload.as_bytes().to_vec();
    
    let request = SendMessageRequest {
        message: Some(msg),
        wait_for_response: false,
        ..Default::default()
    };

    let _response = actor_client
        .send_message(tonic::Request::new(request))
        .await
        .context("Failed to send message")?;

    println!("✅ Message sent successfully");

    Ok(())
}

/// List actors on a node
pub async fn list(node_addr: &str) -> Result<()> {
    // TODO: Implement actor listing
    println!("📋 Listing actors on node: {}", node_addr);
    println!("   (Not yet implemented)");
    Ok(())
}

