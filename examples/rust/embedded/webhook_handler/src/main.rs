// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! Webhook Handler Example (FaaS-style HTTP actor) - SDK Annotations Demo
//!
//! Demonstrates **PlexSpaces Rust SDK annotations** (like Python @actor, @handler):
//! - `#[gen_server_actor]` - marks struct as GenServer actor
//! - `#[plexspaces_handlers]` - scans impl for #[handler] methods
//! - `#[handler("op", call)]` - routes message to handler method
//!
//! ## HTTP Endpoints
//! - **POST** /api/v1/actors/{namespace}/webhook_handler — deliver a webhook
//! - **GET**  /api/v1/actors/{namespace}/webhook_handler?action=list — list recent deliveries
//!
//! This embedded example disables auth at node startup so the local HTTP
//! self-test can exercise the gateway without external JWT or mTLS setup.
//! When auth is disabled, the HTTP gateway derives tenant context from the
//! `x-tenant-id` header, so this example sends that header explicitly.
//!
//! ## Before (manual boilerplate)
//! ```ignore
//! impl ActorTrait for WebhookHandler { fn behavior_type()...; fn handle_message()... }
//! impl GenServer for WebhookHandler { fn handle_request()... }
//! ```
//!
//! ## After (SDK annotations)
//! ```ignore
//! #[gen_server_actor]
//! struct WebhookHandler { ... }
//!
//! #[plexspaces_handlers]
//! impl WebhookHandler {
//!     #[handler("deliver")]  // GenServer defaults to call (no second param needed)
//!     async fn deliver(...) { ... }
//! }
//! ```

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message, NodeBuilder, RequestContext,
    spawn, json, Value,, RequestContextExt};
use std::time::Duration;
use tracing::{error, info};

// -----------------------------------------------------------------------------
// Webhook delivery (stored in actor state)
// -----------------------------------------------------------------------------

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct WebhookDelivery {
    id: String,
    received_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload_preview: Option<String>,
}

impl WebhookDelivery {
    fn new(id: String, payload: &[u8]) -> Self {
        let received_at = chrono::Utc::now().to_rfc3339();
        let payload_preview = String::from_utf8(payload.to_vec())
            .ok()
            .map(|s| {
                if s.len() > 200 {
                    format!("{}...", &s[..200])
                } else {
                    s
                }
            });
        Self {
            id,
            received_at,
            payload_preview,
        }
    }
}

// -----------------------------------------------------------------------------
// Webhook Handler Actor - SDK style with annotations (like Python @gen_server_actor)
// -----------------------------------------------------------------------------

/// Webhook handler actor using SDK annotations.
/// 
/// ## Annotations
/// - `#[gen_server_actor(name = "webhook_handler")]` - generates `impl Actor` with Custom("webhook_handler") type
///   This allows HTTP gateway to route requests to `/api/v1/actors/{namespace}/webhook_handler`
/// - `#[plexspaces_handlers]` - generates `impl GenServer` dispatching to `#[handler]` methods
#[gen_server_actor(name = "webhook_handler")]
struct WebhookHandlerActor {
    deliveries: Vec<WebhookDelivery>,
    max_deliveries: usize,
    total_count: u64,
}

impl WebhookHandlerActor {
    fn new() -> Self {
        Self {
            deliveries: Vec::new(),
            max_deliveries: 100,
            total_count: 0,
        }
    }
}

/// Handler implementations - SDK scans these and generates GenServer dispatch.
/// 
/// Each `#[handler("op")]` method is called when `payload.action == "op"`.
/// For GenServer, all handlers default to "call" (request-reply) - no second param needed.
/// Return `Result<Value, BehaviorError>` - SDK serializes and sends reply automatically.
#[plexspaces_handlers]
impl WebhookHandlerActor {
    /// Handle "list" action - returns list of recent deliveries
    #[handler("list")]
    async fn list(&mut self, _ctx: &ActorContext, _msg: &Message) -> Result<Value, BehaviorError> {
        let total = self.total_count;
        let list: Vec<WebhookDelivery> = self.deliveries.clone();
        Ok(json!({
            "deliveries": list,
            "total": total,
            "action": "list"
        }))
    }

    /// Handle "deliver" action - stores webhook and returns delivery info
    #[handler("deliver")]
    async fn deliver(&mut self, _ctx: &ActorContext, msg: &Message) -> Result<Value, BehaviorError> {
        let id = ulid::Ulid::new().to_string();
        let delivery = WebhookDelivery::new(id.clone(), &msg.payload);
        self.total_count += 1;
        self.deliveries.push(delivery.clone());
        if self.deliveries.len() > self.max_deliveries {
            self.deliveries.remove(0);
        }
        info!(
            "Webhook delivered id={} total={}",
            id, self.total_count
        );
        Ok(json!({
            "id": delivery.id,
            "received_at": delivery.received_at,
            "action": "delivered"
        }))
    }
}

// -----------------------------------------------------------------------------
// Main: node + spawn actor + HTTP self-test
// -----------------------------------------------------------------------------

