// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Order Processing Example - E-Commerce Order Management
//
// Real-world use case: An e-commerce platform managing customer orders
// with create, query, cancel, and list operations.
//
// ## SDK Features Demonstrated
// - `#[gen_server_actor]` - GenServer behavior for request-reply
// - `#[plexspaces_handlers(gen_server)]` - Auto-generated message dispatch
// - `#[handler("op")]` - Route messages to handler methods
// - `spawn_actor()` - Simple actor spawning with SDK helper
//
// ## What This Example Shows
// 1. Define an actor with SDK annotations (no boilerplate)
// 2. Spawn actor using simple SDK helper
// 3. Send messages and receive replies via ActorRef
// 4. Proper tenant/namespace isolation

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, spawn_actor, json, Value, ActorRef,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, Level};

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

// =============================================================================
// Domain Types - Simple and focused
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub sku: String,
    pub name: String,
    pub quantity: u32,
    pub price_cents: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub customer_id: String,
    pub items: Vec<OrderItem>,
    pub total_cents: u32,
    pub status: String,
}

// =============================================================================
// Order Processor Actor - SDK Annotations
// =============================================================================

/// Order processor actor using SDK annotations.
/// 
/// This demonstrates the simplest way to create a stateful actor:
/// 1. Add `#[gen_server_actor]` to struct
/// 2. Add `#[plexspaces_handlers(gen_server)]` to impl block
/// 3. Add `#[handler("op")]` to each handler method
#[gen_server_actor]
struct OrderProcessor {
    orders: HashMap<String, Order>,
    next_id: u32,
}

impl OrderProcessor {
    fn new() -> Self {
        Self {
            orders: HashMap::new(),
            next_id: 1,
        }
    }
    
    fn generate_id(&mut self) -> String {
        let id = format!("ORD-{:05}", self.next_id);
        self.next_id += 1;
        id
    }
}

#[plexspaces_handlers(gen_server)]
impl OrderProcessor {
    /// Create a new order
    #[handler("create")]
    async fn handle_create(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct CreateRequest {
            customer_id: String,
            items: Vec<OrderItem>,
        }
        
        let req: CreateRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        let total_cents: u32 = req.items.iter()
            .map(|i| i.price_cents * i.quantity)
            .sum();
        
        let order = Order {
            id: self.generate_id(),
            customer_id: req.customer_id,
            items: req.items,
            total_cents,
            status: "pending".to_string(),
        };
        
        let order_id = order.id.clone();
        self.orders.insert(order_id.clone(), order);
        
        info!("Created order: {}", order_id);
        
        Ok(json!({
            "order_id": order_id,
            "total_cents": total_cents,
            "status": "pending"
        }))
    }
    
    /// Get order by ID
    #[handler("get")]
    async fn handle_get(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct GetRequest { order_id: String }
        
        let req: GetRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        match self.orders.get(&req.order_id) {
            Some(order) => Ok(json!({
                "order": order
            })),
            None => Err(BehaviorError::ProcessingError(
                format!("Order not found: {}", req.order_id)
            )),
        }
    }
    
    /// Cancel an order
    #[handler("cancel")]
    async fn handle_cancel(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        #[derive(Deserialize)]
        struct CancelRequest { order_id: String }
        
        let req: CancelRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        match self.orders.get_mut(&req.order_id) {
            Some(order) => {
                if order.status == "pending" {
                    order.status = "cancelled".to_string();
                    info!("Cancelled order: {}", req.order_id);
                    Ok(json!({ "status": "cancelled" }))
                } else {
                    Err(BehaviorError::ProcessingError(
                        format!("Cannot cancel order in status: {}", order.status)
                    ))
                }
            }
            None => Err(BehaviorError::ProcessingError(
                format!("Order not found: {}", req.order_id)
            )),
        }
    }
    
    /// List all orders
    #[handler("list")]
    async fn handle_list(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let orders: Vec<&Order> = self.orders.values().collect();
        Ok(json!({
            "orders": orders,
            "count": orders.len()
        }))
    }
}

