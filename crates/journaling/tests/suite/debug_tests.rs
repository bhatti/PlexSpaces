// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Consolidated debug tests for journaling
// Merged from: debug_loop_behavior.rs, debug_write_path.rs, debug_detach_reattach.rs, debug_replay.rs

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_facet::Facet;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_journaling::*;
    use plexspaces_proto::prost_types;
    use std::sync::Arc;
    use std::time::SystemTime;

    /// Helper to create a test SQLite storage (in-memory)
    async fn create_test_storage() -> Arc<dyn JournalStorage> {
        Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap())
    }

    /// Helper to convert DurabilityConfig to Value
    fn config_to_value(config: &DurabilityConfig) -> serde_json::Value {
        serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        })
    }

    // =============================================================================
    // Tests from debug_loop_behavior.rs (3 tests)
    // =============================================================================

    /// Test: Process 5 messages in a loop (exact failing scenario)
    #[tokio::test]
    async fn test_loop_processing() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-loop";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        println!("=== Processing 5 messages in loop ===");
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();

            println!("Iteration {}: before_method", i);
            facet.before_method(method, &payload).await.unwrap();

            let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
            println!(
                "  Entries after before_method: {}",
                entries_after_before.len()
            );

            let result = format!("counter = {}", i).into_bytes();
            println!("Iteration {}: after_method", i);
            facet.after_method(method, &payload, &result).await.unwrap();

            let entries_after_after = storage.replay_from(actor_id, 0).await.unwrap();
            println!(
                "  Entries after after_method: {}",
                entries_after_after.len()
            );
        }

        println!("=== Flushing ===");
        storage.flush().await.unwrap();

        println!("=== Final check ===");
        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());

        for (i, entry) in final_entries.iter().enumerate() {
            println!("  Entry {}: sequence={}", i, entry.sequence);
        }

        assert_eq!(final_entries.len(), 10, "Should have 10 entries");
    }

    /// Test: Process messages with flush after each
    #[tokio::test]
    async fn test_loop_with_flush() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-loop-flush";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();

            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();

            storage.flush().await.unwrap();

            let entries = storage.replay_from(actor_id, 0).await.unwrap();
            println!("Entries after message {}: {}", i, entries.len());
        }

        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(final_entries.len(), 10, "Should have 10 entries");
    }

    /// Test: Check if batch append is the issue
    #[tokio::test]
    async fn test_batch_append_in_loop() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-batch";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();

            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        let entries_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush: {}", entries_before_flush.len());

        storage.flush().await.unwrap();

        let entries_after_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after flush: {}", entries_after_flush.len());

        assert_eq!(entries_after_flush.len(), 10, "Should have 10 entries");
    }

    // =============================================================================
    // Tests from debug_write_path.rs (4 tests)
    // =============================================================================

    /// Test: Check if append_entry is actually being called
    #[tokio::test]
    async fn test_append_entry_called() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-append";

        println!("=== Step 1: Attach ===");
        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        let entries_after_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after attach: {}", entries_after_attach.len());

        println!("=== Step 2: before_method ===");
        let before_result = facet.before_method("test", b"test").await;
        println!("before_method returned: {:?}", before_result);

        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after before_method (before flush): {}",
            entries_after_before.len()
        );

        println!("=== Step 3: Flush after before_method ===");
        storage.flush().await.unwrap();
        let entries_after_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after before_method (after flush): {}",
            entries_after_before_flush.len()
        );

        println!("=== Step 4: after_method ===");
        let after_result = facet.after_method("test", b"test", b"result").await;
        println!("after_method returned: {:?}", after_result);

        let entries_after_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after after_method (before flush): {}",
            entries_after_after.len()
        );

        println!("=== Step 5: Final flush ===");
        storage.flush().await.unwrap();
        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());

        assert_eq!(final_entries.len(), 2, "Should have 2 entries");
    }

    /// Test: Check if the issue is with storage.clone() creating separate instances
    #[tokio::test]
    async fn test_storage_instance_isolation() {
        let storage1 = create_test_storage().await;

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

        let mut facet = DurabilityFacet::new(storage1.clone(), config_to_value(&config), 50);
        let actor_id = "debug-instance";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();

        storage1.flush().await.unwrap();

        let entries1 = storage1.replay_from(actor_id, 0).await.unwrap();
        println!("Entries using storage1: {}", entries1.len());

        let storage2 = storage1.clone();
        let entries2 = storage2.replay_from(actor_id, 0).await.unwrap();
        println!("Entries using storage2 (clone): {}", entries2.len());

        assert_eq!(entries1.len(), 2, "storage1 should see entries");
        assert_eq!(entries2.len(), 2, "storage2 (clone) should see entries");
    }

    /// Test: Check if entries are written but with transaction issues
    #[tokio::test]
    async fn test_transaction_behavior() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-transaction";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        facet.before_method("test", b"test").await.unwrap();

        let entries_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush: {}", entries_before_flush.len());

        storage.flush().await.unwrap();

        let entries_after_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after flush: {}", entries_after_flush.len());

        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();

        let entries_before_flush2 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush2: {}", entries_before_flush2.len());

        storage.flush().await.unwrap();

        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());

        assert_eq!(final_entries.len(), 2, "Should have 2 entries");
    }

    /// Test: Compare checkpoint_interval=10 vs 1000
    #[tokio::test]
    async fn test_checkpoint_interval_effect() {
        let storage1 = create_test_storage().await;
        let storage2 = create_test_storage().await;

        println!("=== Test 1: checkpoint_interval=10 ===");
        let config1 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet1 = DurabilityFacet::new(storage1.clone(), config_to_value(&config1), 50);
        let actor_id1 = "debug-checkpoint-10";

        facet1
            .on_attach(actor_id1, serde_json::json!({}))
            .await
            .unwrap();
        facet1.before_method("test", b"test").await.unwrap();
        facet1
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage1.flush().await.unwrap();

        let entries1 = storage1.replay_from(actor_id1, 0).await.unwrap();
        println!("Entries with checkpoint_interval=10: {}", entries1.len());

        println!("=== Test 2: checkpoint_interval=1000 ===");
        let config2 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet2 = DurabilityFacet::new(storage2.clone(), config_to_value(&config2), 50);
        let actor_id2 = "debug-checkpoint-1000";

        facet2
            .on_attach(actor_id2, serde_json::json!({}))
            .await
            .unwrap();
        facet2.before_method("test", b"test").await.unwrap();
        facet2
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage2.flush().await.unwrap();

        let entries2 = storage2.replay_from(actor_id2, 0).await.unwrap();
        println!("Entries with checkpoint_interval=1000: {}", entries2.len());

        assert_eq!(entries1.len(), 2, "checkpoint_interval=10 should work");
        assert_eq!(entries2.len(), 2, "checkpoint_interval=1000 should work");
    }

    // =============================================================================
    // Tests from debug_detach_reattach.rs (4 tests)
    // =============================================================================

    /// Test: Exact scenario from test_replay_missing_checkpoint
    #[tokio::test]
    async fn test_exact_failing_scenario() {
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

        println!("=== Phase 1: Create first facet and write entries ===");
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "test-actor-4";

        println!("Attaching facet...");
        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        println!("Processing 5 messages...");
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();

            println!("  Message {}: before_method", i);
            facet.before_method(method, &payload).await.unwrap();

            let result = format!("counter = {}", i).into_bytes();
            println!("  Message {}: after_method", i);
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        println!("Flushing storage...");
        storage.flush().await.unwrap();

        println!("=== Phase 2: Verify entries before detach ===");
        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before detach: {}", entries_before.len());

        for (i, entry) in entries_before.iter().enumerate() {
            println!(
                "  Entry {}: sequence={}, actor_id={}",
                i, entry.sequence, entry.actor_id
            );
        }

        assert_eq!(
            entries_before.len(),
            10,
            "Should have 10 entries before restart (5 messages * 2 entries), got {}",
            entries_before.len()
        );

        println!("=== Phase 3: Detach ===");
        facet.on_detach(actor_id).await.unwrap();

        let entries_after_detach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after detach: {}", entries_after_detach.len());
        assert_eq!(
            entries_after_detach.len(),
            10,
            "Entries should persist after detach"
        );

        println!("=== Phase 4: Create new facet and reattach ===");
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
        let mut new_facet =
            DurabilityFacet::new(storage.clone(), config_to_value(&restart_config), 50);

        println!("Reattaching facet...");
        new_facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        println!("=== Phase 5: Verify entries after reattach ===");
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after reattach: {}", entries_after.len());

        for (i, entry) in entries_after.iter().enumerate() {
            println!(
                "  Entry {}: sequence={}, actor_id={}",
                i, entry.sequence, entry.actor_id
            );
        }

        assert_eq!(
            entries_after.len(),
            10,
            "Should have all 10 entries after restart (5 messages * 2 entries), got {}",
            entries_after.len()
        );
    }

    /// Test: Check if storage.clone() is the issue
    #[tokio::test]
    async fn test_storage_clone_behavior() {
        let storage1 = create_test_storage().await;
        let storage2 = storage1.clone();

        let actor_id = "test-clone";

        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                    MessageReceived {
                        message_id: "msg-1".to_string(),
                        sender_id: "sender-1".to_string(),
                        message_type: "test".to_string(),
                        payload: b"test".to_vec(),
                        metadata: Default::default(),
                    },
                ),
            ),
        };

        storage1.append_entry(&entry).await.unwrap();
        storage1.flush().await.unwrap();

        let entries = storage2.replay_from(actor_id, 0).await.unwrap();
        println!("Entries from cloned storage: {}", entries.len());
        assert_eq!(entries.len(), 1, "Cloned storage should see entries");
    }

    /// Test: Check sequence number after replay
    #[tokio::test]
    async fn test_sequence_after_replay() {
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

        let mut facet1 = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "test-seq";

        facet1
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        for i in 1..=3 {
            facet1
                .before_method("test", &format!("{}", i).into_bytes())
                .await
                .unwrap();
            facet1
                .after_method("test", &[], &format!("result-{}", i).into_bytes())
                .await
                .unwrap();
        }

        storage.flush().await.unwrap();

        let entries1 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after first write: {}", entries1.len());
        assert_eq!(entries1.len(), 6, "Should have 6 entries");

        facet1.on_detach(actor_id).await.unwrap();

        let mut facet2 = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet2
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        facet2.before_method("test", b"new").await.unwrap();
        storage.flush().await.unwrap();

        let entries2 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after reattach and new write: {}", entries2.len());

        if !entries2.is_empty() {
            let last_entry = entries2.last().unwrap();
            println!("Last entry sequence: {}", last_entry.sequence);
            assert_eq!(last_entry.sequence, 7, "New entry should have sequence 7");
        }
    }

    /// Test: Multiple detach/reattach cycles
    #[tokio::test]
    async fn test_multiple_detach_reattach() {
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

        let actor_id = "test-multi";

        for cycle in 1..=3 {
            println!("=== Cycle {} ===", cycle);

            let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
            facet
                .on_attach(actor_id, serde_json::json!({}))
                .await
                .unwrap();

            facet
                .before_method("test", &format!("cycle-{}", cycle).into_bytes())
                .await
                .unwrap();
            facet
                .after_method("test", &[], &format!("result-{}", cycle).into_bytes())
                .await
                .unwrap();

            storage.flush().await.unwrap();

            let entries = storage.replay_from(actor_id, 0).await.unwrap();
            println!("Entries after cycle {}: {}", cycle, entries.len());

            facet.on_detach(actor_id).await.unwrap();
        }

        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());
        assert_eq!(
            final_entries.len(),
            6,
            "Should have 6 entries (3 cycles * 2 entries)"
        );
    }

    // =============================================================================
    // Tests from debug_replay.rs (7 tests)
    // =============================================================================

    /// Test: Minimal reproduction - attach with replay_on_activation=true, write one entry
    #[tokio::test]
    async fn test_minimal_replay_activation() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-minimal";

        println!("=== Step 1: Attach facet ===");
        let attach_result = facet.on_attach(actor_id, serde_json::json!({})).await;
        println!("Attach result: {:?}", attach_result);
        assert!(attach_result.is_ok(), "Attach should succeed");

        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before writing: {}", entries_before.len());

        println!("=== Step 2: Write one entry ===");
        let method = "test";
        let payload = b"test".to_vec();

        println!("Calling before_method...");
        let before_result = facet.before_method(method, &payload).await;
        println!("before_method result: {:?}", before_result);
        assert!(before_result.is_ok(), "before_method should succeed");

        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after before_method: {}",
            entries_after_before.len()
        );

        println!("Calling after_method...");
        let result = b"result".to_vec();
        let after_result = facet.after_method(method, &payload, &result).await;
        println!("after_method result: {:?}", after_result);
        assert!(after_result.is_ok(), "after_method should succeed");

        storage.flush().await.unwrap();

        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after after_method: {}", entries_after.len());

        for (i, entry) in entries_after.iter().enumerate() {
            println!(
                "Entry {}: sequence={}, actor_id={}, type={:?}",
                i,
                entry.sequence,
                entry.actor_id,
                entry.entry.as_ref().map(|e| format!("{:?}", e))
            );
        }

        assert_eq!(
            entries_after.len(),
            2,
            "Should have 2 entries (MessageReceived + MessageProcessed)"
        );
    }

    /// Test: Compare replay_on_activation=true vs false
    #[tokio::test]
    async fn test_replay_activation_comparison() {
        let storage1 = create_test_storage().await;
        let storage2 = create_test_storage().await;

        println!("=== Test 1: replay_on_activation = false ===");
        let config1 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet1 = DurabilityFacet::new(storage1.clone(), config_to_value(&config1), 50);
        let actor_id1 = "debug-no-replay";

        facet1
            .on_attach(actor_id1, serde_json::json!({}))
            .await
            .unwrap();
        facet1.before_method("test", b"test").await.unwrap();
        facet1
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage1.flush().await.unwrap();

        let entries1 = storage1.replay_from(actor_id1, 0).await.unwrap();
        println!(
            "Entries with replay_on_activation=false: {}",
            entries1.len()
        );
        assert_eq!(entries1.len(), 2, "Should have 2 entries");

        println!("=== Test 2: replay_on_activation = true ===");
        let config2 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet2 = DurabilityFacet::new(storage2.clone(), config_to_value(&config2), 50);
        let actor_id2 = "debug-with-replay";

        facet2
            .on_attach(actor_id2, serde_json::json!({}))
            .await
            .unwrap();
        facet2.before_method("test", b"test").await.unwrap();
        facet2
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage2.flush().await.unwrap();

        let entries2 = storage2.replay_from(actor_id2, 0).await.unwrap();
        println!("Entries with replay_on_activation=true: {}", entries2.len());
        assert_eq!(entries2.len(), 2, "Should have 2 entries");
    }

    /// Test: Check sequence numbers during write
    #[tokio::test]
    async fn test_sequence_numbers_during_write() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-sequence";

        println!("=== Before attach ===");
        let entries_before_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before attach: {}", entries_before_attach.len());

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        println!("=== After attach, before write ===");
        let entries_after_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after attach: {}", entries_after_attach.len());

        facet.before_method("test", b"test").await.unwrap();

        println!("=== After before_method ===");
        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after before_method: {}",
            entries_after_before.len()
        );
        if !entries_after_before.is_empty() {
            println!("First entry sequence: {}", entries_after_before[0].sequence);
        }

        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage.flush().await.unwrap();

        println!("=== After after_method ===");
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after after_method: {}", entries_after.len());
        for entry in &entries_after {
            println!("Entry sequence: {}", entry.sequence);
        }
    }

    /// Test: Direct storage write vs facet write
    #[tokio::test]
    async fn test_direct_vs_facet_write() {
        let storage = create_test_storage().await;
        let actor_id = "debug-direct";

        println!("=== Test 1: Direct storage write ===");
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence: 1,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                    MessageReceived {
                        message_id: "msg-1".to_string(),
                        sender_id: "sender-1".to_string(),
                        message_type: "test".to_string(),
                        payload: b"test".to_vec(),
                        metadata: Default::default(),
                    },
                ),
            ),
        };

        let seq = storage.append_entry(&entry).await.unwrap();
        println!("Direct write sequence: {}", seq);
        storage.flush().await.unwrap();

        let entries_direct = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries from direct write: {}", entries_direct.len());
        assert_eq!(
            entries_direct.len(),
            1,
            "Should have 1 entry from direct write"
        );

        println!("=== Test 2: Facet write with replay_on_activation=true ===");
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id2 = "debug-facet";

        facet
            .on_attach(actor_id2, serde_json::json!({}))
            .await
            .unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage.flush().await.unwrap();

        let entries_facet = storage.replay_from(actor_id2, 0).await.unwrap();
        println!("Entries from facet write: {}", entries_facet.len());
        assert_eq!(
            entries_facet.len(),
            2,
            "Should have 2 entries from facet write"
        );
    }

    /// Test: Check if entries are written but with wrong actor_id
    #[tokio::test]
    async fn test_actor_id_verification() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-actor-id";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage.flush().await.unwrap();

        let all_entries = storage.replay_from("", 0).await;
        println!("All entries query result: {:?}", all_entries);

        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries for actor_id '{}': {}", actor_id, entries.len());

        for entry in &entries {
            println!(
                "Entry actor_id: '{}', sequence: {}",
                entry.actor_id, entry.sequence
            );
        }
    }

    /// Test: Check if entries are written but query fails
    #[tokio::test]
    async fn test_query_verification() {
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

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-query";

        facet
            .on_attach(actor_id, serde_json::json!({}))
            .await
            .unwrap();

        facet.before_method("test", b"test").await.unwrap();
        facet
            .after_method("test", b"test", b"result")
            .await
            .unwrap();
        storage.flush().await.unwrap();

        let entries_0 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries with from_sequence=0: {}", entries_0.len());

        let entries_1 = storage.replay_from(actor_id, 1).await.unwrap();
        println!("Entries with from_sequence=1: {}", entries_1.len());

        let stats = storage.get_stats(Some(actor_id)).await.unwrap();
        println!("Stats for actor: total_entries={}", stats.total_entries);
    }

    /// Test: Step-by-step with detailed logging
    #[tokio::test]
    async fn test_step_by_step_debug() {
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

        println!("=== Creating facet ===");
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "debug-step";

        println!("=== Step 1: on_attach ===");
        let attach_result = facet.on_attach(actor_id, serde_json::json!({})).await;
        println!("Attach result: {:?}", attach_result);

        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        println!("Checkpoint result: {:?}", checkpoint_result);

        let existing_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Existing entries: {}", existing_entries.len());

        println!("=== Step 2: before_method ===");
        let before_result = facet.before_method("test", b"test").await;
        println!("Before result: {:?}", before_result);

        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!(
            "Entries after before_method: {}",
            entries_after_before.len()
        );

        println!("=== Step 3: after_method ===");
        let after_result = facet.after_method("test", b"test", b"result").await;
        println!("After result: {:?}", after_result);

        println!("=== Step 4: flush ===");
        let flush_result = storage.flush().await;
        println!("Flush result: {:?}", flush_result);

        println!("=== Step 5: verify entries ===");
        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());

        for (i, entry) in final_entries.iter().enumerate() {
            println!("Entry {}: id={}, actor_id={}, sequence={}, type={:?}",
                i, entry.id, entry.actor_id, entry.sequence,
                entry.entry.as_ref().map(|e| {
                    match e {
                        plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(_) => "MessageReceived",
                        plexspaces_proto::v1::journaling::journal_entry::Entry::MessageProcessed(_) => "MessageProcessed",
                        _ => "Other",
                    }
                }));
        }

        assert_eq!(final_entries.len(), 2, "Should have 2 entries");
    }
}
