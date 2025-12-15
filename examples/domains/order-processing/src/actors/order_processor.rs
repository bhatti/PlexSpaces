// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Order Processor Actor using hybrid behavior approach:
// - GenServerBehavior for queries (GetOrder) - request/reply pattern
// - ActorBehavior for commands (CreateOrder, CancelOrder) - fire-and-forget
// - Typed messages (ActorMessage enum)
// - ConfigBootstrap for configuration
// - CoordinationComputeTracker for metrics
//
// ## Behavior Selection Guide
// - **GenServerBehavior**: Use for operations that need a reply (queries)
//   - GetOrder: Returns order details synchronously
//   - GetOrderStatus: Returns current state
// - **ActorBehavior**: Use for operations that don't need a reply (commands/events)
//   - CreateOrder: Creates order, publishes event
//   - CancelOrder: Cancels order, publishes event

use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_behavior::GenServerBehavior;
use plexspaces_mailbox::{ActorMessage, Message};
use plexspaces_node::{ConfigBootstrap, CoordinationComputeTracker};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::{info, warn};

use crate::types::{Order, OrderError, OrderItem};

/// Order configuration (loaded from release.toml or environment)
#[derive(Debug, Deserialize, Default)]
pub struct OrderConfig {
    pub max_orders: usize,
    pub enable_metrics: bool,
}

/// Order messages (will be serialized and sent via ActorMessage::User)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderMessage {
    CreateOrder {
        customer_id: String,
        items: Vec<OrderItem>,
    },
    GetOrder {
        order_id: String,
    },
    CancelOrder {
        order_id: String,
    },
}

/// Order Processor Behavior
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
}

// Hybrid approach: Use both ActorBehavior and GenServerBehavior
// ActorBehavior handles commands (fire-and-forget)
#[async_trait::async_trait]
impl ActorBehavior for OrderProcessorBehavior {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        message: Message,
    ) -> Result<(), BehaviorError> {
        // Handle commands (fire-and-forget) via ActorBehavior
        // Queries are handled by GenServerBehavior below
        if let Ok(typed_msg) = message.as_typed() {
            match typed_msg {
                ActorMessage::User { payload, .. } => {
                    // Deserialize OrderMessage from payload
                    match serde_json::from_slice::<OrderMessage>(&payload) {
                        Ok(order_msg) => {
                            match order_msg {
                                // Commands: handled by ActorBehavior (fire-and-forget)
                                OrderMessage::CreateOrder { customer_id, items } => {
                                    self.handle_create_order(ctx, customer_id, items).await?;
                                }
                                OrderMessage::CancelOrder { order_id } => {
                                    self.handle_cancel_order(ctx, order_id).await?;
                                }
                                // Queries: handled by GenServerBehavior (request/reply)
                                OrderMessage::GetOrder { .. } => {
                                    // This will be handled by GenServerBehavior::handle_request
                                    // For now, just log that it's a query
                                    info!("GetOrder query received - handled by GenServerBehavior");
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to deserialize OrderMessage: {}", e);
                            return Err(BehaviorError::ProcessingError(format!("Deserialization error: {}", e)));
                        }
                    }
                }
                _ => {
                    // Ignore other message types (TimerFired, ReminderFired, System)
                }
            }
        }
        
        Ok(())
    }
    
    fn behavior_type(&self) -> BehaviorType {
        // Using GenServer for queries, but also support ActorBehavior for commands
        BehaviorType::GenServer
    }
}

// GenServerBehavior handles queries (request/reply)
#[async_trait::async_trait]
impl GenServerBehavior for OrderProcessorBehavior {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        // Handle queries that need a reply
        if let Ok(typed_msg) = msg.as_typed() {
            match typed_msg {
                ActorMessage::User { payload, .. } => {
                    match serde_json::from_slice::<OrderMessage>(&payload) {
                        Ok(OrderMessage::GetOrder { order_id }) => {
                            match self.orders.get(&order_id) {
                                Some(order) => {
                                    info!(
                                        order_id = %order.order_id,
                                        state = ?order.state,
                                        "Order retrieved via GenServer"
                                    );
                                    // Return order as reply
                                    let reply_payload = serde_json::to_vec(order)
                                        .map_err(|e| BehaviorError::ProcessingError(format!("Serialization error: {}", e)))?;
                                    Ok(Message::new(reply_payload))
                                }
                                None => {
                                    warn!(order_id = %order_id, "Order not found");
                                    Err(BehaviorError::ProcessingError(format!("Order not found: {}", order_id)))
                                }
                            }
                        }
                        _ => {
                            // Other message types handled by ActorBehavior
                            Err(BehaviorError::UnsupportedMessage)
                        }
                    }
                }
                _ => Err(BehaviorError::UnsupportedMessage),
            }
        } else {
            Err(BehaviorError::ProcessingError("Invalid message format".to_string()))
        }
    }
}

impl OrderProcessorBehavior {
    async fn handle_create_order(
        &mut self,
        _ctx: &ActorContext,
        customer_id: String,
        items: Vec<OrderItem>,
    ) -> Result<(), BehaviorError> {
        // Track coordination vs compute
        if let Some(ref mut tracker) = self.metrics_tracker {
            tracker.start_compute();
        }
        
        let mut order = Order::new(customer_id, items);
        
        // Check max orders limit
        if self.orders.len() >= self.config.max_orders {
            return Err(BehaviorError::ProcessingError("Max orders limit reached".to_string()));
        }
        
        order.start_payment_processing()
            .map_err(|e| BehaviorError::ProcessingError(format!("State transition error: {:?}", e)))?;
        
        self.orders.insert(order.order_id.clone(), order.clone());
        
        if let Some(ref mut tracker) = self.metrics_tracker {
            tracker.end_compute();
            tracker.increment_message();
        }
        
        info!(
            order_id = %order.order_id,
            customer_id = %order.customer_id,
            total = order.total_amount,
            "Order created"
        );
        
        Ok(())
    }
    
    // Note: handle_get_order is now handled by GenServerBehavior::handle_request
    // This method is kept for reference but not used
    
    async fn handle_cancel_order(
        &mut self,
        _ctx: &ActorContext,
        order_id: String,
    ) -> Result<(), BehaviorError> {
        match self.orders.get_mut(&order_id) {
            Some(order) => {
                order.cancel()
                    .map_err(|e| BehaviorError::ProcessingError(format!("Cancel error: {:?}", e)))?;
                
                info!(
                    order_id = %order_id,
                    "Order cancelled"
                );
                
                Ok(())
            }
            None => {
                warn!(order_id = %order_id, "Order not found for cancellation");
                Err(BehaviorError::ProcessingError(format!("Order not found: {}", order_id)))
            }
        }
    }
    
    /// Get final metrics report (for testing/debugging)
    pub fn get_metrics(&mut self) -> Option<plexspaces_proto::metrics::v1::CoordinationComputeMetrics> {
        self.metrics_tracker.take().map(|tracker| tracker.finalize())
    }
}

