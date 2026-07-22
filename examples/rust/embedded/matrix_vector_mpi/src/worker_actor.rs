// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Worker Actor for Matrix-Vector Multiplication
//!
//! Worker actors read matrix rows and vector from TupleSpace,
//! compute the local matrix-vector product, and write results back.
//!
//! Uses SDK annotations:
//! - `#[event_actor]` - GenEvent behavior (fire-and-forget)
//! - `#[plexspaces_handlers(event)]` - Generates EventHandler dispatch
//! - `#[handler("Compute", cast)]` - Fire-and-forget handler

use plexspaces_sdk::{
    event_actor, plexspaces_handlers,
    ActorContext, BehaviorError, Message,
};
use plexspaces_tuplespace::{TupleSpace, Tuple, TupleField, Pattern, PatternField};
use std::sync::Arc;
use anyhow::Result;

/// Worker Actor
///
/// Each worker actor:
/// 1. Reads assigned matrix rows from TupleSpace (scatter pattern)
/// 2. Reads broadcast vector from TupleSpace
/// 3. Computes local matrix-vector product
/// 4. Writes results back to TupleSpace (gather pattern)
/// 5. Signals barrier completion
#[event_actor]
pub struct WorkerActor {
    tuplespace: Arc<TupleSpace>,
    worker_id: usize,
}

impl WorkerActor {
    pub fn new(tuplespace: Arc<TupleSpace>, worker_id: usize) -> Self {
        Self {
            tuplespace,
            worker_id,
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

/// SDK-generated handler dispatch for GenEvent behavior
#[plexspaces_handlers(event)]
impl WorkerActor {
    /// Handle Compute message (fire-and-forget)
    #[handler("Compute", cast)]
    async fn handle_compute(
        &mut self,
        _ctx: &ActorContext,
        msg: &Message,
    ) -> Result<(), BehaviorError> {
        // Deserialize payload (extract worker_id and num_cols from JSON)
        let payload: serde_json::Value = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse payload: {}", e)))?;

        let worker_id = payload["worker_id"]
            .as_u64()
            .ok_or_else(|| BehaviorError::ProcessingError("Missing or invalid worker_id".to_string()))?
            as usize;

        // Verify worker_id matches
        if worker_id != self.worker_id {
            return Err(BehaviorError::ProcessingError(format!(
                "Worker ID mismatch: expected {}, got {}",
                self.worker_id, worker_id
            )));
        }

        // Perform computation
        self.compute().await
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        Ok(())
    }
}

