// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Netflix Conductor/Orkes (JSON-based Workflow DSL for Microservices Orchestration)
// Based on: https://github.com/Netflix/conductor
// Use Case: E-commerce Order Processing Workflow (Payment → Inventory → Shipping → Notification)

use plexspaces_actor::{ActorBuilder, ActorRef};
use plexspaces_behavior::WorkflowBehavior;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId, Reply};
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
    pub customer_id: String,
    pub items: Vec<OrderItem>,
    pub total_amount: f64,
    pub shipping_address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderItem {
    pub product_id: String,
    pub quantity: u32,
    pub price: f64,
}

/// Workflow task (Conductor-style JSON DSL)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowTask {
    pub task_id: String,
    pub task_type: String, // "HTTP", "EVENT", "SUB_WORKFLOW", "DECISION"
    pub name: String,
    pub input_parameters: std::collections::HashMap<String, serde_json::Value>,
    pub status: String, // "SCHEDULED", "IN_PROGRESS", "COMPLETED", "FAILED"
}

/// Workflow definition (Conductor-style JSON DSL)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowDefinition {
    pub workflow_id: String,
    pub name: String,
    pub version: u32,
    pub tasks: Vec<WorkflowTask>,
    pub status: String, // "RUNNING", "COMPLETED", "FAILED", "PAUSED"
}

/// Conductor workflow message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConductorWorkflowMessage {
    StartWorkflow { definition: WorkflowDefinition, order: OrderRequest },
    ExecuteTask { task: WorkflowTask, order: OrderRequest },
    TaskComplete { task_id: String, output: std::collections::HashMap<String, serde_json::Value> },
    WorkflowComplete { workflow_id: String, order_id: String, status: String, results: std::collections::HashMap<String, serde_json::Value> },
}

/// Conductor workflow actor (Netflix Conductor/Orkes-style)
/// Demonstrates: WorkflowBehavior, JSON-based Workflow DSL, Microservices Orchestration
pub struct ConductorWorkflowActor {
    workflow: Option<WorkflowDefinition>,
    order: Option<OrderRequest>,
    task_results: std::collections::HashMap<String, std::collections::HashMap<String, serde_json::Value>>,
}

impl ConductorWorkflowActor {
    pub fn new() -> Self {
        Self {
            workflow: None,
            order: None,
            task_results: std::collections::HashMap::new(),
        }
    }

    async fn execute_task(&self, task: &WorkflowTask, order: &OrderRequest) -> Result<std::collections::HashMap<String, serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
        info!("[CONDUCTOR] Executing task: {} (type: {})", task.name, task.task_type);
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        // Simulate microservice orchestration based on task type
        let mut output = std::collections::HashMap::new();
        match task.task_type.as_str() {
            "HTTP" => {
                // HTTP task: Call external microservice
                match task.name.as_str() {
                    "ProcessPayment" => {
                        output.insert("transaction_id".to_string(), serde_json::json!(format!("txn-{}", ulid::Ulid::new())));
                        output.insert("status".to_string(), serde_json::json!("completed"));
                        output.insert("amount".to_string(), serde_json::json!(order.total_amount));
                        info!("[CONDUCTOR] Payment processed: transaction_id={}", output.get("transaction_id").unwrap());
                    }
                    "UpdateInventory" => {
                        output.insert("inventory_updated".to_string(), serde_json::json!(true));
                        output.insert("items_reserved".to_string(), serde_json::json!(order.items.len()));
                        info!("[CONDUCTOR] Inventory updated: {} items reserved", order.items.len());
                    }
                    "CreateShipment" => {
                        output.insert("tracking_number".to_string(), serde_json::json!(format!("TRACK-{}", ulid::Ulid::new())));
                        output.insert("carrier".to_string(), serde_json::json!("UPS"));
                        info!("[CONDUCTOR] Shipment created: tracking_number={}", output.get("tracking_number").unwrap());
                    }
                    _ => {
                        output.insert("status_code".to_string(), serde_json::json!(200));
                        output.insert("response".to_string(), serde_json::json!("success"));
                    }
                }
            }
            "EVENT" => {
                // Event task: Publish event to event bus
                output.insert("event_published".to_string(), serde_json::json!(true));
                output.insert("event_type".to_string(), serde_json::json!("order_processed"));
                output.insert("order_id".to_string(), serde_json::json!(order.order_id.clone()));
                info!("[CONDUCTOR] Event published: order_processed for order {}", order.order_id);
            }
            "DECISION" => {
                // Decision task: Conditional logic
                if task.name == "CheckDiscountEligibility" {
                    // Check if customer is eligible for discount
                    let eligible = order.total_amount > 100.0;
                    output.insert("eligible".to_string(), serde_json::json!(eligible));
                    output.insert("discount_percent".to_string(), serde_json::json!(if eligible { 10 } else { 0 }));
                    info!("[CONDUCTOR] Discount eligibility checked: eligible={}", eligible);
                }
            }
            "SUB_WORKFLOW" => {
                // Sub-workflow: Execute nested workflow
                output.insert("sub_workflow_id".to_string(), serde_json::json!("fulfillment-sub-workflow"));
                output.insert("status".to_string(), serde_json::json!("completed"));
                info!("[CONDUCTOR] Sub-workflow executed: fulfillment-sub-workflow");
            }
            _ => {
                output.insert("result".to_string(), serde_json::json!("completed"));
            }
        }
        
