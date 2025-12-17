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

//! Worker Actor for Matrix-Vector Multiplication
//!
//! Worker actors read matrix rows and vector from TupleSpace,
//! compute the local matrix-vector product, and write results back.

use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use anyhow::Result;

/// Worker message types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkerMessage {
    /// Start computation for this worker
    Compute {
        worker_id: usize,
        num_cols: usize,
    },
}

/// Worker Actor Behavior
///
/// Each worker actor:
/// 1. Reads assigned matrix rows from TupleSpace (scatter pattern)
/// 2. Reads broadcast vector from TupleSpace
/// 3. Computes local matrix-vector product
/// 4. Writes results back to TupleSpace (gather pattern)
/// 5. Signals barrier completion
pub struct WorkerActor {
    tuplespace: Arc<TupleSpace>,
    worker_id: usize,
    num_cols: usize,
}

impl WorkerActor {
    pub fn new(tuplespace: Arc<TupleSpace>, worker_id: usize, num_cols: usize) -> Self {
        Self {
            tuplespace,
            worker_id,
            num_cols,
        }
    }

    /// Perform worker computation
    async fn compute(&self) -> Result<()> {
        // Read assigned rows
        let pattern_rows = Pattern::new(vec![
            PatternField::Exact(TupleField::String("scatter".to_string())),
            PatternField::Exact(TupleField::String("matrix_rows".to_string())),
            PatternField::Exact(TupleField::Integer(self.worker_id as i64)),
            PatternField::Wildcard,
        ]);

        let rows_tuple = self.tuplespace.read(pattern_rows).await?
            .ok_or_else(|| anyhow::anyhow!("Worker {} rows not found", self.worker_id))?;

        let rows_json = match &rows_tuple.fields()[3] {
            TupleField::String(s) => s,
            _ => return Err(anyhow::anyhow!("Invalid rows data")),
        };
        let worker_rows: Vec<Vec<f64>> = serde_json::from_str(rows_json)?;

        // Read broadcast vector
        let pattern_vector = Pattern::new(vec![
            PatternField::Exact(TupleField::String("broadcast".to_string())),
            PatternField::Exact(TupleField::String("vector".to_string())),
            PatternField::Wildcard,
        ]);

        let vector_tuple = self.tuplespace.read(pattern_vector).await?
            .ok_or_else(|| anyhow::anyhow!("Vector not found"))?;

        let vector_json = match &vector_tuple.fields()[2] {
            TupleField::String(s) => s,
            _ => return Err(anyhow::anyhow!("Invalid vector data")),
        };
        let vector: Vec<f64> = serde_json::from_str(vector_json)?;

        // Compute local matrix-vector product
        let mut local_result = Vec::new();
        for row in &worker_rows {
            let dot_product: f64 = row.iter().zip(&vector)
                .map(|(a, b)| a * b)
                .sum();
            local_result.push(dot_product);
        }

        tracing::info!("Worker {} computed {} results", self.worker_id, local_result.len());

        // Write result for gather
        let result_json = serde_json::to_string(&local_result)?;

        self.tuplespace.write(Tuple::new(vec![
            TupleField::String("gather".to_string()),
            TupleField::String("partial_result".to_string()),
            TupleField::Integer(self.worker_id as i64),
            TupleField::String(result_json),
        ])).await?;

        // Write barrier arrival
        self.tuplespace.write(Tuple::new(vec![
            TupleField::String("barrier".to_string()),
            TupleField::String("compute_done".to_string()),
            TupleField::Integer(self.worker_id as i64),
        ])).await?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl ActorTrait for WorkerActor {
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
            WorkerMessage::Compute { .. } => {
                self.compute().await
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;
            }
        }

        Ok(())
    }
}

