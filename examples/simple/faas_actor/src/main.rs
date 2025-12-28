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

//! FaaS Actor Example
//!
//! Demonstrates FaaS-style actor invocation via HTTP GET/POST requests.
//! This example:
//! 1. Creates a counter actor
//! 2. Spawns it on a node
//! 3. Uses HTTP client to invoke the actor via InvokeActor RPC
//!    - POST /api/v1/actors/{tenant_id}/counter to increment (tell pattern)
//!    - GET /api/v1/actors/{tenant_id}/counter to get count (ask pattern)

use plexspaces_actor::ActorBuilder;
use plexspaces_behavior::GenServer;
use plexspaces_core::{ActorContext, BehaviorError, BehaviorType, Actor as ActorTrait, ActorRegistry};
use plexspaces_mailbox::{Message, mailbox_config_default};
use plexspaces_node::NodeBuilder;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, error};
use base64::{Engine as _, engine::general_purpose};
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};

/// Counter actor that handles increment (POST) and get (GET) operations
struct CounterActor {
    count: i64,
}

impl CounterActor {
    fn new() -> Self {
        Self { count: 0 }
    }
}

#[async_trait::async_trait]
impl ActorTrait for CounterActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for CounterActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse payload as JSON to get action
        let payload_str = String::from_utf8_lossy(&msg.payload);
        let action: Value = serde_json::from_str(&payload_str)
            .unwrap_or_else(|_| {
                // If not JSON, try to parse as simple string
                serde_json::json!({ "action": payload_str })
            });
        
        let action_str = action.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("get");
        
        match action_str {
            "increment" => {
                self.count += 1;
                info!("Counter incremented to: {}", self.count);
                let reply = serde_json::json!({ "count": self.count, "action": "incremented" });
                let reply_msg = Message::new(serde_json::to_vec(&reply).unwrap());
                if let Some(sender_id) = &msg.sender {
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await.map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            "get" => {
                info!("Counter get: {}", self.count);
                let reply = serde_json::json!({ "count": self.count, "action": "get" });
                let reply_msg = Message::new(serde_json::to_vec(&reply).unwrap());
                if let Some(sender_id) = &msg.sender {
                    ctx.send_reply(
                        msg.correlation_id.as_deref(),
                        sender_id,
                        msg.receiver.clone(),
                        reply_msg,
                    ).await.map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                }
            }
            _ => {
                // For tell (POST), just update state if value provided
                if let Some(value) = action.get("value").and_then(|v| v.as_i64()) {
                    self.count = value;
                    info!("Counter set to: {}", self.count);
                }
            }
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info,plexspaces=debug")
        .init();

    info!("🚀 Starting FaaS Actor Example");

    // Disable blob service to avoid sqlx driver requirement
    std::env::set_var("BLOB_ENABLED", "false");
    
    // Set JWT secret for authentication (if not already set)
    let jwt_secret = std::env::var("PLEXSPACES_JWT_SECRET")
        .unwrap_or_else(|_| "faas-actor-example-secret".to_string());
    std::env::set_var("PLEXSPACES_JWT_SECRET", &jwt_secret);

    // Create node with in-memory backends (for testing)
    // Use port 8001 to avoid conflict with MinIO (which uses 9000/9001)
    let node = NodeBuilder::new("counter-node")
        .with_listen_address("0.0.0.0:8001")
        .with_in_memory_backends()
        .build();

    // Start node in background task (start() blocks running the gRPC server)
    let node_arc = std::sync::Arc::new(node);
    let node_for_server = node_arc.clone();
    let start_handle = tokio::spawn(async move {
        if let Err(e) = node_for_server.start().await {
            eprintln!("Node start error: {}", e);
        }
    });
    
    // Wait for node to be ready and services to be registered
    // Services are registered in start(), so we need to wait a bit
    tokio::time::sleep(Duration::from_millis(1000)).await;
    info!("✅ Node started on port 8001 (Startup complete)");
    
    // Register ActorFactory (needed for spawn_actor to work)
    // ActorFactory is normally registered in create_actor_context_arc, but for examples we register it here
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    let service_locator = node_arc.service_locator();
    let actor_factory_impl = Arc::new(ActorFactoryImpl::new(service_locator.clone()));
    service_locator.register_service(actor_factory_impl.clone()).await;

    // Create and spawn counter actor with type information
    let actor_id = "counter-1@counter-node".to_string();
    let behavior: Box<dyn ActorTrait> = Box::new(CounterActor::new());
    
    // Build actor using ActorBuilder with mailbox config
    let mailbox_config = mailbox_config_default();
    let actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .with_mailbox_config(mailbox_config)
        .with_namespace("default".to_string())
        .build()
        .await;
    
    // Spawn actor with type information using spawn_actor
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory_impl.spawn_actor(
        &ctx,
        &actor_id,
        "counter",  // actor_type
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await.map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    info!("✅ Counter actor spawned: {}", actor_id);
    
    // Wait a bit for actor to be fully registered
    tokio::time::sleep(Duration::from_millis(500)).await;
    
    // Verify registration by checking discover_actors_by_type
    let actor_registry: Arc<ActorRegistry> = service_locator.get_service().await
        .ok_or("ActorRegistry not found in ServiceLocator")?;
    
    let discovered = actor_registry.discover_actors_by_type("default", "default", "counter").await;
    if discovered.contains(&actor_id) {
        info!("✅ Verified: Actor found via discover_actors_by_type");
        info!("✅ Counter actor registered with type");
    } else {
        error!("❌ ERROR: Actor NOT found via discover_actors_by_type! Found: {:?}", discovered);
        error!("   Actor ID: {}", actor_id);
        return Err("Actor not found in type index".into());
    }

    // Wait for actor to be ready
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Get HTTP gateway endpoint (gRPC port + 1, following demo pattern)
    // Using port 8001 for gRPC to avoid conflict with MinIO (9000/9001)
    let grpc_port = 8001;
    let http_port = grpc_port + 1;  // HTTP gateway on 8002
    let http_endpoint = format!("http://127.0.0.1:{}", http_port);
    
    // Wait for HTTP gateway server to be ready by checking if port is listening
    info!("⏳ Waiting for HTTP gateway server to be ready on port {}...", http_port);
    let max_retries = 30;
    let mut server_ready = false;
    for i in 0..max_retries {
        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", http_port)).await {
            Ok(_) => {
                server_ready = true;
                info!("✅ HTTP gateway server is ready!");
                break;
            }
            Err(_) => {
                if i < max_retries - 1 {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        }
    }
    
    if !server_ready {
        error!("❌ HTTP gateway server did not become ready after {} retries", max_retries);
        return Err("HTTP gateway server not ready".into());
    }
    
    info!("📡 Testing InvokeActor via HTTP at: {}", http_endpoint);
    info!("   GET  /api/v1/actors/default/default/counter?action=get - Get counter value");
    info!("   POST /api/v1/actors/default/default/counter - Increment counter");

    // Create JWT token for authentication
    #[derive(Serialize, Deserialize)]
    struct JwtClaims {
        sub: String,
        exp: i64,
        iat: i64,
        tenant_id: String,
        roles: Vec<String>,
    }
    
    let claims = JwtClaims {
        sub: "faas-example-user".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
        iat: chrono::Utc::now().timestamp(),
        tenant_id: "default".to_string(),
        roles: vec!["user".to_string()],
    };
    
    let header = Header::new(Algorithm::HS256);
    let key = EncodingKey::from_secret(jwt_secret.as_bytes());
    let jwt_token = encode(&header, &claims, &key)
        .expect("Failed to create JWT token");
    
    info!("✅ Created JWT token for authentication");

    // Test 1: GET - Get initial counter value (should be 0)
    info!("\n📥 Test 1: GET counter (should return 0)");
    let client = reqwest::Client::new();
    let get_url = format!("{}/api/v1/actors/default/default/counter?action=get", http_endpoint);
    
    match client
        .get(&get_url)
        .header("Authorization", format!("Bearer {}", jwt_token))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                let json: Value = response.json().await?;
                info!("✅ GET Response: {}", json);
                // Parse payload: HTTP gateway returns payload as parsed JSON value
                let count = json.get("payload")
                    .and_then(|p| {
                        // Payload is already a JSON value (not a string)
                        if p.is_object() {
                            // If it's already an object, get count directly
                            p.get("count").and_then(|c| c.as_i64())
                        } else if let Some(payload_str) = p.as_str() {
                            // If it's a string, try to parse it as JSON
                            serde_json::from_str::<Value>(payload_str).ok()
                                .and_then(|v| v.get("count").and_then(|c| c.as_i64()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                info!("   Counter value: {}", count);
                assert_eq!(count, 0, "Initial counter should be 0");
            } else {
                error!("❌ GET failed with status: {}", response.status());
                let text = response.text().await?;
                error!("   Response: {}", text);
            }
        }
        Err(e) => {
            error!("❌ GET request failed: {}", e);
            info!("   Note: This is expected if gRPC gateway is not running");
            info!("   The actor is still spawned and can be invoked via gRPC directly");
        }
    }

    // Test 2: POST - Increment counter
    info!("\n📤 Test 2: POST increment counter");
    let post_url = format!("{}/api/v1/actors/default/default/counter", http_endpoint);
    let increment_payload = serde_json::json!({ "action": "increment" });
    
    match client
        .post(&post_url)
        .header("Authorization", format!("Bearer {}", jwt_token))
        .json(&increment_payload)
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                let json: Value = response.json().await?;
                info!("✅ POST Response: {}", json);
            } else {
                error!("❌ POST failed with status: {}", response.status());
                let text = response.text().await?;
                error!("   Response: {}", text);
            }
        }
        Err(e) => {
            error!("❌ POST request failed: {}", e);
            info!("   Note: This is expected if gRPC gateway is not running");
        }
    }

    // Wait a bit for POST to complete processing (tell is fire-and-forget, so we need to wait for actor to process)
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test 3: GET - Get counter value again (should be 1)
    info!("\n📥 Test 3: GET counter again (should return 1)");
    match client
        .get(&get_url)
        .header("Authorization", format!("Bearer {}", jwt_token))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                let json: Value = response.json().await?;
                info!("✅ GET Response: {}", json);
                // Parse payload: HTTP gateway returns payload as parsed JSON value
                let count = json.get("payload")
                    .and_then(|p| {
                        // Payload is already a JSON value (not a string)
                        if p.is_object() {
                            // If it's already an object, get count directly
                            p.get("count").and_then(|c| c.as_i64())
                        } else if let Some(payload_str) = p.as_str() {
                            // If it's a string, try to parse it as JSON
                            serde_json::from_str::<Value>(payload_str).ok()
                                .and_then(|v| v.get("count").and_then(|c| c.as_i64()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                info!("   Counter value: {}", count);
                assert_eq!(count, 1, "Counter should be 1 after increment");
            } else {
                error!("❌ GET failed with status: {}", response.status());
            }
        }
        Err(e) => {
            error!("❌ GET request failed: {}", e);
        }
    }

    // Test 4: Multiple increments via POST
    info!("\n📤 Test 4: POST increment 3 more times");
    for i in 0..3 {
        match client
            .post(&post_url)
            .header("Authorization", format!("Bearer {}", jwt_token))
            .json(&increment_payload)
            .send()
            .await
        {
            Ok(response) => {
                if response.status().is_success() {
                    info!("✅ Increment {} successful", i + 1);
                } else {
                    error!("❌ Increment {} failed: {}", i + 1, response.status());
                }
            }
            Err(e) => {
                error!("❌ Increment {} request failed: {}", i + 1, e);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Wait a bit for all POSTs to complete processing
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Test 5: Final GET (should be 4)
    info!("\n📥 Test 5: GET final counter value (should return 4)");
    match client
        .get(&get_url)
        .header("Authorization", format!("Bearer {}", jwt_token))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                let json: Value = response.json().await?;
                info!("✅ GET Response: {}", json);
                // Parse payload: HTTP gateway returns payload as parsed JSON value
                let count = json.get("payload")
                    .and_then(|p| {
                        // Payload is already a JSON value (not a string)
                        if p.is_object() {
                            // If it's already an object, get count directly
                            p.get("count").and_then(|c| c.as_i64())
                        } else if let Some(payload_str) = p.as_str() {
                            // If it's a string, try to parse it as JSON
                            serde_json::from_str::<Value>(payload_str).ok()
                                .and_then(|v| v.get("count").and_then(|c| c.as_i64()))
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                info!("   Final counter value: {}", count);
                assert_eq!(count, 4, "Counter should be 4 after 4 increments");
            } else {
                error!("❌ GET failed with status: {}", response.status());
            }
        }
        Err(e) => {
            error!("❌ GET request failed: {}", e);
        }
    }

    info!("\n✅ InvokeActor Counter Example completed!");
    info!("   The actor can be invoked via:");
    info!("   - GET  /api/v1/actors/default/default/counter?action=get");
    info!("   - POST /api/v1/actors/default/default/counter with body: {{\"action\":\"increment\"}}");

    // Keep node running for a bit
    tokio::time::sleep(Duration::from_secs(1)).await;
    
    // Note: start_handle will keep running in background
    // In a real application, you'd want to handle shutdown gracefully

    Ok(())
}
