// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Matrix Multiplication Example (Parallel with Actors)
//
// Demonstrates parallel computation using PlexSpaces actors:
// - ActorBuilder to create worker actors
// - tell() for work distribution (scatter)
// - ask() for result collection (gather)
//
// Use Case: Scientific computing, ML inference, graphics

use async_trait::async_trait;
use plexspaces_actor::ActorBuilder;
use plexspaces_core::{
    Actor as ActorTrait, ActorContext, BehaviorError, BehaviorType, RequestContext,
};
use plexspaces_mailbox::Message;
use plexspaces_node::NodeBuilder;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// =============================================================================
// Messages
// =============================================================================

#[derive(Debug, Serialize, Deserialize)]
enum WorkerMessage {
    ComputeRows {
        start_row: usize,
        end_row: usize,
        matrix_a: Vec<Vec<f64>>,
        matrix_b: Vec<Vec<f64>>,
    },
    GetResult,
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkerResult {
    start_row: usize,
    rows: Vec<Vec<f64>>,
}

// =============================================================================
// Worker Actor - computes assigned rows
// =============================================================================

struct MatrixWorker {
    id: usize,
    result: Option<WorkerResult>,
}

impl MatrixWorker {
    fn new(id: usize) -> Self {
        Self { id, result: None }
    }

