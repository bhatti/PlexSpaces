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

//! Integration tests for core PlexSpaces modules

#[cfg(test)]
mod integration_tests {
    use async_trait::async_trait;
    use crate::actor::{Actor as ActorStruct, ActorState};
    use crate::behavior::{GenServer, MessageType, MessageTypeExt, MockBehavior};
    use crate::core::{Actor, ActorContext, BehaviorError, BehaviorType};
    // use crate::journal::MemoryJournal; // Removed - not used in tests
    use crate::mailbox::{mailbox_config_default, Mailbox, MailboxConfig, Message, MessagePriority, OrderingStrategy};
    use std::sync::Arc;

    #[tokio::test]
    async fn test_message_priorities() {
        // Create mailbox with priority ordering
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = Mailbox::new(config, "test-actor@localhost".to_string()).await.unwrap();

        // Enqueue messages with different priorities
        mailbox
            .enqueue(Message::new(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();

        mailbox
            .enqueue(Message::new(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();

        mailbox
            .enqueue(Message::new(b"normal".to_vec()).with_priority(MessagePriority::Normal))
            .await
            .unwrap();

        mailbox
            .enqueue(Message::signal(b"signal".to_vec()))
            .await
            .unwrap();

        mailbox
            .enqueue(Message::system(b"system".to_vec()))
            .await
            .unwrap();

        // Dequeue and verify priority order
        // System (10) > Highest (5) > High (4) > Normal (3) > Low (2)
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload(), b"system");
        assert_eq!(msg1.priority(), MessagePriority::System);

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"signal");
        assert_eq!(msg2.priority(), MessagePriority::Highest); // Signal -> Highest

        let msg3 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg3.payload(), b"high");
        assert_eq!(msg3.priority(), MessagePriority::High);

        let msg4 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg4.payload(), b"normal");
        assert_eq!(msg4.priority(), MessagePriority::Normal);

        let msg5 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg5.payload(), b"low");
        assert_eq!(msg5.priority(), MessagePriority::Low);
    }

    #[tokio::test]
    async fn test_selective_receive() {
        let mailbox = Mailbox::new(mailbox_config_default(), "test-actor@localhost".to_string()).await.unwrap();

        // Enqueue multiple messages
        mailbox
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"target".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"fourth".to_vec()))
            .await
            .unwrap();

        // Selectively receive the "target" message
        let msg = mailbox.dequeue_matching(|m| m.payload() == b"target").await;
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().payload(), b"target");

        // Verify other messages are still in queue
        assert_eq!(mailbox.size().await, 3);

        // Regular dequeue gets first message
        let first = mailbox.dequeue().await.unwrap();
        assert_eq!(first.payload(), b"first");
    }

    #[tokio::test]
    async fn test_actor_lifecycle_with_behavior() {
        let id = "test-actor".to_string();
        let behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), id.clone()).await.unwrap();

        let mut actor = ActorStruct::new(id.clone(), behavior, mailbox, "test-tenant".to_string(), "test-namespace".to_string(), None);

        // Test initial state
        assert_eq!(actor.state().await, ActorState::Creating);

        // Start actor
        actor.start().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Active);

        // Send a message
        let message = Message::new(b"hello".to_vec());
        actor.send(message).await.unwrap();

        // Give time for processing
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        // Stop actor
        actor.stop().await.unwrap();
        assert_eq!(actor.state().await, ActorState::Terminated);
    }

    #[tokio::test]
    async fn test_become_unbecome_pattern() {
        let id = "test-actor".to_string();
        let initial_behavior = Box::new(MockBehavior::new());
        let mailbox = Mailbox::new(mailbox_config_default(), id.clone()).await.unwrap();

        let actor = ActorStruct::new(
            id.clone(),
            initial_behavior,
            mailbox,
            "test-tenant".to_string(),
            "test-namespace".to_string(),
            None, // node_id
        );

        // Change behavior
        let new_behavior = Box::new(MockBehavior::new());
        actor.r#become(new_behavior).await.unwrap();

        // Restore previous behavior
        actor.unbecome().await.unwrap();

        // Try to unbecome when no behavior to restore
        let result = actor.unbecome().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_genserver_behavior() {
        // Create a simple counter GenServer using new trait-based pattern
        #[derive(Default)]
        struct CounterState {
            count: i32,
        }

        struct CounterGenServer {
            state: CounterState,
        }

        impl CounterGenServer {
            fn new() -> Self {
                Self {
                    state: CounterState::default(),
                }
            }
        }

        #[async_trait::async_trait]
        impl Actor for CounterGenServer {
            async fn handle_message(
                &mut self,
                ctx: &ActorContext,
                msg: Message,
            ) -> Result<(), BehaviorError> {
                // Use GenServer's route_message (Go-style: context first)
                use crate::behavior::GenServer;
                self.route_message(ctx, msg).await
            }

            fn behavior_type(&self) -> BehaviorType {
                BehaviorType::GenServer
            }
        }

        #[async_trait::async_trait]
        impl GenServer for CounterGenServer {
            async fn handle_request(
                &mut self,
                ctx: &ActorContext,
                msg: Message,
            ) -> Result<(), BehaviorError> {
                if msg.payload() == b"get" {
                    // Send reply if sender is present
                    if let Some(sender_id) = &msg.sender {
                        let mut reply = Message::new(self.state.count.to_string().into_bytes());
                        reply.receiver = sender_id.clone();
                        reply.sender = Some(msg.receiver.clone());
                        if let Some(corr_id) = &msg.correlation_id {
                            reply.correlation_id = Some(corr_id.clone());
                        }
                        ctx.send_reply(
                            msg.correlation_id.as_deref(),
                            sender_id,
                            msg.receiver.clone(),
                            reply,
                        ).await.map_err(|e| BehaviorError::ProcessingError(e.to_string()))?;
                    }
                    Ok(())
                } else {
                    Err(BehaviorError::ProcessingError("Unknown request".to_string()))
                }
            }

            // Note: handle_event removed from GenServerBehavior (Phase 2 cleanup)
            // For events, use GenEvent instead
        }

        let behavior = CounterGenServer::new();

        // Verify initial state
        assert_eq!(behavior.state.count, 0);
    }

    #[tokio::test]
    async fn test_message_with_metadata() {
        let message = Message::new(b"test".to_vec())
            .with_correlation_id("corr-123".to_string())
            .with_priority(MessagePriority::High)
            .with_metadata("type".to_string(), "call".to_string())
            .with_sender("actor-1".to_string());

        assert_eq!(message.payload(), b"test");
        assert_eq!(message.priority(), MessagePriority::High);
        assert_eq!(message.sender_id(), Some("actor-1"));

        // Verify message type extraction
        assert_eq!(message.message_type_str(), "call");
    }

    #[tokio::test]
    async fn test_message_priority_ordering() {
        // Test that priority values are correctly ordered
        // Use helper function to compare priority values (proto-generated enums don't implement Ord)
        use plexspaces_mailbox::message_priority_value;
        assert!(message_priority_value(&MessagePriority::System) > message_priority_value(&MessagePriority::Highest));
        assert!(message_priority_value(&MessagePriority::Highest) > message_priority_value(&MessagePriority::High));
        assert!(message_priority_value(&MessagePriority::High) > message_priority_value(&MessagePriority::Normal));
        assert!(message_priority_value(&MessagePriority::Normal) > message_priority_value(&MessagePriority::Low));
    }

    #[tokio::test]
    async fn test_actor_ref_tell_ask() {
        use crate::ActorRef; // Re-exported from core

        let mailbox = Arc::new(Mailbox::new(mailbox_config_default(), "test-actor@localhost".to_string()).await.unwrap());
        let actor_ref = ActorRef::new("test-actor@localhost".to_string()).unwrap();

        // Test send pattern (fire and forget) - ActorRef is now pure data, use ActorService
        // For this test, we'll use the mailbox directly since we're testing the actor_ref creation
        let message = Message::new(b"hello".to_vec());
        mailbox.send(message).await.unwrap();

        // Verify message was enqueued
        let msg = mailbox.dequeue().await.unwrap();
        assert_eq!(msg.message_type_str(), "cast"); // message_type_str() returns string
        assert_eq!(msg.payload(), b"hello");
    }
}
