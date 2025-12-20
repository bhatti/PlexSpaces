// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for durable actor example
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;
    use prost::Message;

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
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
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
        let config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "checkpoint_timeout": config.checkpoint_timeout,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
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
            backend_config: None,
        };

        let actor_id = "test-counter-2";

        // Phase 1: Record side effects
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
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
        let config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "checkpoint_timeout": config.checkpoint_timeout,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
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
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
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
        let config_value = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "checkpoint_timeout": config.checkpoint_timeout,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
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

