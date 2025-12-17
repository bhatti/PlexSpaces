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

//! Worker Behavior for Matrix Multiplication
//!
//! Processes matrix multiplication blocks from TupleSpace.

use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{TupleSpace, Pattern, PatternField, TupleField};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::matrix::{Matrix, multiply_blocks};

/// Helper function to process a work tuple (used by background task)
async fn process_work_tuple(
    tuplespace: &Arc<TupleSpace>,
    worker_id: &str,
    work_tuple: plexspaces_tuplespace::Tuple,
) -> Result<(), anyhow::Error> {
    // Extract work metadata
    let block_i = if let TupleField::Integer(i) = &work_tuple.fields()[1] {
        *i as usize
    } else {
        anyhow::bail!("Invalid block_i in work tuple");
    };

    let block_j = if let TupleField::Integer(j) = &work_tuple.fields()[2] {
        *j as usize
    } else {
        anyhow::bail!("Invalid block_j in work tuple");
    };

    let payload = if let TupleField::Binary(data) = &work_tuple.fields()[3] {
        data
    } else {
        anyhow::bail!("Invalid payload in work tuple");
    };

    tracing::debug!("Worker ({}): Processing block ({}, {})", worker_id, block_i, block_j);

    // Deserialize A and B blocks from payload
    let (a_blocks, b_blocks) = deserialize_blocks_helper(payload)?;

    // Compute result block: C[i][j] = Σ(k) A[i][k] × B[k][j]
    let result_block = multiply_blocks(&a_blocks, &b_blocks);

    // Serialize result block
    let result_data = serde_json::to_vec(&result_block)?;

    // Write result tuple to TupleSpace
    let result_tuple = plexspaces_tuplespace::Tuple::new(vec![
        TupleField::String("result".to_string()),
        TupleField::Integer(block_i as i64),
        TupleField::Integer(block_j as i64),
        TupleField::Binary(result_data),
    ]);

    tuplespace.write(result_tuple).await?;
    tracing::debug!("Worker ({}): Completed block ({}, {})", worker_id, block_i, block_j);
    Ok(())
}

/// Helper function to deserialize blocks (used by both methods)
fn deserialize_blocks_helper(payload: &[u8]) -> Result<(Vec<Matrix>, Vec<Matrix>), anyhow::Error> {
    let mut offset = 0;

    // Read number of A blocks
    if payload.len() < offset + 4 {
        anyhow::bail!("Payload too short for A block count");
    }
    let num_a_blocks = u32::from_le_bytes([
        payload[offset],
        payload[offset + 1],
        payload[offset + 2],
        payload[offset + 3],
    ]) as usize;
    offset += 4;

    // Read A blocks
    let mut a_blocks = Vec::with_capacity(num_a_blocks);
    for _ in 0..num_a_blocks {
        if payload.len() < offset + 4 {
            anyhow::bail!("Payload too short for A block size");
        }
        let block_size = u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]) as usize;
        offset += 4;

        if payload.len() < offset + block_size {
            anyhow::bail!("Payload too short for A block data");
        }
        let block_data = &payload[offset..offset + block_size];
        offset += block_size;

        let block: Matrix = serde_json::from_slice(block_data)?;
        a_blocks.push(block);
    }

    // Read B blocks (same count as A blocks)
    let mut b_blocks = Vec::with_capacity(num_a_blocks);
    for _ in 0..num_a_blocks {
        if payload.len() < offset + 4 {
            anyhow::bail!("Payload too short for B block size");
        }
        let block_size = u32::from_le_bytes([
            payload[offset],
            payload[offset + 1],
            payload[offset + 2],
            payload[offset + 3],
        ]) as usize;
        offset += 4;

        if payload.len() < offset + block_size {
            anyhow::bail!("Payload too short for B block data");
        }
        let block_data = &payload[offset..offset + block_size];
        offset += block_size;

        let block: Matrix = serde_json::from_slice(block_data)?;
        b_blocks.push(block);
    }

    Ok((a_blocks, b_blocks))
}

/// Worker message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerMessage {
    /// Start processing work from TupleSpace
    Start,
    /// Process a single work item (for explicit control)
    ProcessWork,
}

/// Worker Behavior
///
/// Processes matrix multiplication work from TupleSpace:
/// 1. Takes work tuples from TupleSpace (destructive read)
/// 2. Deserializes A and B blocks
/// 3. Computes result block
/// 4. Writes result back to TupleSpace
pub struct WorkerBehavior {
    tuplespace: Arc<TupleSpace>,
    worker_id: String,
    running: Arc<RwLock<bool>>,
}

impl WorkerBehavior {
    pub fn new(tuplespace: Arc<TupleSpace>, worker_id: String) -> Self {
        Self {
            tuplespace,
            worker_id,
            running: Arc::new(RwLock::new(false)),
        }
    }