        Ok(output)
    }
}

#[async_trait::async_trait]
impl Actor for ConductorWorkflowActor {
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
impl WorkflowBehavior for ConductorWorkflowActor {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let workflow_msg: ConductorWorkflowMessage = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match workflow_msg {
            ConductorWorkflowMessage::StartWorkflow { definition, order } => {
                info!("[CONDUCTOR] Starting workflow: {} (order: {})", definition.name, order.order_id);
                self.workflow = Some(definition.clone());
                self.order = Some(order.clone());
                
                // Execute tasks in sequence (Conductor: JSON-based workflow DSL)
                // E-commerce order processing: Payment → Inventory → Shipping → Notification
                let mut all_results = std::collections::HashMap::new();
                for task in &definition.tasks {
                    info!("[CONDUCTOR] Executing task: {} ({})", task.name, task.task_type);
                    let output = self.execute_task(task, &order).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Task failed: {}", e)))?;
                    self.task_results.insert(task.task_id.clone(), output.clone());
                    all_results.insert(task.name.clone(), serde_json::Value::Object(
                        output.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
                    ));
                }
                
                let reply = ConductorWorkflowMessage::WorkflowComplete {
                    workflow_id: definition.workflow_id.clone(),
                    order_id: order.order_id.clone(),
                    status: "COMPLETED".to_string(),
                    results: all_results,
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
        info!("[CONDUCTOR] Received signal: {} for workflow", name);
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
                let status = if let Some(ref workflow) = self.workflow {
                    workflow.status.clone()
                } else {
                    "PENDING".to_string()
                };
                let status_json = serde_json::to_string(&status)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(status_json.into_bytes()))
            }
            "workflow_id" => {
                let workflow_id = if let Some(ref workflow) = self.workflow {
                    workflow.workflow_id.clone()
                } else {
                    "unknown".to_string()
                };
                Ok(Message::new(workflow_id.into_bytes()))
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

    info!("=== Netflix Conductor/Orkes vs PlexSpaces Comparison ===");
    info!("Demonstrating JSON-based Workflow DSL for E-commerce Order Processing");
    info!("Use Case: Payment → Inventory → Shipping → Notification");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Conductor uses JSON-based DSL for workflow definition
    let actor_id: ActorId = "conductor-workflow/conductor-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating Conductor workflow actor (JSON-based workflow DSL)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(ConductorWorkflowActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Attach DurabilityFacet (Conductor workflows are durable)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig::default();
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    let actor_ref = node.spawn_actor(actor).await?;

    let mailbox = node.actor_registry()
        .lookup_mailbox(actor_ref.id())
        .await?
        .ok_or("Actor not found in registry")?;
    
    let workflow = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ Conductor workflow actor created: {}", workflow.id());
    info!("✅ WorkflowBehavior: JSON-based workflow DSL");
    info!("✅ DurabilityFacet: Durable execution for microservices orchestration");
    info!("✅ Task types: HTTP (microservices), EVENT (event bus), DECISION (conditional logic)");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test workflow execution (Conductor: JSON-based workflow definition)
    // E-commerce order processing workflow
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: E-commerce Order Processing Workflow (Conductor pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let order = OrderRequest {
        order_id: "order-12345".to_string(),
        customer_id: "customer-789".to_string(),
        items: vec![
            OrderItem { product_id: "prod-1".to_string(), quantity: 2, price: 29.99 },
            OrderItem { product_id: "prod-2".to_string(), quantity: 1, price: 49.99 },
        ],
        total_amount: 109.97,
        shipping_address: "123 Main St, City, State 12345".to_string(),
    };
    
    let tasks = vec![
        WorkflowTask {
            task_id: "task-1".to_string(),
            task_type: "HTTP".to_string(),
            name: "ProcessPayment".to_string(),
            input_parameters: {
                let mut params = std::collections::HashMap::new();
                params.insert("url".to_string(), serde_json::json!("https://api.payment.com/charge"));
                params.insert("method".to_string(), serde_json::json!("POST"));
                params.insert("amount".to_string(), serde_json::json!(order.total_amount));
                params
            },
            status: "SCHEDULED".to_string(),
        },
        WorkflowTask {
            task_id: "task-2".to_string(),
            task_type: "HTTP".to_string(),
            name: "UpdateInventory".to_string(),
            input_parameters: {
                let mut params = std::collections::HashMap::new();
                params.insert("url".to_string(), serde_json::json!("https://api.inventory.com/reserve"));
                params.insert("items".to_string(), serde_json::json!(order.items));
                params
            },
            status: "SCHEDULED".to_string(),
        },
        WorkflowTask {
            task_id: "task-3".to_string(),
            task_type: "DECISION".to_string(),
            name: "CheckDiscountEligibility".to_string(),
            input_parameters: {
                let mut params = std::collections::HashMap::new();
                params.insert("total_amount".to_string(), serde_json::json!(order.total_amount));
                params
            },
            status: "SCHEDULED".to_string(),
        },
        WorkflowTask {
            task_id: "task-4".to_string(),
            task_type: "HTTP".to_string(),
            name: "CreateShipment".to_string(),
            input_parameters: {
                let mut params = std::collections::HashMap::new();
                params.insert("url".to_string(), serde_json::json!("https://api.shipping.com/create"));
                params.insert("address".to_string(), serde_json::json!(order.shipping_address));
                params
            },
            status: "SCHEDULED".to_string(),
        },
        WorkflowTask {
            task_id: "task-5".to_string(),
            task_type: "EVENT".to_string(),
            name: "PublishOrderProcessed".to_string(),
            input_parameters: {
                let mut params = std::collections::HashMap::new();
                params.insert("event_type".to_string(), serde_json::json!("order_processed"));
                params.insert("order_id".to_string(), serde_json::json!(order.order_id));
                params
            },
            status: "SCHEDULED".to_string(),
        },
    ];
    
    let workflow_def = WorkflowDefinition {
        workflow_id: "order-processing-workflow".to_string(),
        name: "E-commerce Order Processing".to_string(),
        version: 1,
        tasks,
        status: "RUNNING".to_string(),
    };
    
    let msg = Message::new(serde_json::to_vec(&ConductorWorkflowMessage::StartWorkflow {
        definition: workflow_def,
        order: order.clone(),
    })?)
        .with_message_type("workflow_run".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: ConductorWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let ConductorWorkflowMessage::WorkflowComplete { workflow_id, order_id, status, results } = reply {
        info!("✅ Workflow completed: workflow_id={}, order_id={}, status={}", workflow_id, order_id, status);
        info!("✅ Workflow results:");
        for (task_name, result) in results {
            info!("   - {}: {:?}", task_name, result);
        }
        assert_eq!(order_id, "order-12345");
        assert_eq!(status, "COMPLETED");
    }

    info!("=== Comparison Complete ===");
    info!("✅ WorkflowBehavior: JSON-based workflow DSL (Conductor pattern)");
    info!("✅ DurabilityFacet: Durable execution for microservices orchestration");
    info!("✅ Task orchestration: HTTP (microservices), EVENT (event bus), DECISION (conditional logic)");
    info!("✅ E-commerce workflow: Payment → Inventory → Discount Check → Shipping → Notification");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_conductor_workflow() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "conductor-workflow/test-1@test-node".to_string();
        let behavior = Box::new(ConductorWorkflowActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        let storage = MemoryJournalStorage::new();
        let durability_facet = Box::new(DurabilityFacet::new(storage, DurabilityConfig::default()));
        actor.attach_facet(durability_facet, 50, serde_json::json!({})).await.unwrap();
        
        let actor_ref = node.spawn_actor(actor).await.unwrap();

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

        let order = OrderRequest {
            order_id: "test-order".to_string(),
            customer_id: "test-customer".to_string(),
            items: vec![],
            total_amount: 50.0,
            shipping_address: "test address".to_string(),
        };
        let tasks = vec![
            WorkflowTask {
                task_id: "test-task".to_string(),
                task_type: "HTTP".to_string(),
                name: "TestTask".to_string(),
                input_parameters: std::collections::HashMap::new(),
                status: "SCHEDULED".to_string(),
            },
        ];
        let workflow_def = WorkflowDefinition {
            workflow_id: "test-workflow".to_string(),
            name: "TestWorkflow".to_string(),
            version: 1,
            tasks,
            status: "RUNNING".to_string(),
        };
        let msg = Message::new(serde_json::to_vec(&ConductorWorkflowMessage::StartWorkflow {
            definition: workflow_def,
            order,
        }).unwrap())
            .with_message_type("workflow_run".to_string());
        let result = workflow
            .ask(msg, Duration::from_secs(10))
            .await
            .unwrap();

        let reply: ConductorWorkflowMessage = serde_json::from_slice(result.payload()).unwrap();
        if let ConductorWorkflowMessage::WorkflowComplete { workflow_id, status, .. } = reply {
            assert_eq!(workflow_id, "test-workflow");
            assert_eq!(status, "COMPLETED");
        } else {
            panic!("Expected WorkflowComplete message");
        }
    }
}
