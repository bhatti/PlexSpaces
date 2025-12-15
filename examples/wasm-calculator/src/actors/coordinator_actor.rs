// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Coordinator actor for complex multi-step calculations

use crate::models::*;
use plexspaces_core::Actor;
use plexspaces_mailbox::Message;
use serde_json;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Coordinator state
#[derive(Debug, Clone)]
pub struct CoordinatorState {
    /// Active calculations (calculation_id -> ComplexCalculation)
    pub calculations: HashMap<String, ComplexCalculation>,

    /// Completed calculations count
    pub completed_count: u64,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            calculations: HashMap::new(),
            completed_count: 0,
        }
    }
}

/// Coordinator actor for multi-step calculations
pub struct CoordinatorActor {
    state: Arc<RwLock<CoordinatorState>>,
}

impl CoordinatorActor {
    /// Create new coordinator actor
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(CoordinatorState::default())),
        }
    }
}

impl Default for CoordinatorActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Actor for CoordinatorActor {
    fn behavior_type(&self) -> plexspaces_core::BehaviorType {
        plexspaces_core::BehaviorType::GenServer
    }

    async fn handle_message(
        &mut self,
        _ctx: &plexspaces_core::ActorContext,
        msg: Message,
    ) -> Result<(), plexspaces_core::BehaviorError> {
        let message_type = msg.message_type.as_str();
        let payload = &msg.payload;

        match message_type {
            "start_calculation" => {
                // Deserialize complex calculation
                let calc: ComplexCalculation = serde_json::from_slice(payload)
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;

                tracing::info!(
                    "Starting complex calculation {} with {} steps",
                    calc.calculation_id,
                    calc.steps.len()
                );

                // Store calculation
                {
                    let mut state = self.state.write().await;
                    state.calculations.insert(calc.calculation_id.clone(), calc.clone());
                }

                tracing::info!("Complex calculation {} started", calc.calculation_id);
                Ok(())
            }

            "get_status" => {
                let state = self.state.read().await;

                tracing::info!(
                    "Coordinator status: active={}, completed={}",
                    state.calculations.len(),
                    state.completed_count
                );

                Ok(())
            }

            _ => {
                Err(plexspaces_core::BehaviorError::UnsupportedMessage)
            }
        }
    }
}
