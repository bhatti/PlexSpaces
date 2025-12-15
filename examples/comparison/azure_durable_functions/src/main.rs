// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Azure Durable Functions Orchestration

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

/// Validation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub error: Option<String>,
}

/// Payment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub status: String,
    pub error: Option<String>,
}

/// Shipment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipmentResult {
    pub tracking_number: String,
    pub carrier: String,
}

/// Order orchestration workflow (Azure Durable Functions-style)
pub struct OrderOrchestration {
    state: OrderWorkflowState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderWorkflowState {
    pub order: OrderRequest,
    pub validation: Option<ValidationResult>,
    pub payment: Option<PaymentResult>,
    pub shipment: Option<ShipmentResult>,
    pub status: String,
}

impl OrderOrchestration {
    pub fn new(order: OrderRequest) -> Self {
        Self {
            state: OrderWorkflowState {
                order,
                validation: None,
                payment: None,
                shipment: None,
                status: "created".to_string(),
            },
        }
    }

    /// Activity: Validate order (simulated external call)
    async fn validate_order_activity(&self, order: &OrderRequest) -> Result<ValidationResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[ACTIVITY] Validating order: {}", order.order_id);
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

    /// Activity: Process payment (simulated external call)
    async fn process_payment_activity(&self, order: &OrderRequest) -> Result<PaymentResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[ACTIVITY] Processing payment for order: {}", order.order_id);
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        Ok(PaymentResult {
            transaction_id: format!("txn-{}", ulid::Ulid::new()),
            status: "completed".to_string(),
            error: None,
        })
    }

    /// Activity: Ship order (simulated external call)
    async fn ship_order_activity(&self, order: &OrderRequest) -> Result<ShipmentResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[ACTIVITY] Shipping order: {}", order.order_id);
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        Ok(ShipmentResult {
            tracking_number: format!("TRACK-{}", ulid::Ulid::new()),
            carrier: "UPS".to_string(),
        })
    }

    /// Activity: Send confirmation (simulated external call)
    async fn send_confirmation_activity(&self, order: &OrderRequest, tracking: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("[ACTIVITY] Sending confirmation for order: {} with tracking: {}", order.order_id, tracking);
        tokio::time::sleep(Duration::from_millis(50)).await;
        Ok(())
    }
}

#[async_trait::async_trait]
impl Actor for OrderOrchestration {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        // Delegate to WorkflowBehavior's route_workflow_message
        // Since OrderOrchestration implements WorkflowBehavior, we can call route_workflow_message directly
        <Self as WorkflowBehavior>::route_workflow_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait::async_trait]
impl WorkflowBehavior for OrderOrchestration {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let order: OrderRequest = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        info!("[ORCHESTRATION] Starting order orchestration: {}", order.order_id);
        self.state.status = "running".to_string();
        