const TENANT_ID: &str = "acme-corp";
const NAMESPACE: &str = "webhooks";
const GRPC_PORT: u16 = 8001;
const HTTP_PORT: u16 = 8002;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter("info,plexspaces=debug")
        .init();

    info!("🚀 Webhook Handler Example (SDK Annotations Demo)");
    info!("   Using #[gen_server_actor], #[plexspaces_handlers], #[handler]");
    info!("   POST = deliver webhook, GET ?action=list = list recent deliveries");

    std::env::set_var("BLOB_ENABLED", "false");
    std::env::set_var("PLEXSPACES_DISABLE_AUTH", "1");

    let node_arc = NodeBuilder::new("webhook-handler-node")
        .with_listen_addr(format!("0.0.0.0:{}", GRPC_PORT))
        .with_in_memory_backends()
        .with_auth_disabled()
        .build_started()
        .await;

    wait_for_port(HTTP_PORT).await?;
    info!("✅ Node listening (gRPC {} HTTP {})", GRPC_PORT, HTTP_PORT);

    let service_locator = node_arc.service_locator();
    let ctx = RequestContext::new_without_auth(TENANT_ID.to_string(), NAMESPACE.to_string());

    let actor_ref = spawn(
        &ctx,
        service_locator.clone(),
        "webhook-handler-1".to_string(),
        NAMESPACE,
        WebhookHandlerActor::new(),
    )
    .await
    .map_err(|e| format!("spawn: {}", e))?;

    info!("✅ Webhook handler actor spawned: {}", actor_ref.id());

    let http_base = format!("http://127.0.0.1:{}", HTTP_PORT);
    let url_list = format!(
        "{}/api/v1/actors/{}/webhook_handler?action=list",
        http_base, NAMESPACE
    );
    let url_deliver = format!(
        "{}/api/v1/actors/{}/webhook_handler",
        http_base, NAMESPACE
    );

    let client = reqwest::Client::new();

    info!("📥 GET list (initial, should be empty)");
    let list0 = client
        .get(&url_list)
        .header("x-tenant-id", TENANT_ID)
        .send()
        .await?;
    let list0_status = list0.status();
    if !list0_status.is_success() {
        error!("GET list failed: {}", list0_status);
        let text = list0.text().await?;
        return Err(format!("GET list: {} {}", list0_status, text).into());
    }
    let json0: Value = list0.json().await?;
    let total0 = json0
        .get("payload")
        .and_then(|p| p.get("total"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    info!("   total={}", total0);
    assert_eq!(total0, 0, "initial total should be 0");

    info!("📤 POST deliver (webhook payload)");
    // Include "action": "deliver" to route to #[handler("deliver")]
    // Use /ask to get request-reply (base POST is fire-and-forget)
    let body1 = json!({ "action": "deliver", "type": "github.push", "repo": "acme/backend", "commits": 3 });
    let post1 = client
        .post(format!("{}/ask", &url_deliver))
        .header("x-tenant-id", TENANT_ID)
        .json(&body1)
        .send()
        .await?;
    let post1_status = post1.status();
    if !post1_status.is_success() {
        error!("POST deliver failed: {}", post1_status);
        let text = post1.text().await?;
        return Err(format!("POST deliver: {} {}", post1_status, text).into());
    }
    let resp1: Value = post1.json().await?;
    let id1 = resp1
        .get("payload")
        .and_then(|p| p.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    info!("   delivered id={}", id1);
    assert!(!id1.is_empty(), "delivery should return id");

    info!("📤 POST deliver (second webhook)");
    // Include "action": "deliver" to route to #[handler("deliver")]
    let body2 = json!({ "action": "deliver", "type": "stripe.payment", "amount_cents": 9999 });
    let _post2 = client
        .post(format!("{}/ask", &url_deliver))
        .header("x-tenant-id", TENANT_ID)
        .json(&body2)
        .send()
        .await?;

    tokio::time::sleep(Duration::from_millis(200)).await;

    info!("📥 GET list (should show 2 deliveries)");
    let list2 = client
        .get(&url_list)
        .header("x-tenant-id", TENANT_ID)
        .send()
        .await?;
    if !list2.status().is_success() {
        error!("GET list (2) failed: {}", list2.status());
        return Err("GET list (2) failed".into());
    }
    let json2: Value = list2.json().await?;
    let total2 = json2
        .get("payload")
        .and_then(|p| p.get("total"))
        .and_then(|t| t.as_u64())
        .unwrap_or(0);
    let deliveries = json2
        .get("payload")
        .and_then(|p| p.get("deliveries"))
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0);
    info!("   total={} deliveries.len={}", total2, deliveries);
    assert_eq!(total2, 2, "total should be 2 after two POSTs");
    assert!(deliveries >= 1 && deliveries <= 2, "deliveries list should have 1–2 items");

    info!("✅ Webhook Handler example completed.");
    info!("   GET  {}", url_list);
    info!("   POST {} with JSON body to deliver", url_deliver);

    tokio::time::sleep(Duration::from_secs(1)).await;
    Ok(())
}

async fn wait_for_port(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let addr = format!("127.0.0.1:{}", port);
    for _ in 0..50 {
        if tokio::net::TcpStream::connect(&addr).await.is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    Err("HTTP gateway did not become ready".into())
}

fn make_jwt(tenant_id: &str, secret: &str) -> Result<String, Box<dyn std::error::Error>> {
    #[derive(serde::Serialize, serde::Deserialize)]
    struct Claims {
        sub: String,
        exp: i64,
        iat: i64,
        tenant_id: String,
        roles: Vec<String>,
    }
    let claims = Claims {
        sub: "webhook-handler-example".to_string(),
        exp: (chrono::Utc::now() + chrono::Duration::hours(1)).timestamp(),
        iat: chrono::Utc::now().timestamp(),
        tenant_id: tenant_id.to_string(),
        roles: vec!["user".to_string()],
    };
    let header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::HS256);
    let token = jsonwebtoken::encode(
        &header,
        &claims,
        &jsonwebtoken::EncodingKey::from_secret(secret.as_bytes()),
    )?;
    Ok(token)
}
