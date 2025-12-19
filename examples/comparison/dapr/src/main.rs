// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Dapr (Unified Durable Workflows with Actors)

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::{GenServer, WorkflowBehavior};
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Message};
use plexspaces_journaling::{DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message as MailboxMessage;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Order workflow message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderWorkflowMessage {
    Start { order_id: String, amount: f64 },
    ProcessPayment { order_id: String, amount: f64 },
    SendNotification { order_id: String },
    Complete { order_id: String },
    WorkflowResult { order_id: String, status: String },
}

/// Order workflow actor (Dapr-style unified durable workflow)
/// Demonstrates: WorkflowBehavior, DurabilityFacet, Unified Actor + Workflow Model
pub struct OrderWorkflowActor {
    order_id: String,
    amount: f64,
    status: String,
}

impl OrderWorkflowActor {
    pub fn new() -> Self {
        Self {
            order_id: String::new(),
            amount: 0.0,
            status: "pending".to_string(),
        }
    }
}

#[async_trait::async_trait]
impl Actor for OrderWorkflowActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: MailboxMessage,
        
    ) -> Result<(), BehaviorError> {
        // Delegate to WorkflowBehavior
        <Self as WorkflowBehavior>::route_workflow_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait::async_trait]
impl WorkflowBehavior for OrderWorkflowActor {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: MailboxMessage,
    ) -> Result<MailboxMessage, BehaviorError> {
        let workflow_msg: OrderWorkflowMessage = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match workflow_msg {
            OrderWorkflowMessage::Start { order_id, amount } => {
                info!("[WORKFLOW] Starting order workflow: {} (amount: {})", order_id, amount);
                self.order_id = order_id.clone();
                self.amount = amount;
                self.status = "processing".to_string();
                
                // Step 1: Process payment
                info!("[WORKFLOW] Step 1: Processing payment");
                tokio::time::sleep(Duration::from_millis(100)).await;
                self.status = "payment_processed".to_string();
                
                // Step 2: Send notification
                info!("[WORKFLOW] Step 2: Sending notification");
                tokio::time::sleep(Duration::from_millis(50)).await;
                self.status = "notification_sent".to_string();
                
                // Step 3: Complete
                info!("[WORKFLOW] Step 3: Completing order");
                self.status = "completed".to_string();
                
                let reply = OrderWorkflowMessage::WorkflowResult {
                    order_id: self.order_id.clone(),
                    status: self.status.clone(),
                };
                Ok(MailboxMessage::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Dapr vs PlexSpaces Comparison ===");
    info!("Demonstrating Dapr Unified Durable Workflows (Actor + Workflow model)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Dapr combines actors with durable workflows
    let actor_id: ActorId = "order-workflow/dapr-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating order workflow actor (Dapr-style unified model)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(OrderWorkflowActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Attach DurabilityFacet (Dapr workflows are durable)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig::default();
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    // Spawn using ActorFactory with spawn_actor
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = actor.id().clone();
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "Workflow", // actor_type from WorkflowActor
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;
    let actor_ref = plexspaces_core::ActorRef::new(actor_id)
        .map_err(|e| format!("Failed to create ActorRef: {}", e))?;

    let mailbox = node.actor_registry()
        .lookup_mailbox(actor_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let workflow = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ Order workflow actor created: {}", workflow.id());
    info!("✅ WorkflowBehavior: Unified actor + workflow model");
    info!("✅ DurabilityFacet: Durable execution enabled");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test workflow execution
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Execute order workflow (unified actor + workflow)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = MailboxMessage::new(serde_json::to_vec(&OrderWorkflowMessage::Start {
        order_id: "order-123".to_string(),
        amount: 99.99,
    })?)
        .with_message_type("workflow_start".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: OrderWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let OrderWorkflowMessage::WorkflowResult { order_id, status } = reply {
        info!("✅ Workflow completed: order_id={}, status={}", order_id, status);
        assert_eq!(order_id, "order-123");
        assert_eq!(status, "completed");
    }

    info!("=== Comparison Complete ===");
    info!("✅ WorkflowBehavior: Unified actor + workflow model (Dapr pattern)");
    info!("✅ DurabilityFacet: Durable execution with state persistence");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_order_workflow() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "order-workflow/test-1@test-node".to_string();
        let behavior = Box::new(OrderWorkflowActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
        actor.attach_facet(durability_facet, 50, serde_json::json!({})).await.unwrap();
        
        // Spawn using ActorFactory
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = actor.id().clone();
        let _message_sender = let ctx = plexspaces_core::RequestContext::internal();
            actor_factory.spawn_actor(
                &ctx,
                &actor.id().clone(),
                "GenServer", // actor_type
                vec![], // initial_state
                None, // config
                std::collections::HashMap::new(), // labels
            ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        let actor_ref = plexspaces_core::ActorRef::new(actor_id)
            .map_err(|e| format!("Failed to create ActorRef: {}", e)).unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let workflow = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let msg = MailboxMessage::new(serde_json::to_vec(&OrderWorkflowMessage::Start {
            order_id: "test-order".to_string(),
            amount: 50.0,
        }).unwrap())
            .with_message_type("workflow_start".to_string());
        let result = workflow
            .ask(msg, Duration::from_secs(10))
            .await
            .unwrap();

        let reply: OrderWorkflowMessage = serde_json::from_slice(result.payload()).unwrap();
        if let OrderWorkflowMessage::WorkflowResult { order_id, status } = reply {
            assert_eq!(order_id, "test-order");
            assert_eq!(status, "completed");
        } else {
            panic!("Expected WorkflowResult message");
        }
    }
}
