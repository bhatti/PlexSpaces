// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// GenServer Calculator Example
//
// Demonstrates Erlang/OTP GenServer pattern with trait extension:
// - GenServerBehavior extends ActorBehavior
// - handle_call() for synchronous request-reply
// - handle_cast() for asynchronous fire-and-forget
// - handle_info() for system messages

use async_trait::async_trait;
use plexspaces_behavior::GenServer;
use plexspaces_core::{Actor, BehaviorError, BehaviorType};
use plexspaces_mailbox::Message;
use serde::{Deserialize, Serialize};

/// Calculator state (owned by GenServer)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorState {
    /// Running total
    pub total: f64,
    /// Operation count
    pub operation_count: u64,
    /// History of last 10 operations
    pub history: Vec<String>,
}

impl Default for CalculatorState {
    fn default() -> Self {
        Self {
            total: 0.0,
            operation_count: 0,
            history: Vec::new(),
        }
    }
}

/// Operation request
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum CalculatorOp {
    Add { value: f64 },
    Subtract { value: f64 },
    Multiply { value: f64 },
    Divide { value: f64 },
    Clear,
    GetTotal,
    GetHistory,
}

/// Operation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorResult {
    pub total: f64,
    pub operation_count: u64,
}

/// Calculator GenServer
///
/// This demonstrates the NEW trait-based GenServer pattern where
/// GenServerBehavior extends ActorBehavior with typed handler methods.
pub struct CalculatorGenServer {
    state: CalculatorState,
}

impl CalculatorGenServer {
    pub fn new() -> Self {
        Self {
            state: CalculatorState::default(),
        }
    }

    pub fn boxed() -> Box<dyn Actor> {
        Box::new(Self::new())
    }

    fn update_history(&mut self, op: &str) {
        self.state.history.push(op.to_string());
        if self.state.history.len() > 10 {
            self.state.history.remove(0);
        }
    }

    fn create_result(&self) -> CalculatorResult {
        CalculatorResult {
            total: self.state.total,
            operation_count: self.state.operation_count,
        }
    }
}

/// Implement Actor - the unified interface
#[async_trait]
impl Actor for CalculatorGenServer {
    async fn handle_message(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Delegate to route_message() from GenServer trait
        self.route_message(ctx, msg).await
    }

    fn behavior_type(&self) -> BehaviorType {
        BehaviorType::GenServer
    }
}