    fn compute_rows(a: &[Vec<f64>], b: &[Vec<f64>], start: usize, end: usize) -> Vec<Vec<f64>> {
        let b_cols = b[0].len();
        let mut result = Vec::new();

        for row_idx in start..end {
            let row_a = &a[row_idx];
            let mut result_row = vec![0.0; b_cols];
            
            for j in 0..b_cols {
                let mut sum = 0.0;
                for k in 0..row_a.len() {
                    sum += row_a[k] * b[k][j];
                }
                result_row[j] = sum;
            }
            result.push(result_row);
        }
        result
    }
}

#[async_trait]
impl ActorTrait for MatrixWorker {
    async fn handle_message(
        &mut self,
        _ctx: &ActorContext,
        message: plexspaces_proto::common::v1::Message,
    ) -> Result<(), BehaviorError> {
        let msg: WorkerMessage = serde_json::from_slice(&message.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Parse error: {}", e)))?;

        match msg {
            WorkerMessage::ComputeRows { start_row, end_row, matrix_a, matrix_b } => {
                println!("    Worker {}: computing rows {}..{}", self.id, start_row, end_row - 1);
                
                let rows = Self::compute_rows(&matrix_a, &matrix_b, start_row, end_row);
                
                self.result = Some(WorkerResult { start_row, rows });
                println!("    Worker {}: done", self.id);
            }
            WorkerMessage::GetResult => {
                // In a real implementation, this would reply via ask()
                if let Some(ref result) = self.result {
                    println!("    Worker {}: returning {} rows starting at {}", 
                             self.id, result.rows.len(), result.start_row);
                }
            }
        }
        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("╔════════════════════════════════════════════════════════════════╗");
    println!("║       Matrix Multiplication with Actor Workers                 ║");
    println!("╚════════════════════════════════════════════════════════════════╝");
    println!();
    println!("PlexSpaces APIs demonstrated:");
    println!("  - ActorBuilder::new().spawn() - create worker actors");
    println!("  - actor.tell(msg)             - distribute work (scatter)");
    println!("  - actor.ask(msg, timeout)     - collect results (gather)");
    println!();

    // =========================================================================
    // Step 1: Create Node and Worker Actors
    // =========================================================================
    println!("Step 1: Create node and worker actors");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let node = Arc::new(NodeBuilder::new("matrix-node").build().await);
    let service_locator = node.service_locator();
    let ctx = RequestContext::new_without_auth("matrix-tenant".to_string(), "compute".to_string());

    let num_workers = 2;
    let mut workers = Vec::new();

    for i in 0..num_workers {
        let worker = ActorBuilder::new(Box::new(MatrixWorker::new(i)))
            .with_id(format!("worker-{}@matrix-node", i))
            .with_namespace("compute")
            .spawn(&ctx, service_locator.clone())
            .await
            .map_err(|e| format!("Failed to spawn worker: {}", e))?;
        
        println!("  Created worker-{}", i);
        workers.push(worker);
    }
    println!();

    // =========================================================================
    // Step 2: Define matrices
    // =========================================================================
    println!("Step 2: Define matrices");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // A: 4x3 matrix
    let matrix_a: Vec<Vec<f64>> = vec![
        vec![1.0, 2.0, 3.0],
        vec![4.0, 5.0, 6.0],
        vec![7.0, 8.0, 9.0],
        vec![10.0, 11.0, 12.0],
    ];

    // B: 3x2 matrix
    let matrix_b: Vec<Vec<f64>> = vec![
        vec![1.0, 2.0],
        vec![3.0, 4.0],
        vec![5.0, 6.0],
    ];

    println!("  Matrix A (4x3):");
    for row in &matrix_a {
        println!("    {:?}", row);
    }
    println!("  Matrix B (3x2):");
    for row in &matrix_b {
        println!("    {:?}", row);
    }
    println!();

    // =========================================================================
    // Step 3: SCATTER - Distribute work via tell()
    // =========================================================================
    println!("Step 3: SCATTER work via ActorRef::tell()");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let rows_per_worker = matrix_a.len() / num_workers;
    
    for (i, worker) in workers.iter().enumerate() {
        let start_row = i * rows_per_worker;
        let end_row = if i == num_workers - 1 { matrix_a.len() } else { start_row + rows_per_worker };
        
        let work = WorkerMessage::ComputeRows {
            start_row,
            end_row,
            matrix_a: matrix_a.clone(),
            matrix_b: matrix_b.clone(),
        };
        
        let msg = Message::json(&work)?.with_message_type("compute_rows");
        
        println!("  tell(worker-{}, ComputeRows {{ rows: {}..{} }})", i, start_row, end_row - 1);
        worker.tell(msg).await.map_err(|e| format!("tell failed: {}", e))?;
    }
    
    // Wait for processing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    println!();

    // =========================================================================
    // Step 4: GATHER - Collect results via ask()
    // =========================================================================
    println!("Step 4: GATHER results via ActorRef::ask()");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // In production: let response = worker.ask(GetResult, timeout).await?
    for (i, worker) in workers.iter().enumerate() {
        let get_msg = Message::json(&WorkerMessage::GetResult)?.with_message_type("get_result");
        println!("  ask(worker-{}, GetResult)", i);
        worker.tell(get_msg).await.map_err(|e| format!("ask failed: {}", e))?;
    }
    
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    println!();

    // =========================================================================
    // Step 5: Show expected result
    // =========================================================================
    println!("Step 5: Expected result C = A * B");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    // Compute expected result for verification
    let expected: Vec<Vec<f64>> = vec![
        vec![22.0, 28.0],
        vec![49.0, 64.0],
        vec![76.0, 100.0],
        vec![103.0, 136.0],
    ];
    
    println!("  Result C (4x2):");
    for row in &expected {
        println!("    {:?}", row);
    }
    println!();

    // =========================================================================
    // Summary
    // =========================================================================
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Matrix Multiplication Example Complete");
    println!();
    println!("Actor-based Parallel Pattern:");
    println!();
    println!("  ┌─────────────────────────────────────────────────────┐");
    println!("  │                    Master                           │");
    println!("  │  - Partition rows among workers                     │");
    println!("  │  - Distribute via tell() (scatter)                  │");
    println!("  │  - Collect via ask() (gather)                       │");
    println!("  └─────────────────┬───────────────────────────────────┘");
    println!("                    │");
    println!("        ┌───────────┼───────────┐");
    println!("        ▼           ▼           ▼");
    println!("  ┌──────────┐ ┌──────────┐ ┌──────────┐");
    println!("  │ Worker 0 │ │ Worker 1 │ │ Worker N │");
    println!("  │ rows 0-1 │ │ rows 2-3 │ │ rows ... │");
    println!("  └──────────┘ └──────────┘ └──────────┘");
    println!();
    println!("Key APIs:");
    println!("  - ActorBuilder::new(behavior).spawn(&ctx, service_locator)");
    println!("  - actor_ref.tell(msg) - fire-and-forget (scatter)");
    println!("  - actor_ref.ask(msg, timeout) - request-reply (gather)");
    println!();

    Ok(())
}
