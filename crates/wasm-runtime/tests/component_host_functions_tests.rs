// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Unit tests for component host function implementations
//! Tests the host function bindings for WASM components

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_wasm_runtime::component_host::{
        LoggingImpl, MessagingImpl, TuplespaceImpl, ChannelsImpl, DurabilityImpl,
    };
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::types::{Context, SpawnOptions};
    use plexspaces_core::ActorId;
    use std::sync::Arc;
    use plexspaces_wasm_runtime::HostFunctions;

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
        }
    }

    fn create_test_host_functions() -> Arc<HostFunctions> {
        Arc::new(HostFunctions::new())
    }

    #[tokio::test]
    async fn test_logging_impl() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let mut logging = LoggingImpl { actor_id: actor_id.clone() };

        // ACT & ASSERT: All logging functions should not panic
        logging.trace("test trace message".to_string()).await;
        logging.debug("test debug message".to_string()).await;
        logging.info("test info message".to_string()).await;
        logging.warn("test warn message".to_string()).await;
        logging.error("test error message".to_string()).await;

        // Note: LogLevel and other generated types are not easily accessible in tests
        // These tests verify the implementations don't panic

        // Test span functions
        let span_id = logging.start_span("test-span".to_string()).await;
        logging.end_span(span_id).await;
        logging.add_span_event("test-event".to_string(), vec![]).await;
    }

    #[tokio::test]
    async fn test_messaging_impl_tell() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // ACT: Test tell (should work even without message sender configured)
        let result = messaging.tell(
            "target-actor".to_string(),
            "test-msg".to_string(),
            vec![1, 2, 3],
        ).await;

        // ASSERT: Should succeed (even if message sender not configured, it logs and returns success)
        assert!(result.is_ok(), "tell should succeed");
        let message_id = result.unwrap();
        assert!(!message_id.is_empty(), "message_id should not be empty");
    }

    #[tokio::test]
    async fn test_messaging_impl_self_id() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions);

        // ACT
        let self_id = messaging.self_id().await;

        // ASSERT
        assert_eq!(self_id, actor_id.to_string());
    }

    #[tokio::test]
    async fn test_messaging_impl_now() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT
        let timestamp1 = messaging.now().await;
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        let timestamp2 = messaging.now().await;

        // ASSERT
        assert!(timestamp2 > timestamp1, "timestamp should increase");
        assert!(timestamp1 > 0, "timestamp should be positive");
    }

    #[tokio::test]
    async fn test_messaging_impl_sleep() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT: Sleep for 50ms
        let start = std::time::Instant::now();
        messaging.sleep(50).await;
        let duration = start.elapsed();

        // ASSERT: Should sleep approximately 50ms (allow some tolerance)
        assert!(duration.as_millis() >= 45, "should sleep at least 45ms");
        assert!(duration.as_millis() <= 100, "should not sleep too long");
    }

    #[tokio::test]
    async fn test_messaging_impl_ask_without_sender() {
        // ARRANGE: Test ask without message sender configured
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT & ASSERT: ask should return error when message sender not configured
        let ask_result = messaging.ask(
            "target".to_string(),
            "msg".to_string(),
            vec![],
            1000,
        ).await;
        assert!(ask_result.is_err(), "ask should fail when message sender not configured");
        if let Err(e) = ask_result {
            assert!(!e.message.is_empty(), "error message should not be empty");
        }
    }

    #[tokio::test]
    async fn test_messaging_impl_ask_with_mock_sender() {
        // ARRANGE: Create mock message sender that handles ask/reply
        use plexspaces_wasm_runtime::MessageSender;
        use std::sync::Arc;
        use tokio::sync::RwLock;
        use std::collections::HashMap;

        struct MockMessageSender {
            replies: Arc<RwLock<HashMap<String, Vec<u8>>>>,
        }

        #[async_trait::async_trait]
        impl MessageSender for MockMessageSender {
            async fn send_message(&self, _from: &str, _to: &str, _message: &str) -> Result<(), String> {
                Ok(())
            }

            async fn ask(
                &self,
                _from: &str,
                to: &str,
                message_type: &str,
                payload: Vec<u8>,
                timeout_ms: u64,
            ) -> Result<Vec<u8>, String> {
                // Simulate reply: echo back payload with prefix
                if timeout_ms == 0 {
                    return Err("Timeout".to_string());
                }
                let reply_key = format!("{}:{}", to, message_type);
                let mut replies = self.replies.write().await;
                if let Some(reply) = replies.remove(&reply_key) {
                    Ok(reply)
                } else {
                    // Default reply: echo payload
                    Ok(format!("reply:{}", String::from_utf8_lossy(&payload)).into_bytes())
                }
            }
        }

        let mock_sender = MockMessageSender {
            replies: Arc::new(RwLock::new(HashMap::new())),
        };
        let host_functions = Arc::new(plexspaces_wasm_runtime::HostFunctions::with_message_sender(
            Box::new(mock_sender),
        ));
        let actor_id = ActorId::from("test-actor".to_string());
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT: Test ask with mock sender
        let ask_result = messaging.ask(
            "target-actor".to_string(),
            "get".to_string(),
            b"request-data".to_vec(),
            5000,
        ).await;

        // ASSERT: Should succeed and return reply
        assert!(ask_result.is_ok(), "ask should succeed with mock sender");
        let reply = ask_result.unwrap();
        let reply_str = String::from_utf8_lossy(&reply);
        assert!(reply_str.contains("reply:"), "reply should contain prefix");
        assert!(reply_str.contains("request-data"), "reply should echo request");
    }

    #[tokio::test]
    async fn test_messaging_impl_ask_timeout() {
        // ARRANGE: Create mock sender that simulates timeout
        use plexspaces_wasm_runtime::MessageSender;

        struct TimeoutMessageSender;

        #[async_trait::async_trait]
        impl MessageSender for TimeoutMessageSender {
            async fn send_message(&self, _from: &str, _to: &str, _message: &str) -> Result<(), String> {
                Ok(())
            }

            async fn ask(
                &self,
                _from: &str,
                _to: &str,
                _message_type: &str,
                _payload: Vec<u8>,
                timeout_ms: u64,
            ) -> Result<Vec<u8>, String> {
                if timeout_ms < 100 {
                    tokio::time::sleep(tokio::time::Duration::from_millis(timeout_ms + 10)).await;
                    Err("Timeout".to_string())
                } else {
                    Ok(b"reply".to_vec())
                }
            }
        }

        let host_functions = Arc::new(plexspaces_wasm_runtime::HostFunctions::with_message_sender(
            Box::new(TimeoutMessageSender),
        ));
        let actor_id = ActorId::from("test-actor".to_string());
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT: Test ask with short timeout
        let ask_result = messaging.ask(
            "target".to_string(),
            "msg".to_string(),
            vec![],
            50, // Short timeout
        ).await;

        // ASSERT: Should timeout
        assert!(ask_result.is_err(), "ask should timeout");
    }

    #[tokio::test]
    async fn test_tuplespace_impl_placeholders() {
        // ARRANGE
        let mut tuplespace = TuplespaceImpl;

        // ACT & ASSERT: Placeholder implementations should not panic
        let write_result = tuplespace.write(test_context("", ""), vec![]).await;
        assert!(write_result.is_ok(), "write placeholder should succeed");

        let read_result = tuplespace.read(test_context("", ""), vec![]).await;
        assert!(read_result.is_ok(), "read placeholder should succeed");
        assert!(read_result.unwrap().is_none(), "read should return None (placeholder)");

        let count_result = tuplespace.count(test_context("", ""), vec![]).await;
        assert!(count_result.is_ok(), "count placeholder should succeed");
        assert_eq!(count_result.unwrap(), 0, "count should return 0 (placeholder)");
    }

    #[tokio::test]
    async fn test_channels_impl_send_to_queue() {
        // ARRANGE
        let host_functions = create_test_host_functions();
        let mut channels = ChannelsImpl {
            host_functions: host_functions.clone(),
        };

        // ACT: Test send_to_queue
        let result = channels.send_to_queue(
            test_context("", ""),
            "test-queue".to_string(),
            "test-msg".to_string(),
            vec![1, 2, 3],
        ).await;

        // ASSERT: Should succeed (even if channel service not configured)
        assert!(result.is_ok(), "send_to_queue should succeed");
    }

    #[tokio::test]
    async fn test_channels_impl_receive_from_queue() {
        // ARRANGE
        let host_functions = create_test_host_functions();
        let mut channels = ChannelsImpl {
            host_functions: host_functions.clone(),
        };

        // ACT: Test receive_from_queue (with timeout 0 = poll immediately)
        let result = channels.receive_from_queue(test_context("", ""), "test-queue".to_string(), 0).await;

        // ASSERT: Should return None if no message available
        assert!(result.is_ok(), "receive_from_queue should succeed");
        // Result may be Some or None depending on implementation
    }

    #[tokio::test]
    async fn test_channels_impl_publish_to_topic() {
        // ARRANGE
        let host_functions = create_test_host_functions();
        let mut channels = ChannelsImpl {
            host_functions,
        };

        // ACT: Test publish_to_topic
        let result = channels.publish_to_topic(
            test_context("", ""),
            "test-topic".to_string(),
            "test-msg".to_string(),
            vec![1, 2, 3],
        ).await;

        // ASSERT: Should succeed
        assert!(result.is_ok(), "publish_to_topic should succeed");
    }

    #[tokio::test]
    async fn test_durability_impl_placeholders() {
        // ARRANGE
        let mut durability = DurabilityImpl;

        // ACT & ASSERT: Placeholder implementations should not panic
        let persist_result = durability.persist(test_context("", ""), "test-event".to_string(), vec![]).await;
        assert!(persist_result.is_ok(), "persist placeholder should succeed");
        assert_eq!(persist_result.unwrap(), 0, "persist should return 0 (placeholder)");

        let checkpoint_result = durability.checkpoint(test_context("", "")).await;
        assert!(checkpoint_result.is_ok(), "checkpoint placeholder should succeed");

        let is_replaying_result = durability.is_replaying(test_context("", "")).await;
        assert!(is_replaying_result.is_ok(), "is_replaying should succeed");
        assert_eq!(is_replaying_result.unwrap(), false, "is_replaying should return false");
    }

    #[tokio::test]
    async fn test_error_handling() {
        // ARRANGE
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT & ASSERT: Test that errors are properly formatted
        let ask_result = messaging.ask(
            "target".to_string(),
            "msg".to_string(),
            vec![],
            1000,
        ).await;

        assert!(ask_result.is_err());
        if let Err(e) = ask_result {
            assert!(!e.message.is_empty(), "error message should not be empty");
            // Error code should be NotImplemented
            // (We can't easily check the enum value without importing generated types)
        }
    }

    #[tokio::test]
    async fn test_messaging_impl_spawn_without_sender() {
        // ARRANGE: Test spawn without message sender configured
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        let spawn_options = SpawnOptions {
            actor_id: None,
            labels: vec![],
            mailbox_size: None,
            durable: false,
            supervisor: None,
        };

        // ACT & ASSERT: spawn should return error when message sender not configured
        let spawn_result = messaging.spawn(
            "worker@1.0.0".to_string(),
            vec![],
            spawn_options,
        ).await;
        assert!(spawn_result.is_err(), "spawn should fail when message sender not configured");
    }

    #[tokio::test]
    async fn test_messaging_impl_spawn_with_mock_sender() {
        // ARRANGE: Create mock message sender that handles spawn
        use plexspaces_wasm_runtime::MessageSender;

        struct MockSpawnSender;

        #[async_trait::async_trait]
        impl MessageSender for MockSpawnSender {
            async fn send_message(&self, _from: &str, _to: &str, _message: &str) -> Result<(), String> {
                Ok(())
            }

            async fn ask(
                &self,
                _from: &str,
                _to: &str,
                _message_type: &str,
                _payload: Vec<u8>,
                _timeout_ms: u64,
            ) -> Result<Vec<u8>, String> {
                Err("Not implemented".to_string())
            }

            async fn spawn_actor(
                &self,
                _from: &str,
                module_ref: &str,
                _initial_state: Vec<u8>,
                actor_id: Option<String>,
                _labels: Vec<(String, String)>,
                _durable: bool,
            ) -> Result<String, String> {
                // Generate actor ID if not provided
                let spawned_id = actor_id.unwrap_or_else(|| format!("spawned-{}", ulid::Ulid::new()));
                Ok(spawned_id)
            }

            async fn stop_actor(
                &self,
                _from: &str,
                actor_id: &str,
                _timeout_ms: u64,
            ) -> Result<(), String> {
                if actor_id.is_empty() {
                    Err("Actor ID cannot be empty".to_string())
                } else {
                    Ok(())
                }
            }
        }

        let host_functions = Arc::new(plexspaces_wasm_runtime::HostFunctions::with_message_sender(
            Box::new(MockSpawnSender),
        ));
        let actor_id = ActorId::from("test-actor".to_string());
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT: Test spawn with mock sender
        let spawn_options = SpawnOptions {
            actor_id: None,
            labels: vec![],
            mailbox_size: None,
            durable: false,
            supervisor: None,
        };
        let spawn_result = messaging.spawn(
            "worker@1.0.0".to_string(),
            vec![],
            spawn_options,
        ).await;

        // ASSERT: Should succeed and return actor ID
        assert!(spawn_result.is_ok(), "spawn should succeed with mock sender");
        let spawned_id = spawn_result.unwrap();
        assert!(!spawned_id.is_empty(), "spawned actor ID should not be empty");
        assert!(spawned_id.starts_with("spawned-"), "spawned ID should have prefix");
    }

    #[tokio::test]
    async fn test_messaging_impl_stop_without_sender() {
        // ARRANGE: Test stop without message sender configured
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT & ASSERT: stop should return error when message sender not configured
        let stop_result = messaging.stop("target-actor".to_string(), 5000).await;
        assert!(stop_result.is_err(), "stop should fail when message sender not configured");
    }

    #[tokio::test]
    async fn test_messaging_impl_stop_with_mock_sender() {
        // ARRANGE: Create mock message sender that handles stop
        use plexspaces_wasm_runtime::MessageSender;

        struct MockStopSender;

        #[async_trait::async_trait]
        impl MessageSender for MockStopSender {
            async fn send_message(&self, _from: &str, _to: &str, _message: &str) -> Result<(), String> {
                Ok(())
            }

            async fn ask(
                &self,
                _from: &str,
                _to: &str,
                _message_type: &str,
                _payload: Vec<u8>,
                _timeout_ms: u64,
            ) -> Result<Vec<u8>, String> {
                Err("Not implemented".to_string())
            }

            async fn spawn_actor(
                &self,
                _from: &str,
                _module_ref: &str,
                _initial_state: Vec<u8>,
                _actor_id: Option<String>,
                _labels: Vec<(String, String)>,
                _durable: bool,
            ) -> Result<String, String> {
                Err("Not implemented".to_string())
            }

            async fn stop_actor(
                &self,
                _from: &str,
                actor_id: &str,
                timeout_ms: u64,
            ) -> Result<(), String> {
                if actor_id.is_empty() {
                    return Err("Actor ID cannot be empty".to_string());
                }
                if timeout_ms == 0 {
                    return Err("Timeout cannot be 0".to_string());
                }
                Ok(())
            }
        }

        let host_functions = Arc::new(plexspaces_wasm_runtime::HostFunctions::with_message_sender(
            Box::new(MockStopSender),
        ));
        let actor_id = ActorId::from("test-actor".to_string());
        let mut messaging = MessagingImpl::new(actor_id, host_functions);

        // ACT: Test stop with mock sender
        let stop_result = messaging.stop("target-actor".to_string(), 5000).await;

        // ASSERT: Should succeed
        assert!(stop_result.is_ok(), "stop should succeed with mock sender");
    }
}

