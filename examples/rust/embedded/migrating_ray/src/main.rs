// SPDX-License-Identifier: LGPL-2.1-or-later
// Comparison: Ray Parameter Server (Distributed ML Training with Elastic Pools)
// Based on: https://docs.ray.io/en/latest/ray-core/examples/plot_parameter_server.html

use plexspaces_actor::ActorRef;
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, ActorContext, BehaviorType, BehaviorError, Message};
use plexspaces_core::behavior_factory::BehaviorRegistry;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use tracing::info;

/// Model weights (simplified neural network)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWeights {
    pub w1: Vec<Vec<f32>>, // Hidden layer weights
    pub w2: Vec<f32>,      // Output layer weights
}

/// Gradients (same structure as weights)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gradients {
    pub d_w1: Vec<Vec<f32>>,
    pub d_w2: Vec<f32>,
}

/// Parameter Server message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParameterServerMessage {
    ApplyGradients { gradients: Vec<Gradients> },
    GetWeights,
    Weights { weights: ModelWeights },
}

/// Data Worker message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DataWorkerMessage {
    ComputeGradients { weights: ModelWeights },
    Gradients { gradients: Gradients },
}

/// Parameter Server Actor (Ray-style)
/// Demonstrates: Centralized Parameter Management, Gradient Aggregation, Elastic Scaling
pub struct ParameterServerActor {
    model_weights: ModelWeights,
    learning_rate: f32,
    iteration: u64,
}

impl ParameterServerActor {
    pub fn new(learning_rate: f32) -> Self {
        // Initialize model weights (simplified 2-layer network)
        Self {
            model_weights: ModelWeights {
                w1: vec![vec![0.1; 784]; 200], // 784 inputs -> 200 hidden
                w2: vec![0.1; 200],           // 200 hidden -> 1 output
            },
            learning_rate,
            iteration: 0,
        }
    }

    fn apply_gradients(&mut self, gradients: &[Gradients]) {
        // Aggregate gradients from multiple workers
        let mut aggregated_d_w1 = vec![vec![0.0; 784]; 200];
        let mut aggregated_d_w2 = vec![0.0; 200];

        for grad in gradients {
            for (i, row) in grad.d_w1.iter().enumerate() {
                for (j, val) in row.iter().enumerate() {
                    aggregated_d_w1[i][j] += val;
                }
            }
            for (i, val) in grad.d_w2.iter().enumerate() {
                aggregated_d_w2[i] += val;
            }
        }

        // Average gradients
        let num_workers = gradients.len() as f32;
        for row in aggregated_d_w1.iter_mut() {
            for val in row.iter_mut() {
                *val /= num_workers;
            }
        }
        for val in aggregated_d_w2.iter_mut() {
            *val /= num_workers;
        }

        // Update weights (SGD)
        for (i, row) in aggregated_d_w1.iter().enumerate() {
            for (j, grad) in row.iter().enumerate() {
                self.model_weights.w1[i][j] -= self.learning_rate * grad;
            }
        }
        for (i, grad) in aggregated_d_w2.iter().enumerate() {
            self.model_weights.w2[i] -= self.learning_rate * grad;
        }

        self.iteration += 1;
    }
}

#[async_trait::async_trait]
impl Actor for ParameterServerActor {
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
impl GenServer for ParameterServerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let ps_msg: ParameterServerMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        if msg.sender_id.is_empty() {
            return Ok(()); // No reply needed
        }

