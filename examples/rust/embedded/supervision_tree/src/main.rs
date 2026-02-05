// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Supervision Tree Example - E-Commerce Order Processing
//
// Demonstrates Erlang/OTP-style fault tolerance with supervision trees
// using a real-world e-commerce order processing use case.
//
// ## SDK Annotations Used
// - `#[actor]` - marks struct as an actor
// - `#[gen_server_actor]` - generates GenServer behavior
// - `#[plexspaces_handlers(gen_server)]` - generates handler dispatch
// - `#[handler("op")]` - request-reply handlers
//
// ## Supervision Strategies
// - OneForOne: If one worker fails, only restart that worker
// - OneForAll: If one worker fails, restart ALL workers
// - RestForOne: If one worker fails, restart it and all workers started after it
//
// Use Case: E-commerce order processing with payment, inventory, and shipping workers

use plexspaces_sdk::{
    actor, gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, RequestContext, spawn_actor,
};
use plexspaces_actor::supervisor::{Supervisor, SupervisorEvent, SupervisionStrategy};
use plexspaces_actor::child_spec::{ChildSpec, RestartStrategy, StartFn, StartedChild};
use plexspaces_actor::Actor;
use plexspaces_core::{ActorError, ActorRef as CoreActorRef};
use plexspaces_mailbox::{Mailbox, MailboxConfig};
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

// Required for macro-generated code
extern crate plexspaces_core;

