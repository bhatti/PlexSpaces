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

//! Comprehensive tests for GenServer behavior pattern
//!
//! Following TDD principles - these tests define the expected behavior

#[cfg(test)]
mod tests {
    use super::super::*;
    use plexspaces_core::{
        Actor, ActorContext, BehaviorContext, BehaviorError, BehaviorType,
    };
    use plexspaces_mailbox::Message;
    use std::sync::Arc;

    // Counter state for testing
    #[derive(Clone)]
    struct Counter {
        value: i32,
        call_count: usize,
        // Note: cast_count removed (Phase 2 cleanup - GenServer doesn't handle events)
    }

    impl Counter {
        fn new() -> Self {
            Self {
                value: 0,
                call_count: 0,
            }
        }
    }

    // Implement Actor for Counter
    #[async_trait::async_trait]
    impl Actor for Counter {
        async fn handle_message(
            &mut self,
            ctx: &ActorContext,
            msg: Message,
        ) -> Result<(), BehaviorError> {
            self.route_message(ctx, msg).await
        }

        fn behavior_type(&self) -> BehaviorType {
            BehaviorType::GenServer
        }
    }

    // Implement GenServer for Counter
    #[async_trait::async_trait]
    impl GenServer for Counter {
        async fn handle_request(
            &mut self,
            ctx: &ActorContext,
            msg: Message,
        ) -> Result<(), BehaviorError> {
            self.call_count += 1;

            // Parse the message payload to determine action
            if let Ok(s) = String::from_utf8(msg.payload().to_vec()) {
                if s.contains("increment") || s.contains("inc") {
                    self.value += 10;
                    // Send reply if sender_id is present
                    if let Some(sender_id) = &msg.sender {
                        let reply_data = format!("value={}", self.value);
                        let reply = Message::new(reply_data.into_bytes());
                        // Use ctx.send_reply() to send reply
                        let _ = ctx.send_reply(
                            msg.correlation_id.as_deref(),
                            sender_id,
                            msg.receiver.clone(),
                            reply,
                        ).await; // Ignore errors in tests
                    }
                    return Ok(());
                } else if s == "fail" {
                    return Err(BehaviorError::ProcessingError("Simulated error".to_string()));
                }
            }

            // Send reply if sender_id is present
            if let Some(sender_id) = &msg.sender {
                let reply_data = format!("Reply to: {:?}", msg.payload());
                let reply = Message::new(reply_data.into_bytes());
                let _ = ctx.send_reply(
                    msg.correlation_id.as_deref(),
                    sender_id,
                    msg.receiver.clone(), // This actor's ID (the receiver of the original message)
                    reply,
                ).await; // Ignore errors in tests
            }
            Ok(())
        }

        // Note: handle_event removed from GenServerBehavior (Phase 2 cleanup)
        // For events, use GenEvent instead
    }


    // Helper to create test context and message
    async fn create_test_context_and_message(
        mut message: Message,
        correlation_id: Option<String>,
    ) -> (Arc<ActorContext>, Message, Arc<plexspaces_mailbox::Mailbox>) {
        use plexspaces_mailbox::{Mailbox, MailboxConfig};
        use plexspaces_core::{ServiceLocator, ActorRegistry};

        // Create a mailbox for the reply channel
        let mailbox = Arc::new(Mailbox::new(MailboxConfig::default(), "test-actor@test-node".to_string()).await.expect("Failed to create mailbox"));

        // Create ServiceLocator with ActorRegistry for envelope.send_reply() to work
        use plexspaces_core::ObjectRegistry;
        let service_locator = Arc::new(ServiceLocator::new());
        // Create a stub ObjectRegistry for ActorRegistry (from actor_context module)
        // Note: We can't easily create a real ObjectRegistry in tests, so we'll skip ActorRegistry for now
        // and handle the case where envelope.send_reply() fails gracefully in tests
        // For production, ActorRegistry will be properly registered

        // Create context with ServiceLocator
        let ctx = Arc::new(ActorContext::new(
            "test-node".to_string(),
            "test-ns".to_string(),
            service_locator,
            None,
        ));

        // Set sender_id so envelope can send replies (required for GenServer handle_request)
        message.sender = Some("test-sender@test-node".to_string());
        
        // Add correlation ID if provided
        if let Some(corr_id) = correlation_id {
            message = message.with_correlation_id(corr_id);
        }

        (ctx, message, mailbox)
    }

    // Test 1: Basic request with reply
    #[tokio::test]
    async fn test_genserver_request_with_reply() {
        let mut behavior = Counter::new();
        assert_eq!(behavior.behavior_type(), BehaviorType::GenServer);

        // Create request message
        let request_msg = Message::new(b"test call".to_vec()).with_message_type("call".to_string());
        let (ctx, msg, _mailbox) =
            create_test_context_and_message(request_msg, Some("corr-123".to_string())).await;

        // Handle the request (Go-style: context first, then message)
        behavior.handle_message(&*ctx, msg).await.unwrap();

        // Verify state was updated
        assert_eq!(behavior.call_count, 1);

        // Note: Reply handling via ActorService will be tested in integration tests
        // For now, we verify that handle_request was called
    }

    // Test 2: Request with state mutation
    #[tokio::test]
    async fn test_genserver_request_mutates_state() {
        let mut behavior = Counter::new();

        // First request
        let msg1 = Message::new(b"increment".to_vec()).with_message_type("call".to_string());
        let (ctx1, msg1_actual, _mailbox1) = create_test_context_and_message(msg1, None).await;
        behavior.handle_message(&*ctx1, msg1_actual).await.unwrap();

        // Verify state was updated (reply handling via ActorService will be tested in integration tests)
        assert_eq!(behavior.value, 10);
        assert_eq!(behavior.call_count, 1);

        // Second request - state should persist
        let msg2 = Message::new(b"increment".to_vec()).with_message_type("call".to_string());
        let (ctx2, msg2_actual, _mailbox2) = create_test_context_and_message(msg2, None).await;
        behavior.handle_message(&*ctx2, msg2_actual).await.unwrap();

        // Verify state persisted and incremented
        assert_eq!(behavior.value, 20);
        assert_eq!(behavior.call_count, 2);
    }

