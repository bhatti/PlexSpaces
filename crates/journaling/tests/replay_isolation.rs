// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Isolation tests to debug replay issues
// These tests isolate specific scenarios from the failing deterministic_replay tests

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;
    use plexspaces_proto::prost_types;
    use std::time::SystemTime;

    /// Helper to create a test SQLite storage (in-memory)
    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    /// Helper to convert DurabilityConfig to Value
    fn config_to_value(config: &DurabilityConfig) -> serde_json::Value {
        serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "checkpoint_timeout": config.checkpoint_timeout,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        })
    }

    /// Test: Exact scenario from test_replay_missing_checkpoint
    /// Process 5 messages, detach, reattach with replay, verify entries
    #[tokio::test]
    async fn test_replay_missing_checkpoint_scenario() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-replay-isolation";

        // Attach
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        // Flush to ensure entries are written
        storage.flush().await.unwrap();

        // Verify entries exist BEFORE detach
        let entries_before_detach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before detach: {}", entries_before_detach.len());
        assert_eq!(
            entries_before_detach.len(),
            10,
            "Should have 10 entries before detach (5 messages * 2 entries)"
        );

        // Detach
        facet.on_detach(actor_id).await.unwrap();

        // Verify entries still exist AFTER detach
        let entries_after_detach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after detach: {}", entries_after_detach.len());
        assert_eq!(
            entries_after_detach.len(),
            10,
            "Should have 10 entries after detach"
        );

        // Create new facet with same config
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);

        // Attach should trigger replay
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify entries still exist after reattach
        let entries_after_reattach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after reattach: {}", entries_after_reattach.len());
        assert_eq!(
            entries_after_reattach.len(),
            10,
            "Should have 10 entries after reattach"
        );
    }

    /// Test: Checkpoint scenario from test_actor_restart_with_checkpoint
    #[tokio::test]
    async fn test_checkpoint_scenario() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 50,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-checkpoint-isolation";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 100 messages
        for i in 1..=100 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();

            // Trigger checkpoint at message 50
            if i == 50 {
                storage.flush().await.unwrap();
                
                let checkpoint = Checkpoint {
                    actor_id: actor_id.to_string(),
                    sequence: 100, // 50 messages * 2 entries each
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    state_data: b"counter = 50".to_vec(),
                    compression: CompressionType::CompressionTypeNone as i32,
                    metadata: Default::default(),
                    state_schema_version: 1,
                };
                storage.save_checkpoint(&checkpoint).await.unwrap();
            }
        }
        
        storage.flush().await.unwrap();

        // Verify checkpoint exists
        // Note: Automatic checkpointing at sequence 200 (checkpoint_interval=50) will create a checkpoint
        let checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert!(checkpoint.sequence >= 100, "Checkpoint should be at sequence >= 100");

        // Verify all entries exist
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("All entries: {}", all_entries.len());
        assert_eq!(all_entries.len(), 200, "Should have exactly 200 entries (100 messages * 2)");

        // Detach
        facet.on_detach(actor_id).await.unwrap();

        // Create new facet
        let mut new_facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify entries still exist after restart
        let entries_after_restart = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after restart: {}", entries_after_restart.len());
        assert_eq!(
            entries_after_restart.len(),
            200,
            "Should have all 200 entries after restart"
        );
        
        // Verify checkpoint still exists
        let checkpoint_after_restart = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(
            checkpoint_after_restart.sequence,
            checkpoint.sequence,
            "Checkpoint should persist after restart"
        );
    }

    /// Test: Verify that on_attach with replay_on_activation=true doesn't lose entries
    #[tokio::test]
    async fn test_replay_doesnt_lose_entries() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet1 = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-no-loss";

        // Attach and write entries
        facet1.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        
        // Write 3 messages
        for i in 1..=3 {
            let method = "test";
            let payload = format!("payload-{}", i).into_bytes();
            facet1.before_method(method, &payload).await.unwrap();
            let result = format!("result-{}", i).into_bytes();
            facet1.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify entries
        let entries1 = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries1.len(), 6, "Should have 6 entries");

        // Detach
        facet1.on_detach(actor_id).await.unwrap();

        // Verify entries still exist
        let entries2 = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries2.len(), 6, "Should still have 6 entries after detach");

        // Create new facet and reattach
        let mut facet2 = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet2.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify entries still exist
        let entries3 = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries3.len(), 6, "Should still have 6 entries after reattach with replay");
    }
}

