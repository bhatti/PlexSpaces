// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>

//! Calculator actor (will be compiled to WASM)
//!
//! This actor performs basic arithmetic operations. It demonstrates:
//! - Stateless computation (calculations are pure functions)
//! - Durable execution (calculations are journaled)
//! - WASM compatibility (can be compiled to WASM)
//!
//! ## Compiling to WASM
//! ```bash
//! # Compile this actor to WASM (future work)
//! cargo build --target wasm32-wasip2 --release
//! ```

use crate::models::*;
use async_trait::async_trait;
use plexspaces_core::Actor;
use plexspaces_mailbox::Message;
use serde_json;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Calculator actor state
#[derive(Debug, Clone)]
pub struct CalculatorActorState {
    /// Number of calculations performed
    pub calculation_count: u64,

    /// Last calculation result
    pub last_result: Option<f64>,
}

impl Default for CalculatorActorState {
    fn default() -> Self {
        Self {
            calculation_count: 0,
            last_result: None,
        }
    }
}

/// Calculator actor (will be WASM actor)
pub struct CalculatorActor {
    state: Arc<RwLock<CalculatorActorState>>,
}

impl CalculatorActor {
    /// Create new calculator actor
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(CalculatorActorState::default())),
        }
    }
}

impl Default for CalculatorActor {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl Actor for CalculatorActor {
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
            "calculate" => {
                // Deserialize request
                let request: CalculationRequest = serde_json::from_slice(payload)
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e.to_string()))?;

                tracing::info!(
                    "Performing calculation: {:?} with operands {:?}",
                    request.operation,
                    request.operands
                );

                // Execute calculation
                let result = request.execute()
                    .map_err(|e| plexspaces_core::BehaviorError::ProcessingError(e))?;

                // Update state
                {
                    let mut state = self.state.write().await;
                    state.calculation_count += 1;
                    state.last_result = Some(result);
                }

                tracing::info!("Calculation result: {}", result);
                Ok(())
            }

            "get_stats" => {
                let state = self.state.read().await;

                tracing::info!(
                    "Calculator stats: count={}, last_result={:?}",
                    state.calculation_count,
                    state.last_result
                );

                Ok(())
            }

            _ => {
                Err(plexspaces_core::BehaviorError::UnsupportedMessage)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::ActorContext;
    use plexspaces_mailbox::Message;
    use std::sync::Arc;

    #[tokio::test]
    async fn test_calculator_actor_addition() {
        let mut actor = CalculatorActor::new();

        // Create context using minimal constructor
        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let actor_ctx = Arc::new(ActorContext::new(
            "test-node".to_string(),
            "test".to_string(),
            "default".to_string(),
            service_locator,
            None,
        ));

        // Create calculation request
        let request = CalculationRequest::new(
            Operation::Add,
            vec![5.0, 3.0],
            "test-requester".to_string()
        );

        let request_bytes = serde_json::to_vec(&request).expect("Failed to serialize");

        // Create message
        let message = Message::new(request_bytes)
            .with_message_type("calculate".to_string())
            .with_sender("test-requester".to_string());

        // Handle message
        actor.handle_message(&*actor_ctx, message).await
            .expect("Failed to handle message");

        // Check state was updated
        let state = actor.state.read().await;
        assert_eq!(state.calculation_count, 1);
        assert_eq!(state.last_result, Some(8.0));
    }

    #[tokio::test]
    async fn test_calculator_actor_division_by_zero() {
        let mut actor = CalculatorActor::new();

        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let actor_ctx = Arc::new(ActorContext::new(
            "test-node".to_string(),
            "test".to_string(),
            "default".to_string(),
            service_locator,
            None,
        ));

        let request = CalculationRequest::new(
            Operation::Divide,
            vec![10.0, 0.0],
            "test-requester".to_string()
        );

        let request_bytes = serde_json::to_vec(&request).expect("Failed to serialize");

        let message = Message::new(request_bytes)
            .with_message_type("calculate".to_string());

        // Should return error for division by zero
        let result = actor.handle_message(&*actor_ctx, message).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_calculator_actor_stats() {
        let mut actor = CalculatorActor::new();

        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("test-node".to_string()), None, None).await;
        let actor_ctx = Arc::new(ActorContext::new(
            "test-node".to_string(),
            "test".to_string(),
            "default".to_string(),
            service_locator,
            None,
        ));

        // Perform calculation
        let request = CalculationRequest::new(
            Operation::Multiply,
            vec![4.0, 3.0],
            "test-requester".to_string()
        );

        let request_bytes = serde_json::to_vec(&request).expect("Failed to serialize");

        let message = Message::new(request_bytes)
            .with_message_type("calculate".to_string());

        actor.handle_message(&*actor_ctx, message).await.expect("Failed to calculate");

        // Get stats
        let stats_message = Message::new(vec![])
            .with_message_type("get_stats".to_string());

        actor.handle_message(&*actor_ctx, stats_message).await
            .expect("Failed to get stats");

        // Verify state was updated correctly
        let state = actor.state.read().await;
        assert_eq!(state.calculation_count, 1);
        assert_eq!(state.last_result, Some(12.0));
    }
}
