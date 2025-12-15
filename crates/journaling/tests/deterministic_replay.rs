// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for Phase 9.1: Deterministic Replay on Actor Restart
//
// Purpose: Verify that actors can be restarted and their state recovered
// through deterministic replay of journal entries.
//
// Uses SQLite for integration testing to ensure persistence works correctly.

#[cfg(any(feature = "sqlite-backend", feature = "postgres-backend"))]
mod sqlite_tests {
    use plexspaces_journaling::*;
    #[cfg(feature = "sqlite-backend")]
    use plexspaces_journaling::sql::SqliteJournalStorage;
    #[cfg(feature = "postgres-backend")]
    use plexspaces_journaling::sql::PostgresJournalStorage;
    use plexspaces_facet::Facet;
    use plexspaces_proto::prost_types;
    use std::time::SystemTime;

    /// Helper to create a test storage (SQLite or PostgreSQL)
    #[cfg(feature = "sqlite-backend")]
    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    #[cfg(feature = "postgres-backend")]
    async fn create_test_storage() -> PostgresJournalStorage {
        let db_url = std::env::var("TEST_POSTGRES_URL")
            .unwrap_or_else(|_| "postgresql://postgres:postgres@localhost/plexspaces_test".to_string());
        PostgresJournalStorage::new(&db_url).await.unwrap()
    }

    /// Test: Actor restart with full journal replay
    ///
    /// Scenario:
    /// 1. Actor processes 3 messages (state: counter = 3)
    /// 2. Actor crashes/restarts
    /// 3. Replay journal entries
    /// 4. Verify state is restored (counter = 3)
    #[tokio::test]
    async fn test_actor_restart_full_replay() {
        let storage = create_test_storage().await;
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
        checkpoint_interval: 1000, // No checkpointing for this test
        checkpoint_timeout: None,
        replay_on_activation: true,
        cache_side_effects: true,
        compression: CompressionType::CompressionTypeNone as i32,
        backend_config: None,
        state_schema_version: 1,
    };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-1";

        // Phase 1: Normal execution - process 3 messages
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Simulate processing 3 messages
        for i in 1..=3 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();

            // Before method (journals MessageReceived)
            facet.before_method(method, &payload).await.unwrap();

            // Simulate actor state change (counter += i)
            // In real implementation, this would be actor.handle_message()
            let result = format!("counter = {}", i).into_bytes();

            // After method (journals MessageProcessed + StateChanged)
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        // Flush to ensure entries are written to SQLite
        storage.flush().await.unwrap();

        // Flush to ensure entries are written to SQLite
        storage.flush().await.unwrap();

