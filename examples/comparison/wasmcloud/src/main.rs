// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: wasmCloud (WASM Actors with Capability Providers)

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Reply};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// HTTP request (simulated capability provider)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpRequest {
    pub url: String,
    pub method: String,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

/// HTTP response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpResponse {
    pub status_code: u16,
    pub headers: std::collections::HashMap<String, String>,
    pub body: Vec<u8>,
}

/// Key-Value store request (simulated capability provider)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KVRequest {
    Get { key: String },
    Set { key: String, value: Vec<u8> },
    Delete { key: String },
}

/// Key-Value store response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KVResponse {
    Value { key: String, value: Option<Vec<u8>> },
    Success { key: String },
}

/// WASM Actor message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WasmActorMessage {
    ProcessRequest { request: HttpRequest },
    HttpResult { response: HttpResponse },
    GetCache { key: String },
    SetCache { key: String, value: Vec<u8> },
    CacheResult { response: KVResponse },
}

/// WASM Actor (wasmCloud-style with capability providers)
/// Demonstrates: WASM Actors, Capability Providers (HTTP, KeyValue), Polyglot Support
pub struct WasmActor {
    actor_id: String,
    cache: std::collections::HashMap<String, Vec<u8>>, // Simulated KV store
}

impl WasmActor {
    pub fn new(actor_id: String) -> Self {
        Self {
            actor_id,
            cache: std::collections::HashMap::new(),
        }
    }

    // Simulated HTTP capability provider
    async fn http_call(&self, request: &HttpRequest) -> Result<HttpResponse, Box<dyn std::error::Error + Send + Sync>> {
        info!("[HTTP CAPABILITY] {} {} (actor: {})", request.method, request.url, self.actor_id);
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Simulate HTTP response
        Ok(HttpResponse {
            status_code: 200,
            headers: std::collections::HashMap::new(),
            body: format!("Response from {}", request.url).into_bytes(),
        })
    }

    // Simulated KeyValue capability provider
    fn kv_get(&self, key: &str) -> Option<Vec<u8>> {
        self.cache.get(key).cloned()
    }

    fn kv_set(&mut self, key: String, value: Vec<u8>) {
        self.cache.insert(key, value);
    }
}

#[async_trait::async_trait]
impl Actor for WasmActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for WasmActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let wasm_msg: WasmActorMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match wasm_msg {
            WasmActorMessage::ProcessRequest { request } => {
                info!("[WASM ACTOR {}] Processing HTTP request", self.actor_id);
                
                // Use HTTP capability provider
                let response = self.http_call(&request).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("HTTP call failed: {}", e)))?;
                
                // Cache response using KV capability provider
                let cache_key = format!("cache:{}", request.url);
                self.kv_set(cache_key.clone(), response.body.clone());
                
                let reply = WasmActorMessage::HttpResult { response };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            WasmActorMessage::GetCache { key } => {
                let value = self.kv_get(&key);
                let reply = WasmActorMessage::CacheResult {
                    response: KVResponse::Value { key, value },
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            WasmActorMessage::SetCache { key, value } => {
                self.kv_set(key.clone(), value);
                let reply = WasmActorMessage::CacheResult {
                    response: KVResponse::Success { key },
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== wasmCloud vs PlexSpaces Comparison ===");
    info!("Demonstrating WASM Actors with Capability Providers");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // wasmCloud actors are WASM components with capability providers
    let actor_id: ActorId = "wasm-actor/example-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating WASM actor with capability providers");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(WasmActor::new(actor_id.clone()));
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    let actor_ref = node.spawn_actor(actor).await?;

    let mailbox = node.actor_registry()
        .lookup_mailbox(actor_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let wasm_actor = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ WASM actor created: {}", wasm_actor.id());
    info!("✅ Capability providers: HTTP, KeyValue");
    info!("✅ Polyglot support: Can run WASM components from any language");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test HTTP capability provider
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: HTTP capability provider");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let http_request = HttpRequest {
        url: "https://api.example.com/data".to_string(),
        method: "GET".to_string(),
        headers: std::collections::HashMap::new(),
        body: None,
    };
    
    let msg = Message::new(serde_json::to_vec(&WasmActorMessage::ProcessRequest {
        request: http_request.clone(),
    })?)
        .with_message_type("call".to_string());
    let result = wasm_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: WasmActorMessage = serde_json::from_slice(result.payload())?;
    if let WasmActorMessage::HttpResult { response } = reply {
        info!("✅ HTTP response: status={}, body={}", 
            response.status_code, String::from_utf8_lossy(&response.body));
    }

    // Test KeyValue capability provider
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: KeyValue capability provider (caching)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let cache_key = format!("cache:{}", http_request.url);
    let msg = Message::new(serde_json::to_vec(&WasmActorMessage::GetCache {
        key: cache_key.clone(),
    })?)
        .with_message_type("call".to_string());
    let result = wasm_actor
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: WasmActorMessage = serde_json::from_slice(result.payload())?;
    if let WasmActorMessage::CacheResult { response } = reply {
        if let KVResponse::Value { key, value } = response {
            info!("✅ Cache get: key={}, value={:?}", key, value.is_some());
        }
    }

    info!("=== Comparison Complete ===");
    info!("✅ WASM Actors: Polyglot support (can run WASM from any language)");
    info!("✅ Capability Providers: HTTP, KeyValue, and more");
    info!("✅ Secure by default: Capabilities granted explicitly");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_wasm_actor() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "wasm-actor/test-1@test-node".to_string();
        let behavior = Box::new(WasmActor::new(actor_id.clone()));
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        let actor_ref = node.spawn_actor(actor).await.unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let wasm_actor = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let request = HttpRequest {
            url: "https://test.com".to_string(),
            method: "GET".to_string(),
            headers: std::collections::HashMap::new(),
            body: None,
        };
        let msg = Message::new(serde_json::to_vec(&WasmActorMessage::ProcessRequest {
            request,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = wasm_actor
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: WasmActorMessage = serde_json::from_slice(result.payload()).unwrap();
        if let WasmActorMessage::HttpResult { response } = reply {
            assert_eq!(response.status_code, 200);
        } else {
            panic!("Expected HttpResult message");
        }
    }
}
