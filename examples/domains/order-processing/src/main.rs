// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Order Processing Example
//
// Demonstrates:
// - ActorBehavior pattern
// - Typed messages (ActorMessage enum)
// - ConfigBootstrap for configuration
// - CoordinationComputeTracker for metrics
// - Simplified actor setup using NodeBuilder

use order_processing::OrderProcessorBehavior;
use order_processing::actors::order_processor::OrderMessage;
use order_processing::types::OrderItem;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::ActorBehavior;
use plexspaces_mailbox::Message;
use plexspaces_node::{ConfigBootstrap, NodeBuilder};
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
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║     Order Processing Example                                 ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
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

    let grpc_port = config.grpc_port;
    let registry_port = config.registry_port;

    info!("📋 Configuration:");
    info!("   Node ID: {}", node_id);
    info!("   gRPC Port: {}", grpc_port);
    info!("   Registry Port: {}", registry_port);
    println!();

    // Create node using NodeBuilder
    info!("🏗️  Creating node...");
    let node = NodeBuilder::new(node_id.clone())
        .build();
    info!("✅ Node created: {}", node_id);
    println!();

    // Create order processor actor using ActorBuilder
    info!("🎭 Creating order processor actor...");
    let behavior = Box::new(OrderProcessorBehavior::new());
    let actor = ActorBuilder::new(behavior)
        .with_name("order-processor")
        .with_mailbox_config(plexspaces_mailbox::MailboxConfig {
            capacity: 1000,
            ..Default::default()
        })
        .build();
    
    let actor_id = actor.id().clone();
    info!("✅ Actor created: {}", actor_id);
    println!();

    // Spawn actor
    let actor_ref = node.spawn_actor(actor).await?;
    info!("✅ Actor spawned");
    println!();

    // Wait a bit for actor to initialize
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Create a sample order
    info!("🛒 Creating sample order...");
    let order_msg = OrderMessage::CreateOrder {
        customer_id: "customer-123".to_string(),
        items: vec![
            OrderItem::new("SKU-001".to_string(), "Premium Widget".to_string(), 2, 2999),
            OrderItem::new("SKU-002".to_string(), "Deluxe Gadget".to_string(), 1, 4999),
        ],
    };
    
    // Serialize and send message
    let payload = serde_json::to_vec(&order_msg)?;
    let message = Message::new(payload)
        .with_message_type("CreateOrder".to_string());
    
    actor_ref.tell(message).await?;
    info!("✅ Order creation message sent");
    println!();

    // Wait for processing
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Get order (for demo)
    info!("📋 Retrieving order...");
    let get_msg = OrderMessage::GetOrder {
        order_id: "test-order-id".to_string(), // Would need to track order_id from creation
    };
    
    let payload = serde_json::to_vec(&get_msg)?;
    let message = Message::new(payload)
        .with_message_type("GetOrder".to_string());
    
    actor_ref.tell(message).await?;
    info!("✅ Get order message sent");
    println!();

    // Wait a bit more
    tokio::time::sleep(Duration::from_millis(500)).await;

    info!("✅ Example complete!");
    info!("   Check logs above for order processing details");
    println!();

    Ok(())
}

