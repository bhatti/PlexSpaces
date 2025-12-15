// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Master Behavior for Matrix Multiplication
//!
//! Coordinates block-based parallel matrix multiplication via TupleSpace.

use plexspaces_core::{ActorBehavior, BehaviorType, ActorContext};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Instant;
use anyhow::Result;

use crate::matrix::{Matrix, multiply_blocks};

/// Master message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MasterMessage {
    /// Start matrix multiplication computation
    Start {
        matrix_size: usize,
        block_size: usize,
    },
}

/// Master Behavior
///
/// Coordinates distributed matrix multiplication:
/// 1. Generates input matrices
/// 2. Divides into blocks and distributes work via TupleSpace
/// 3. Waits for workers to complete
/// 4. Gathers results and verifies correctness
pub struct MasterBehavior {
    tuplespace: Arc<TupleSpace>,
}

impl MasterBehavior {
    pub fn new(tuplespace: Arc<TupleSpace>) -> Self {
        Self { tuplespace }
    }

    /// Run the master: divide, distribute, gather, assemble
    async fn run_computation(
        &self,
        matrix_size: usize,
        block_size: usize,
    ) -> Result<()> {
        let start_time = Instant::now();

        tracing::info!("Master: Starting matrix multiplication...");
        tracing::info!("  Matrix size: {}×{}", matrix_size, matrix_size);
        tracing::info!("  Block size: {}×{}", block_size, block_size);

        // Step 1: Generate input matrices
        tracing::info!("Step 1: Generating matrices...");
        let matrix_a = Matrix::random(matrix_size, matrix_size);
        let matrix_b = Matrix::random(matrix_size, matrix_size);

        // Compute expected result (sequential, for verification)
        let expected_start = Instant::now();
        let expected = matrix_a.multiply(&matrix_b);
        let sequential_time = expected_start.elapsed();
        tracing::info!("  Sequential time: {:?}", sequential_time);

        // Step 2: Divide matrices into blocks
        let num_blocks_per_dim = matrix_size / block_size;
        let total_work_items = num_blocks_per_dim * num_blocks_per_dim;

        tracing::info!("Step 2: Dividing into {}×{} blocks ({} total)...",
            num_blocks_per_dim, num_blocks_per_dim, total_work_items);

        let divide_start = Instant::now();
        self.distribute_work(&matrix_a, &matrix_b, num_blocks_per_dim, block_size).await?;
        let divide_time = divide_start.elapsed();

        tracing::info!("  Distribution time: {:?}", divide_time);
        tracing::info!("  {} work tuples written to TupleSpace", total_work_items);

        // Step 3: Wait for workers to complete (poll for results)
        tracing::info!("Step 3: Waiting for workers to compute {} blocks...", total_work_items);
        let compute_start = Instant::now();

        loop {
            let count = self.count_results(num_blocks_per_dim).await?;
            if count == total_work_items {
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }

        let compute_time = compute_start.elapsed();
        tracing::info!("  Computation time: {:?}", compute_time);

        // Step 4: Gather results
        tracing::info!("Step 4: Gathering {} result blocks from TupleSpace...", total_work_items);
        let gather_start = Instant::now();

        let result = self.gather_results(num_blocks_per_dim, block_size, matrix_size).await?;

        let gather_time = gather_start.elapsed();
        tracing::info!("  Gather time: {:?}", gather_time);
        tracing::info!("  Result matrix assembled");

        // Step 5: Verify correctness
        tracing::info!("Step 5: Verifying result...");
        let epsilon = 1e-10;
        if result.approx_equal(&expected, epsilon) {
            tracing::info!("  ✓ Result matches expected values!");
        } else {
            tracing::error!("  ✗ Result does NOT match expected values!");
            anyhow::bail!("Verification failed");
        }

        // Performance summary
        let total_time = start_time.elapsed();

        tracing::info!("Performance Summary:");
        tracing::info!("  Sequential time:     {:?}", sequential_time);
        tracing::info!("  Parallel total:      {:?}", total_time);
        tracing::info!("  - Distribution:      {:?}", divide_time);
        tracing::info!("  - Computation:       {:?}", compute_time);
        tracing::info!("  - Gathering:         {:?}", gather_time);
        tracing::info!("  Speedup:             {:.2}×",
            sequential_time.as_secs_f64() / compute_time.as_secs_f64());
        tracing::info!("  Overhead:            {:?} ({:.1}%)",
            divide_time + gather_time,
            ((divide_time + gather_time).as_secs_f64() / total_time.as_secs_f64()) * 100.0);

        Ok(())
    }

    /// Distribute work to TupleSpace
    async fn distribute_work(
        &self,
        matrix_a: &Matrix,
        matrix_b: &Matrix,
        num_blocks_per_dim: usize,
        block_size: usize,
    ) -> Result<()> {
        for block_i in 0..num_blocks_per_dim {
            for block_j in 0..num_blocks_per_dim {
                // Extract A blocks: A[block_i][0], A[block_i][1], ..., A[block_i][num_blocks-1]
                let mut a_blocks_data = Vec::new();
                for k in 0..num_blocks_per_dim {
                    let block = matrix_a.get_block(
                        block_i * block_size,
                        k * block_size,
                        block_size,
                        block_size,
                    );
                    let serialized = serde_json::to_vec(&block)?;
                    a_blocks_data.push(serialized);
                }

                // Extract B blocks: B[0][block_j], B[1][block_j], ..., B[num_blocks-1][block_j]
                let mut b_blocks_data = Vec::new();
                for k in 0..num_blocks_per_dim {
                    let block = matrix_b.get_block(
                        k * block_size,
                        block_j * block_size,
                        block_size,
                        block_size,
                    );
                    let serialized = serde_json::to_vec(&block)?;
                    b_blocks_data.push(serialized);
                }

                // Flatten into single binary payload
                let mut payload = Vec::new();
                payload.extend_from_slice(&(a_blocks_data.len() as u32).to_le_bytes());
                for data in a_blocks_data {
                    payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    payload.extend_from_slice(&data);
                }
                for data in b_blocks_data {
                    payload.extend_from_slice(&(data.len() as u32).to_le_bytes());
                    payload.extend_from_slice(&data);
                }

                // Write work tuple: ("work", block_i, block_j, payload)
                let work_tuple = Tuple::new(vec![
                    TupleField::String("work".to_string()),
                    TupleField::Integer(block_i as i64),
                    TupleField::Integer(block_j as i64),
                    TupleField::Binary(payload),
                ]);

                self.tuplespace.write(work_tuple).await?;
            }
        }

        Ok(())
    }

    /// Count completed results
    async fn count_results(&self, num_blocks_per_dim: usize) -> Result<usize> {
        let mut count = 0;

        for block_i in 0..num_blocks_per_dim {
            for block_j in 0..num_blocks_per_dim {
                let pattern = Pattern::new(vec![
                    PatternField::Exact(TupleField::String("result".to_string())),
                    PatternField::Exact(TupleField::Integer(block_i as i64)),
                    PatternField::Exact(TupleField::Integer(block_j as i64)),
                    PatternField::Wildcard,
                ]);

                if self.tuplespace.exists(pattern).await? {
                    count += 1;
                }
            }
        }

        Ok(count)
    }

    /// Gather results from TupleSpace
    async fn gather_results(
        &self,
        num_blocks_per_dim: usize,
        block_size: usize,
        matrix_size: usize,
    ) -> Result<Matrix> {
        let mut result = Matrix::zeros(matrix_size, matrix_size);

        for block_i in 0..num_blocks_per_dim {
            for block_j in 0..num_blocks_per_dim {
                // Read result tuple: ("result", block_i, block_j, result_block)
                let pattern = Pattern::new(vec![
                    PatternField::Exact(TupleField::String("result".to_string())),
                    PatternField::Exact(TupleField::Integer(block_i as i64)),
                    PatternField::Exact(TupleField::Integer(block_j as i64)),
                    PatternField::Wildcard,
                ]);

                let tuple = self.tuplespace.read(pattern).await?
                    .ok_or_else(|| anyhow::anyhow!("Result block ({}, {}) not found", block_i, block_j))?;

                // Extract result block
                if let TupleField::Binary(data) = &tuple.fields()[3] {
                    let block: Matrix = serde_json::from_slice(data)?;

                    // Insert into result matrix
                    result.set_block(
                        block_i * block_size,
                        block_j * block_size,
                        &block,
                    );
                } else {
                    anyhow::bail!("Invalid result tuple format");
                }
            }
        }

        Ok(result)
    }
}

#[async_trait::async_trait]
impl ActorBehavior for MasterBehavior {
    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenEvent
    }

    async fn handle_message(
        &mut self,
        _context: &ActorContext,
        message: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        // Deserialize message
        let payload = message.payload();
        let master_msg: MasterMessage = serde_json::from_slice(payload)
            .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;

        match master_msg {
            MasterMessage::Start { matrix_size, block_size } => {
                self.run_computation(matrix_size, block_size).await
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
            }
        }

        Ok(())
    }
}

