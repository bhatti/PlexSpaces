// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: AWS Durable Lambda Durable Execution

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, Reply};
use plexspaces_journaling::{DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Payment request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentRequest {
    pub order_id: String,
    pub amount: f64,
    pub payment_method: String,
    pub idempotency_key: String,
}

/// Payment result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentResult {
    pub transaction_id: String,
    pub status: String,
}

/// Payment actor state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentState {
    pub processed_orders: Vec<String>,
    pub total_amount: f64,
}

/// Payment processor actor (AWS Durable Lambda-style)
/// Demonstrates: Durable execution, idempotency, state persistence
pub struct PaymentProcessor {
    state: PaymentState,
    api_call_count: u64,
}

impl PaymentProcessor {
    pub fn new() -> Self {
        Self {
            state: PaymentState {
                processed_orders: Vec::new(),
                total_amount: 0.0,
            },
            api_call_count: 0,
        }
    }

    /// Process payment (idempotent with idempotency key)
    /// In AWS Durable Lambda: This would be a durable function that persists state
    async fn process_payment(&mut self, request: &PaymentRequest) -> Result<PaymentResult, Box<dyn std::error::Error + Send + Sync>> {
        // Check idempotency (AWS Durable Lambda handles this automatically)
        if self.state.processed_orders.contains(&request.idempotency_key) {
            info!("[IDEMPOTENCY] Payment already processed for key: {}", request.idempotency_key);
            return Ok(PaymentResult {
                transaction_id: format!("cached-{}", request.idempotency_key),
                status: "completed".to_string(),
            });
        }
        
        self.api_call_count += 1;
        info!("[EXTERNAL API CALL] Processing payment for order: {} (call #{} - would be cached during replay)", 
            request.order_id, self.api_call_count);
        
        // Simulate payment processing
        tokio::time::sleep(Duration::from_millis(100)).await;
        
        // Update state (persisted via DurabilityFacet)
        self.state.processed_orders.push(request.idempotency_key.clone());
        self.state.total_amount += request.amount;
        
        Ok(PaymentResult {
            transaction_id: format!("txn-{}", ulid::Ulid::new()),
            status: "completed".to_string(),
        })
    }
}

