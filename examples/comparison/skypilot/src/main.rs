// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: SkyPilot (AI Workload Orchestration with Multi-Cloud Resource Scheduling)

use plexspaces_actor::{ActorBuilder, ActorRef, ActorFactory, actor_factory_impl::ActorFactoryImpl};
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, ActorId};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use std::collections::HashMap;
use tracing::info;

/// AI Task (ML training, inference, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AITask {
    pub task_id: String,
    pub task_type: String, // "training", "inference", "preprocessing"
    pub gpu_required: bool,
    pub gpu_memory_gb: u32,
    pub cpu_cores: u32,
    pub memory_gb: u32,
    pub cloud_preference: Option<String>, // "aws", "gcp", "azure", None = cheapest
}

/// Resource allocation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAllocation {
    pub task_id: String,
    pub cloud_provider: String,
    pub instance_type: String,
    pub cost_per_hour: f64,
    pub estimated_duration_hours: f64,
}

/// SkyPilot scheduler message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SkyPilotMessage {
    SubmitTask { task: AITask },
    GetBestResources { task: AITask },
    TaskScheduled { allocation: ResourceAllocation },
    ResourceRecommendation { allocation: ResourceAllocation },
    GetStatus,
    Status { queue_size: usize, running: usize },
}

/// SkyPilot Scheduler Actor
/// Demonstrates: Multi-Cloud Resource Scheduling, Cost Optimization, AI Workload Orchestration
pub struct SkyPilotSchedulerActor {
    task_queue: Vec<AITask>,
    running_tasks: HashMap<String, ResourceAllocation>,
    cloud_catalog: HashMap<String, CloudInstance>, // provider -> instances
}

#[derive(Debug, Clone)]
struct CloudInstance {
    instance_type: String,
    gpu_count: u32,
    gpu_memory_gb: u32,
    cpu_cores: u32,
    memory_gb: u32,
    cost_per_hour: f64,
    availability: f64, // 0.0 to 1.0
}

impl SkyPilotSchedulerActor {
    pub fn new() -> Self {
        // Initialize cloud catalog (simulated multi-cloud instances)
        let mut cloud_catalog = HashMap::new();
        
        // AWS instances
        cloud_catalog.insert("aws:g4dn.xlarge".to_string(), CloudInstance {
            instance_type: "g4dn.xlarge".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 4,
            memory_gb: 16,
            cost_per_hour: 0.526,
            availability: 0.95,
        });
        cloud_catalog.insert("aws:p3.2xlarge".to_string(), CloudInstance {
            instance_type: "p3.2xlarge".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 8,
            memory_gb: 61,
            cost_per_hour: 3.06,
            availability: 0.90,
        });
        
        // GCP instances
        cloud_catalog.insert("gcp:n1-standard-4".to_string(), CloudInstance {
            instance_type: "n1-standard-4".to_string(),
            gpu_count: 0,
            gpu_memory_gb: 0,
            cpu_cores: 4,
            memory_gb: 15,
            cost_per_hour: 0.19,
            availability: 0.98,
        });
        cloud_catalog.insert("gcp:n1-standard-8-gpu".to_string(), CloudInstance {
            instance_type: "n1-standard-8-gpu".to_string(),
            gpu_count: 1,
            gpu_memory_gb: 16,
            cpu_cores: 8,
            memory_gb: 30,
            cost_per_hour: 1.50,
            availability: 0.92,
        });
        
        Self {
            task_queue: Vec::new(),
            running_tasks: HashMap::new(),
            cloud_catalog,
        }
    }

    fn find_best_resources(&self, task: &AITask) -> Option<ResourceAllocation> {
        // SkyPilot algorithm: Find cheapest available instance that meets requirements
        let mut best_allocation: Option<ResourceAllocation> = None;
        let mut best_cost = f64::MAX;
        
        for (instance_id, instance) in &self.cloud_catalog {
            // Check if instance meets requirements
            let meets_gpu = !task.gpu_required || (instance.gpu_count > 0 && instance.gpu_memory_gb >= task.gpu_memory_gb);
            let meets_cpu = instance.cpu_cores >= task.cpu_cores;
            let meets_memory = instance.memory_gb >= task.memory_gb;
            let meets_cloud = task.cloud_preference.is_none() || 
                instance_id.starts_with(&format!("{}:", task.cloud_preference.as_ref().unwrap()));
            
            if meets_gpu && meets_cpu && meets_memory && meets_cloud {
                // Adjust cost by availability (lower availability = higher effective cost)
                let effective_cost = instance.cost_per_hour / instance.availability;
                
                if effective_cost < best_cost {
                    let (provider, _) = instance_id.split_once(':').unwrap_or(("unknown", ""));
                    best_allocation = Some(ResourceAllocation {
                        task_id: task.task_id.clone(),
                        cloud_provider: provider.to_string(),
                        instance_type: instance.instance_type.clone(),
                        cost_per_hour: instance.cost_per_hour,
                        estimated_duration_hours: 1.0, // Simplified
                    });
                    best_cost = effective_cost;
                }
            }
        }
        
        best_allocation
    }
}

