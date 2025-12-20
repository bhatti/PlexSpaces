// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Cadence (Workflow Orchestration - Temporal's Predecessor)
// Based on: Cadence is Temporal's predecessor, using Thrift/TChannel instead of gRPC

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::WorkflowBehavior;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_journaling::{DurabilityFacet, DurabilityConfig, MemoryJournalStorage};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::info;

/// Order workflow message (Cadence-style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderWorkflowMessage {
    Start { order_id: String, amount: f64 },
    ExecuteActivity { activity_name: String, input: Vec<u8> },
    ActivityResult { result: Vec<u8> },
    WorkflowComplete { order_id: String, status: String },
}

/// Order workflow actor (Cadence-style workflow orchestration)
/// Demonstrates: WorkflowBehavior, Activity Patterns, Durable Execution (Temporal predecessor)
pub struct OrderWorkflowActor {
    order_id: String,
    amount: f64,
    status: String,
    activity_results: std::collections::HashMap<String, Vec<u8>>,
}

impl OrderWorkflowActor {
    pub fn new() -> Self {
        Self {
            order_id: String::new(),
            amount: 0.0,
            status: "pending".to_string(),
            activity_results: std::collections::HashMap::new(),
        }
    }

    async fn execute_activity(&self, activity_name: &str, input: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        info!("[CADENCE] Executing activity: {} (input size: {})", activity_name, input.len());
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Simulate activity execution
        match activity_name {
            "ProcessPayment" => Ok(format!("payment_completed:{}", String::from_utf8_lossy(input)).into_bytes()),
            "SendNotification" => Ok(format!("notification_sent:{}", String::from_utf8_lossy(input)).into_bytes()),
            _ => Ok(format!("activity_result:{}", String::from_utf8_lossy(input)).into_bytes()),
        }
    }
}

#[async_trait::async_trait]
impl Actor for OrderWorkflowActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        reply: &dyn Reply,
    ) -> Result<(), BehaviorError> {
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
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let workflow_msg: OrderWorkflowMessage = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match workflow_msg {
            OrderWorkflowMessage::Start { order_id, amount } => {
                info!("[CADENCE] Starting workflow: {} (amount: {})", order_id, amount);
                self.order_id = order_id.clone();
                self.amount = amount;
                self.status = "running".to_string();
                
                // Step 1: Execute activity (Cadence: workflow.ExecuteActivity)
                info!("[CADENCE] Step 1: Executing ProcessPayment activity");
                let payment_input = format!("order:{}", order_id).into_bytes();
                let payment_result = self.execute_activity("ProcessPayment", &payment_input).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Activity failed: {}", e)))?;
                self.activity_results.insert("payment".to_string(), payment_result.clone());
                
                // Step 2: Execute another activity
                info!("[CADENCE] Step 2: Executing SendNotification activity");
                let notification_result = self.execute_activity("SendNotification", &payment_result).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Activity failed: {}", e)))?;
                self.activity_results.insert("notification".to_string(), notification_result);
                
                self.status = "completed".to_string();
                
                let reply = OrderWorkflowMessage::WorkflowComplete {
                    order_id: self.order_id.clone(),
                    status: self.status.clone(),
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }

    async fn signal(
        &mut self,
        _ctx: &ActorContext,
        name: String,
        _data: Message,
    ) -> Result<(), BehaviorError> {
        info!("[CADENCE] Received signal: {} for order: {}", name, self.order_id);
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
                let status_json = serde_json::to_string(&self.status)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(status_json.into_bytes()))
            }
            "order_id" => {
                Ok(Message::new(self.order_id.clone().into_bytes()))
            }
            _ => Err(BehaviorError::ProcessingError("Unknown query".to_string())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    info!("=== Cadence vs PlexSpaces Comparison ===");
    info!("Demonstrating Cadence Workflow Orchestration (Temporal's Predecessor)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Cadence is Temporal's predecessor (uses Thrift/TChannel instead of gRPC)
    let actor_id: ActorId = "order-workflow/cadence-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating order workflow actor (Cadence-style workflow orchestration)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Create DurabilityFacet (Cadence workflows are durable)
    let storage = MemoryJournalStorage::new();
    let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
    
    // Spawn using ActorFactory with facets
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
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
    
    // Create ActorRef directly - no need to access mailbox
    let workflow = plexspaces_actor::ActorRef::remote(
        actor_id.clone(),
        node.id().as_str().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ Order workflow actor created: {}", workflow.id());
    info!("✅ WorkflowBehavior: Cadence-style workflow orchestration");
    info!("✅ DurabilityFacet: Durable execution (Temporal predecessor pattern)");
    info!("✅ Activity patterns: ExecuteActivity for workflow steps");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test workflow execution (Cadence: workflow.ExecuteWorkflow)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Execute workflow with activities (Cadence pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = Message::new(serde_json::to_vec(&OrderWorkflowMessage::Start {
        order_id: "order-123".to_string(),
        amount: 99.99,
    })?)
        .with_message_type("workflow_run".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: OrderWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let OrderWorkflowMessage::WorkflowComplete { order_id, status } = reply {
        info!("✅ Workflow completed: order_id={}, status={}", order_id, status);
        assert_eq!(order_id, "order-123");
        assert_eq!(status, "completed");
    }

    info!("=== Comparison Complete ===");
    info!("✅ WorkflowBehavior: Cadence-style workflow orchestration (Temporal predecessor)");
    info!("✅ DurabilityFacet: Durable execution with state persistence");
    info!("✅ Activity patterns: ExecuteActivity for workflow steps");

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
        // Create DurabilityFacet
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, serde_json::json!({}), 50));
        
        // Spawn using ActorFactory with facets
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        
        // Spawn using ActorFactory with facets
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

        let msg = Message::new(serde_json::to_vec(&OrderWorkflowMessage::Start {
            order_id: "test-order".to_string(),
            amount: 50.0,
        }).unwrap())
            .with_message_type("workflow_run".to_string());
        let result = workflow
            .ask(msg, Duration::from_secs(10))
            .await
            .unwrap();

        let reply: OrderWorkflowMessage = serde_json::from_slice(result.payload()).unwrap();
        if let OrderWorkflowMessage::WorkflowComplete { order_id, status } = reply {
            assert_eq!(order_id, "test-order");
            assert_eq!(status, "completed");
        } else {
            panic!("Expected WorkflowComplete message");
        }
    }
}