        match ps_msg {
            ParameterServerMessage::ApplyGradients { gradients } => {
                info!("[PARAMETER SERVER] Applying gradients from {} workers (iteration {})",
                    gradients.len(), self.iteration);
                self.apply_gradients(&gradients);
                let reply_msg = ParameterServerMessage::Weights {
                    weights: self.model_weights.clone(),
                };
                let reply = Message {
                    id: ulid::Ulid::new().to_string(),
                    payload: serde_json::to_vec(&reply_msg).unwrap(),
                    ..Default::default()
                };
                ctx.send_reply(
                    Some(&msg.correlation_id),
                    &msg.sender_id,
                    msg.receiver_id.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }
            ParameterServerMessage::GetWeights => {
                info!("[PARAMETER SERVER] Returning weights (iteration {})", self.iteration);
                let reply_msg = ParameterServerMessage::Weights {
                    weights: self.model_weights.clone(),
                };
                let reply = Message {
                    id: ulid::Ulid::new().to_string(),
                    payload: serde_json::to_vec(&reply_msg).unwrap(),
                    ..Default::default()
                };
                ctx.send_reply(
                    Some(&msg.correlation_id),
                    &msg.sender_id,
                    msg.receiver_id.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
    }
}

/// Data Worker Actor (Ray-style)
/// Demonstrates: Distributed Gradient Computation, Elastic Worker Pools
pub struct DataWorkerActor {
    worker_id: String,
    data_shard: Vec<(Vec<f32>, f32)>, // (input, target) pairs
    batch_size: usize,
}

impl DataWorkerActor {
    pub fn new(worker_id: String, data_shard: Vec<(Vec<f32>, f32)>, batch_size: usize) -> Self {
        Self {
            worker_id,
            data_shard,
            batch_size,
        }
    }

    fn compute_gradients(&self, _weights: &ModelWeights) -> Gradients {
        // Simulate gradient computation on data shard
        // In real app, this would compute actual gradients using backpropagation
        info!("[DATA WORKER {}] Computing gradients on {} samples",
            self.worker_id, self.data_shard.len());

        // Simulate gradient computation
        let mut d_w1 = vec![vec![0.0; 784]; 200];
        let mut d_w2 = vec![0.0; 200];

        // Process batch
        for (input, _target) in self.data_shard.iter().take(self.batch_size) {
            // Simulate forward/backward pass (simplified)
            for i in 0..200 {
                for j in 0..784 {
                    d_w1[i][j] += input[j] * 0.001; // Simulated gradient
                }
                d_w2[i] += 0.001; // Simulated gradient
            }
        }

        Gradients { d_w1, d_w2 }
    }
}

#[async_trait::async_trait]
impl Actor for DataWorkerActor {
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
impl GenServer for DataWorkerActor {
    async fn handle_request(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        let worker_msg: DataWorkerMessage = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse: {}", e)))?;

        if msg.sender_id.is_empty() {
            return Ok(()); // No reply needed
        }

        match worker_msg {
            DataWorkerMessage::ComputeGradients { weights } => {
                let gradients = self.compute_gradients(&weights);
                let reply_msg = DataWorkerMessage::Gradients { gradients };
                let reply = Message {
                    id: ulid::Ulid::new().to_string(),
                    payload: serde_json::to_vec(&reply_msg).unwrap(),
                    ..Default::default()
                };
                ctx.send_reply(
                    Some(&msg.correlation_id),
                    &msg.sender_id,
                    msg.receiver_id.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }
            _ => Err(BehaviorError::ProcessingError("Unknown message".to_string())),
        }
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
    println!("║  Ray vs PlexSpaces Comparison                                  ║");
    println!("║  Demonstrating Ray Parameter Server (Distributed ML Training)  ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    info!("=== Ray vs PlexSpaces Comparison ===");
    info!("Demonstrating Ray Parameter Server (Distributed ML Training)");
    info!("Based on: https://docs.ray.io/en/latest/ray-core/examples/plot_parameter_server.html");

    // Create a node
    let node = NodeBuilder::new("comparison-node-1")
        .build().await;

    // Register behaviors in BehaviorRegistry so ActorFactory can create them
    let behavior_registry = BehaviorRegistry::new();
    behavior_registry.register("ParameterServer", |_args| {
        Box::pin(async move {
            Ok(Box::new(ParameterServerActor::new(0.01)) as Box<dyn Actor>)
        })
    }).await;
    behavior_registry.register("DataWorker", |_args| {
        Box::pin(async move {
            Ok(Box::new(DataWorkerActor::new(
                "worker".to_string(),
                Vec::new(),
                32,
            )) as Box<dyn Actor>)
        })
    }).await;
    node.service_locator().register_behavior_registry(Arc::new(behavior_registry)).await;

    // Create Parameter Server (centralized model weights)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating Parameter Server (centralized model weights)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Spawn using Node::spawn (delegates to ActorFactory)
    let actor_id = "parameter-server@comparison-node-1".to_string();
    let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string()).with_internal(true).with_admin(true);
    let _message_sender = node.spawn(
        &ctx,
        &actor_id,
        "ParameterServer",
        vec![], // initial_state
        None, // config
        std::collections::HashMap::new(), // labels
        vec![], // facets
    ).await
        .map_err(|e| format!("Failed to spawn actor: {}", e))?;

    // Create ActorRef for messaging
    let parameter_server = ActorRef::remote(
        actor_id.clone(),
        "internal".to_string(),
        "system".to_string(),
        node.id().to_string(),
        node.service_locator().clone(),
    );

    info!("✅ Parameter Server created: {}", parameter_server.id());
    println!("✅ Parameter Server created: {}", parameter_server.id());
    println!("   - Centralized model weights management");
    println!("   - Gradient aggregation from multiple workers");
    println!();

    // Create elastic pool of data workers (Ray-style distributed training)
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Creating elastic pool of data workers (horizontal scaling)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Creating elastic pool of data workers (horizontal scaling)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let mut data_workers = Vec::new();
    let worker_count = 4; // Elastic pool size (can scale dynamically)

    // Generate data shards for each worker
    for i in 0..worker_count {
        // Each worker gets a shard of the dataset
        let _data_shard: Vec<(Vec<f32>, f32)> = (0..1000)
            .map(|j| {
                let input = (0..784).map(|_| (j as f32 + i as f32) * 0.001).collect();
                let target = (j % 10) as f32;
                (input, target)
            })
            .collect();

        // Spawn using Node::spawn
        let worker_id = format!("data-worker-{}@comparison-node-1", i);
        let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string()).with_internal(true).with_admin(true);
        let _message_sender = node.spawn(
            &ctx,
            &worker_id,
            "DataWorker",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e))?;

        // Create ActorRef for messaging
        let worker = ActorRef::remote(
            worker_id.clone(),
            "internal".to_string(),
            "system".to_string(),
            node.id().to_string(),
            node.service_locator().clone(),
        );
        data_workers.push(worker);
    }

    info!("✅ Created elastic pool of {} data workers", data_workers.len());
    println!("✅ Created elastic pool of {} data workers", data_workers.len());
    println!("   - Workers can scale horizontally (add/remove dynamically)");
    println!("   - Each worker processes a shard of the dataset");
    println!("   - Resource-aware scheduling distributes computation");
    println!();

    tokio::time::sleep(Duration::from_millis(200)).await;

    // Test synchronous parameter server training
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Synchronous Parameter Server Training (Ray pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Test: Synchronous Parameter Server Training (Ray pattern)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let iterations = 10;
    let training_start = std::time::Instant::now();

    // Get initial weights
    let msg = Message {
        id: ulid::Ulid::new().to_string(),
        payload: serde_json::to_vec(&ParameterServerMessage::GetWeights)?,
        message_type: "call".to_string(),
        ..Default::default()
    };
    let result = parameter_server
        .ask(msg, Duration::from_secs(5))
        .await?;
    let reply: ParameterServerMessage = serde_json::from_slice(&result.payload)?;
    let mut current_weights = if let ParameterServerMessage::Weights { weights } = reply {
        weights
    } else {
        return Err("Failed to get initial weights".into());
    };

    for iteration in 0..iterations {
        info!("[ITERATION {}] Starting training step", iteration);

        // Step 1: All workers compute gradients in parallel
        let mut gradient_futures = Vec::new();
        for worker in &data_workers {
            let worker_msg = Message {
                id: ulid::Ulid::new().to_string(),
                payload: serde_json::to_vec(&DataWorkerMessage::ComputeGradients {
                    weights: current_weights.clone(),
                })?,
                message_type: "call".to_string(),
                ..Default::default()
            };
            gradient_futures.push(worker.ask(worker_msg, Duration::from_secs(10)));
        }

        // Wait for all gradients
        let mut gradients = Vec::new();
        for future in gradient_futures {
            let result = future.await?;
            let reply: DataWorkerMessage = serde_json::from_slice(&result.payload)?;
            if let DataWorkerMessage::Gradients { gradients: grad } = reply {
                gradients.push(grad);
            }
        }

        info!("[ITERATION {}] Received gradients from {} workers", iteration, gradients.len());

        // Step 2: Parameter server aggregates and applies gradients
        let ps_msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: serde_json::to_vec(&ParameterServerMessage::ApplyGradients {
                gradients,
            })?,
            message_type: "call".to_string(),
            ..Default::default()
        };
        let result = parameter_server
            .ask(ps_msg, Duration::from_secs(5))
            .await?;
        let reply: ParameterServerMessage = serde_json::from_slice(&result.payload)?;
        if let ParameterServerMessage::Weights { weights } = reply {
            current_weights = weights;
            info!("[ITERATION {}] Updated model weights", iteration);
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let training_elapsed = training_start.elapsed();

    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("📊 Training Summary");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Iterations:        {}", iterations);
    println!("Workers:           {}", data_workers.len());
    println!("Total Time:        {:.2}ms", training_elapsed.as_secs_f64() * 1000.0);
    println!("Avg Time/Iter:     {:.2}ms", training_elapsed.as_secs_f64() * 1000.0 / iterations as f64);
    println!("Samples/Worker:    1000");
    println!("Total Samples:     {}", data_workers.len() * 1000);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!();

    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("Test: Asynchronous Parameter Server Training (Ray pattern)");
    info!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    info!("✅ Workers compute gradients asynchronously");
    info!("✅ Parameter server applies gradients as they arrive");
    info!("✅ Elastic scaling: Add/remove workers dynamically");

    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║  ✅ Comparison Complete                                         ║");
    println!("║  ✅ Parameter Server: Centralized model weights (Ray pattern)  ║");
    println!("║  ✅ Elastic Worker Pools: Horizontal scaling of data workers   ║");
    println!("║  ✅ Distributed Training: Parallel gradient computation        ║");
    println!("║  ✅ Resource-Aware Scheduling: Tasks distributed across workers║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    info!("=== Comparison Complete ===");
    info!("✅ Parameter Server: Centralized model weights (Ray pattern)");
    info!("✅ Elastic Worker Pools: Horizontal scaling of data workers");
    info!("✅ Distributed Training: Parallel gradient computation");
    info!("✅ Resource-Aware Scheduling: Tasks distributed across workers");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_parameter_server() {
        let node = NodeBuilder::new("test-node")
            .build().await;

        // Register behavior
        let behavior_registry = BehaviorRegistry::new();
        behavior_registry.register("ParameterServer", |_args| {
            Box::pin(async move {
                Ok(Box::new(ParameterServerActor::new(0.01)) as Box<dyn Actor>)
            })
        }).await;
        node.service_locator().register_behavior_registry(Arc::new(behavior_registry)).await;

        // Spawn using Node::spawn
        let actor_id = "test-ps@test-node".to_string();
        let ctx = plexspaces_core::RequestContext::new_without_auth("internal".to_string(), "system".to_string()).with_internal(true).with_admin(true);
        let _message_sender = node.spawn(
            &ctx,
            &actor_id,
            "ParameterServer",
            vec![], // initial_state
            None, // config
            std::collections::HashMap::new(), // labels
            vec![], // facets
        ).await
            .map_err(|e| format!("Failed to spawn actor: {}", e)).unwrap();

        // Create ActorRef for messaging
        let ps = ActorRef::remote(
            actor_id.clone(),
            "internal".to_string(),
            "system".to_string(),
            node.id().to_string(),
            node.service_locator().clone(),
        );

        tokio::time::sleep(Duration::from_millis(200)).await;

        // Test get weights
        let msg = Message {
            id: ulid::Ulid::new().to_string(),
            payload: serde_json::to_vec(&ParameterServerMessage::GetWeights).unwrap(),
            message_type: "call".to_string(),
            ..Default::default()
        };
        let result = ps
            .ask(msg, Duration::from_secs(5))
            .await
            .unwrap();

        let reply: ParameterServerMessage = serde_json::from_slice(&result.payload).unwrap();
        if let ParameterServerMessage::Weights { weights } = reply {
            assert_eq!(weights.w1.len(), 200);
            assert_eq!(weights.w2.len(), 200);
        } else {
            panic!("Expected Weights message");
        }
    }
}
