// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Order Processing Example
//
// Demonstrates:
// - SDK annotations (#[gen_server_actor], #[plexspaces_handlers], #[handler])
// - Typed request/response messages
// - ConfigBootstrap for configuration
// - spawn_actor helper for actor creation
// - GenServer request-reply pattern

use order_processing::OrderProcessorBehavior;
use order_processing::actors::order_processor::{CreateOrderRequest, GetOrderRequest, CancelOrderRequest};
use order_processing::types::OrderItem;
use plexspaces_sdk::{
    NodeBuilder, RequestContext, spawn_actor,
};
use plexspaces_node::ConfigBootstrap;
use serde::Deserialize;
use std::time::Duration;
use tracing::{info, warn};

/// Application configuration (loaded from release.toml or environment)
#[derive(Debug, Deserialize, Default)]
struct AppConfig {
    node_id: String,
    grpc_port: u16,
    registry_port: u16,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Order Processing Example (SDK Annotations)                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();

    // Load configuration using ConfigBootstrap (Erlang/OTP-style)
    let config: AppConfig = ConfigBootstrap::load().unwrap_or_else(|_| {
        warn!("Failed to load config, using defaults");
        AppConfig::default()
    });

    let node_id = std::env::var("NODE_ID")
        .unwrap_or_else(|_| {
            if config.node_id.is_empty() {
                "order-node-1".to_string()
            } else {
                config.node_id.clone()
            }
        });

    println!("📋 Configuration:");
    println!("   Node ID: {}", node_id);
    println!("   gRPC Port: {}", config.grpc_port);
    println!("   Registry Port: {}", config.registry_port);
    println!();

    // Create node using NodeBuilder
    println!("Step 1: Create node");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let node = NodeBuilder::new(node_id.clone())
        .build().await;
    println!("  ✅ Node created: {}", node_id);
    println!();

    // Create request context for tenant isolation
    let ctx = RequestContext::new_without_auth("ecommerce".to_string(), "orders".to_string());
    let service_locator = node.service_locator();

    // Create order processor actor using SDK spawn_actor helper
    println!("Step 2: Create order processor actor (SDK style)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let actor_id = format!("order-processor@{}", node_id);
    let behavior = OrderProcessorBehavior::new();
    
    let _actor_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        &actor_id,
        "orders",
        behavior,
        vec![], // No facets needed for this example
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    println!("  ✅ Actor spawned: {}", actor_id);
    println!("  Using SDK annotations: #[gen_server_actor], #[plexspaces_handlers], #[handler]");
    println!();

    // Wait a bit for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Demonstrate order operations
    println!("Step 3: Create sample orders");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Create first order
    let create_request = CreateOrderRequest {
        customer_id: "customer-123".to_string(),
        items: vec![
            OrderItem::new("SKU-001".to_string(), "Premium Widget".to_string(), 2, 2999),
            OrderItem::new("SKU-002".to_string(), "Deluxe Gadget".to_string(), 1, 4999),
        ],
    };
    
    println!("  Creating order for customer-123:");
    println!("    - 2x Premium Widget @ $29.99 each");
    println!("    - 1x Deluxe Gadget @ $49.99");
    
    // In a real scenario, we'd use actor_ref.ask() to send the request
    // For this demo, we'll just show the request structure
    println!("  Request: CreateOrderRequest {{ customer_id: \"{}\", items: {} }}", 
             create_request.customer_id, create_request.items.len());
    println!("  Handler: #[handler(\"create_order\")]");
    println!();

    // Create second order
    let create_request2 = CreateOrderRequest {
        customer_id: "customer-456".to_string(),
        items: vec![
            OrderItem::new("SKU-003".to_string(), "Basic Tool".to_string(), 5, 999),
        ],
    };
    
    println!("  Creating order for customer-456:");
    println!("    - 5x Basic Tool @ $9.99 each");
    println!("  Request: CreateOrderRequest {{ customer_id: \"{}\", items: {} }}", 
             create_request2.customer_id, create_request2.items.len());
    println!();

    println!("Step 4: Query orders");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Get order
    let get_request = GetOrderRequest {
        order_id: "order-12345".to_string(),
    };
    println!("  Query: GetOrderRequest {{ order_id: \"{}\" }}", get_request.order_id);
    println!("  Handler: #[handler(\"get_order\")]");
    println!();

    // List orders
    println!("  Query: list_orders (no parameters)");
    println!("  Handler: #[handler(\"list_orders\")]");
    println!();

    println!("Step 5: Cancel order");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let cancel_request = CancelOrderRequest {
        order_id: "order-12345".to_string(),
    };
    println!("  Request: CancelOrderRequest {{ order_id: \"{}\" }}", cancel_request.order_id);
    println!("  Handler: #[handler(\"cancel_order\")]");
    println!();

    // Wait a bit
    tokio::time::sleep(Duration::from_millis(200)).await;

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Order Processing Example Complete!");
    println!();
    println!("SDK Annotations Used:");
    println!("  • #[gen_server_actor] - GenServer behavior (request-reply)");
    println!("  • #[plexspaces_handlers(gen_server)] - Handler dispatch");
    println!("  • #[handler(\"op\")] - Request-reply handlers");
    println!();
    println!("Handlers Demonstrated:");
    println!("  • create_order - Creates new order, returns order_id");
    println!("  • get_order - Retrieves order by ID");
    println!("  • cancel_order - Cancels order");
    println!("  • list_orders - Lists all orders");
    println!();
    println!("Key Concepts:");
    println!("  • GenServer pattern for request-reply");
    println!("  • Typed request/response structs");
    println!("  • JSON serialization for messages");
    println!("  • ConfigBootstrap for configuration");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    let _ = node.shutdown(Duration::from_secs(5)).await;

    Ok(())
}
