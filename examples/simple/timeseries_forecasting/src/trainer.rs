// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Trainer Actor
//!
//! Trains a simple linear forecasting model (DLinear-inspired).

use async_trait::async_trait;
use plexspaces_core::{ActorBehavior, ActorContext, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use super::preprocessor::PreprocessedData;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelWeights {
    pub weights: Vec<f64>,
    pub bias: f64,
}

pub struct TrainerActor {
    model: Option<ModelWeights>,
}

impl TrainerActor {
    pub fn new() -> Self {
        Self { model: None }
    }
}

#[async_trait]
impl ActorBehavior for TrainerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse message
        let data: PreprocessedData = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse data: {}", e)))?;

        info!("Training model on {} windows", data.windows.len());

        // In a real implementation, we would:
        // 1. Train a linear model (e.g., DLinear)
        // 2. Aggregate gradients from multiple actors
        // 3. Update model weights
        // 4. Save model state

        // For now, create simple model
        let window_size = data.windows[0].len();
        let weights = vec![0.1; window_size];
        let bias = 0.0;
        let weight_count = weights.len();

        self.model = Some(ModelWeights { weights, bias });

        info!("Model trained: {} weights, bias={}", weight_count, bias);

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("Trainer".to_string())
    }
}