// =============================================================================
// Main - Demonstrates the complete flow
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .with_env_filter("order_processing=info,plexspaces=warn")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Order Processing Example (E-Commerce)                      ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Use Case: E-commerce order management with CRUD operations");
    println!();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    println!("Step 1: Create PlexSpaces Node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let node = NodeBuilder::new("order-node")
        .with_clustering_enabled(false)
        .build().await;
    let node = Arc::new(node);
    
    // Start node in background
    let node_for_start = node.clone();
    tokio::spawn(async move {
        let _ = node_for_start.start().await;
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    println!("  ✓ Node 'order-node' created and started");
    println!();

    // =========================================================================
    // Step 2: Spawn Order Processor Actor
    // =========================================================================
    println!("Step 2: Spawn Order Processor Actor");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let ctx = RequestContext::new_without_auth(
        "acme-store".to_string(),  // tenant
        "orders".to_string(),       // namespace
    );
    
    let actor_ref: ActorRef = spawn_actor(
        &ctx,
        node.service_locator(),
        "order-processor@order-node",
        "orders",
        OrderProcessor::new(),
        vec![],  // no facets needed
    ).await?;
    
    println!("  ✓ OrderProcessor spawned: {}", actor_ref.id());
    println!("  Using: #[gen_server_actor], #[handler(\"op\")]");
    println!();

    // =========================================================================
    // Step 3: Create Orders
    // =========================================================================
    println!("Step 3: Create Orders");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Order 1: Alice buys widgets
    // Note: "op" field in payload routes to #[handler("create")]
    let create_msg = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: serde_json::to_vec(&json!({
            "op": "create",
            "customer_id": "alice",
            "items": [
                {"sku": "WIDGET-001", "name": "Premium Widget", "quantity": 2, "price_cents": 2999},
                {"sku": "GADGET-002", "name": "Deluxe Gadget", "quantity": 1, "price_cents": 4999}
            ]
        }))?,
        ..Default::default()
    };
    
    let response = actor_ref.ask(create_msg, Duration::from_secs(5)).await?;
    let result: Value = serde_json::from_slice(&response.payload)?;
    println!("  Created order for Alice:");
    println!("    Order ID: {}", result["order_id"]);
    println!("    Total: ${:.2}", result["total_cents"].as_u64().unwrap_or(0) as f64 / 100.0);
    
    let order1_id = result["order_id"].as_str().unwrap().to_string();
    
    // Order 2: Bob buys tools
    let create_msg2 = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: serde_json::to_vec(&json!({
            "op": "create",
            "customer_id": "bob",
            "items": [
                {"sku": "TOOL-003", "name": "Power Drill", "quantity": 1, "price_cents": 8999}
            ]
        }))?,
        ..Default::default()
    };
    
    let response2 = actor_ref.ask(create_msg2, Duration::from_secs(5)).await?;
    let result2: Value = serde_json::from_slice(&response2.payload)?;
    println!("  Created order for Bob:");
    println!("    Order ID: {}", result2["order_id"]);
    println!("    Total: ${:.2}", result2["total_cents"].as_u64().unwrap_or(0) as f64 / 100.0);
    println!();

    // =========================================================================
    // Step 4: List Orders
    // =========================================================================
    println!("Step 4: List All Orders");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let list_msg = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: serde_json::to_vec(&json!({ "op": "list" }))?,
        ..Default::default()
    };
    let response = actor_ref.ask(list_msg, Duration::from_secs(5)).await?;
    let result: Value = serde_json::from_slice(&response.payload)?;
    println!("  Total orders: {}", result["count"]);
    println!();

    // =========================================================================
    // Step 5: Get Order Details
    // =========================================================================
    println!("Step 5: Get Order Details");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let get_msg = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: serde_json::to_vec(&json!({ "op": "get", "order_id": order1_id }))?,
        ..Default::default()
    };
    let response = actor_ref.ask(get_msg, Duration::from_secs(5)).await?;
    let result: Value = serde_json::from_slice(&response.payload)?;
    println!("  Order {}: {} items, status={}",
        result["order"]["id"],
        result["order"]["items"].as_array().map(|a| a.len()).unwrap_or(0),
        result["order"]["status"]
    );
    println!();

    // =========================================================================
    // Step 6: Cancel Order
    // =========================================================================
    println!("Step 6: Cancel Order");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let cancel_msg = Message {
        id: ulid::Ulid::new().to_string(),
        message_type: "call".to_string(),
        payload: serde_json::to_vec(&json!({ "op": "cancel", "order_id": order1_id }))?,
        ..Default::default()
    };
    let response = actor_ref.ask(cancel_msg, Duration::from_secs(5)).await?;
    let result: Value = serde_json::from_slice(&response.payload)?;
    println!("  Order {} cancelled: status={}", order1_id, result["status"]);
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Order Processing Example Complete!");
    println!();
    println!("SDK Annotations Used:");
    println!("  • #[gen_server_actor] - GenServer behavior (request-reply)");
    println!("  • #[plexspaces_handlers(gen_server)] - Handler dispatch");
    println!("  • #[handler(\"op\")] - Route messages to methods");
    println!();
    println!("Key Patterns:");
    println!("  • spawn_actor() - Simple actor creation");
    println!("  • actor_ref.ask() - Request-reply messaging");
    println!("  • Message::new().with_metadata() - Create typed messages");
    println!();
    println!("Real-World Use Cases:");
    println!("  • E-commerce order management");
    println!("  • Inventory tracking");
    println!("  • Shopping cart services");
    println!("  • Payment processing");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