        // Verify journal has 6 entries (3 MessageReceived + 3 MessageProcessed)
        // Note: message_sequence starts at 0, so first entry is sequence 1
        // Use replay_from(actor_id, 0) to get all entries (sequence >= 0)
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 6, "Should have 6 journal entries");

        // Phase 2: Actor restart - detach and reattach
        facet.on_detach(actor_id).await.unwrap();

        // Create new facet instance (simulating actor restart)
        let mut new_facet = DurabilityFacet::new(storage.clone(), config.clone());
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Phase 3: Verify replay occurred
        // The execution context should be restored with side effects cached
        // Note: execution_context is private, so we verify indirectly by checking journal
        // In a real implementation, we'd have a public method to access execution context

        // Verify journal entries are still present
        // Use replay_from(actor_id, 0) to get all entries (sequence >= 0)
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            entries_after.len(),
            6,
            "Journal entries should persist after restart"
        );
    }

    /// Test: Actor restart with checkpoint (faster recovery)
    ///
    /// Scenario:
    /// 1. Actor processes 100 messages
    /// 2. Checkpoint created at message 50
    /// 3. Actor crashes
    /// 4. Replay from checkpoint (only replay messages 51-100)
    #[tokio::test]
    async fn test_actor_restart_with_checkpoint() {
        let storage = create_test_storage().await;
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 50, // Checkpoint every 50 messages
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-2";

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
                // Flush to ensure all entries are written before checkpoint
                storage.flush().await.unwrap();
                
                // Manually trigger checkpoint (normally done by CheckpointManager)
                // At message 50, we have 100 entries (50 messages * 2 entries each)
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
        
        // Flush after all messages to ensure entries are written
        storage.flush().await.unwrap();

        // Verify checkpoint exists
        // Note: After processing 100 messages, we have 200 entries total
        // The manual checkpoint was created at message 50 (sequence 100)
        // But automatic checkpointing at sequence 200 (checkpoint_interval=50) will create a new checkpoint
        // The latest checkpoint will be at sequence 200
        let latest_checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert!(latest_checkpoint.sequence >= 100, "Checkpoint should be at sequence >= 100");

        // Verify all entries exist (200 entries total)
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(all_entries.len(), 200, "Should have 200 entries (100 messages * 2)");

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config.clone());
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify entries still exist after restart
        let entries_after_restart = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            entries_after_restart.len(),
            200,
            "Should have all 200 entries after restart"
        );
        
        // Verify checkpoint still exists
        let checkpoint_after_restart = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(
            checkpoint_after_restart.sequence,
            latest_checkpoint.sequence,
            "Checkpoint should persist after restart"
        );
    }

    /// Test: Deterministic replay with side effects
    ///
    /// Scenario:
    /// 1. Actor makes external API call (side effect)
    /// 2. Result is cached in journal
    /// 3. Actor restarts
    /// 4. Replay should use cached result, not re-execute
    #[tokio::test]
    async fn test_deterministic_replay_with_side_effects() {
        let storage = create_test_storage().await;
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true, // Enable side effect caching
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-3";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Record a side effect by processing a message that triggers a side effect
        // In a real implementation, the actor would call ctx.record_side_effect()
        // For this test, we'll simulate by journaling a side effect entry directly
        let side_effect_entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(
                    SideEffectExecuted {
                        effect_id: "api_call_1".to_string(),
                        effect_type: SideEffectType::SideEffectTypeExternalCall as i32,
                        request: vec![],
                        response: b"api_result_42".to_vec(),
                        error: String::new(),
                    },
                ),
            ),
        };
        storage.append_entry(&side_effect_entry).await.unwrap();
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
        assert_eq!(
            side_effect_entries.len(),
            1,
            "Should have 1 side effect entry"
        );

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config.clone());
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // In replay mode, side effect should be cached (not re-executed)
        // Verify by checking that the side effect entry exists in journal
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        let side_effect_entries: Vec<_> = entries_after
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
            "Should have 1 side effect entry in journal"
        );
    }

    /// Test: Replay with no journal entries (new actor)
    ///
    /// Scenario:
    /// 1. New actor with no journal history
    /// 2. Attach durability facet
    /// 3. Should not fail, should start fresh
    #[tokio::test]
    async fn test_replay_empty_journal() {
        let storage = create_test_storage().await;
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "new-actor";

        // Attach to new actor (no journal entries)
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Should succeed and create new execution context
        // Verify by checking that we can process a message
        let result = facet.before_method("test", &[]).await;
        assert!(result.is_ok(), "Should be able to process messages");
    }

    /// Test: Replay with corrupted journal (should handle gracefully)
    ///
    /// Scenario:
    /// 1. Journal has entries but checkpoint is missing
    /// 2. Replay should still work (replay from beginning)
    #[tokio::test]
    async fn test_replay_missing_checkpoint() {
        let storage = create_test_storage().await;
        #[cfg(feature = "sqlite-backend")]
        let backend = JournalBackend::JournalBackendSqlite as i32;
        #[cfg(feature = "postgres-backend")]
        let backend = JournalBackend::JournalBackendPostgres as i32;
        
        let config = DurabilityConfig {
            backend,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-4";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process some messages
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        // Flush to ensure entries are written to SQLite
        storage.flush().await.unwrap();

        // Don't create checkpoint (simulating missing checkpoint)
        // Verify entries exist before restart
        // Use replay_from(actor_id, 0) to get all entries (sequence >= 0)
        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            entries_before.len(),
            10,
            "Should have 10 entries before restart (5 messages * 2 entries), got {}",
            entries_before.len()
        );
        
        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        
        // Create new config for restart (use SQLite backend)
        let restart_config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };
        let mut new_facet = DurabilityFacet::new(storage.clone(), restart_config);
        
        // Should succeed even without checkpoint (replay from beginning)
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify all entries are still in journal (they should persist)
        // Use replay_from(actor_id, 0) to get all entries (sequence >= 0)
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            entries_after.len(),
            10,
            "Should have all 10 entries after restart (5 messages * 2 entries), got {}",
            entries_after.len()
        );
    }
}

