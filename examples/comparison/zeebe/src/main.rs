// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Zeebe (BPMN Workflow Engine with Event Sourcing)
// Based on: https://camunda.com/products/cloud/workflow-engine/
// Use Case: Loan Approval Workflow (Application → Credit Check → Approval → Disbursement)

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

/// Loan application
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanApplication {
    pub application_id: String,
    pub applicant_name: String,
    pub loan_amount: f64,
    pub credit_score: u32,
    pub employment_status: String,
    pub annual_income: f64,
}

/// BPMN workflow step (Zeebe-style)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BPMNStep {
    pub step_id: String,
    pub step_type: String, // "start", "task", "gateway", "end"
    pub name: String,
    pub variables: std::collections::HashMap<String, serde_json::Value>,
}

/// Workflow instance (Zeebe-style with event sourcing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInstance {
    pub instance_id: String,
    pub workflow_key: String,
    pub version: u32,
    pub current_step: String,
    pub application: LoanApplication,
    pub approval_status: Option<String>,
    pub events: Vec<WorkflowEvent>, // Event sourcing
}

/// Workflow event (Zeebe-style event sourcing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowEvent {
    pub event_type: String, // "WORKFLOW_STARTED", "STEP_COMPLETED", "WORKFLOW_COMPLETED"
    pub timestamp: u64,
    pub step_name: String,
    pub data: std::collections::HashMap<String, serde_json::Value>,
}

/// Zeebe workflow message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ZeebeWorkflowMessage {
    DeployWorkflow { workflow_key: String, bpmn_xml: String },
    StartInstance { workflow_key: String, application: LoanApplication },
    CompleteStep { instance_id: String, step_id: String, result: std::collections::HashMap<String, serde_json::Value> },
    InstanceComplete { instance_id: String, application_id: String, approval_status: String, events: Vec<WorkflowEvent> },
}

/// Zeebe workflow actor (BPMN workflow engine with event sourcing)
/// Demonstrates: WorkflowBehavior, BPMN Workflow Engine, Event Sourcing
pub struct ZeebeWorkflowActor {
    workflow_instances: std::collections::HashMap<String, WorkflowInstance>,
    events: Vec<WorkflowEvent>, // Event log (event sourcing)
}

impl ZeebeWorkflowActor {
    pub fn new() -> Self {
        Self {
            workflow_instances: std::collections::HashMap::new(),
            events: Vec::new(),
        }
    }

    fn record_event(&mut self, event: WorkflowEvent) {
        self.events.push(event.clone());
        info!("[ZEEBE] Event recorded: {} - {} (total events: {})", 
            event.event_type, event.step_name, self.events.len());
    }

    async fn execute_step(&self, step_name: &str, application: &LoanApplication) -> Result<std::collections::HashMap<String, serde_json::Value>, Box<dyn std::error::Error + Send + Sync>> {
        info!("[ZEEBE] Executing BPMN step: {}", step_name);
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut result = std::collections::HashMap::new();
        match step_name {
            "SubmitApplication" => {
                result.insert("status".to_string(), serde_json::json!("submitted"));
                result.insert("application_id".to_string(), serde_json::json!(application.application_id.clone()));
            }
            "CreditCheck" => {
                // Simulate credit check
                let credit_approved = application.credit_score >= 650;
                result.insert("credit_approved".to_string(), serde_json::json!(credit_approved));
                result.insert("credit_score".to_string(), serde_json::json!(application.credit_score));
                info!("[ZEEBE] Credit check: score={}, approved={}", application.credit_score, credit_approved);
            }
            "RiskAssessment" => {
                // Simulate risk assessment
                let risk_level = if application.loan_amount > application.annual_income * 0.5 {
                    "high"
                } else {
                    "low"
                };
                result.insert("risk_level".to_string(), serde_json::json!(risk_level));
                result.insert("risk_approved".to_string(), serde_json::json!(risk_level == "low"));
                info!("[ZEEBE] Risk assessment: level={}", risk_level);
            }
            "ApprovalDecision" => {
                // Gateway: Approve or reject based on previous steps
                let credit_approved = application.credit_score >= 650;
                let risk_approved = application.loan_amount <= application.annual_income * 0.5;
                let approved = credit_approved && risk_approved;
                result.insert("approved".to_string(), serde_json::json!(approved));
                result.insert("decision_reason".to_string(), serde_json::json!(
                    if approved { "All checks passed" } else { "Credit or risk check failed" }
                ));
                info!("[ZEEBE] Approval decision: approved={}", approved);
            }
            "DisburseLoan" => {
                result.insert("disbursement_id".to_string(), serde_json::json!(format!("disb-{}", ulid::Ulid::new())));
                result.insert("amount".to_string(), serde_json::json!(application.loan_amount));
                result.insert("status".to_string(), serde_json::json!("disbursed"));
                info!("[ZEEBE] Loan disbursed: amount={}", application.loan_amount);
            }
            "RejectApplication" => {
                result.insert("rejection_reason".to_string(), serde_json::json!("Credit or risk check failed"));
                result.insert("status".to_string(), serde_json::json!("rejected"));
                info!("[ZEEBE] Application rejected");
            }
            _ => {
                result.insert("status".to_string(), serde_json::json!("completed"));
            }
        }
        
        Ok(result)
    }
}

