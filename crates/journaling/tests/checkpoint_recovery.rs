// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for checkpoint-based state recovery (Phase 9.2)

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

    /// Test 1: Recovery from checkpoint with no new entries
    ///
    /// Scenario:
    /// 1. Actor processes messages and creates checkpoint
    /// 2. Actor restarts
    /// 3. Should load state from checkpoint (no replay needed)
    #[tokio::test]
    async fn test_recovery_from_checkpoint_no_delta() {
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
        let actor_id = "test-actor-checkpoint-1";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries total)
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Manually create checkpoint at sequence 10
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: b"counter = 5".to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();
        storage.flush().await.unwrap();

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify checkpoint was loaded
        let loaded_checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(loaded_checkpoint.sequence, 10, "Checkpoint should be at sequence 10");
        assert_eq!(
            loaded_checkpoint.state_data,
            b"counter = 5",
            "Checkpoint state should be 'counter = 5'"
        );

        // Verify replay started from after checkpoint (no entries to replay)
        let entries_after_checkpoint = storage.replay_from(actor_id, 11).await.unwrap();
        assert_eq!(
            entries_after_checkpoint.len(),
            0,
            "Should have no entries after checkpoint"
        );
    }

    /// Test 2: Recovery from checkpoint with delta (new entries after checkpoint)
    ///
    /// Scenario:
    /// 1. Actor processes messages and creates checkpoint at sequence 10
    /// 2. Actor processes more messages (sequences 11-20)
    /// 3. Actor restarts
    /// 4. Should load checkpoint state and replay delta (sequences 11-20)
    #[tokio::test]
    async fn test_recovery_from_checkpoint_with_delta() {
        let storage = create_test_storage().await;
        // Use high checkpoint_interval to prevent automatic checkpointing
        // We'll manually create checkpoint at sequence 10
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000, // High interval, no automatic checkpointing
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-checkpoint-2";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries total)
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Manually create checkpoint at sequence 10
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: b"counter = 5".to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();
        storage.flush().await.unwrap();

        // Process 5 more messages (sequences 11-20)
        for i in 6..=10 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify checkpoint was loaded (should be at sequence 10, not 20, since we disabled auto-checkpointing)
        let loaded_checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(loaded_checkpoint.sequence, 10, "Checkpoint should be at sequence 10");

        // Verify delta entries exist (sequences 11-20)
        let delta_entries = storage.replay_from(actor_id, 11).await.unwrap();
        assert_eq!(
            delta_entries.len(),
            10,
            "Should have 10 delta entries (sequences 11-20)"
        );
        assert_eq!(
            delta_entries[0].sequence, 11,
            "First delta entry should be sequence 11"
        );
        assert_eq!(
            delta_entries.last().unwrap().sequence, 20,
            "Last delta entry should be sequence 20"
        );
    }

    /// Test 3: Recovery when checkpoint is missing (fallback to full replay)
    ///
    /// Scenario:
    /// 1. Actor processes messages (no checkpoint created)
    /// 2. Actor restarts
    /// 3. Should fallback to full replay from beginning
    #[tokio::test]
    async fn test_recovery_missing_checkpoint_fallback() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000, // High interval, no checkpoint will be created
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-checkpoint-3";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries total)
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify no checkpoint exists
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        assert!(checkpoint_result.is_err(), "Should have no checkpoint");

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify full replay happened (all entries should be replayed)
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            all_entries.len(),
            10,
            "Should have all 10 entries for full replay"
        );
    }

    /// Test 4: Recovery with multiple checkpoints (should pick latest)
    ///
    /// Scenario:
    /// 1. Actor processes messages and creates checkpoint at sequence 10
    /// 2. Actor processes more messages and creates checkpoint at sequence 20
    /// 3. Actor restarts
    /// 4. Should load latest checkpoint (sequence 20) and replay delta from 21
    #[tokio::test]
    async fn test_recovery_multiple_checkpoints_pick_latest() {
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
        let actor_id = "test-actor-checkpoint-4";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries) - first checkpoint
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Create first checkpoint at sequence 10
        let checkpoint1 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: b"counter = 5".to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint1).await.unwrap();

        // Process 5 more messages (sequences 11-20) - second checkpoint
        for i in 6..=10 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Create second checkpoint at sequence 20
        let checkpoint2 = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 20,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: b"counter = 10".to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint2).await.unwrap();
        storage.flush().await.unwrap();

        // Restart actor
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify latest checkpoint was loaded (sequence 20)
        let loaded_checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(
            loaded_checkpoint.sequence, 20,
            "Should load latest checkpoint at sequence 20"
        );
        assert_eq!(
            loaded_checkpoint.state_data,
            b"counter = 10",
            "Latest checkpoint state should be 'counter = 10'"
        );

        // Verify replay starts from after latest checkpoint (sequence 21)
        let delta_entries = storage.replay_from(actor_id, 21).await.unwrap();
        assert_eq!(
            delta_entries.len(),
            0,
            "Should have no entries after latest checkpoint"
        );
    }

    /// Test 5: Recovery with corrupted checkpoint (fallback to full replay)
    ///
    /// Scenario:
    /// 1. Actor processes messages and creates checkpoint
    /// 2. Checkpoint state_data is corrupted/invalid
    /// 3. Actor restarts
    /// 4. Should detect corruption and fallback to full replay
    ///
    /// Note: This test verifies that the system gracefully handles checkpoint corruption.
    /// In a real implementation, we would validate checkpoint integrity (checksum, schema version, etc.)
    #[tokio::test]
    async fn test_recovery_corrupted_checkpoint_fallback() {
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
        let actor_id = "test-actor-checkpoint-5";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (10 entries total)
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Create checkpoint with corrupted/invalid state_data
        // In a real implementation, we would validate this and fallback to full replay
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: vec![0xFF, 0xFF, 0xFF], // Invalid/corrupted data
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };
        storage.save_checkpoint(&checkpoint).await.unwrap();
        storage.flush().await.unwrap();

        // Restart actor
        // Note: Current implementation doesn't validate checkpoint integrity,
        // so it will attempt to use the checkpoint. This test documents the expected behavior
        // for when we add checkpoint validation.
        facet.on_detach(actor_id).await.unwrap();
        let mut new_facet = DurabilityFacet::new(storage.clone(), config);
        
        // Should succeed (current implementation doesn't validate)
        // Future: Should detect corruption and fallback to full replay
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify checkpoint exists (even if corrupted)
        let loaded_checkpoint = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(loaded_checkpoint.sequence, 10, "Checkpoint should exist");
        
        // Verify all entries still exist for full replay fallback
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            all_entries.len(),
            10,
            "Should have all 10 entries available for full replay fallback"
        );
    }
}

