// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Comprehensive integration tests for all WASM host functions
//! Validates that all core abstractions are available via WASM

#[cfg(feature = "component-model")]
mod tests {
    use plexspaces_core::ActorId;
    use plexspaces_wasm_runtime::component_host::plexspaces::actor::{
        durability::Host as DurabilityHost,
        logging::Host as LoggingHost,
        messaging::Host as MessagingHost,
        tuplespace::Host as TuplespaceHost,
        types::{Context, SpawnOptions},
    };
    use plexspaces_wasm_runtime::component_host::{
        ChannelsImpl, DurabilityImpl, LoggingImpl, MessagingImpl, TuplespaceImpl,
    };
    use plexspaces_wasm_runtime::HostFunctions;
    use std::sync::Arc;

    // Helper to create context for tests
    fn test_context(tenant_id: &str, namespace: &str) -> Context {
        Context {
            tenant_id: tenant_id.to_string(),
            namespace: namespace.to_string(),
            headers: vec![],
        }
    }

    fn create_test_host_functions() -> Arc<HostFunctions> {
        Arc::new(HostFunctions::new())
    }

    fn create_test_host_functions_with_channel_service() -> Arc<HostFunctions> {
        use async_trait::async_trait;
        use futures::stream;
        use plexspaces_core::ChannelService;
        use plexspaces_proto::common::v1::Message;
        use std::collections::HashMap;

        struct MockChannelService {
            queues: Arc<tokio::sync::RwLock<HashMap<String, Vec<Message>>>>,
            topics: Arc<tokio::sync::RwLock<HashMap<String, Vec<Message>>>>,
        }

        impl MockChannelService {
            fn new() -> Self {
                Self {
                    queues: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                    topics: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
                }
            }
        }

        #[async_trait]
        impl ChannelService for MockChannelService {
            async fn send_to_queue(
                &self,
                queue_name: &str,
                message: Message,
            ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
                let mut queues = self.queues.write().await;
                queues
                    .entry(queue_name.to_string())
                    .or_insert_with(Vec::new)
                    .push(message);
                Ok("msg-001".to_string())
            }

            async fn publish_to_topic(
                &self,
                topic_name: &str,
                message: Message,
            ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
                let mut topics = self.topics.write().await;
                topics
                    .entry(topic_name.to_string())
                    .or_insert_with(Vec::new)
                    .push(message);
                Ok("msg-001".to_string())
            }

            async fn subscribe_to_topic(
                &self,
                _topic_name: &str,
            ) -> Result<
                futures::stream::BoxStream<'static, Message>,
                Box<dyn std::error::Error + Send + Sync>,
            > {
                Ok(Box::pin(stream::empty()))
            }

            async fn receive_from_queue(
                &self,
                queue_name: &str,
                _timeout: Option<std::time::Duration>,
            ) -> Result<Option<Message>, Box<dyn std::error::Error + Send + Sync>> {
                let mut queues = self.queues.write().await;
                if let Some(queue) = queues.get_mut(queue_name) {
                    if !queue.is_empty() {
                        return Ok(Some(queue.remove(0)));
                    }
                }
                Ok(None)
            }
        }

        let channel_service = Arc::new(MockChannelService::new());
        Arc::new(HostFunctions::with_channel_service(channel_service))
    }

    /// Test that all messaging functions are accessible
    #[tokio::test]
    async fn test_messaging_functions_accessible() {
        use MessagingHost;
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // Test all messaging functions
        let self_id = messaging.self_id().await;
        assert_eq!(self_id, actor_id.to_string());

        let now = messaging.now().await;
        assert!(now > 0);

        let parent_id: Option<String> = messaging.parent_id().await;
        assert!(parent_id.is_none()); // No parent in test

        // Test tell (should work even without message sender)
        let _ = messaging
            .tell("target".to_string(), "msg".to_string(), vec![])
            .await;

        // Test ask (should return error without message sender, but function exists)
        use plexspaces_wasm_runtime::component_host::plexspaces::actor::types::ActorError;
        let ask_result: Result<Vec<u8>, ActorError> = messaging
            .ask("target".to_string(), "msg".to_string(), vec![], 1000)
            .await;
        assert!(ask_result.is_err()); // Expected without message sender

        // Test reply (will fail without pending ask, which is expected behavior)
        let reply_result: Result<(), ActorError> =
            messaging.reply("corr-123".to_string(), vec![]).await;
        assert!(reply_result.is_err()); // Reply fails without pending ask (expected behavior)

        // Test spawn (should return error without message sender)
        let spawn_options = SpawnOptions {
            actor_id: None,
            labels: vec![],
            mailbox_size: None,
            durable: false,
            supervisor: None,
        };
        let spawn_result: Result<String, ActorError> = messaging
            .spawn("module@1.0.0".to_string(), vec![], spawn_options)
            .await;
        assert!(spawn_result.is_err()); // Expected without message sender

        // Test stop (should return error without message sender)
        let stop_result = messaging.stop("target".to_string(), 5000).await;
        assert!(stop_result.is_err()); // Expected without message sender

        // Test link/unlink/monitor/demonitor (all return error without message sender)
        assert!(messaging.link("target".to_string()).await.is_err()); // Requires message sender
        assert!(messaging.unlink("target".to_string()).await.is_err()); // Requires message sender
                                                                        // monitor returns error without message sender
        assert!(messaging.monitor("target".to_string()).await.is_err());
        // demonitor returns error (not implemented)
        assert!(messaging.demonitor(0).await.is_err());

        // Test send_after/cancel_timer
        let timer_id = messaging
            .send_after(100, "msg".to_string(), vec![])
            .await
            .unwrap();
        assert!(messaging.cancel_timer(timer_id).await.is_ok());

        // Test sleep
        let start = std::time::Instant::now();
        messaging.sleep(50).await;
        let duration = start.elapsed();
        assert!(duration.as_millis() >= 45);
    }