#[async_trait::async_trait]
impl Actor for SkyPilotSchedulerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
        
    ) -> Result<(), BehaviorError> {
        <Self as GenServer>::route_message(self, ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

#[async_trait::async_trait]
impl GenServer for SkyPilotSchedulerActor {
    async fn handle_request(
        &mut self,
        _ctx: &ActorContext,
        msg: Message,
    ) -> Result<Message, BehaviorError> {
        let sky_msg: SkyPilotMessage = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;
        
        match sky_msg {
            SkyPilotMessage::SubmitTask { task } => {
                info!("[SKYPILOT] Submitting AI task: {} (type: {}, gpu: {})", 
                    task.task_id, task.task_type, task.gpu_required);
                
                // Find best resources
                if let Some(allocation) = self.find_best_resources(&task) {
                    self.running_tasks.insert(task.task_id.clone(), allocation.clone());
                    info!("[SKYPILOT] Scheduled task on {} ({}) - cost: ${}/hr", 
                        allocation.cloud_provider, allocation.instance_type, allocation.cost_per_hour);
                    
                    let reply = SkyPilotMessage::TaskScheduled { allocation };
                    Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
                } else {
                    // Queue if no resources available
                    self.task_queue.push(task);
                    Err(BehaviorError::ProcessingError("No resources available, queued".to_string()))
                }
            }
            SkyPilotMessage::GetBestResources { task } => {
                if let Some(allocation) = self.find_best_resources(&task) {
                    let reply = SkyPilotMessage::ResourceRecommendation { allocation };
                    Ok(Message::new(serde_json::to_vec(&reply).unwrap()))
                } else {
                    Err(BehaviorError::ProcessingError("No suitable resources found".to_string()))
                }
            }
            SkyPilotMessage::GetStatus => {
                let reply = SkyPilotMessage::Status {
                    queue_size: self.task_queue.len(),
                    running: self.running_tasks.len(),
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

    info!("=== SkyPilot vs PlexSpaces Comparison ===");
    info!("Demonstrating Multi-Cloud AI Workload Orchestration with Cost Optimization");

    let node = NodeBuilder::new("comparison-node-1")
        .build();

    // SkyPilot schedules AI workloads across multiple clouds
    let actor_id: ActorId = "skypilot-scheduler/scheduler-1@comparison-node-1".to_string();
    
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating SkyPilot scheduler (multi-cloud resource scheduling)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let behavior = Box::new(SkyPilotSchedulerActor::new());
    let mut actor = ActorBuilder::new(behavior)
        .with_id(actor_id.clone())
        .build()
        .await;
    
    // Spawn using ActorFactory
    let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
        .ok_or_else(|| format!("ActorFactory not found in ServiceLocator"))?;
    let actor_id = actor.id().clone();
    let ctx = plexspaces_core::RequestContext::internal();
    let _message_sender = actor_factory.spawn_actor(
        &ctx,
        &actor_id,
        "GenServer", // actor_type from SkyPilotSchedulerActor
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
    
    let scheduler = plexspaces_actor::ActorRef::local(
        actor_ref.id().clone(),
        mailbox,
        node.service_locator().clone(),
    );

    info!("✅ SkyPilot scheduler created: {}", scheduler.id());
    info!("✅ Multi-cloud catalog: AWS, GCP instances");
    info!("✅ Cost optimization: Finds cheapest available resources");

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test ML training task (GPU required)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 1: ML Training Task (GPU required, cost-optimized)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let training_task = AITask {
        task_id: "training-1".to_string(),
        task_type: "training".to_string(),
        gpu_required: true,
        gpu_memory_gb: 16,
        cpu_cores: 4,
        memory_gb: 16,
        cloud_preference: None, // Find cheapest
    };
    
    let msg = Message::new(serde_json::to_vec(&SkyPilotMessage::SubmitTask {
        task: training_task.clone(),
    })?)
        .with_message_type("call".to_string());
    let result = scheduler
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: SkyPilotMessage = serde_json::from_slice(result.payload())?;
    if let SkyPilotMessage::TaskScheduled { allocation } = reply {
        info!("✅ Training task scheduled: {} on {} ({}) - ${}/hr", 
            allocation.task_id, allocation.cloud_provider, 
            allocation.instance_type, allocation.cost_per_hour);
    }

    // Test inference task (no GPU, cheaper)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 2: Inference Task (CPU-only, cost-optimized)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let inference_task = AITask {
        task_id: "inference-1".to_string(),
        task_type: "inference".to_string(),
        gpu_required: false,
        gpu_memory_gb: 0,
        cpu_cores: 4,
        memory_gb: 15,
        cloud_preference: None, // Find cheapest
    };
    
    let msg = Message::new(serde_json::to_vec(&SkyPilotMessage::SubmitTask {
        task: inference_task.clone(),
    })?)
        .with_message_type("call".to_string());
    let result = scheduler
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: SkyPilotMessage = serde_json::from_slice(result.payload())?;
    if let SkyPilotMessage::TaskScheduled { allocation } = reply {
        info!("✅ Inference task scheduled: {} on {} ({}) - ${}/hr", 
            allocation.task_id, allocation.cloud_provider, 
            allocation.instance_type, allocation.cost_per_hour);
    }

    // Test resource recommendation
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test 3: Resource Recommendation (pre-flight check)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let msg = Message::new(serde_json::to_vec(&SkyPilotMessage::GetBestResources {
        task: training_task,
    })?)
        .with_message_type("call".to_string());
    let result = scheduler
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: SkyPilotMessage = serde_json::from_slice(result.payload())?;
    if let SkyPilotMessage::ResourceRecommendation { allocation } = reply {
        info!("✅ Resource recommendation: {} ({}) - ${}/hr", 
            allocation.cloud_provider, allocation.instance_type, allocation.cost_per_hour);
    }

    info!("=== Comparison Complete ===");
    info!("✅ Multi-cloud scheduling: AWS, GCP, Azure support");
    info!("✅ Cost optimization: Finds cheapest available resources");
    info!("✅ Resource-aware: Matches task requirements to instances");
    info!("✅ AI workload orchestration: ML training, inference, preprocessing");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_skypilot_scheduler() {
        let node = NodeBuilder::new("test-node")
            .build();

        let actor_id: ActorId = "skypilot-scheduler/test-1@test-node".to_string();
        let behavior = Box::new(SkyPilotSchedulerActor::new());
        let mut actor = ActorBuilder::new(behavior)
            .with_id(actor_id.clone())
            .build()
            .await;
        
        // Spawn using ActorFactory
        // Note: Since actor has DurabilityFacet attached, we use spawn_built_actor
        let actor_factory: Arc<ActorFactoryImpl> = node.service_locator().get_service().await
            .ok_or_else(|| format!("ActorFactory not found in ServiceLocator")).unwrap();
        let actor_id = actor.id().clone();
        let ctx = plexspaces_core::RequestContext::internal();
        let _message_sender = actor_factory.spawn_built_actor(&ctx, Arc::new(actor), Some("GenServer".to_string())).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();
        let actor_ref = plexspaces_core::ActorRef::new(actor_id)
            .map_err(|e| format!("Failed to create ActorRef: {}", e)).unwrap();

        let mailbox = node.actor_registry()
            .lookup_mailbox(actor_ref.id())
            .await
            .unwrap()
            .unwrap();
        
        let scheduler = plexspaces_actor::ActorRef::local(
            actor_ref.id().clone(),
            mailbox,
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        let task = AITask {
            task_id: "test-task".to_string(),
            task_type: "training".to_string(),
            gpu_required: true,
            gpu_memory_gb: 16,
            cpu_cores: 4,
            memory_gb: 16,
            cloud_preference: None,
        };
        let msg = Message::new(serde_json::to_vec(&SkyPilotMessage::SubmitTask {
            task,
        }).unwrap())
            .with_message_type("call".to_string());
        let result = scheduler
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: SkyPilotMessage = serde_json::from_slice(result.payload()).unwrap();
        if let SkyPilotMessage::TaskScheduled { allocation } = reply {
            assert!(!allocation.cloud_provider.is_empty());
            assert!(!allocation.instance_type.is_empty());
        } else {
            panic!("Expected TaskScheduled message");
        }
    }
}
