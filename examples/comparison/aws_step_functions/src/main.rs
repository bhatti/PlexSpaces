// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: AWS Step Functions State Machine Workflow

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::WorkflowBehavior;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, Reply};
use plexspaces_journaling::{DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Order request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderRequest {
    pub order_id: String,
    pub customer_email: String,
    pub amount: f64,
    pub payment_method: String,
    pub shipping_address: Address,
    pub items: Vec<OrderItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Address {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub sku: String,
    pub quantity: u32,
    pub price: f64,
}

/// Order state machine workflow (AWS Step Functions-style)
/// Demonstrates: State machine workflow with explicit state transitions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderState {
    ValidateOrder,
    ProcessPayment,
    ShipOrder,
    OrderCompleted,
    OrderFailed(String),
}

/// Order workflow state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderWorkflowState {
    pub order: OrderRequest,
    pub current_state: OrderState,
    pub validation_result: Option<ValidationResult>,
    pub payment_result: Option<PaymentResult>,
    pub shipment_result: Option<ShipmentResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub status: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipmentResult {
    pub tracking_number: String,
    pub carrier: String,
}

/// Order state machine workflow actor
pub struct OrderStateMachine {
    state: OrderWorkflowState,
}

impl OrderStateMachine {
    pub fn new(order: OrderRequest) -> Self {
        Self {
            state: OrderWorkflowState {
                order,
                current_state: OrderState::ValidateOrder,
                validation_result: None,
                payment_result: None,
                shipment_result: None,
            },
        }
    }

    /// State: ValidateOrder (Task in AWS Step Functions)
    async fn validate_order(&self, order: &OrderRequest) -> Result<ValidationResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[STATE: ValidateOrder] Validating order: {}", order.order_id);
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        if order.amount <= 0.0 {
            return Ok(ValidationResult {
                valid: false,
                error: Some("Invalid amount".to_string()),
            });
        }
        
        Ok(ValidationResult {
            valid: true,
            error: None,
        })
    }

    /// State: ProcessPayment (Task in AWS Step Functions)
    async fn process_payment(&self, order: &OrderRequest) -> Result<PaymentResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[STATE: ProcessPayment] Processing payment for order: {}", order.order_id);
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        Ok(PaymentResult {
            transaction_id: format!("txn-{}", ulid::Ulid::new()),
            status: "completed".to_string(),
            error: None,
        })
    }

    /// State: ShipOrder (Task in AWS Step Functions)
    async fn ship_order(&self, order: &OrderRequest) -> Result<ShipmentResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[STATE: ShipOrder] Shipping order: {}", order.order_id);
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        Ok(ShipmentResult {
            tracking_number: format!("TRACK-{}", ulid::Ulid::new()),
            carrier: "UPS".to_string(),
        })
    }
}

#[async_trait::async_trait]
impl Actor for OrderStateMachine {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        // Delegate to WorkflowBehavior's route_workflow_message
        <Self as WorkflowBehavior>::route_workflow_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait::async_trait]
impl WorkflowBehavior for OrderStateMachine {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let order: OrderRequest = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        info!("[WORKFLOW] Starting order state machine: {}", order.order_id);
        