#[async_trait::async_trait]
impl Actor for ZeebeWorkflowActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        
    ) -> Result<(), BehaviorError> {
        <Self as WorkflowBehavior>::route_workflow_message(self, ctx, msg, reply).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Workflow
    }
}

#[async_trait::async_trait]
impl WorkflowBehavior for ZeebeWorkflowActor {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let workflow_msg: ZeebeWorkflowMessage = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match workflow_msg {
            ZeebeWorkflowMessage::StartInstance { workflow_key, application } => {
                let instance_id = format!("instance-{}", ulid::Ulid::new());
                info!("[ZEEBE] Starting loan approval workflow instance: {} (application: {})", 
                    instance_id, application.application_id);
                
                // Record workflow started event (event sourcing)
                self.record_event(WorkflowEvent {
                    event_type: "WORKFLOW_STARTED".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    step_name: "start".to_string(),
                    data: {
                        let mut data = std::collections::HashMap::new();
                        data.insert("instance_id".to_string(), serde_json::json!(instance_id.clone()));
                        data.insert("workflow_key".to_string(), serde_json::json!(workflow_key.clone()));
                        data.insert("application_id".to_string(), serde_json::json!(application.application_id.clone()));
                        data
                    },
                });
                
                // Create workflow instance
                let mut instance = WorkflowInstance {
                    instance_id: instance_id.clone(),
                    workflow_key: workflow_key.clone(),
                    version: 1,
                    current_step: "SubmitApplication".to_string(),
                    application: application.clone(),
                    approval_status: None,
                    events: vec![],
                };
                
                // Execute BPMN workflow steps: Application → Credit Check → Risk Assessment → Approval Decision → Disburse/Reject
                let steps = vec!["SubmitApplication", "CreditCheck", "RiskAssessment", "ApprovalDecision"];
                let mut step_results = std::collections::HashMap::new();
                let mut approved = false;
                
                for step in &steps {
                    instance.current_step = step.to_string();
                    
                    // Execute step
                    let result = self.execute_step(step, &instance.application).await
                        .map_err(|e| BehaviorError::ProcessingError(format!("Step failed: {}", e)))?;
                    step_results.insert(step.to_string(), result.clone());
                    
                    // Record step completion event (event sourcing)
                    self.record_event(WorkflowEvent {
                        event_type: "STEP_COMPLETED".to_string(),
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        step_name: step.to_string(),
                        data: result.clone(),
                    });
                    
                    // Check approval decision
                    if step == &"ApprovalDecision" {
                        if let Some(approved_val) = result.get("approved") {
                            approved = approved_val.as_bool().unwrap_or(false);
                        }
                    }
                }
                
                // Execute final step based on approval decision (BPMN gateway)
                let final_step = if approved {
                    "DisburseLoan"
                } else {
                    "RejectApplication"
                };
                instance.current_step = final_step.to_string();
                let final_result = self.execute_step(final_step, &instance.application).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Final step failed: {}", e)))?;
                step_results.insert(final_step.to_string(), final_result.clone());
                
                // Record final step event
                self.record_event(WorkflowEvent {
                    event_type: "STEP_COMPLETED".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    step_name: final_step.to_string(),
                    data: final_result.clone(),
                });
                
                // Record workflow completion event
                let approval_status = if approved { "APPROVED" } else { "REJECTED" };
                instance.approval_status = Some(approval_status.to_string());
                self.record_event(WorkflowEvent {
                    event_type: "WORKFLOW_COMPLETED".to_string(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    step_name: "end".to_string(),
                    data: {
                        let mut data = std::collections::HashMap::new();
                        data.insert("instance_id".to_string(), serde_json::json!(instance_id.clone()));
                        data.insert("approval_status".to_string(), serde_json::json!(approval_status));
                        data
                    },
                });
                
                instance.events = self.events.clone();
                self.workflow_instances.insert(instance_id.clone(), instance);
                
                let reply = ZeebeWorkflowMessage::InstanceComplete {
                    instance_id,
                    application_id: application.application_id.clone(),
                    approval_status: approval_status.to_string(),
                    events: self.events.clone(),
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
        info!("[ZEEBE] Received signal: {} for workflow", name);
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
                let status_json = serde_json::to_string(&format!("{} instances, {} events", 
                    self.workflow_instances.len(), self.events.len()))
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(status_json.into_bytes()))
            }
            "events" => {
                let events_json = serde_json::to_string(&self.events)
                    .map_err(|e| BehaviorError::ProcessingError(format!("Serialization failed: {}", e)))?;
                Ok(Message::new(events_json.into_bytes()))
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

    info!("=== Zeebe vs PlexSpaces Comparison ===");
    info!("Demonstrating BPMN Workflow Engine with Event Sourcing");
    info!("Use Case: Loan Approval Workflow (Application → Credit Check → Risk Assessment → Approval → Disbursement)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // Zeebe is a BPMN workflow engine with event sourcing
    let actor_id: ActorId = "zeebe-workflow/zeebe-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating Zeebe workflow actor (BPMN workflow engine with event sourcing)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(ZeebeWorkflowActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Attach DurabilityFacet (Zeebe uses event sourcing for durability)
    let storage = MemoryJournalStorage::new();
    let durability_config = DurabilityConfig::default();
    let durability_facet = Box::new(DurabilityFacet::new(storage, durability_config));
    actor
        .attach_facet(durability_facet, 50, serde_json::json!({}))
        .await?;
    
    // Spawn using ActorFactory
    use plexspaces_actor::{ActorFactory, actor_factory_impl::ActorFactoryImpl};
    use std::sync::Arc;
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = actor.id().clone();
    let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
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

    info!("✅ Zeebe workflow actor created: {}", workflow.id());
    info!("✅ WorkflowBehavior: BPMN workflow engine");
    info!("✅ DurabilityFacet: Event sourcing for durability");
    info!("✅ Event log: Immutable event history (Zeebe pattern)");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test workflow execution (Zeebe: event sourcing)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Loan Approval Workflow with Event Sourcing (Zeebe pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    // Test case 1: Approved loan
    let application1 = LoanApplication {
        application_id: "loan-app-001".to_string(),
        applicant_name: "John Doe".to_string(),
        loan_amount: 50000.0,
        credit_score: 720,
        employment_status: "employed".to_string(),
        annual_income: 150000.0,
    };
    
    let msg = Message::new(serde_json::to_vec(&ZeebeWorkflowMessage::StartInstance {
        workflow_key: "loan-approval-workflow".to_string(),
        application: application1.clone(),
    })?)
        .with_message_type("workflow_run".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: ZeebeWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let ZeebeWorkflowMessage::InstanceComplete { instance_id, application_id, approval_status, events } = reply {
        info!("✅ Workflow instance completed: instance_id={}, application_id={}, status={}", 
            instance_id, application_id, approval_status);
        info!("✅ Event log: {} events recorded", events.len());
        for event in &events {
            info!("   - Event: {} - {} (timestamp: {})", event.event_type, event.step_name, event.timestamp);
        }
        assert_eq!(approval_status, "APPROVED");
    }

    // Test case 2: Rejected loan (low credit score)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Rejected Loan Application (low credit score)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    let application2 = LoanApplication {
        application_id: "loan-app-002".to_string(),
        applicant_name: "Jane Smith".to_string(),
        loan_amount: 30000.0,
        credit_score: 580, // Low credit score
        employment_status: "employed".to_string(),
        annual_income: 80000.0,
    };
    
    let msg = Message::new(serde_json::to_vec(&ZeebeWorkflowMessage::StartInstance {
        workflow_key: "loan-approval-workflow".to_string(),
        application: application2.clone(),
    })?)
        .with_message_type("workflow_run".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: ZeebeWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let ZeebeWorkflowMessage::InstanceComplete { approval_status, .. } = reply {
        info!("✅ Workflow instance completed: status={}", approval_status);
        assert_eq!(approval_status, "REJECTED");
    }

    info!("=== Comparison Complete ===");
    info!("✅ WorkflowBehavior: BPMN workflow engine (Zeebe pattern)");
    info!("✅ DurabilityFacet: Event sourcing for durability");
    info!("✅ Event log: Immutable event history for replay");
    info!("✅ BPMN workflow: Application → Credit Check → Risk Assessment → Approval Decision → Disburse/Reject");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_zeebe_workflow() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "zeebe-workflow/test-1@test-node".to_string();
        let behavior = Box::new(ZeebeWorkflowActor::new());
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
        let _message_sender = actor_factory.spawn_built_actor(Arc::new(actor), None, None, None).await
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

        let application = LoanApplication {
            application_id: "test-app".to_string(),
            applicant_name: "Test User".to_string(),
            loan_amount: 10000.0,
            credit_score: 700,
            employment_status: "employed".to_string(),
            annual_income: 50000.0,
        };
        let msg = Message::new(serde_json::to_vec(&ZeebeWorkflowMessage::StartInstance {
            workflow_key: "test-workflow".to_string(),
            application,
        }).unwrap())
            .with_message_type("workflow_run".to_string());
        let result = workflow
            .ask(msg, Duration::from_secs(10))
            .await
            .unwrap();

        let reply: ZeebeWorkflowMessage = serde_json::from_slice(result.payload()).unwrap();
        if let ZeebeWorkflowMessage::InstanceComplete { instance_id, events, .. } = reply {
            assert!(!instance_id.is_empty());
            assert!(!events.is_empty());
        } else {
            panic!("Expected InstanceComplete message");
        }
    }
}