    /// Process a single work item from TupleSpace
    async fn process_work(&self) -> Result<(), anyhow::Error> {
        // Try to take a work tuple (destructive read - ensures no duplicate work)
        let pattern = Pattern::new(vec![
            PatternField::Exact(TupleField::String("work".to_string())),
            PatternField::Wildcard,  // Any block_i
            PatternField::Wildcard,  // Any block_j
            PatternField::Wildcard,  // A/B blocks payload
        ]);

        // Non-blocking take with timeout
        let work_tuple = match tokio::time::timeout(
            tokio::time::Duration::from_millis(100),
            self.tuplespace.take(pattern)
        ).await {
            Ok(Ok(Some(tuple))) => tuple,
            Ok(Ok(None)) => {
                // No work available
                return Ok(());
            }
            Ok(Err(e)) => {
                tracing::warn!("Worker ({}): Error taking work: {}", self.worker_id, e);
                return Ok(());
            }
            Err(_) => {
                // Timeout - no work available
                return Ok(());
            }
        };

        // Extract work metadata
        let block_i = if let TupleField::Integer(i) = &work_tuple.fields()[1] {
            *i as usize
        } else {
            tracing::warn!("Worker ({}): Invalid block_i in work tuple", self.worker_id);
            return Ok(());
        };

        let block_j = if let TupleField::Integer(j) = &work_tuple.fields()[2] {
            *j as usize
        } else {
            tracing::warn!("Worker ({}): Invalid block_j in work tuple", self.worker_id);
            return Ok(());
        };

        let payload = if let TupleField::Binary(data) = &work_tuple.fields()[3] {
            data
        } else {
            tracing::warn!("Worker ({}): Invalid payload in work tuple", self.worker_id);
            return Ok(());
        };

        tracing::debug!("Worker ({}): Processing block ({}, {})", self.worker_id, block_i, block_j);

        // Deserialize A and B blocks from payload
        let (a_blocks, b_blocks) = match deserialize_blocks_helper(payload) {
            Ok(blocks) => blocks,
            Err(e) => {
                tracing::warn!("Worker ({}): Failed to deserialize blocks: {}", self.worker_id, e);
                return Ok(());
            }
        };

        // Compute result block: C[i][j] = Σ(k) A[i][k] × B[k][j]
        let result_block = multiply_blocks(&a_blocks, &b_blocks);

        // Serialize result block
        let result_data = match serde_json::to_vec(&result_block) {
            Ok(data) => data,
            Err(e) => {
                tracing::warn!("Worker ({}): Failed to serialize result: {}", self.worker_id, e);
                return Ok(());
            }
        };

        // Write result tuple to TupleSpace
        let result_tuple = plexspaces_tuplespace::Tuple::new(vec![
            TupleField::String("result".to_string()),
            TupleField::Integer(block_i as i64),
            TupleField::Integer(block_j as i64),
            TupleField::Binary(result_data),
        ]);

        if let Err(e) = self.tuplespace.write(result_tuple).await {
            tracing::warn!("Worker ({}): Failed to write result: {}", self.worker_id, e);
            return Ok(());
        }

        tracing::debug!("Worker ({}): Completed block ({}, {})", self.worker_id, block_i, block_j);
        Ok(())
    }


    /// Start background processing loop
    async fn start_processing(&self) {
        let mut running = self.running.write().await;
        if *running {
            return; // Already running
        }
        *running = true;
        drop(running);

        let tuplespace = self.tuplespace.clone();
        let worker_id = self.worker_id.clone();
        let running_flag = self.running.clone();

        // Spawn background task
        tokio::spawn(async move {
            tracing::info!("Worker ({}): Starting background processing...", worker_id);
            loop {
                // Check if we should stop
                {
                    let r = running_flag.read().await;
                    if !*r {
                        break;
                    }
                }

                // Try to process work (simplified - just call process_work logic inline)
                let pattern = plexspaces_tuplespace::Pattern::new(vec![
                    plexspaces_tuplespace::PatternField::Exact(
                        plexspaces_tuplespace::TupleField::String("work".to_string())
                    ),
                    plexspaces_tuplespace::PatternField::Wildcard,
                    plexspaces_tuplespace::PatternField::Wildcard,
                    plexspaces_tuplespace::PatternField::Wildcard,
                ]);

                match tokio::time::timeout(
                    tokio::time::Duration::from_millis(100),
                    tuplespace.take(pattern)
                ).await {
                    Ok(Ok(Some(work_tuple))) => {
                        // Process work tuple
                        if let Err(e) = process_work_tuple(&tuplespace, &worker_id, work_tuple).await {
                            tracing::warn!("Worker ({}): Error processing work: {}", worker_id, e);
                        }
                    }
                    _ => {
                        // No work available or timeout
                    }
                }

                // Small delay to avoid busy-waiting
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
            tracing::info!("Worker ({}): Background processing stopped", worker_id);
        });
    }

    /// Stop background processing
    async fn stop_processing(&self) {
        let mut running = self.running.write().await;
        *running = false;
    }
}

#[async_trait::async_trait]
impl ActorTrait for WorkerBehavior {
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
        let worker_msg: WorkerMessage = serde_json::from_slice(payload)
            .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;

        match worker_msg {
            WorkerMessage::Start => {
                self.start_processing().await;
            }
            WorkerMessage::ProcessWork => {
                self.process_work().await
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
            }
        }

        Ok(())
    }
}

