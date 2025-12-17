// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Server Actor
//!
//! Serves online predictions via actor messages.

use async_trait::async_trait;
use plexspaces_core::{Actor as ActorTrait, ActorContext, ActorId, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use serde::{Deserialize, Serialize};
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionRequest {
    pub input: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResponse {
    pub prediction: f64,
}

pub struct ServerActor {
    // Actor state (would contain model in real implementation)
}

impl ServerActor {
    pub fn new() -> Self {
        Self {}
    }
}

#[async_trait]
impl ActorTrait for ServerActor {
    async fn handle_message(
        &mut self,
        ctx: &ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Parse message
        let request: PredictionRequest = serde_json::from_slice(&msg.payload)
            .map_err(|e| BehaviorError::ProcessingError(format!("Failed to parse request: {}", e)))?;

        info!("Serving prediction for input of length {}", request.input.len());

        // In a real implementation, we would:
        // 1. Load model weights
        // 2. Run inference
        // 3. Return prediction

        // For now, return simple prediction
        let prediction = request.input.iter().sum::<f64>() / request.input.len() as f64;

        let response = PredictionResponse { prediction };

        info!("Prediction: {:.4}", response.prediction);

        Ok(())
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::Custom("Server".to_string())
    }
}