        // State 1: ValidateOrder
        // In AWS Step Functions: "ValidateOrder" Task state
        // In PlexSpaces: Explicit state transition with error handling
        self.state.current_state = OrderState::ValidateOrder;
        let validation = self.validate_order(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Validation failed: {}", e)))?;
        
        self.state.validation_result = Some(validation.clone());
        if !validation.valid {
            self.state.current_state = OrderState::OrderFailed(
                validation.error.unwrap_or_else(|| "Validation failed".to_string())
            );
            return Err(BehaviorError::ProcessingError("Order validation failed".to_string()));
        }
        
        // State 2: ProcessPayment
        // In AWS Step Functions: "ProcessPayment" Task state with retry
        // In PlexSpaces: Explicit state transition with error handling
        self.state.current_state = OrderState::ProcessPayment;
        let payment = self.process_payment(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Payment failed: {}", e)))?;
        
        self.state.payment_result = Some(payment.clone());
        if payment.status != "completed" {
            self.state.current_state = OrderState::OrderFailed(
                payment.error.unwrap_or_else(|| "Payment failed".to_string())
            );
            return Err(BehaviorError::ProcessingError("Payment failed".to_string()));
        }
        
        // State 3: ShipOrder
        // In AWS Step Functions: "ShipOrder" Task state
        // In PlexSpaces: Explicit state transition
        self.state.current_state = OrderState::ShipOrder;
        let shipment = self.ship_order(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Shipping failed: {}", e)))?;
        
        self.state.shipment_result = Some(shipment.clone());
        
        // State 4: OrderCompleted
        // In AWS Step Functions: "OrderCompleted" Succeed state
        // In PlexSpaces: Final state reached
        self.state.current_state = OrderState::OrderCompleted;
        info!("[WORKFLOW] Order state machine completed: {}", order.order_id);
        
        let result = format!("Order {} processed successfully", order.order_id);
        Ok(Message::new(result.into_bytes()))
    }

    async fn signal(
        &mut self,
        _ctx: &ActorContext,
        name: String,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        info!("[WORKFLOW] Received signal: {} for order: {}", name, self.state.order.order_id);
        // Handle external signals (e.g., cancel order)
        Ok(())
    }

    async fn query(
        &self,
        _ctx: &ActorContext,
        name: String,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        match name.as_str() {
            "state" => {
                let state_json = serde_json::to_string(&self.state.current_state)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(state_json.into_bytes()))
            }
            "status" => {
                let status = match &self.state.current_state {
                    OrderState::OrderCompleted => "completed",
                    OrderState::OrderFailed(_) => "failed",
                    _ => "in_progress",
                };
                Ok(Message::new(status.to_string().into_bytes()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown query".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== AWS Step Functions vs PlexSpaces Comparison ===");
    info!("Demonstrating State Machine Workflow Orchestration");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Create order
    let order = OrderRequest {
        order_id: "ORD-12345".to_string(),
        customer_email: "customer@example.com".to_string(),
        amount: 99.99,
        payment_method: "credit_card".to_string(),
        shipping_address: Address {
            street: "123 Main St".to_string(),
            city: "Anytown".to_string(),
            state: "CA".to_string(),
            zip: "12345".to_string(),
        },
        items: vec![
            OrderItem {
                sku: "ITEM-001".to_string(),
                quantity: 2,
                price: 49.99,
            },
        ],
    };

    // Create state machine workflow (AWS Step Functions-style)
    let behavior = Box::new(OrderStateMachine::new(order.clone()));
    let mut actor = ActorBuilder::new(behavior)
        .with_id(format!("order-state-machine-{}@comparison-node-1", order.order_id))
        .build()
        .await;
    
    // Attach DurabilityFacet (AWS Step Functions-style durable execution)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig {
        checkpoint_interval: 3, // Checkpoint every 3 state transitions
        ..Default::default()
    };
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    info!("✅ DurabilityFacet attached - state machine journaling enabled");
    
    let core_ref = node
        .spawn_actor(actor)
        .await?;

    // Convert to actor::ActorRef for ask method
    let mailbox = node.actor_registry()
        .lookup_mailbox(core_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let state_machine = plexspaces_actor::ActorRef::local(
        core_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("State machine workflow spawned: {}", state_machine.id());

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start state machine (AWS Step Functions: StartExecution)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Starting state machine (AWS Step Functions: StartExecution)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let input = Message::new(serde_json::to_vec(&order)?)
        .with_message_type("workflow_run".to_string());
    
    let result = state_machine
        .ask(input, Duration::from_secs(30))
        .await?;

    let result_str = String::from_utf8(result.payload().to_vec())?;
    info!("✅ State machine completed: {}", result_str);
    assert!(result_str.contains("processed successfully"));

    // Query state machine status (AWS Step Functions: DescribeExecution)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Querying state machine status (AWS Step Functions: DescribeExecution)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let query_msg = Message::new(b"status".to_vec())
        .with_message_type("workflow_query:status".to_string());
    
    let query_result = state_machine
        .ask(query_msg, Duration::from_secs(5))
        .await?;

    let status: String = String::from_utf8(query_result.payload().to_vec())?;
    info!("✅ State machine status: {}", status);
    assert_eq!(status, "completed");

    info!("=== Comparison Complete ===");
    info!("✅ State machine workflow: Explicit state transitions (AWS Step Functions pattern)");
    info!("✅ WorkflowBehavior: Orchestration with state tracking");
    info!("✅ DurabilityFacet: All state transitions journaled");
    info!("✅ Query support: Can query current state and status");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_order_state_machine() {
        let node = NodeBuilder::new("test-node")
            .build();

        let order = OrderRequest {
            order_id: "TEST-001".to_string(),
            customer_email: "test@example.com".to_string(),
            amount: 50.0,
            payment_method: "credit_card".to_string(),
            shipping_address: Address {
                street: "123 Test St".to_string(),
                city: "Test City".to_string(),
                state: "CA".to_string(),
                zip: "12345".to_string(),
            },
            items: vec![],
        };

        let behavior = Box::new(OrderStateMachine::new(order.clone()));
        let mut actor = ActorBuilder::new(behavior)
            .with_id("test-state-machine@test-node".to_string())
            .build()
            .await;
        
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
        actor
            .attach_facet(durability_facet, 50, serde_json::json!({}))
            .await
            .unwrap();
        
        let core_ref = node
            .spawn_actor(actor)
            .await
            .unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(core_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let state_machine = plexspaces_actor::ActorRef::local(
            core_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let input = Message::new(serde_json::to_vec(&order).unwrap())
            .with_message_type("workflow_run".to_string());
        
        let result = state_machine
            .ask(input, Duration::from_secs(10))
            .await
            .unwrap();

        let result_str = String::from_utf8(result.payload().to_vec()).unwrap();
        assert!(result_str.contains("processed successfully"));
    }
}