// =============================================================================
// Domain Types - E-Commerce Order Processing
// =============================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub order_id: String,
    pub customer_id: String,
    pub items: Vec<OrderItem>,
    pub total_cents: u64,
    pub status: OrderStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: String,
    pub quantity: u32,
    pub price_cents: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrderStatus {
    Pending,
    PaymentProcessing,
    PaymentCompleted,
    InventoryReserved,
    Shipped,
    Delivered,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub order_id: String,
    pub amount_cents: u64,
    pub customer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResponse {
    pub order_id: String,
    pub success: bool,
    pub transaction_id: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryRequest {
    pub order_id: String,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryResponse {
    pub order_id: String,
    pub reserved: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingRequest {
    pub order_id: String,
    pub customer_id: String,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShippingResponse {
    pub order_id: String,
    pub tracking_number: Option<String>,
    pub error: Option<String>,
}

// =============================================================================
// Payment Worker Actor - SDK Annotations
// =============================================================================

/// Payment processing worker with GenServer behavior.
/// 
/// ## Annotations
/// - `#[gen_server_actor]` - generates GenServer behavior (request-reply)
/// - `#[plexspaces_handlers(gen_server)]` - generates handler dispatch
/// - `#[handler("process_payment")]` - handles payment requests
#[gen_server_actor]
struct PaymentWorker {
    worker_id: String,
    processed_count: u64,
    failed_count: u64,
}

impl PaymentWorker {
    fn new(worker_id: &str) -> Self {
        Self {
            worker_id: worker_id.to_string(),
            processed_count: 0,
            failed_count: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl PaymentWorker {
    /// Process a payment request
    #[handler("process_payment")]
    async fn handle_process_payment(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let request: PaymentRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        println!("    💳 [{}] Processing payment for order {}", self.worker_id, request.order_id);
        
        // Simulate payment processing (in real app, call payment gateway)
        let success = request.amount_cents < 100000; // Fail orders > $1000 for demo
        
        let response = if success {
            self.processed_count += 1;
            PaymentResponse {
                order_id: request.order_id,
                success: true,
                transaction_id: Some(format!("TXN-{}", ulid::Ulid::new())),
                error: None,
            }
        } else {
            self.failed_count += 1;
            PaymentResponse {
                order_id: request.order_id,
                success: false,
                transaction_id: None,
                error: Some("Amount exceeds limit".to_string()),
            }
        };
        
        serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
    
    /// Get worker stats
    #[handler("get_stats")]
    async fn handle_get_stats(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let stats = serde_json::json!({
            "worker_id": self.worker_id,
            "processed_count": self.processed_count,
            "failed_count": self.failed_count,
        });
        
        serde_json::to_vec(&stats)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Inventory Worker Actor - SDK Annotations
// =============================================================================

/// Inventory management worker with GenServer behavior.
#[gen_server_actor]
struct InventoryWorker {
    worker_id: String,
    reserved_count: u64,
}

impl InventoryWorker {
    fn new(worker_id: &str) -> Self {
        Self {
            worker_id: worker_id.to_string(),
            reserved_count: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl InventoryWorker {
    /// Reserve inventory for an order
    #[handler("reserve_inventory")]
    async fn handle_reserve_inventory(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let request: InventoryRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        println!("    📦 [{}] Reserving inventory for order {}", self.worker_id, request.order_id);
        
        // Simulate inventory check (in real app, check database)
        self.reserved_count += 1;
        
        let response = InventoryResponse {
            order_id: request.order_id,
            reserved: true,
            error: None,
        };
        
        serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Shipping Worker Actor - SDK Annotations
// =============================================================================

/// Shipping coordination worker with GenServer behavior.
#[gen_server_actor]
struct ShippingWorker {
    worker_id: String,
    shipped_count: u64,
}

impl ShippingWorker {
    fn new(worker_id: &str) -> Self {
        Self {
            worker_id: worker_id.to_string(),
            shipped_count: 0,
        }
    }
}

#[plexspaces_handlers(gen_server)]
impl ShippingWorker {
    /// Create shipping label and schedule pickup
    #[handler("create_shipment")]
    async fn handle_create_shipment(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Vec<u8>, BehaviorError> {
        let request: ShippingRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Invalid request: {}", e)))?;
        
        println!("    🚚 [{}] Creating shipment for order {}", self.worker_id, request.order_id);
        
        // Simulate shipping label creation
        self.shipped_count += 1;
        
        let response = ShippingResponse {
            order_id: request.order_id,
            tracking_number: Some(format!("TRACK-{}", ulid::Ulid::new())),
            error: None,
        };
        
        serde_json::to_vec(&response)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║     Supervision Tree Example - E-Commerce Order Processing     ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("Demonstrates Erlang/OTP-style fault tolerance with SDK annotations");
    println!();

    // Create node
    let node = Arc::new(NodeBuilder::new("ecommerce-node").build().await);
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth(
        "ecommerce-tenant".to_string(),
        "order-processing".to_string(),
    );

    // =========================================================================
    // Example 1: OneForOne Strategy - Independent Workers
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 1: OneForOne Strategy (Independent Workers)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("OneForOne: If one payment worker fails, only restart that worker.");
    println!("Use case: Multiple independent payment processors");
    println!();

    // Spawn payment workers using SDK annotations
    println!("  Spawning PaymentWorker actors with SDK annotations...");
    
    for i in 1..=3 {
        let worker_id = format!("payment-worker-{}", i);
        let actor_id = format!("{}@ecommerce-node", worker_id);
        
        let _actor_ref = spawn_actor(
            &ctx,
            service_locator.clone(),
            &actor_id,
            "payment",
            PaymentWorker::new(&worker_id),
            vec![],
        )
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        
        println!("    ✓ {} spawned (GenServer)", actor_id);
    }
    println!();

    // Create supervisor for payment workers
    let (supervisor, mut event_rx) = Supervisor::new(
        "payment-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 5,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Add workers to supervisor
    for i in 1..=3 {
        let worker_id = format!("payment-worker-{}@ecommerce-node", i);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        supervisor.add_child(spec).await?;
        
        // Consume ChildStarted event
        if let Some(SupervisorEvent::ChildStarted(id)) = event_rx.recv().await {
            println!("    ✓ Supervisor registered: {}", id);
        }
    }

    // Show supervisor stats
    let stats = supervisor.stats().await;
    println!();
    println!("  Supervisor stats:");
    println!("    - Strategy: OneForOne (max 5 restarts in 60s)");
    println!("    - Total restarts: {}", stats.total_restarts);
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Payment supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 2: OneForAll Strategy - Dependent Workers
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 2: OneForAll Strategy (Dependent Workers)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("OneForAll: If any worker fails, restart ALL workers.");
    println!("Use case: Workers share state (e.g., distributed cache)");
    println!();

    // Spawn inventory workers
    println!("  Spawning InventoryWorker actors with SDK annotations...");
    
    for i in 1..=3 {
        let worker_id = format!("inventory-worker-{}", i);
        let actor_id = format!("{}@ecommerce-node", worker_id);
        
        let _actor_ref = spawn_actor(
            &ctx,
            service_locator.clone(),
            &actor_id,
            "inventory",
            InventoryWorker::new(&worker_id),
            vec![],
        )
        .await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
        
        println!("    ✓ {} spawned (GenServer)", actor_id);
    }
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "inventory-supervisor".to_string(),
        SupervisionStrategy::OneForAll {
            max_restarts: 3,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    for i in 1..=3 {
        let worker_id = format!("inventory-worker-{}@ecommerce-node", i);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        supervisor.add_child(spec).await?;
        let _ = event_rx.recv().await;
        println!("    ✓ Supervisor registered: {}", worker_id);
    }
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Inventory supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 3: RestForOne Strategy - Ordered Dependencies
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 3: RestForOne Strategy (Ordered Dependencies)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("RestForOne: If a worker fails, restart it AND all workers started after it.");
    println!("Use case: Order pipeline (Payment → Inventory → Shipping)");
    println!();

    // Spawn order pipeline workers
    println!("  Spawning order pipeline actors (order matters!)...");
    
    // Payment must succeed before inventory
    let _payment_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        "pipeline-payment@ecommerce-node",
        "pipeline",
        PaymentWorker::new("pipeline-payment"),
        vec![],
    )
    .await
    .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    println!("    ✓ pipeline-payment@ecommerce-node (1st - Payment)");

    // Inventory must succeed before shipping
    let _inventory_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        "pipeline-inventory@ecommerce-node",
        "pipeline",
        InventoryWorker::new("pipeline-inventory"),
        vec![],
    )
    .await
    .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    println!("    ✓ pipeline-inventory@ecommerce-node (2nd - Inventory)");

    // Shipping depends on both
    let _shipping_ref = spawn_actor(
        &ctx,
        service_locator.clone(),
        "pipeline-shipping@ecommerce-node",
        "pipeline",
        ShippingWorker::new("pipeline-shipping"),
        vec![],
    )
    .await
    .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    println!("    ✓ pipeline-shipping@ecommerce-node (3rd - Shipping)");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "pipeline-supervisor".to_string(),
        SupervisionStrategy::RestForOne {
            max_restarts: 3,
            within_seconds: 60,
        },
        service_locator.clone(),
    );

    // Add in order (order matters for RestForOne!)
    for name in &["pipeline-payment", "pipeline-inventory", "pipeline-shipping"] {
        let worker_id = format!("{}@ecommerce-node", name);
        let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
        supervisor.add_child(spec).await?;
        let _ = event_rx.recv().await;
        println!("    ✓ Supervisor registered: {} (order preserved)", name);
    }
    println!();

    println!("  RestForOne behavior:");
    println!("    - If Payment fails → restart Payment, Inventory, Shipping");
    println!("    - If Inventory fails → restart Inventory, Shipping");
    println!("    - If Shipping fails → restart only Shipping");
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Pipeline supervisor shutdown complete");
    println!();

    // =========================================================================
    // Example 4: Failure Recovery Demonstration
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Example 4: Failure Recovery (Automatic Restart)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    println!("Simulating a worker failure and observing automatic restart...");
    println!();

    let (supervisor, mut event_rx) = Supervisor::new(
        "recovery-supervisor".to_string(),
        SupervisionStrategy::OneForOne {
            max_restarts: 3,
            within_seconds: 10,
        },
        service_locator.clone(),
    );

    let worker_id = "crashable-worker@ecommerce-node".to_string();
    let spec = create_worker_spec(&worker_id, RestartStrategy::Permanent);
    supervisor.add_child(spec).await?;
    let _ = event_rx.recv().await;
    println!("    ✓ Worker started: {}", worker_id);

    // Simulate failure
    println!("    → Simulating crash...");
    supervisor.handle_failure(&worker_id, "simulated payment gateway timeout".to_string(), None).await?;

    // Collect events
    let timeout = sleep(Duration::from_millis(500));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                if let Some(event) = event {
                    match event {
                        SupervisorEvent::ChildFailed(id, reason) => {
                            println!("    ✗ Worker failed: {} (reason: {})", id, reason);
                        }
                        SupervisorEvent::ChildRestarted(id, count) => {
                            println!("    ✓ Worker restarted: {} (restart #{})", id, count);
                        }
                        _ => {}
                    }
                }
            }
            _ = &mut timeout => {
                break;
            }
        }
    }

    let stats = supervisor.stats().await;
    println!();
    println!("  Recovery stats:");
    println!("    - Total restarts: {}", stats.total_restarts);
    println!("    - Successful restarts: {}", stats.successful_restarts);
    println!("    - Failed restarts: {}", stats.failed_restarts);
    println!();

    let mut supervisor = supervisor;
    supervisor.shutdown().await?;
    println!("  ✓ Recovery supervisor shutdown complete");
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("✅ Supervision Tree Example Complete!");
    println!();
    println!("SDK Annotations Used:");
    println!("  • #[gen_server_actor] - GenServer behavior (request-reply)");
    println!("  • #[plexspaces_handlers(gen_server)] - Handler dispatch");
    println!("  • #[handler(\"op\")] - Request-reply handlers");
    println!();
    println!("Supervision Strategies:");
    println!("  • OneForOne: Isolate failures (independent workers)");
    println!("  • OneForAll: Restart all (shared state workers)");
    println!("  • RestForOne: Restart downstream (ordered pipeline)");
    println!();
    println!("Restart Strategies:");
    println!("  • Permanent: Always restart (critical workers)");
    println!("  • Transient: Restart only on crash (not normal exit)");
    println!("  • Temporary: Never restart (one-shot tasks)");
    println!();
    println!("Real-World Use Cases:");
    println!("  • Payment processing (OneForOne - independent)");
    println!("  • Inventory management (OneForAll - shared cache)");
    println!("  • Order pipeline (RestForOne - ordered dependencies)");
    println!();

    Ok(())
}

// =============================================================================
// Helper: Create worker ChildSpec
// =============================================================================

/// Create a ChildSpec for a supervised worker
fn create_worker_spec(worker_id: &str, restart: RestartStrategy) -> ChildSpec {
    let id = worker_id.to_string();
    let id_for_factory = id.clone();
    
    // Create async factory for the worker
    let start_fn: StartFn = Arc::new(move || {
        let actor_id = id_for_factory.clone();
        Box::pin(async move {
            // Create mailbox
            let mailbox = Mailbox::new(MailboxConfig::default(), actor_id.clone())
                .await
                .map_err(|e| ActorError::InvalidState(e.to_string()))?;
            
            // Create actor with mock behavior (in real app, use actual worker)
            let actor = Actor::new(
                actor_id.clone(),
                Box::new(plexspaces_behavior::MockBehavior::new()),
                mailbox,
                "ecommerce-tenant".to_string(),
                "order-processing".to_string(),
                None,
            );
            
            // Create actor reference
            let actor_ref = CoreActorRef::new(actor_id)
                .map_err(|e| ActorError::InvalidState(e.to_string()))?;
            
            Ok(StartedChild::Worker { actor, actor_ref })
        })
    });
    
    ChildSpec::worker(id.clone(), id, start_fn)
        .with_restart(restart)
}
