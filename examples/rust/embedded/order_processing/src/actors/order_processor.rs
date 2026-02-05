// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Order Processor Actor using SDK annotations:
// - #[gen_server_actor] - generates Actor trait implementation
// - #[plexspaces_handlers(gen_server)] - generates GenServer dispatch
// - #[handler("op")] - routes messages to handler methods
//
// ## Message Routing
// - "create_order" -> handle_create_order (creates new order)
// - "get_order" -> handle_get_order (returns order details)
// - "cancel_order" -> handle_cancel_order (cancels order)
// - "list_orders" -> handle_list_orders (returns all orders)

use plexspaces_sdk::{
    gen_server_actor, plexspaces_handlers, handler,
    ActorContext, BehaviorError, Message, json, Value,
};
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

use crate::types::{Order, OrderItem};

// Required for macro-generated code
extern crate plexspaces_core;
extern crate plexspaces_behavior;

/// Order configuration (loaded from release.toml or environment)
#[derive(Debug, Deserialize, Default)]
pub struct OrderConfig {
    pub max_orders: usize,
    pub enable_metrics: bool,
}

/// Order Processor Actor using SDK annotations.
/// 
/// ## Annotations
/// - `#[gen_server_actor]` - generates `impl Actor` with GenServer behavior
/// - `#[plexspaces_handlers(gen_server)]` - generates `impl GenServer` dispatching to `#[handler]` methods
#[gen_server_actor]
pub struct OrderProcessorBehavior {
    orders: HashMap<String, Order>,
    config: OrderConfig,
    metrics_tracker: Option<CoordinationComputeTracker>,
}

impl OrderProcessorBehavior {
    pub fn new() -> Self {
        // Load config using ConfigBootstrap (Erlang/OTP-style)
        let config: OrderConfig = ConfigBootstrap::load().unwrap_or_default();
        
        let enable_metrics = config.enable_metrics;
        
        Self {
            orders: HashMap::new(),
            config,
            metrics_tracker: if enable_metrics {
                Some(CoordinationComputeTracker::new("order-processor".to_string()))
            } else {
                None
            },
        }
    }
    
    /// Get final metrics report (for testing/debugging)
    pub fn get_metrics(&mut self) -> Option<plexspaces_proto::metrics::v1::CoordinationComputeMetrics> {
        self.metrics_tracker.take().map(|tracker| tracker.finalize())
    }
}

/// Request/response types for order operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOrderRequest {
    pub customer_id: String,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetOrderRequest {
    pub order_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CancelOrderRequest {
    pub order_id: String,
}

/// Handler implementations - SDK scans these and generates GenServer dispatch.
/// 
/// Each `#[handler("op")]` method is called when `payload.action == "op"`.
/// For GenServer, all handlers default to "call" (request-reply).
/// Return `Result<Value, BehaviorError>` - SDK serializes and sends reply automatically.
#[plexspaces_handlers(gen_server)]
impl OrderProcessorBehavior {
    /// Handle "create_order" action - creates a new order
    #[handler("create_order")]
    async fn handle_create_order(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        // Track coordination vs compute
        if let Some(ref mut tracker) = self.metrics_tracker {
            tracker.start_compute();
        }
        
        // Parse request from payload
        let request: CreateOrderRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse request: {}", e)))?;
        
        // Check max orders limit
        if self.orders.len() >= self.config.max_orders && self.config.max_orders > 0 {
            return Err(BehaviorError::ProcessingError("Max orders limit reached".to_string()));
        }
        
        let mut order = Order::new(request.customer_id.clone(), request.items);
        
        order.start_payment_processing()
            .map_err(|e| BehaviorError::ProcessingError(format!("State transition error: {:?}", e)))?;
        
        let order_id = order.order_id.clone();
        let total = order.total_amount;
        
        self.orders.insert(order.order_id.clone(), order);
        
        if let Some(ref mut tracker) = self.metrics_tracker {
            tracker.end_compute();
            tracker.increment_message();
        }
        
        info!(
            order_id = %order_id,
            customer_id = %request.customer_id,
            total = total,
            "Order created"
        );
        
        Ok(json!({
            "success": true,
            "order_id": order_id,
            "total_amount": total,
            "status": "payment_processing"
        }))
    }
    
    /// Handle "get_order" action - returns order details
    #[handler("get_order")]
    async fn handle_get_order(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        // Parse request from payload
        let request: GetOrderRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse request: {}", e)))?;
        
        match self.orders.get(&request.order_id) {
            Some(order) => {
                info!(
                    order_id = %order.order_id,
                    state = ?order.state,
                    "Order retrieved"
                );
                
                Ok(json!({
                    "success": true,
                    "order": {
                        "order_id": order.order_id,
                        "customer_id": order.customer_id,
                        "total_amount": order.total_amount,
                        "state": format!("{:?}", order.state),
                        "item_count": order.items.len()
                    }
                }))
            }
            None => {
                warn!(order_id = %request.order_id, "Order not found");
                Err(BehaviorError::ProcessingError(format!("Order not found: {}", request.order_id)))
            }
        }
    }
    
    /// Handle "cancel_order" action - cancels an order
    #[handler("cancel_order")]
    async fn handle_cancel_order(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<Value, BehaviorError> {
        // Parse request from payload
        let request: CancelOrderRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse request: {}", e)))?;
        
        match self.orders.get_mut(&request.order_id) {
            Some(order) => {
                order.cancel()
                    .map_err(|e| BehaviorError::ProcessingError(format!("Cancel error: {:?}", e)))?;
                
                info!(
                    order_id = %request.order_id,
                    "Order cancelled"
                );
                
                Ok(json!({
                    "success": true,
                    "order_id": request.order_id,
                    "status": "cancelled"
                }))
            }
            None => {
                warn!(order_id = %request.order_id, "Order not found for cancellation");
                Err(BehaviorError::ProcessingError(format!("Order not found: {}", request.order_id)))
            }
        }
    }
    
    /// Handle "list_orders" action - returns all orders
    #[handler("list_orders")]
    async fn handle_list_orders(
        &mut self,
        _ctx: &ActorContext,
        _msg: &Message,
    ) -> Result<Value, BehaviorError> {
        let orders: Vec<Value> = self.orders.values()
            .map(|order| json!({
                "order_id": order.order_id,
                "customer_id": order.customer_id,
                "total_amount": order.total_amount,
                "state": format!("{:?}", order.state),
                "item_count": order.items.len()
            }))
            .collect();
        
        info!(count = orders.len(), "Listed orders");
        
        Ok(json!({
            "success": true,
            "orders": orders,
            "count": orders.len()
        }))
    }
}