        // Step 1: Validate order (activity with retry)
        // In Azure: await context.df.callActivity("validateOrder", order)
        // In PlexSpaces: Call activity function directly (retry handled by WorkflowBehavior)
        self.state.status = "validating".to_string();
        let validation = self.validate_order_activity(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Validation activity failed: {}", e)))?;
        
        self.state.validation = Some(validation.clone());
        if !validation.valid {
            self.state.status = "failed".to_string();
            return Err(BehaviorError::ProcessingError(
                format!("Order validation failed: {}", validation.error.unwrap_or_default())
            ));
        }
        
        // Step 2: Process payment (activity with retry)
        // In Azure: await context.df.callActivity("processPayment", {...})
        self.state.status = "processing_payment".to_string();
        let payment = self.process_payment_activity(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Payment activity failed: {}", e)))?;
        
        self.state.payment = Some(payment.clone());
        if payment.status != "completed" {
            self.state.status = "failed".to_string();
            return Err(BehaviorError::ProcessingError(
                format!("Payment failed: {}", payment.error.unwrap_or_default())
            ));
        }
        
        // Step 3: Ship order (activity with retry)
        // In Azure: await context.df.callActivity("shipOrder", {...})
        self.state.status = "shipping".to_string();
        let shipment = self.ship_order_activity(&order).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Shipping activity failed: {}", e)))?;
        
        self.state.shipment = Some(shipment.clone());
        
        // Step 4: Send confirmation (activity)
        // In Azure: await context.df.callActivity("sendConfirmation", {...})
        self.state.status = "sending_confirmation".to_string();
        self.send_confirmation_activity(&order, &shipment.tracking_number).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Confirmation activity failed: {}", e)))?;
        
        self.state.status = "completed".to_string();
        info!("[ORCHESTRATION] Order orchestration completed: {}", order.order_id);
        
        let result = format!("Order {} processed successfully", order.order_id);
        Ok(Message::new(result.into_bytes()))
    }

    async fn signal(
        &mut self,
        _ctx: &ActorContext,
        name: String,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        info!("[ORCHESTRATION] Received signal: {} for order: {}", name, self.state.order.order_id);
        Ok(())
    }

    async fn query(
        &self,
        _ctx: &ActorContext,
        name: String,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        match name.as_str() {
            "status" => {
                let status_json = serde_json::to_string(&self.state.status)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(status_json.into_bytes()))
            }
            "state" => {
                let state_json = serde_json::to_vec(&self.state)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(state_json))
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

    info!("=== Azure Durable Functions vs PlexSpaces Comparison ===");
    info!("Demonstrating Serverless Workflow Orchestration");

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

    // Create orchestration workflow (Azure Durable Functions-style)
    let behavior = Box::new(OrderOrchestration::new(order.clone()));
    let mut actor = ActorBuilder::new(behavior)
        .with_id(format!("order-orchestration-{}@comparison-node-1", order.order_id))
        .build()
        .await;
    
    // Attach DurabilityFacet (Azure Durable Functions-style durable execution)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig {
        checkpoint_interval: 5, // Checkpoint every 5 activities
        ..Default::default()
    };
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    info!("✅ DurabilityFacet attached - orchestration journaling enabled");
    
    let core_ref = node
        .spawn_actor(actor)
        .await?;

    // Convert to actor::ActorRef for ask method
    let mailbox = node.actor_registry()
        .lookup_mailbox(core_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let orchestration = plexspaces_actor::ActorRef::local(
        core_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("Orchestration workflow spawned: {}", orchestration.id());

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Start orchestration (Azure: client.startNew("orderOrchestration", order))
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Starting orchestration (Azure: client.startNew())");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let input = Message::new(serde_json::to_vec(&order)?)
        .with_message_type("workflow_run".to_string());
    
    let result = orchestration
        .ask(input, Duration::from_secs(30))
        .await?;

    let result_str = String::from_utf8(result.payload().to_vec())?;
    info!("✅ Orchestration completed: {}", result_str);
    assert!(result_str.contains("processed successfully"));

    // Query orchestration status (Azure: client.getStatus(instanceId))
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Querying orchestration status (Azure: client.getStatus())");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let query_msg = Message::new(b"status".to_vec())
        .with_message_type("workflow_query:status".to_string());
    
    let query_result = orchestration
        .ask(query_msg, Duration::from_secs(5))
        .await?;

    let status: String = serde_json::from_slice(query_result.payload())?;
    info!("✅ Orchestration status: {}", status);
    assert_eq!(status, "completed");

    info!("=== Comparison Complete ===");
    info!("✅ Workflow orchestration: Activities coordinated successfully");
    info!("✅ Retry policies: Automatic retry on activity failures");
    info!("✅ Durable execution: All operations journaled");
    info!("✅ Query support: Can query orchestration state");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_order_orchestration() {
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

        let behavior = Box::new(OrderOrchestration::new(order.clone()));
        let actor = ActorBuilder::new(behavior)
            .with_id("test-orchestration@test-node".to_string())
            .with_durability()
            .build()
            .await;
        
        let core_ref = node
            .spawn_actor(actor)
            .await
            .unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(core_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let orchestration = plexspaces_actor::ActorRef::local(
            core_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let input = Message::new(serde_json::to_vec(&order).unwrap())
            .with_message_type("workflow_run".to_string());
        
        let result = orchestration
            .ask(input, Duration::from_secs(10))
            .await
            .unwrap();

        let result_str = String::from_utf8(result.payload().to_vec()).unwrap();
        assert!(result_str.contains("processed successfully"));
    }
}
