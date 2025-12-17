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

//! Comprehensive tests for GenEvent behavior pattern
//!
//! Following TDD principles - these tests define the expected behavior

#[cfg(test)]
mod tests {
    use super::super::*;
    use async_trait::async_trait;
    use plexspaces_core::{
        Actor, ActorContext, BehaviorContext, BehaviorError, BehaviorType,
    };
    use plexspaces_mailbox::Message;
    use std::sync::{Arc, Mutex};

    // Shared event log for testing
    type EventLog = Arc<Mutex<Vec<String>>>;

    // Test handler that logs events
    struct LoggingHandler {
        id: String,
        log: EventLog,
    }

    impl LoggingHandler {
        fn new(id: &str, log: EventLog) -> Self {
            Self {
                id: id.to_string(),
                log,
            }
        }
    }

    #[async_trait]
    impl EventHandler for LoggingHandler {
        async fn handle_event(
            &mut self,
            ctx: &plexspaces_core::ActorContext,
            event: Message,
        ) -> Result<(), BehaviorError> {
            let payload = String::from_utf8_lossy(event.payload());
            self.log
                .lock()
                .unwrap()
                .push(format!("{}: {}", self.id, payload));
            Ok(())
        }
    }

    // Test handler that fails
    struct FailingHandler;

    #[async_trait]
    impl EventHandler for FailingHandler {
        async fn handle_event(
            &mut self,
            _ctx: &plexspaces_core::ActorContext,
            _event: Message,
        ) -> Result<(), BehaviorError> {
            Err(BehaviorError::ProcessingError("Handler failed".to_string()))
        }
    }

    // MockReply removed - Reply trait no longer exists, use ActorRef::send_reply() instead

    // Helper to create test context and message (Go-style)
    fn create_test_context_and_message(message: Message) -> (Arc<ActorContext>, Message) {
        let ctx = Arc::new(ActorContext::minimal(
            "test-genevent".to_string(),
            "test-node".to_string(),
            "test-ns".to_string(), // namespace
        ));
        (ctx, message)
    }

    // Test 1: Basic event handling with single handler
    #[tokio::test]
    async fn test_genevent_single_handler() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Box::new(LoggingHandler::new("h1", log.clone()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(handler);

        assert_eq!(behavior.behavior_type(), BehaviorType::GenEvent);

        // Send event
        let msg = Message::new(b"test event".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Verify handler received event
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0], "h1: test event");
    }

    // Test 2: Multiple handlers receive same event
    #[tokio::test]
    async fn test_genevent_multiple_handlers() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(Box::new(LoggingHandler::new("h1", log.clone())));
        behavior.add_handler(Box::new(LoggingHandler::new("h2", log.clone())));
        behavior.add_handler(Box::new(LoggingHandler::new("h3", log.clone())));

        // Send event
        let msg = Message::new(b"broadcast".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Verify all handlers received event
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert!(events.contains(&"h1: broadcast".to_string()));
        assert!(events.contains(&"h2: broadcast".to_string()));
        assert!(events.contains(&"h3: broadcast".to_string()));
    }

