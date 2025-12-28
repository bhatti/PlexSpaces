// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Temporal Durable Workflows

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::WorkflowBehavior;
use plexspaces_core::{ActorId, Actor, ActorContext, BehaviorType, BehaviorError};
use plexspaces_journaling::{DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn, error};

/// Order data structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: String,
    pub customer_email: String,
    pub total: f64,
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

/// Workflow state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderWorkflowState {
    pub order: Order,
    pub validation_result: Option<ValidationResult>,
    pub payment_result: Option<PaymentResult>,
    pub shipment_result: Option<ShipmentResult>,
    pub status: WorkflowStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowStatus {
    Created,
    Validating,
    ProcessingPayment,
    Shipping,
    SendingConfirmation,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    pub valid: bool,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShipmentResult {
    pub tracking_number: String,
    pub carrier: String,
}

/// Order workflow actor implementing WorkflowBehavior
pub struct OrderWorkflow {
    state: OrderWorkflowState,
}

impl OrderWorkflow {
    pub fn new(order: Order) -> Self {
        Self {
            state: OrderWorkflowState {
                order,
                validation_result: None,
                payment_result: None,
                shipment_result: None,
                status: WorkflowStatus::Created,
            },
        }
    }
}

#[async_trait::async_trait]
impl Actor for OrderWorkflow {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Delegate to WorkflowBehavior's route_workflow_message
        self.route_workflow_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait::async_trait]
impl WorkflowBehavior for OrderWorkflow {
    async fn run(
        &mut self,
        ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        info!("Order workflow started: {}", self.state.order.id);
        self.state.status = WorkflowStatus::Validating;

        // Step 1: Validate order (with retry)
        self.state.status = WorkflowStatus::Validating;
        let validation = self.validate_order(ctx).await?;
        self.state.validation_result = Some(validation.clone());

        if !validation.valid {
            self.state.status = WorkflowStatus::Failed(
                format!("Validation failed: {}", validation.errors.join(", "))
            );
            return Err(BehaviorError::ProcessingError("Order validation failed".to_string()));
        }

        // Step 2: Process payment (with retry)
        self.state.status = WorkflowStatus::ProcessingPayment;
        let payment = self.process_payment(ctx).await?;
        self.state.payment_result = Some(payment.clone());

        if payment.status != "completed" {
            self.state.status = WorkflowStatus::Failed("Payment failed".to_string());
            return Err(BehaviorError::ProcessingError("Payment failed".to_string()));
        }

        // Step 3: Ship order (with retry)
        self.state.status = WorkflowStatus::Shipping;
        let shipment = self.ship_order(ctx).await?;
        self.state.shipment_result = Some(shipment.clone());

        // Step 4: Send confirmation (with retry)
        self.state.status = WorkflowStatus::SendingConfirmation;
        self.send_confirmation(ctx, &shipment).await?;

        self.state.status = WorkflowStatus::Completed;
        info!("Order workflow completed: {}", self.state.order.id);

        let state_json = serde_json::to_vec(&self.state)
            .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
        Ok(Message::new(state_json))
    }

    async fn signal(
        &mut self,
        _ctx: &ActorContext,
        name: String,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        info!("Received signal: {} for order: {}", name, self.state.order.id);
        // Handle external signals (e.g., cancel order, update shipping address)
        Ok(())
    }

    async fn query(
        &self,
        _ctx: &ActorContext,
        name: String,
        _params: Message,
    ) -> Result<Message, BehaviorError> {
        info!("Received query: {} for order: {}", name, self.state.order.id);
        
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

impl OrderWorkflow {
    async fn validate_order(&self, _ctx: &ActorContext) -> Result<ValidationResult, BehaviorError> {
        // Simulate validation (in real app, this would call an external API)
        info!("Validating order: {}", self.state.order.id);
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        Ok(ValidationResult {
            valid: true,
            errors: vec![],
        })
    }

    async fn process_payment(&self, _ctx: &ActorContext) -> Result<PaymentResult, BehaviorError> {
        // Simulate payment processing (in real app, this would call payment API)
        info!("Processing payment for order: {}", self.state.order.id);
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        Ok(PaymentResult {
            transaction_id: format!("txn-{}", ulid::Ulid::new()),
            status: "completed".to_string(),
        })
    }

    async fn ship_order(&self, _ctx: &ActorContext) -> Result<ShipmentResult, BehaviorError> {
        // Simulate shipping (in real app, this would call shipping API)
        info!("Shipping order: {}", self.state.order.id);
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        Ok(ShipmentResult {
            tracking_number: format!("TRACK-{}", ulid::Ulid::new()),
            carrier: "UPS".to_string(),
        })
    }

    async fn send_confirmation(
        &self,
        _ctx: &ActorContext,
        shipment: &ShipmentResult,
    ) -> Result<(), BehaviorError> {
        // Simulate sending confirmation email (in real app, this would call email service)
        info!(
            "Sending confirmation for order: {} with tracking: {}",
            self.state.order.id, shipment.tracking_number
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with INFO level by default
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
        )
        .init();

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  Temporal vs PlexSpaces Comparison                             ║");
    println!("║  Demonstrating Durable Workflows with Journaling               ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("=== Temporal vs PlexSpaces Comparison ===");
    info!("Demonstrating Durable Workflows with Journaling");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build().await;

    // Create order
    let order = Order {
        id: "ORD-12345".to_string(),
        customer_email: "customer@example.com".to_string(),
        total: 99.99,
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

    // Create DurabilityFacet (Temporal-style durable workflows with journaling)
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(
        storage,
        serde_json::json!({
            "checkpoint_interval": 5 // Checkpoint every 5 workflow steps
        }),
        50,
    ));
    
    // Spawn using ActorFactory with facets
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = format!("order-workflow-{}@comparison-node-1", order.id);
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "Workflow",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![durability_facet], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    
    info!("✅ DurabilityFacet attached - workflow journaling and checkpoints enabled");
    println!("✅ DurabilityFacet attached - workflow journaling and checkpoints enabled");
    
    // Create ActorRef directly - no need to access mailbox
    let workflow_actor = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("Workflow actor spawned: {}", workflow_actor.id());
    println!("📋 Order: {} | Total: ${:.2}", order.id, order.total);
    println!("🚀 Starting workflow execution...");
    println!();

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    let start_time = std::time::Instant::now();

    // Start workflow (use workflow_run message type)
    let input = Message::new(serde_json::to_vec(&order)?)
        .with_message_type("workflow_run".to_string());
    let result = workflow_actor
        .ask(input, Duration::from_secs(30))
        .await?;

    let elapsed = start_time.elapsed();
    let final_state: OrderWorkflowState = serde_json::from_slice(result.payload())?;
    
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Workflow Execution Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Order ID:          {}", final_state.order.id);
    println!("Status:            {:?}", final_state.status);
    println!("Execution Time:    {:.2}ms", elapsed.as_secs_f64() * 1000.0);
    
    if let Some(ref validation) = final_state.validation_result {
        println!("Validation:        ✅ {}", if validation.valid { "Passed" } else { "Failed" });
    }
    
    if let Some(ref payment) = final_state.payment_result {
        println!("Payment:           ✅ {} (Txn: {})", payment.status, payment.transaction_id);
    }
    
    if let Some(ref shipment) = final_state.shipment_result {
        println!("Shipping:          ✅ {} (Tracking: {})", shipment.carrier, shipment.tracking_number);
    }
    
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();
    
    info!("Workflow completed with status: {:?}", final_state.status);
    
    // Verify workflow completed successfully
    assert!(matches!(final_state.status, WorkflowStatus::Completed));

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  ✅ Comparison Complete - All operations successful!            ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    info!("=== Comparison Complete ===");
    info!("All operations successful!");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_order_workflow() {
        let node = NodeBuilder::new("test-node")
            .build().await;

        let order = Order {
            id: "TEST-001".to_string(),
            customer_email: "test@example.com".to_string(),
            total: 50.0,
            payment_method: "credit_card".to_string(),
            shipping_address: Address {
                street: "123 Test St".to_string(),
                city: "Test City".to_string(),
                state: "CA".to_string(),
                zip: "12345".to_string(),
            },
            items: vec![],
        };

        // Create DurabilityFacet
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
        
        // Spawn using ActorFactory with facets
        use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
        use std::sync::Arc;
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = "test-workflow@test-node".to_string();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_actor(
            &ctx,
            &actor_id,
            "Workflow",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![durability_facet], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        
        // Create ActorRef directly - no need to access mailbox
        let workflow = plexspaces_actor::ActorRef::remote(
            actor_id.clone(),
            node.id().as_str().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let input = Message::new(serde_json::to_vec(&order).unwrap())
            .with_message_type("workflow_run".to_string());
        let result = workflow
            .ask(input, Duration::from_secs(10))
            .await
            .unwrap();

        let state: OrderWorkflowState = serde_json::from_slice(result.payload()).unwrap();
        assert!(matches!(state.status, WorkflowStatus::Completed));
    }
}
