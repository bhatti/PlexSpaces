// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Preprocessor Actor
//!
//! Normalizes time-series data, handles missing values, and creates sliding windows.

use async_trait::async_trait;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use super::data_loader::DataChunk;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreprocessedData {
    pub windows: Vec<Vec<f64>>,
    pub targets: Vec<f64>,
}

pub struct PreprocessorActor {
    // Actor state
}

impl PreprocessorActor {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ActorTrait for PreprocessorActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse message
        let chunk: DataChunk = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse chunk: {}", e)))?;

        info!("Preprocessing chunk {} with {} points", chunk.partition_id, chunk.data.len());

        // In a real implementation, we would:
        // 1. Normalize data (z-score, min-max, etc.)
        // 2. Handle missing values
        // 3. Create sliding windows for training
        // 4. Send preprocessed data to trainer

        // For now, create simple windows
        let window_size = 10;
        let windows = chunk.data
            .windows(window_size)
            .map(|w| w.to_vec())
            .collect::<Vec<_>>();
        let targets = chunk.data[window_size..].to_vec();

        let preprocessed = PreprocessedData { windows, targets };

        info!("Preprocessed chunk {}: {} windows", chunk.partition_id, preprocessed.windows.len());

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("Preprocessor".to_string())
    }
}

