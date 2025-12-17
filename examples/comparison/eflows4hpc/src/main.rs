// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: eFlows4HPC (Unified Workflow Platform for HPC + Big Data + ML)
// Based on: https://www.hpcwire.com/off-the-wire/eflows4hpc-advances-hpc-applications-delivering-unified-solutions-for-complex-workflow-challenges-in-diverse-domains/

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

/// HPC simulation step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HCPStep {
    pub step_id: String,
    pub step_type: String, // "simulation", "data_analysis", "ml_training"
    pub input_data: Vec<u8>,
    pub parameters: std::collections::HashMap<String, String>,
}

/// Big data analytics step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsStep {
    pub step_id: String,
    pub analytics_type: String, // "aggregation", "filtering", "transformation"
    pub input_data: Vec<u8>,
    pub query: String,
}

/// ML training step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MLStep {
    pub step_id: String,
    pub model_type: String,
    pub training_data: Vec<u8>,
    pub hyperparameters: std::collections::HashMap<String, f64>,
}

/// Unified workflow message (eFlows4HPC-style: HPC + Big Data + ML)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UnifiedWorkflowMessage {
    StartWorkflow { workflow_id: String, steps: Vec<WorkflowStep> },
    ExecuteHCPStep { step: HCPStep },
    ExecuteAnalyticsStep { step: AnalyticsStep },
    ExecuteMLStep { step: MLStep },
    StepComplete { step_id: String, output: Vec<u8> },
    WorkflowComplete { workflow_id: String, results: Vec<WorkflowResult> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowStep {
    HPC(HCPStep),
    Analytics(AnalyticsStep),
    ML(MLStep),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowResult {
    pub step_id: String,
    pub step_type: String,
    pub output: Vec<u8>,
    pub metrics: std::collections::HashMap<String, f64>,
}

/// Unified workflow actor (eFlows4HPC-style: HPC + Big Data + ML integration)
/// Demonstrates: WorkflowBehavior, Unified Platform, HPC Integration, Multi-Domain Workflows
pub struct UnifiedWorkflowActor {
    workflow_id: String,
    steps: Vec<WorkflowStep>,
    completed_results: Vec<WorkflowResult>,
    status: String,
}

impl UnifiedWorkflowActor {
    pub fn new() -> Self {
        Self {
            workflow_id: String::new(),
            steps: Vec::new(),
            completed_results: Vec::new(),
            status: "pending".to_string(),
        }
    }

    async fn execute_hpc_step(&self, step: &HCPStep) -> Result<WorkflowResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[eFlows4HPC] Executing HPC step: {} (type: {})", step.step_id, step.step_type);
        tokio::time::sleep(Duration::from_millis(50)).await;
        
        let mut metrics = std::collections::HashMap::new();
        metrics.insert("execution_time".to_string(), 0.05);
        metrics.insert("cpu_utilization".to_string(), 0.85);
        
        Ok(WorkflowResult {
            step_id: step.step_id.clone(),
            step_type: "hpc".to_string(),
            output: vec![0x01, 0x02, 0x03],
            metrics,
        })
    }

    async fn execute_analytics_step(&self, step: &AnalyticsStep) -> Result<WorkflowResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[eFlows4HPC] Executing analytics step: {} (type: {})", step.step_id, step.analytics_type);
        tokio::time::sleep(Duration::from_millis(30)).await;
        
        let mut metrics = std::collections::HashMap::new();
        metrics.insert("records_processed".to_string(), 1000.0);
        metrics.insert("throughput".to_string(), 33333.0);
        
        Ok(WorkflowResult {
            step_id: step.step_id.clone(),
            step_type: "analytics".to_string(),
            output: vec![0x04, 0x05, 0x06],
            metrics,
        })
    }

    async fn execute_ml_step(&self, step: &MLStep) -> Result<WorkflowResult, Box<dyn std::error::Error + Send + Sync>> {
        info!("[eFlows4HPC] Executing ML step: {} (type: {})", step.step_id, step.model_type);
        tokio::time::sleep(Duration::from_millis(40)).await;
        
        let mut metrics = std::collections::HashMap::new();
        metrics.insert("accuracy".to_string(), 0.92);
        metrics.insert("loss".to_string(), 0.08);
        
        Ok(WorkflowResult {
            step_id: step.step_id.clone(),
            step_type: "ml".to_string(),
            output: vec![0x07, 0x08, 0x09],
            metrics,
        })
    }
}