/// Implement GenServer - extends Actor with typed handlers
#[async_trait]
impl GenServer for CalculatorGenServer {
    /// Handle synchronous request (like Orleans Task<T>, gen_server:call)
    ///
    /// Used for operations that need a response.
    /// Combines calculator operations that return results.
    async fn handle_request(
        &mut self,
        ctx: &plexspaces_core::ActorContext,
        msg: Message,
    ) -> Result<(), BehaviorError> {
        // Deserialize operation
        let op: CalculatorOp = serde_json::from_slice(msg.payload())
            .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

        // Only send reply if sender_id is present
        let sender_id = match &msg.sender {
            Some(id) => id,
            None => return Ok(()), // Fire-and-forget, no reply needed
        };

        match op {
            CalculatorOp::Add { value } => {
                self.state.total += value;
                self.state.operation_count += 1;
                self.update_history(&format!("Add {}", value));

                let result = self.create_result();
                let payload = serde_json::to_vec(&result)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("result".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::Subtract { value } => {
                self.state.total -= value;
                self.state.operation_count += 1;
                self.update_history(&format!("Subtract {}", value));

                let result = self.create_result();
                let payload = serde_json::to_vec(&result)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("result".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::Multiply { value } => {
                self.state.total *= value;
                self.state.operation_count += 1;
                self.update_history(&format!("Multiply {}", value));

                let result = self.create_result();
                let payload = serde_json::to_vec(&result)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("result".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::Divide { value } => {
                if value == 0.0 {
                    return Err(BehaviorError::ProcessingError(
                        "Division by zero".to_string(),
                    ));
                }
                self.state.total /= value;
                self.state.operation_count += 1;
                self.update_history(&format!("Divide {}", value));

                let result = self.create_result();
                let payload = serde_json::to_vec(&result)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("result".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::GetTotal => {
                let result = self.create_result();
                let payload = serde_json::to_vec(&result)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("result".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::GetHistory => {
                let payload = serde_json::to_vec(&self.state.history)
                    .map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;

                let reply = Message::new(payload).with_message_type("history".to_string());
                ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(),
                    reply,
                ).await
                    .map_err(|e| BehaviorError::ProcessingError(format!("Failed to send reply: {}", e)))?;
                Ok(())
            }

            CalculatorOp::Clear => {
                // Clear should be an event (fire-and-forget), not a request
                Err(BehaviorError::UnsupportedMessage)
            }
        }
    }

    // Note: GenServerBehavior only handles synchronous requests (handle_request).
    // For fire-and-forget events, use GenEventBehavior instead.
    // This example demonstrates GenServer for request/reply patterns only.
}

/// Main function for running the GenServer calculator example
fn main() {
    println!("GenServer Calculator Example");
    println!("============================");
    println!();
    println!("This example demonstrates the GenServer behavior pattern with a calculator.");
    println!();
    println!("Key features:");
    println!("  - handle_request(): Synchronous operations (add, subtract, multiply, divide)");
    println!("  - handle_event(): Asynchronous operations (clear)");
    println!("  - Type-safe message handling");
    println!("  - State management (total, operation count, history)");
    println!();
    println!("Run tests with: cargo test --example genserver_calculator");
    println!();
    println!("See the #[cfg(test)] module below for comprehensive test examples.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_behavior::MessageType;
    use plexspaces_core::ActorContext;
    use std::collections::HashMap;
    use std::sync::Arc;

    fn create_test_context() -> Arc<ActorContext> {
        // Use minimal_with_config to avoid deprecated minimal() with actor_id
        use plexspaces_node::create_default_service_locator;
        let service_locator = create_default_service_locator(Some("node1".to_string()), None, None).await;
        Arc::new(ActorContext::new(
            "node1".to_string(),
            "test".to_string(),
            "default".to_string(),
            service_locator,
            None,
        ))
    }

    #[tokio::test]
    async fn test_genserver_calculator_add() {
        let mut calc = CalculatorGenServer::new();

        // Create add operation
        let op = CalculatorOp::Add { value: 10.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();

        let ctx = create_test_context();

        // Execute via unified handle_message interface
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Verify state (via another call)
        let op = CalculatorOp::GetTotal;
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();

        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Check state directly
        assert_eq!(calc.state.total, 10.0);
        assert_eq!(calc.state.operation_count, 1);
    }

    #[tokio::test]
    async fn test_genserver_calculator_operations() {
        let mut calc = CalculatorGenServer::new();

        // Add 10
        let op = CalculatorOp::Add { value: 10.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Multiply by 2
        let op = CalculatorOp::Multiply { value: 2.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Subtract 5
        let op = CalculatorOp::Subtract { value: 5.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Total should be (10 * 2) - 5 = 15
        assert_eq!(calc.state.total, 15.0);
        assert_eq!(calc.state.operation_count, 3);
    }

    #[tokio::test]
    async fn test_genserver_calculator_clear_cast() {
        let mut calc = CalculatorGenServer::new();

        // Add 10
        let op = CalculatorOp::Add { value: 10.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        assert_eq!(calc.state.total, 10.0);

        // Clear (cast - fire and forget)
        let op = CalculatorOp::Clear;
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Cast.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // Total should be 0 now
        assert_eq!(calc.state.total, 0.0);
        assert_eq!(calc.state.operation_count, 0);
    }

    #[tokio::test]
    async fn test_genserver_calculator_divide_by_zero() {
        let mut calc = CalculatorGenServer::new();

        // Divide by zero
        let op = CalculatorOp::Divide { value: 0.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();

        let ctx = create_test_context();
        let result = calc.handle_message(&*ctx, msg).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_genserver_calculator_history() {
        let mut calc = CalculatorGenServer::new();

        let ctx = create_test_context();
        // Perform operations
        for value in [5.0, 10.0, 3.0] {
            let op = CalculatorOp::Add { value };
            let payload = serde_json::to_vec(&op).unwrap();
            let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
            msg.sender = Some("test-sender".to_string());
            msg.receiver = "test-actor".to_string();
            calc.handle_message(&*ctx, msg).await.unwrap();
        }

        // Get history
        let op = CalculatorOp::GetHistory;
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type(MessageType::Call.to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        calc.handle_message(&*ctx, msg).await.unwrap();

        // History should have 3 operations
        assert_eq!(calc.state.history.len(), 3);
        assert_eq!(calc.state.history[0], "Add 5");
        assert_eq!(calc.state.history[1], "Add 10");
        assert_eq!(calc.state.history[2], "Add 3");
    }

    #[tokio::test]
    async fn test_trait_extension_design() {
        // This test demonstrates the key insight:
        // - ActorBehavior::handle_message is the UNIFIED interface
        // - GenServerBehavior EXTENDS it with typed handlers
        // - No redundancy - handle_message delegates to route_message()

        let mut calc = CalculatorGenServer::new();

        // We can call handle_message (unified interface)
        let op = CalculatorOp::Add { value: 42.0 };
        let payload = serde_json::to_vec(&op).unwrap();
        let mut msg = Message::new(payload).with_message_type("call".to_string());
        msg.sender = Some("test-sender".to_string());
        msg.receiver = "test-actor".to_string();
        let ctx = create_test_context();

        calc.handle_message(&*ctx, msg).await.unwrap();

        // Verify state was updated
        assert_eq!(calc.state.total, 42.0);
        assert_eq!(calc.state.operation_count, 1);
    }
}
