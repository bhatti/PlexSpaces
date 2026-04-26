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
// - `spawn()` - Simple actor spawning with SDK helper
//
// ## What This Example Shows
// 1. Define an actor with SDK annotations (no boilerplate)
// 2. Spawn actor using simple SDK helper
// 3. Send messages and receive replies via ActorRef
// 4. Proper tenant/namespace isolation

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, RequestContext, Message,
    NodeBuilder, spawn, call_message, json, Value, ActorRef,
};
use plexspaces_node::CoordinationComputeTracker;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, Level};
use anyhow::Result;

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
async fn main() -> Result<()> {
    // Initialize tracing - ensure INFO level for metrics output
    // Use try_init() to avoid panic if already initialized (e.g., in tests)
    let _ = tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .try_init();

    info!("╔════════════════════════════════════════════════════════════════╗");
    info!("║     Order Processing Example (E-Commerce)                      ║");
    info!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("Use Case: E-commerce order management with CRUD operations");
    info!("Multi-tenancy: RequestContext with tenant/namespace (no internal())");
    println!();

    // Initialize metrics tracker
    let mut metrics_tracker = CoordinationComputeTracker::new("order-processing".to_string());
    let total_start = Instant::now();

    // =========================================================================
    // Step 1: Create Node
    // =========================================================================
    info!("Step 1: Create PlexSpaces Node");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let node = NodeBuilder::new("order-node")
        .with_clustering_enabled(false)
        .build_started().await;
    
    info!("  ✓ Node 'order-node' created and started");
    println!();

    // =========================================================================
    // Step 2: Spawn Order Processor Actor
    // =========================================================================
    info!("Step 2: Spawn Order Processor Actor");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let ctx = RequestContext::new_without_auth(
        "acme-store".to_string(),  // tenant
        "orders".to_string(),       // namespace
    );
    
    let actor_ref: ActorRef = spawn(
        &ctx,
        node.service_locator(),
        "order-processor",
        "orders",
        OrderProcessor::new(),
    ).await.map_err(|e| anyhow::anyhow!("Failed to spawn actor: {}", e))?;
    
    info!("  ✓ OrderProcessor spawned: {}", actor_ref.id());
    info!("  Using: #[gen_server_actor], #[handler(\"op\")]");
    println!();

    // =========================================================================
    // Step 3: Create Orders
    // =========================================================================
    info!("Step 3: Create Orders");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Order 1: Alice buys widgets
    metrics_tracker.start_coordinate();
    let create_start = Instant::now();
    let create_msg = call_message(json!({
        "op": "create",
        "customer_id": "alice",
        "items": [
            {"sku": "WIDGET-001", "name": "Premium Widget", "quantity": 2, "price_cents": 2999},
            {"sku": "GADGET-002", "name": "Deluxe Gadget", "quantity": 1, "price_cents": 4999}
        ]
    }));
    
    metrics_tracker.start_compute();
    let response = actor_ref.ask(create_msg, Duration::from_secs(5)).await?;
    metrics_tracker.end_compute();
    metrics_tracker.increment_message();
    
    let result: Value = serde_json::from_slice(&response.payload)?;
    let create_time = create_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Created order for Alice:");
    info!("    Order ID: {}", result["order_id"]);
    info!("    Total: ${:.2}", result["total_cents"].as_u64().unwrap_or(0) as f64 / 100.0);
    info!("    Create time: {:.2}ms", create_time.as_secs_f64() * 1000.0);
    
    let order1_id = result["order_id"].as_str().unwrap().to_string();
    
    // Order 2: Bob buys tools
    metrics_tracker.start_coordinate();
    let create2_start = Instant::now();
    let create_msg2 = call_message(json!({
        "op": "create",
        "customer_id": "bob",
        "items": [
            {"sku": "TOOL-003", "name": "Power Drill", "quantity": 1, "price_cents": 8999}
        ]
    }));
    
    metrics_tracker.start_compute();
    let response2 = actor_ref.ask(create_msg2, Duration::from_secs(5)).await?;
    metrics_tracker.end_compute();
    metrics_tracker.increment_message();
    
    let result2: Value = serde_json::from_slice(&response2.payload)?;
    let create2_time = create2_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Created order for Bob:");
    info!("    Order ID: {}", result2["order_id"]);
    info!("    Total: ${:.2}", result2["total_cents"].as_u64().unwrap_or(0) as f64 / 100.0);
    info!("    Create time: {:.2}ms", create2_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 4: List Orders
    // =========================================================================
    info!("Step 4: List All Orders");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let list_start = Instant::now();
    let list_msg = call_message(json!({ "op": "list" }));
    
    metrics_tracker.start_compute();
    let response = actor_ref.ask(list_msg, Duration::from_secs(5)).await?;
    metrics_tracker.end_compute();
    metrics_tracker.increment_message();
    
    let result: Value = serde_json::from_slice(&response.payload)?;
    let list_time = list_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Total orders: {}", result["count"]);
    info!("  List time: {:.2}ms", list_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 5: Get Order Details
    // =========================================================================
    info!("Step 5: Get Order Details");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let get_start = Instant::now();
    let get_msg = call_message(json!({ "op": "get", "order_id": order1_id }));
    
    metrics_tracker.start_compute();
    let response = actor_ref.ask(get_msg, Duration::from_secs(5)).await?;
    metrics_tracker.end_compute();
    metrics_tracker.increment_message();
    
    let result: Value = serde_json::from_slice(&response.payload)?;
    let get_time = get_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Order {}: {} items, status={}",
        result["order"]["id"],
        result["order"]["items"].as_array().map(|a| a.len()).unwrap_or(0),
        result["order"]["status"]
    );
    info!("  Get time: {:.2}ms", get_time.as_secs_f64() * 1000.0);
    println!();

    // =========================================================================
    // Step 6: Cancel Order
    // =========================================================================
    info!("Step 6: Cancel Order");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    metrics_tracker.start_coordinate();
    let cancel_start = Instant::now();
    let cancel_msg = call_message(json!({ "op": "cancel", "order_id": order1_id }));
    
    metrics_tracker.start_compute();
    let response = actor_ref.ask(cancel_msg, Duration::from_secs(5)).await?;
    metrics_tracker.end_compute();
    metrics_tracker.increment_message();
    
    let result: Value = serde_json::from_slice(&response.payload)?;
    let cancel_time = cancel_start.elapsed();
    metrics_tracker.end_coordinate();
    
    info!("  Order {} cancelled: status={}", order1_id, result["status"]);
    info!("  Cancel time: {:.2}ms", cancel_time.as_secs_f64() * 1000.0);
    println!();

    // Finalize metrics
    let total_time = total_start.elapsed();
    let metrics = metrics_tracker.finalize();
    
    // Calculate benchmark metrics
    let total_ops = 5; // create (2), list (1), get (1), cancel (1)
    let total_time_secs = total_time.as_secs_f64();
    let ops_per_sec = if total_time_secs > 0.0 {
        total_ops as f64 / total_time_secs
    } else {
        0.0
    };
    
    // Print metrics prominently with clear coordination vs computation breakdown
    info!("\n{}", "=".repeat(80));
    info!("📊 PERFORMANCE METRICS & BENCHMARKS");
    info!("{}", "=".repeat(80));
    
    info!("\nOperations Performed:");
    info!("  Create orders: 2");
    info!("  List orders: 1");
    info!("  Get order: 1");
    info!("  Cancel order: 1");
    info!("  Total operations: {}", total_ops);
    
    info!("\n{}", "─".repeat(80));
    info!("⚡ LATENCY BREAKDOWN (Coordination vs Computation)");
    info!("{}", "─".repeat(80));
    info!("  Create (Alice): {:>12.2} ms", create_time.as_secs_f64() * 1000.0);
    info!("  Create (Bob):   {:>12.2} ms", create2_time.as_secs_f64() * 1000.0);
    info!("  List:           {:>12.2} ms", list_time.as_secs_f64() * 1000.0);
    info!("  Get:            {:>12.2} ms", get_time.as_secs_f64() * 1000.0);
    info!("  Cancel:         {:>12.2} ms", cancel_time.as_secs_f64() * 1000.0);
    info!("  {}", "─".repeat(30));
    info!("  Coordination: {:>10.2} ms (total)", metrics.coordinate_duration_ms as f64);
    info!("  Computation:  {:>10.2} ms (total)", metrics.compute_duration_ms as f64);
    info!("  Total Time:    {:>10.2} ms ({:.2} seconds)", 
        total_time.as_secs_f64() * 1000.0, total_time_secs);
    
    info!("\n{}", "─".repeat(80));
    info!("📈 COORDINATION vs COMPUTATION ANALYSIS");
    info!("{}", "─".repeat(80));
    info!("  Computation time:      {:>12.2} ms", metrics.compute_duration_ms as f64);
    info!("  Coordination time:    {:>12.2} ms", metrics.coordinate_duration_ms as f64);
    info!("  Granularity ratio:     {:>12.2}× (compute/coordinate)", metrics.granularity_ratio);
    info!("  Efficiency:            {:>12.2}% (compute/total)", metrics.efficiency * 100.0);
    info!("  Message count:         {:>12}", metrics.message_count);
    
    // Cost analysis - show percentage breakdown
    let coord_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.coordinate_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    let compute_cost_pct = if metrics.total_duration_ms > 0 {
        (metrics.compute_duration_ms as f64 / metrics.total_duration_ms as f64) * 100.0
    } else {
        0.0
    };
    info!("\n  Cost Breakdown:");
    info!("    Coordination overhead: {:>8.2}% of total time", coord_cost_pct);
    info!("    Computation:           {:>8.2}% of total time", compute_cost_pct);
    
    info!("\n{}", "─".repeat(80));
    info!("🚀 BENCHMARK METRICS");
    info!("{}", "─".repeat(80));
    info!("  Operations/sec:    {:>12.2} ops/s", ops_per_sec);
    info!("  Avg latency:       {:>12.2} ms/op", 
        if total_ops > 0 { (total_time_secs * 1000.0) / total_ops as f64 } else { 0.0 });
    
    info!("\n{}", "─".repeat(80));
    info!("💡 ANALYSIS & RECOMMENDATIONS");
    info!("{}", "─".repeat(80));
    if metrics.granularity_ratio < 10.0 {
        info!("  ⚠️  WARNING: Overhead too high! Consider:");
        info!("     - Larger batch operations (process multiple orders)");
        info!("     - Current ratio: {:.2}× (should be >= 10×)", metrics.granularity_ratio);
    } else if metrics.granularity_ratio < 100.0 {
        info!("  ✓  ACCEPTABLE: Reasonable granularity for CRUD operations");
        info!("     - Ratio: {:.2}× (good for transactional workloads)", metrics.granularity_ratio);
    } else {
        info!("  ✓  EXCELLENT: Good compute/coordinate ratio");
        info!("     - Ratio: {:.2}× (ideal for efficient processing)", metrics.granularity_ratio);
    }
    
    if coord_cost_pct > 20.0 {
        info!("  ⚠️  Coordination overhead is {:.1}% - consider batching operations", coord_cost_pct);
    } else {
        info!("  ✓  Coordination overhead is {:.1}% - acceptable", coord_cost_pct);
    }
    
    info!("{}", "=".repeat(80));

    // =========================================================================
    // Summary
    // =========================================================================
    info!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("✅ Order Processing Example Complete!");
    println!();
    info!("SDK Annotations Used:");
    info!("  • #[gen_server_actor] - GenServer behavior (request-reply)");
    info!("  • #[plexspaces_handlers(gen_server)] - Handler dispatch");
    info!("  • #[handler(\"op\")] - Route messages to methods");
    println!();
    info!("Key Patterns:");
    info!("  • spawn() - Simple actor creation");
    info!("  • call_message() - Create request-reply messages");
    info!("  • actor_ref.ask() - Request-reply messaging");
    println!();
    info!("Real-World Use Cases:");
    info!("  • E-commerce order management");
    info!("  • Inventory tracking");
    info!("  • Shopping cart services");
    info!("  • Payment processing");
    println!();

    // Graceful shutdown
    info!("Shutting down...");
    node.shutdown(Duration::from_secs(5)).await?;

    Ok(())
}