#[async_trait::async_trait]
impl Actor for UnifiedWorkflowActor {
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
impl WorkflowBehavior for UnifiedWorkflowActor {
    async fn run(
        &mut self,
        _ctx: &ActorContext,
        input: Message,
    ) -> Result<Message, BehaviorError> {
        let workflow_msg: UnifiedWorkflowMessage = serde_json::from_slice(input.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match workflow_msg {
            UnifiedWorkflowMessage::StartWorkflow { workflow_id, steps } => {
                info!("[eFlows4HPC] Starting unified workflow: {} ({} steps: HPC + Analytics + ML)", 
                    workflow_id, steps.len());
                self.workflow_id = workflow_id.clone();
                self.steps = steps.clone();
                self.status = "running".to_string();
                
                // Execute steps in sequence (eFlows4HPC integrates HPC, Big Data, ML)
                let mut results = Vec::new();
                for step in &self.steps {
                    let result = match step {
                        WorkflowStep::HPC(hpc_step) => {
                            self.execute_hpc_step(hpc_step).await
                                .map_err(|e| BehaviorError::ProcessingError(format!("HPC step failed: {}", e)))?
                        }
                        WorkflowStep::Analytics(analytics_step) => {
                            self.execute_analytics_step(analytics_step).await
                                .map_err(|e| BehaviorError::ProcessingError(format!("Analytics step failed: {}", e)))?
                        }
                        WorkflowStep::ML(ml_step) => {
                            self.execute_ml_step(ml_step).await
                                .map_err(|e| BehaviorError::ProcessingError(format!("ML step failed: {}", e)))?
                        }
                    };
                    results.push(result);
                }
                
                self.completed_results = results.clone();
                self.status = "completed".to_string();
                
                let reply = UnifiedWorkflowMessage::WorkflowComplete {
                    workflow_id: self.workflow_id.clone(),
                    results,
                };
                Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
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

    info!("=== eFlows4HPC vs PlexSpaces Comparison ===");
    info!("Demonstrating Unified Workflow Platform (HPC + Big Data + ML Integration)");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // eFlows4HPC integrates HPC simulation, big data analytics, and ML training
    let actor_id: ActorId = "unified-workflow/eflows4hpc-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating unified workflow actor (eFlows4HPC-style: HPC + Analytics + ML)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(UnifiedWorkflowActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Attach DurabilityFacet (unified workflows need durability)
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

    info!("✅ Unified workflow actor created: {}", workflow.id());
    info!("✅ WorkflowBehavior: Unified platform (HPC + Big Data + ML)");
    info!("✅ DurabilityFacet: Durable execution for complex workflows");
    info!("✅ Multi-domain integration: HPC simulation → Analytics → ML training");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test unified workflow (HPC → Analytics → ML)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Unified workflow (HPC simulation → Big Data analytics → ML training)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let steps = vec![
        WorkflowStep::HPC(HCPStep {
            step_id: "hpc-1".to_string(),
            step_type: "simulation".to_string(),
            input_data: vec![0x01],
            parameters: std::collections::HashMap::new(),
        }),
        WorkflowStep::Analytics(AnalyticsStep {
            step_id: "analytics-1".to_string(),
            analytics_type: "aggregation".to_string(),
            input_data: vec![0x02],
            query: "SELECT * FROM data".to_string(),
        }),
        WorkflowStep::ML(MLStep {
            step_id: "ml-1".to_string(),
            model_type: "neural_network".to_string(),
            training_data: vec![0x03],
            hyperparameters: std::collections::HashMap::new(),
        }),
    ];
    
    let msg = Message::new(serde_json::to_vec(&UnifiedWorkflowMessage::StartWorkflow {
        workflow_id: "unified-1".to_string(),
        steps,
    })?)
        .with_message_type("workflow_start".to_string());
    let result = workflow
        .ask(msg, Duration::from_secs(10))
        .await?;
    let reply: UnifiedWorkflowMessage = serde_json::from_slice(result.payload())?;
    if let UnifiedWorkflowMessage::WorkflowComplete { workflow_id, results } = reply {
        info!("✅ Unified workflow completed: {} ({} steps executed)", workflow_id, results.len());
        for result in results {
            info!("  - Step {} ({}): metrics={:?}", result.step_id, result.step_type, result.metrics);
        }
    }

    info!("=== Comparison Complete ===");
    info!("✅ WorkflowBehavior: Unified platform (HPC + Big Data + ML integration)");
    info!("✅ DurabilityFacet: Durable execution for complex workflows");
    info!("✅ Multi-domain workflows: Seamless integration across domains");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unified_workflow() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "unified-workflow/test-1@test-node".to_string();
        let behavior = Box::new(UnifiedWorkflowActor::new());
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

        let steps = vec![
            WorkflowStep::HPC(HCPStep {
                step_id: "test-hpc".to_string(),
                step_type: "test".to_string(),
                input_data: vec![],
                parameters: std::collections::HashMap::new(),
            }),
        ];
        let msg = Message::new(serde_json::to_vec(&UnifiedWorkflowMessage::StartWorkflow {
            workflow_id: "test-workflow".to_string(),
            steps,
        }).unwrap())
            .with_message_type("workflow_start".to_string());
        let result = workflow
            .ask(msg, Duration::from_secs(10))
            .await
            .unwrap();

        let reply: UnifiedWorkflowMessage = serde_json::from_slice(result.payload()).unwrap();
        if let UnifiedWorkflowMessage::WorkflowComplete { workflow_id, results } = reply {
            assert_eq!(workflow_id, "test-workflow");
            assert!(!results.is_empty());
        } else {
            panic!("Expected WorkflowComplete message");
        }
    }
}
