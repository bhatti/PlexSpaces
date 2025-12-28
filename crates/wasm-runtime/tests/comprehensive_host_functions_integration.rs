// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Comprehensive integration tests for all WASM host functions
//! Validates that all core abstractions are available via WASM

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

    /// Test that all messaging functions are accessible
    #[tokio::test]
    async fn test_messaging_functions_accessible() {
        let actor_id = ActorId::from("test-actor".to_string());
        let host_functions = create_test_host_functions();
        let mut messaging = MessagingImpl::new(actor_id.clone(), host_functions.clone());

        // Test all messaging functions
        let self_id = messaging.self_id().await;
        assert_eq!(self_id, actor_id.to_string());

        let now = messaging.now().await;
        assert!(now > 0);

        let parent_id = messaging.parent_id().await;
        assert!(parent_id.is_none()); // No parent in test

        // Test tell (should work even without message sender)
        let _ = messaging.tell("target".to_string(), "msg".to_string(), vec![]).await;

        // Test ask (should return error without message sender, but function exists)
        let ask_result = messaging.ask("target".to_string(), "msg".to_string(), vec![], 1000).await;
        assert!(ask_result.is_err()); // Expected without message sender

        // Test reply
        let reply_result = messaging.reply("corr-123".to_string(), vec![]).await;
        assert!(reply_result.is_ok()); // Reply always succeeds (routing handled by system)

        // Test spawn (should return error without message sender)
        let spawn_options = SpawnOptions {
            actor_id: None,
            labels: vec![],
            mailbox_size: None,
            durable: false,
            supervisor: None,
        };
        let spawn_result = messaging.spawn("module@1.0.0".to_string(), vec![], spawn_options).await;
        assert!(spawn_result.is_err()); // Expected without message sender

        // Test stop (should return error without message sender)
        let stop_result = messaging.stop("target".to_string(), 5000).await;
        assert!(stop_result.is_err()); // Expected without message sender

        // Test link/unlink/monitor/demonitor (all should succeed as placeholders)
        assert!(messaging.link("target".to_string()).await.is_ok());
        assert!(messaging.unlink("target".to_string()).await.is_ok());
        let monitor_ref = messaging.monitor("target".to_string()).await.unwrap();
        assert!(messaging.demonitor(monitor_ref).await.is_ok());

        // Test send_after/cancel_timer
        let timer_id = messaging.send_after(100, "msg".to_string(), vec![]).await.unwrap();
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
        let host_functions = create_test_host_functions();
        let mut channels = ChannelsImpl {
            host_functions: host_functions.clone(),
        };

        // Test send_to_queue
        let _ = channels.send_to_queue(test_context("", ""), "queue".to_string(), "msg".to_string(), vec![]).await;

        // Test receive_from_queue
        let _ = channels.receive_from_queue(test_context("", ""), "queue".to_string(), 0).await;

        // Test publish_to_topic
        let _ = channels.publish_to_topic(test_context("", ""), "topic".to_string(), "msg".to_string(), vec![]).await;

        // Test ack/nack
        assert!(channels.ack(test_context("", ""), "queue".to_string(), "msg-123".to_string()).await.is_ok());
        assert!(channels.nack(test_context("", ""), "queue".to_string(), "msg-123".to_string(), true).await.is_ok());

        // Test subscribe/unsubscribe
        let sub_id = channels.subscribe_to_topic(test_context("", ""), "topic".to_string(), None).await.unwrap();
        assert!(channels.unsubscribe_from_topic(sub_id).await.is_ok());
    }

    /// Test that all tuplespace functions are accessible
    #[tokio::test]
    async fn test_tuplespace_functions_accessible() {
        let actor_id = ActorId::from("test-actor".to_string());
        let mut tuplespace = TuplespaceImpl::new(None, actor_id);

        // All tuplespace functions should be accessible
        // They may return errors if TupleSpaceProvider is not configured, but functions exist
        let _ = tuplespace.write(test_context("", ""), vec![]).await;
        let _ = tuplespace.read(test_context("", ""), vec![]).await;
        let _ = tuplespace.take(test_context("", ""), vec![]).await;
        let _ = tuplespace.count(test_context("", ""), vec![]).await;
        let _ = tuplespace.read_all(test_context("", ""), vec![], 10).await;
        let _ = tuplespace.write_with_ttl(test_context("", ""), vec![], 1000).await;
        let _ = tuplespace.read_blocking(test_context("", ""), vec![], 100).await;
        let _ = tuplespace.take_blocking(test_context("", ""), vec![], 100).await;
    }

    /// Test that all durability functions are accessible
    #[tokio::test]
    async fn test_durability_functions_accessible() {
        let mut durability = DurabilityImpl;

        // All durability functions should be accessible
        let _ = durability.persist(test_context("", ""), "event".to_string(), vec![]).await;
        let _ = durability.persist_batch(test_context("", ""), vec![]).await;
        let _ = durability.checkpoint(test_context("", "")).await;
        let _ = durability.get_sequence(test_context("", "")).await;
        let _ = durability.get_checkpoint_sequence(test_context("", "")).await;
        let is_replaying = durability.is_replaying(test_context("", "")).await.unwrap();
        assert_eq!(is_replaying, false); // Not replaying in test
        let _ = durability.cache_side_effect(test_context("", ""), "key".to_string(), vec![]).await;
        let _ = durability.read_journal(test_context("", ""), 0, 0, 100).await;
        let _ = durability.compact(test_context("", ""), 0).await;
    }

    /// Test that all logging functions are accessible
    #[tokio::test]
    async fn test_logging_functions_accessible() {
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





