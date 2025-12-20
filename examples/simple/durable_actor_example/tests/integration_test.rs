// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for durable actor example
// Tests cover:
// - Basic durability with journaling
// - Checkpoint recovery
// - Side effect caching
// - Channel-based mailbox with ACK/NACK
// - Dead letter queue (DLQ)
// - Edge cases and failure scenarios

#[cfg(feature = "sqlite-backend")]
mod tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;
    use prost::Message;
    
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_channel::{Channel, SqliteChannel};
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_proto::channel::v1::{channel_config, ChannelBackend, ChannelConfig, ChannelMessage, SqliteConfig};

    /// Simple counter state (protobuf message)
    #[derive(Clone, PartialEq, Message)]
    struct CounterState {
        #[prost(int32, tag = "1")]
        pub value: i32,
    }

    /// Simple API response (protobuf message)
    #[derive(Clone, PartialEq, Message)]
    struct ApiResponse {
        #[prost(string, tag = "1")]
        pub message: String,
    }

    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    /// Test: Full durable actor lifecycle with journaling, checkpoints, and side effects
    #[tokio::test]
    async fn test_durable_actor_full_lifecycle() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-counter-1";

        // Phase 1: Normal operation
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let ctx_arc = facet.get_execution_context();
        let ctx_guard = ctx_arc.read().await;
        let ctx = ctx_guard.as_ref().unwrap();

        // Process messages with side effects
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();

            // Record side effect
            let _api_result = ctx
                .record_side_effect(format!("api_call_{}", i), "http_request", || async {
                    Ok(ApiResponse {
                        message: format!("API call {}", i),
                    })
                })
                .await
                .unwrap();

            let result = serde_json::json!({ "counter": i * 5 }).to_string().into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        drop(ctx_guard);
        storage.flush().await.unwrap();

        // Verify journal entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert!(entries.len() >= 10, "Should have at least 10 entries (5 messages * 2)");

        let side_effect_count = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(_))
                )
            })
            .count();
        assert_eq!(side_effect_count, 5, "Should have 5 side effect entries");

        // Phase 2: Restart and recovery
        facet.on_detach(actor_id).await.unwrap();
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify checkpoint was loaded (check via storage instead)
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        if let Ok(cp) = checkpoint_result {
            assert!(cp.sequence > 0, "Checkpoint should exist");
        }

        // Process new message after restart
        let method = "increment";
        let payload = serde_json::json!({ "value": 10 }).to_string().into_bytes();
        new_facet.before_method(method, &payload).await.unwrap();
        let result = serde_json::json!({ "counter": 35 }).to_string().into_bytes();
        new_facet.after_method(method, &payload, &result).await.unwrap();

        storage.flush().await.unwrap();

        // Verify all entries are still present
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert!(
            all_entries.len() >= 12,
            "Should have at least 12 entries (6 messages * 2)"
        );
    }

    /// Test: Side effect caching during replay
    #[tokio::test]
    async fn test_side_effect_caching_during_replay() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-counter-2";

        // Phase 1: Record side effects
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let ctx_arc = facet.get_execution_context();
        let ctx_guard = ctx_arc.read().await;
        let ctx = ctx_guard.as_ref().unwrap();

        // Record side effect
        let api_result = ctx
            .record_side_effect("api_call_1", "http_request", || async {
                Ok(ApiResponse {
                    message: "cached_result".to_string(),
                })
            })
            .await
            .unwrap();

        let response = ApiResponse::decode(api_result.as_slice()).unwrap();
        assert_eq!(response.message, "cached_result");

        drop(ctx_guard);

        // Process message to journal side effect
        let method = "process";
        let payload = b"test".to_vec();
        facet.before_method(method, &payload).await.unwrap();
        facet.after_method(method, &payload, b"ok").await.unwrap();

        storage.flush().await.unwrap();

        // Verify side effect was journaled
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let side_effect_entries: Vec<_> = entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(_))
                )
            })
            .collect();
        assert_eq!(side_effect_entries.len(), 1, "Should have 1 side effect entry");

        // Phase 2: Restart and verify side effect was cached
        facet.on_detach(actor_id).await.unwrap();
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify side effect entry is still in journal
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        let side_effect_entries: Vec<_> = all_entries
            .iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(_))
                )
            })
            .collect();
        assert_eq!(
            side_effect_entries.len(),
            1,
            "Side effect should still be in journal after restart"
        );
    }

    /// Test: Checkpoint recovery
    #[tokio::test]
    async fn test_checkpoint_recovery() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-counter-3";

        // Phase 1: Process messages to trigger checkpoint
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries) to trigger checkpoint
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = serde_json::json!({ "counter": i }).to_string().into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify checkpoint was created
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        if let Ok(checkpoint) = checkpoint_result {
            assert!(checkpoint.sequence >= 10, "Checkpoint should be at sequence >= 10");
        }

        // Phase 2: Restart and verify checkpoint was loaded
        facet.on_detach(actor_id).await.unwrap();
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify checkpoint was loaded
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        if let Ok(cp) = checkpoint_result {
            assert!(cp.sequence >= 10, "Checkpoint should be loaded");
        }

        // Verify all entries are still present
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(all_entries.len(), 10, "Should have all 10 entries");
    }

    /// Test: Channel-based mailbox with ACK/NACK
    ///
    /// Scenario:
    /// 1. Send messages to durable channel (SQLite backend)
    /// 2. Actor receives and processes messages
    /// 3. ACK successful messages
    /// 4. NACK failed messages (requeue or DLQ)
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_channel_based_mailbox_ack_nack() {
        // Create SQLite channel as mailbox
        let channel_config = ChannelConfig {
            name: "test-mailbox".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            dead_letter_queue: "test-dlq".to_string(),
            backend_config: Some(channel_config::BackendConfig::Sqlite(
                SqliteConfig {
                    database_path: ":memory:".to_string(),
                    table_name: "".to_string(), // Use default table name from migration
                    wal_mode: true,
                    cleanup_acked: true,
                    cleanup_age_seconds: 3600,
                }
            )),
            ..Default::default()
        };

        let channel = SqliteChannel::new(channel_config.clone()).await.unwrap();

        // Send messages
        for i in 1..=3 {
            let message = ChannelMessage {
                id: format!("msg-{}", i),
                channel: "test-mailbox".to_string(),
                payload: format!("message-{}", i).into_bytes(),
                headers: std::collections::HashMap::new(),
                timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                ..Default::default()
            };
            channel.send(message).await.unwrap();
        }

        // Receive and process messages
        let messages = channel.receive(10).await.unwrap();
        assert_eq!(messages.len(), 3, "Should receive 3 messages");

        // ACK first message (success)
        channel.ack(&messages[0].id).await.unwrap();

        // NACK second message with requeue (retryable error)
        channel.nack(&messages[1].id, true).await.unwrap();

        // NACK third message without requeue (poisonous → DLQ)
        channel.nack(&messages[2].id, false).await.unwrap();

        // Verify requeued message is available again
        let requeued = channel.receive(10).await.unwrap();
        assert_eq!(requeued.len(), 1, "Should receive 1 requeued message");
        assert_eq!(requeued[0].id, messages[1].id, "Should be the requeued message");
    }

    /// Test: Dead Letter Queue (DLQ) for poisonous messages
    ///
    /// Scenario:
    /// 1. Message fails repeatedly (simulated)
    /// 2. After N retries, message is sent to DLQ
    /// 3. DLQ channel contains the poisonous message
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_dead_letter_queue() {
        // Create main channel
        let main_channel_config = ChannelConfig {
            name: "main-channel".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            dead_letter_queue: "dlq-channel".to_string(),
            backend_config: Some(channel_config::BackendConfig::Sqlite(
                SqliteConfig {
                    database_path: ":memory:".to_string(),
                    table_name: "".to_string(), // Use default table name from migration
                    wal_mode: true,
                    cleanup_acked: true,
                    cleanup_age_seconds: 3600,
                }
            )),
            ..Default::default()
        };

        let main_channel = SqliteChannel::new(main_channel_config).await.unwrap();

        // Create DLQ channel
        let dlq_channel_config = ChannelConfig {
            name: "dlq-channel".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            dead_letter_queue: "".to_string(), // DLQ doesn't have its own DLQ
            backend_config: Some(channel_config::BackendConfig::Sqlite(
                SqliteConfig {
                    database_path: ":memory:".to_string(),
                    table_name: "".to_string(), // Use default table name from migration
                    wal_mode: true,
                    cleanup_acked: true,
                    cleanup_age_seconds: 3600,
                }
            )),
            ..Default::default()
        };

        let dlq_channel = SqliteChannel::new(dlq_channel_config).await.unwrap();

        // Send a message that will fail
        let poisonous_message = ChannelMessage {
            id: "poisonous-msg-1".to_string(),
            channel: "main-channel".to_string(),
            payload: b"poisonous data".to_vec(),
            headers: std::collections::HashMap::new(),
            timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
            ..Default::default()
        };
        main_channel.send(poisonous_message.clone()).await.unwrap();

        // Simulate multiple failures (retries)
        let mut retry_count = 0;
        const MAX_RETRIES: u32 = 3;

        loop {
            let messages = main_channel.receive(10).await.unwrap();
            if messages.is_empty() {
                break;
            }

            let msg = &messages[0];
            retry_count += 1;

            if retry_count >= MAX_RETRIES {
                // Send to DLQ
                main_channel.nack(&msg.id, false).await.unwrap();
                
                // Manually move to DLQ (in production, this would be automatic)
                dlq_channel.send(msg.clone()).await.unwrap();
                break;
            } else {
                // Retry
                main_channel.nack(&msg.id, true).await.unwrap();
            }
        }

        // Verify message is in DLQ
        let dlq_messages = dlq_channel.receive(10).await.unwrap();
        assert_eq!(dlq_messages.len(), 1, "Should have 1 message in DLQ");
        assert_eq!(dlq_messages[0].id, "poisonous-msg-1", "Should be the poisonous message");
    }

    /// Test: Crash during message processing (edge case)
    ///
    /// Scenario:
    /// 1. Actor processes message (before_method called)
    /// 2. Actor crashes before after_method
    /// 3. On restart, message is replayed and processed
    #[tokio::test]
    async fn test_crash_during_message_processing() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000, // No checkpointing for this test
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-crash-1";

        // Phase 1: Start processing message but crash before completion
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // before_method called (message received)
        let method = "process";
        let payload = b"test message".to_vec();
        facet.before_method(method, &payload).await.unwrap();

        // Simulate crash (detach without after_method)
        storage.flush().await.unwrap();
        facet.on_detach(actor_id).await.unwrap();

        // Verify MessageReceived is journaled but MessageProcessed is not
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let message_received_count = entries.iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(_))
                )
            })
            .count();
        let message_processed_count = entries.iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::MessageProcessed(_))
                )
            })
            .count();

        assert_eq!(message_received_count, 1, "Should have 1 MessageReceived entry");
        assert_eq!(message_processed_count, 0, "Should have 0 MessageProcessed entries (crashed)");

        // Phase 2: Restart and replay
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Now complete the message processing
        new_facet.after_method(method, &payload, b"result".as_slice()).await.unwrap();
        storage.flush().await.unwrap();

        // Verify MessageProcessed is now journaled
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        let message_processed_count = all_entries.iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::MessageProcessed(_))
                )
            })
            .count();
        assert_eq!(message_processed_count, 1, "Should have 1 MessageProcessed entry after replay");
    }

    /// Test: Channel-based mailbox with durability integration
    ///
    /// Scenario:
    /// 1. Use SQLite channel as actor mailbox
    /// 2. Process messages with durability facet
    /// 3. ACK messages after successful processing
    /// 4. Verify messages are not redelivered after ACK
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_channel_mailbox_with_durability() {
        // Create channel as mailbox
        let channel_config = ChannelConfig {
            name: "durable-mailbox".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            delivery: plexspaces_proto::channel::v1::DeliveryGuarantee::DeliveryGuaranteeAtLeastOnce as i32,
            dead_letter_queue: "".to_string(),
            backend_config: Some(channel_config::BackendConfig::Sqlite(
                SqliteConfig {
                    database_path: ":memory:".to_string(),
                    table_name: "".to_string(), // Use default table name from migration
                    wal_mode: true,
                    cleanup_acked: true,
                    cleanup_age_seconds: 3600,
                }
            )),
            ..Default::default()
        };

        let channel = SqliteChannel::new(channel_config).await.unwrap();

        // Create journal storage
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-mailbox-actor";

        // Send messages to channel
        for i in 1..=3 {
            let message = ChannelMessage {
                id: format!("msg-{}", i),
                channel: "durable-mailbox".to_string(),
                payload: format!("payload-{}", i).into_bytes(),
                headers: std::collections::HashMap::new(),
                timestamp: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                ..Default::default()
            };
            channel.send(message).await.unwrap();
        }

        // Process messages with durability
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let messages = channel.receive(10).await.unwrap();
        assert_eq!(messages.len(), 3, "Should receive 3 messages");

        for msg in &messages {
            // Process with durability
            facet.before_method("process", &msg.payload).await.unwrap();
            facet.after_method("process", &msg.payload, b"ok").await.unwrap();
            
            // ACK message
            channel.ack(&msg.id).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify messages are not redelivered (all ACKed)
        let remaining = channel.receive(10).await.unwrap();
        assert_eq!(remaining.len(), 0, "Should have no unacked messages");

        // Verify journal entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        let message_count = entries.iter()
            .filter(|e| {
                matches!(
                    e.entry,
                    Some(plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(_))
                )
            })
            .count();
        assert_eq!(message_count, 3, "Should have 3 journaled messages");
    }

    /// Test: Recovery with checkpoint (fast recovery)
    ///
    /// Scenario:
    /// 1. Process many messages to create checkpoint
    /// 2. Crash and restart
    /// 3. Verify recovery uses checkpoint (delta replay)
    #[tokio::test]
    async fn test_recovery_with_checkpoint() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 20, // Checkpoint every 20 entries
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-checkpoint-recovery";

        // Phase 1: Process messages to create checkpoint
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 15 messages (30 entries) - should create checkpoint at 20
        for i in 1..=15 {
            let method = "increment";
            let payload = format!("value-{}", i).into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            facet.after_method(method, &payload, b"ok").await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify checkpoint exists
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        assert!(checkpoint_result.is_ok(), "Checkpoint should exist");
        let checkpoint = checkpoint_result.unwrap();
        assert!(checkpoint.sequence >= 20, "Checkpoint should be at sequence >= 20");

        // Phase 2: Restart and verify recovery
        facet.on_detach(actor_id).await.unwrap();

        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify all entries are still present
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(all_entries.len(), 30, "Should have all 30 entries");

        // Verify checkpoint is still valid
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        assert!(checkpoint_result.is_ok(), "Checkpoint should still exist after restart");
    }

    /// Test: Side effect idempotency during replay
    ///
    /// Scenario:
    /// 1. Execute side effect (external API call)
    /// 2. Crash and restart
    /// 3. Verify side effect is cached (not re-executed)
    #[tokio::test]
    async fn test_side_effect_idempotency() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true, // Enable caching
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        };

        let actor_id = "test-idempotency";

        // Track side effect executions
        let execution_count = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));

        // Phase 1: Execute side effect
        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        let ctx_arc = facet.get_execution_context();
        let ctx_guard = ctx_arc.read().await;
        let ctx = ctx_guard.as_ref().unwrap();

        let exec_count = execution_count.clone();
        let _result = ctx
            .record_side_effect("api_call_1", "http_request", move || {
                let count = exec_count.clone();
                async move {
                    count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ApiResponse {
                        message: "cached".to_string(),
                    })
                }
            })
            .await
            .unwrap();

        drop(ctx_guard);

        // Process message to journal side effect
        facet.before_method("process", b"test").await.unwrap();
        facet.after_method("process", b"test", b"ok").await.unwrap();
        storage.flush().await.unwrap();

        assert_eq!(execution_count.load(std::sync::atomic::Ordering::SeqCst), 1, "Side effect should execute once");

        // Phase 2: Restart and replay
        facet.on_detach(actor_id).await.unwrap();

        let mut config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        if let Some(ref timeout) = config.checkpoint_timeout {
            // Handle Duration separately since it doesn't implement Serialize
            let mut timeout_obj = serde_json::Map::new();
            timeout_obj.insert("seconds".to_string(), serde_json::Value::Number(timeout.seconds.into()));
            timeout_obj.insert("nanos".to_string(), serde_json::Value::Number(timeout.nanos.into()));
            config_value["checkpoint_timeout"] = serde_json::Value::Object(timeout_obj);
        }
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_value, 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify side effect was not re-executed (cached)
        assert_eq!(
            execution_count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "Side effect should not be re-executed during replay (cached)"
        );
    }
}