    /// Test that all channel functions are accessible
    #[tokio::test]
    async fn test_channel_functions_accessible() {
        let host_functions = create_test_host_functions_with_channel_service();

        // Test send_to_queue via HostFunctions
        let _ = host_functions
            .send_to_queue("queue", "msg", vec![])
            .await;

        // Test receive_from_queue via HostFunctions
        let _ = host_functions
            .receive_from_queue("queue", 0)
            .await;

        // Test publish_to_topic via HostFunctions
        let _ = host_functions
            .publish_to_topic("topic", "msg", vec![])
            .await;
    }

    /// Test that all tuplespace functions are accessible
    #[tokio::test]
    async fn test_tuplespace_functions_accessible() {
        use TuplespaceHost;
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace = TuplespaceImpl::new(None, actor_id);

        // All tuplespace functions should be accessible
        // They may return errors if TupleSpaceProvider is not configured, but functions exist
        let _ = tuplespace.write(test_context("", ""), vec![]).await;
        let _ = tuplespace.read(test_context("", ""), vec![]).await;
        let _ = tuplespace.take(test_context("", ""), vec![]).await;
        let _ = tuplespace.count(test_context("", ""), vec![]).await;
        let _ = tuplespace.read_all(test_context("", ""), vec![], 10).await;
        let _ = tuplespace
            .write_with_ttl(test_context("", ""), vec![], 1000)
            .await;
        let _ = tuplespace
            .read_blocking(test_context("", ""), vec![], 100)
            .await;
        let _ = tuplespace
            .take_blocking(test_context("", ""), vec![], 100)
            .await;
    }

    /// Test that all durability functions are accessible
    #[tokio::test]
    async fn test_durability_functions_accessible() {
        use DurabilityHost;
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut durability = DurabilityImpl::new(actor_id, host_functions);

        // All durability functions should be accessible
        let _ = durability
            .persist(test_context("", ""), "event".to_string(), vec![])
            .await;
        let _ = durability.persist_batch(test_context("", ""), vec![]).await;
        let _ = durability.checkpoint(test_context("", "")).await;
        let _ = durability.get_sequence(test_context("", "")).await;
        let _ = durability
            .get_checkpoint_sequence(test_context("", ""))
            .await;
        let is_replaying = durability.is_replaying(test_context("", "")).await.unwrap();
        assert_eq!(is_replaying, false); // Not replaying in test
        let _ = durability
            .cache_side_effect(test_context("", ""), "key".to_string(), vec![])
            .await;
        let _ = durability
            .read_journal(test_context("", ""), 0, 0, 100)
            .await;
        let _ = durability.compact(test_context("", ""), 0).await;
    }

    /// Test that all logging functions are accessible
    #[tokio::test]
    async fn test_logging_functions_accessible() {
        use LoggingHost;
        let actor_id = ActorId::from("test-actor".to_string());
        let mut logging = LoggingImpl { actor_id };

        // All logging functions should be accessible
        logging.trace("trace".to_string()).await;
        logging.debug("debug".to_string()).await;
        logging.info("info".to_string()).await;
        logging.warn("warn".to_string()).await;
        logging.error("error".to_string()).await;

        let span_id = logging.start_span("span".to_string()).await;
        logging.end_span(span_id).await;
        logging.add_span_event("event".to_string(), vec![]).await;
    }
}