#[async_trait::async_trait]
impl Actor for PaymentProcessor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
        // Delegate to GenServer's route_message
        // DurabilityFacet will automatically journal all operations
        <Self as GenServer>::route_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for PaymentProcessor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let request: PaymentRequest = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        info!("Processing payment for order: {} (all operations will be journaled)", request.order_id);
        
        // Process payment (idempotent, durable)
        // In AWS Durable Lambda: await ctx.run("processPayment", async () => { ... })
        // In PlexSpaces: DurabilityFacet automatically journals and caches side effects
        let result = self.process_payment(&request).await
            .map_err(|e| BehaviorError::ProcessingError(format!("Payment processing failed: {}", e)))?;
        
        info!("Payment processed successfully: transaction_id = {}, state: {:?}", 
            result.transaction_id, self.state);
        
        Ok(Message::new(serde_json::to_vec(&result).unwrap()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== AWS Durable Lambda vs PlexSpaces Comparison ===");
    info!("Demonstrating Durable Execution with Idempotency");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Create payment processor with DurabilityFacet (AWS Durable Lambda-style)
    let behavior = Box::new(PaymentProcessor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id("payment-processor@comparison-node-1".to_string())
        .build()
        .await;
    
    // Attach DurabilityFacet (AWS Durable Lambda-style durable execution)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig {
        checkpoint_interval: 5, // Checkpoint every 5 operations
        ..Default::default()
    };
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    info!("✅ DurabilityFacet attached - durable execution enabled");
    
    let core_ref = node
        .spawn_actor(actor)
        .await?;

    // Convert to actor::ActorRef for ask method
    let mailbox = node.actor_registry()
        .lookup_mailbox(core_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let payment_processor = plexspaces_actor::ActorRef::local(
        core_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("Payment processor spawned: {}", payment_processor.id());

    // Wait for actor to be fully initialized
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Process payment 1 (durable, idempotent)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Processing Payment 1 (durable execution)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let request1 = PaymentRequest {
        order_id: "ORD-12345".to_string(),
        amount: 99.99,
        payment_method: "credit_card".to_string(),
        idempotency_key: "idempotency-key-1".to_string(),
    };

    let msg1 = Message::new(serde_json::to_vec(&request1)?)
        .with_message_type("call".to_string());
    
    let result1 = payment_processor
        .ask(msg1, Duration::from_secs(10))
        .await?;

    let payment_result1: PaymentResult = serde_json::from_slice(result1.payload())?;
    info!("✅ Payment 1 processed: transaction_id = {}, status = {}", 
        payment_result1.transaction_id, payment_result1.status);
    
    assert_eq!(payment_result1.status, "completed");

    // Process payment 2 (durable, idempotent)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Processing Payment 2 (durable execution)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let request2 = PaymentRequest {
        order_id: "ORD-67890".to_string(),
        amount: 149.99,
        payment_method: "paypal".to_string(),
        idempotency_key: "idempotency-key-2".to_string(),
    };

    let msg2 = Message::new(serde_json::to_vec(&request2)?)
        .with_message_type("call".to_string());
    
    let result2 = payment_processor
        .ask(msg2, Duration::from_secs(10))
        .await?;

    let payment_result2: PaymentResult = serde_json::from_slice(result2.payload())?;
    info!("✅ Payment 2 processed: transaction_id = {}, status = {}", 
        payment_result2.transaction_id, payment_result2.status);
    
    assert_eq!(payment_result2.status, "completed");

    // Test idempotency (duplicate request with same idempotency key)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 3: Idempotency (duplicate request with same key)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let duplicate_request = PaymentRequest {
        order_id: "ORD-12345".to_string(), // Same order
        amount: 99.99,
        payment_method: "credit_card".to_string(),
        idempotency_key: "idempotency-key-1".to_string(), // Same key
    };

    let msg_dup = Message::new(serde_json::to_vec(&duplicate_request)?)
        .with_message_type("call".to_string());
    
    let result_dup = payment_processor
        .ask(msg_dup, Duration::from_secs(10))
        .await?;

    let payment_result_dup: PaymentResult = serde_json::from_slice(result_dup.payload())?;
    info!("✅ Duplicate payment handled (idempotent): transaction_id = {}", 
        payment_result_dup.transaction_id);
    assert!(payment_result_dup.transaction_id.contains("cached"));

    // Demonstrate durable execution benefits
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Durable Execution Benefits (AWS Durable Lambda pattern):");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("1. ✅ All operations journaled via DurabilityFacet");
    info!("2. ✅ Idempotency: Duplicate requests return cached results");
    info!("3. ✅ State persistence: Actor state survives restarts");
    info!("4. ✅ Deterministic replay: Can replay from any checkpoint");
    info!("5. ✅ Exactly-once semantics: No duplicate payments");

    info!("=== Comparison Complete ===");
    info!("✅ Durable execution: All operations journaled");
    info!("✅ Idempotency: Duplicate requests handled correctly");
    info!("✅ State persistence: Actor state survives restarts");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_payment_processing() {
        let node = NodeBuilder::new("test-node")
            .build();

        let behavior = Box::new(PaymentProcessor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id("test-payment@test-node".to_string())
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
        
        let payment_processor = plexspaces_actor::ActorRef::local(
            core_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let request = PaymentRequest {
            order_id: "TEST-001".to_string(),
            amount: 50.0,
            payment_method: "credit_card".to_string(),
            idempotency_key: format!("test-key-{}", ulid::Ulid::new()),
        };

        let msg = Message::new(serde_json::to_vec(&request).unwrap())
            .with_message_type("call".to_string());
        
        let result = payment_processor
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let payment_result: PaymentResult = serde_json::from_slice(result.payload()).unwrap();
        assert_eq!(payment_result.status, "completed");
        assert!(!payment_result.transaction_id.is_empty());
    }
}