    // Test 3: Empty handler list (no handlers)
    #[tokio::test]
    async fn test_genevent_no_handlers() {
        let mut behavior = GenEventBehavior::new();

        // Send event with no handlers - should succeed
        let msg = Message::new(b"ignored".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        // Should not fail even with no handlers
        let result = behavior.handle_message(&*ctx, msg_actual).await;
        assert!(result.is_ok());
    }

    // Test 4: Handler error propagation
    #[tokio::test]
    async fn test_genevent_handler_error() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(Box::new(LoggingHandler::new("h1", log.clone())));
        behavior.add_handler(Box::new(FailingHandler));
        behavior.add_handler(Box::new(LoggingHandler::new("h3", log.clone())));

        // Send event
        let msg = Message::new(b"test".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        // Should fail because one handler fails
        let result = behavior.handle_message(&*ctx, msg_actual).await;
        assert!(result.is_err());

        // First handler should have processed
        let events = log.lock().unwrap();
        assert!(events.contains(&"h1: test".to_string()));
    }

    // Test 5: Sequential event delivery
    #[tokio::test]
    async fn test_genevent_sequential_events() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Box::new(LoggingHandler::new("h1", log.clone()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(handler);

        // Send multiple events
        for i in 1..=5 {
            let msg = Message::new(format!("event{}", i).into_bytes());
            let (ctx, msg_actual) = create_test_context_and_message(msg);
            behavior.handle_message(&*ctx, msg_actual).await.unwrap();
        }

        // Verify all events received in order
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 5);
        for i in 1..=5 {
            assert_eq!(events[i - 1], format!("h1: event{}", i));
        }
    }

    // Test 6: Handler removal (clear all handlers)
    #[tokio::test]
    async fn test_genevent_remove_handlers() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(Box::new(LoggingHandler::new("h1", log.clone())));
        behavior.add_handler(Box::new(LoggingHandler::new("h2", log.clone())));

        // Send event - should reach handlers
        let msg1 = Message::new(b"before clear".to_vec());
        let (ctx1, msg1_actual) = create_test_context_and_message(msg1);
        behavior.handle_message(&*ctx1, msg1_actual).await.unwrap();

        assert_eq!(log.lock().unwrap().len(), 2);

        // Remove all handlers
        behavior.clear_handlers();

        // Send event - should not reach any handlers
        let msg2 = Message::new(b"after clear".to_vec());
        let (ctx2, msg2_actual) = create_test_context_and_message(msg2);
        behavior.handle_message(&*ctx2, msg2_actual).await.unwrap();

        // Still only 2 events (before clear)
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    // Test 7: Handler state mutation across events
    #[tokio::test]
    async fn test_genevent_handler_state() {
        struct CountingHandler {
            count: usize,
            log: EventLog,
        }

        #[async_trait]
        impl EventHandler for CountingHandler {
            async fn handle_event(
                &mut self,
                _ctx: &plexspaces_core::ActorContext,
                event: Message,
            ) -> Result<(), BehaviorError> {
                self.count += 1;
                let payload = String::from_utf8_lossy(event.payload());
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("count={}: {}", self.count, payload));
                Ok(())
            }
        }

        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Box::new(CountingHandler {
            count: 0,
            log: log.clone(),
        });

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(handler);

        // Send multiple events
        for i in 1..=3 {
            let msg = Message::new(format!("evt{}", i).into_bytes());
            let (ctx, msg_actual) = create_test_context_and_message(msg);
            behavior.handle_message(&*ctx, msg_actual).await.unwrap();
        }

        // Verify handler state persisted across events
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], "count=1: evt1");
        assert_eq!(events[1], "count=2: evt2");
        assert_eq!(events[2], "count=3: evt3");
    }

    // Test 8: Large event payload
    #[tokio::test]
    async fn test_genevent_large_payload() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let handler = Box::new(LoggingHandler::new("h1", log.clone()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(handler);

        // Send large event (1MB)
        let large_payload = vec![b'x'; 1024 * 1024];
        let msg = Message::new(large_payload.clone());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Verify handler received large event
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert!(events[0].starts_with("h1: "));
        assert_eq!(events[0].len() - 4, large_payload.len()); // "h1: " prefix
    }

    // Test 9: Concurrent-safe event handling
    #[tokio::test]
    async fn test_genevent_concurrent_handlers() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let mut behavior = GenEventBehavior::new();
        // Add 10 handlers
        for i in 1..=10 {
            behavior.add_handler(Box::new(LoggingHandler::new(
                &format!("h{}", i),
                log.clone(),
            )));
        }

        // Send event
        let msg = Message::new(b"concurrent".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Verify all 10 handlers received event
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 10);
        for i in 1..=10 {
            assert!(events.contains(&format!("h{}: concurrent", i)));
        }
    }

    // Test 10: Handler ordering preservation
    #[tokio::test]
    async fn test_genevent_handler_order() {
        let log = Arc::new(Mutex::new(Vec::new()));

        let mut behavior = GenEventBehavior::new();
        behavior.add_handler(Box::new(LoggingHandler::new("first", log.clone())));
        behavior.add_handler(Box::new(LoggingHandler::new("second", log.clone())));
        behavior.add_handler(Box::new(LoggingHandler::new("third", log.clone())));

        // Send event
        let msg = Message::new(b"order".to_vec());
        let (ctx, msg_actual) = create_test_context_and_message(msg);

        behavior.handle_message(&*ctx, msg_actual).await.unwrap();

        // Verify handlers executed in order
        let events = log.lock().unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0], "first: order");
        assert_eq!(events[1], "second: order");
        assert_eq!(events[2], "third: order");
    }
}