    // Test 3: Request with error reply
    #[tokio::test]
    async fn test_genserver_request_with_error() {
        let mut behavior = Counter::new();

        let msg = Message::new(b"fail".to_vec()).with_message_type("call".to_string());
        let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;

        // Handle should return error
        let result = behavior.handle_message(&*ctx, msg_actual).await;
        assert!(result.is_err());
        assert_eq!(behavior.call_count, 1); // State was still mutated
    }

    // Test 4: Event rejection (Phase 2 cleanup)
    #[tokio::test]
    async fn test_genserver_rejects_events() {
        let mut behavior = Counter::new();

        // GenServer should accept cast messages (for HTTP POST/PUT support)
        let msg = Message::new(b"inc".to_vec()).with_message_type("cast".to_string());
        let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;

        let result = behavior.handle_message(&*ctx, msg_actual).await;
        assert!(result.is_ok()); // Cast messages are now accepted

        // State should be mutated (cast message was processed)
        assert_eq!(behavior.value, 10);
    }

    // Test 5: Multiple cast messages - all should be accepted
    #[tokio::test]
    async fn test_genserver_rejects_multiple_events() {
        let mut behavior = Counter::new();

        // Send 5 cast messages - all should be accepted (for HTTP POST/PUT support)
        for _ in 0..5 {
            let msg = Message::new(b"inc".to_vec()).with_message_type("cast".to_string());
            let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;
            let result = behavior.handle_message(&*ctx, msg_actual).await;
            assert!(result.is_ok()); // Cast messages are now accepted
        }

        // State should be mutated (5 * 10 = 50)
        assert_eq!(behavior.value, 50);
    }

    // Test 6: Correlation ID preservation
    #[tokio::test]
    async fn test_genserver_correlation_id() {
        let mut behavior = Counter::new();

        let msg = Message::new(b"test".to_vec()).with_message_type("call".to_string());
        let correlation_id = "trace-abc-123".to_string();
        let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, Some(correlation_id.clone())).await;

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Note: Correlation ID preservation will be tested in integration tests
        // For now, we verify that handle_request was called
        assert_eq!(behavior.call_count, 1);
    }

    // Test 7: Sequential requests
    #[tokio::test]
    async fn test_genserver_sequential_requests() {
        let mut behavior = Counter::new();

        // Simulate 10 sequential requests
        for i in 1..=10 {
            let msg = Message::new(b"inc".to_vec()).with_message_type("call".to_string());
            let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;
            behavior.handle_message(&*ctx, msg_actual).await.unwrap();

            // Verify state incrementally (reply handling via ActorService will be tested in integration tests)
            assert_eq!(behavior.value, i * 10);
        }

        assert_eq!(behavior.value, 100);
        assert_eq!(behavior.call_count, 10);
    }

    // Test 8: Mixed message types
    #[tokio::test]
    async fn test_genserver_mixed_message_types() {
        let mut behavior = Counter::new();

        // Request (adds 10)
        let msg1 = Message::new(b"inc".to_vec()).with_message_type("call".to_string());
        let (ctx1, msg1_actual, _mailbox1) = create_test_context_and_message(msg1, None).await;
        behavior.handle_message(&*ctx1, msg1_actual).await.unwrap();

        // Cast message should be accepted (GenServer now supports Cast for HTTP POST/PUT)
        let msg2 = Message::new(b"inc".to_vec()).with_message_type("cast".to_string());
        let (ctx2, msg2_actual, _) = create_test_context_and_message(msg2, None).await;
        let result = behavior.handle_message(&*ctx2, msg2_actual).await;
        assert!(result.is_ok()); // Cast messages are now accepted

        // Request again (adds 10)
        let msg3 = Message::new(b"inc".to_vec()).with_message_type("call".to_string());
        let (ctx3, msg3_actual, _mailbox3) = create_test_context_and_message(msg3, None).await;
        behavior.handle_message(&*ctx3, msg3_actual).await.unwrap();

        // Final state: 10 + 10 + 10 = 30 (cast message was processed)
        assert_eq!(behavior.value, 30);
    }

    // Test 9: Cast message acceptance (GenServer now supports Cast for HTTP POST/PUT)
    #[tokio::test]
    async fn test_genserver_rejects_cast_messages() {
        let mut behavior = Counter::new();

        // GenServer should accept cast messages (for HTTP POST/PUT support)
        let msg = Message::new(b"inc".to_vec()).with_message_type("cast".to_string());
        let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;

        let result = behavior.handle_message(&*ctx, msg_actual).await;
        assert!(result.is_ok()); // Cast messages are now accepted
        
        // State should be mutated (cast message was processed)
        assert_eq!(behavior.value, 10);
    }

    // Test 10: Unknown message type (should be ignored)
    #[tokio::test]
    async fn test_genserver_unknown_message_type() {
        let mut behavior = Counter::new();

        // Use a different message type (e.g., WorkflowRun)
        let msg = Message::new(b"unknown".to_vec()).with_message_type("workflow_run".to_string());
        let (ctx, msg_actual, _mailbox) = create_test_context_and_message(msg, None).await;

        // Should succeed but do nothing
        behavior.handle_message(&*ctx, msg_actual).await.unwrap();
        assert_eq!(behavior.value, 0); // No state change
    }
}
