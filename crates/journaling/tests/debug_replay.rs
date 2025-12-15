// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Debug tests to isolate replay issues
// These tests add extensive logging to understand what's happening

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

    /// Test: Minimal reproduction - attach with replay_on_activation=true, write one entry
    #[tokio::test]
    async fn test_minimal_replay_activation() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true, // KEY: This is enabled
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-minimal";

        println!("=== Step 1: Attach facet ===");
        let attach_result = facet.on_attach(actor_id, serde_json::json!({})).await;
        println!("Attach result: {:?}", attach_result);
        assert!(attach_result.is_ok(), "Attach should succeed");

        // Check entries before writing
        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before writing: {}", entries_before.len());

        println!("=== Step 2: Write one entry ===");
        let method = "test";
        let payload = b"test".to_vec();
        
        println!("Calling before_method...");
        let before_result = facet.before_method(method, &payload).await;
        println!("before_method result: {:?}", before_result);
        assert!(before_result.is_ok(), "before_method should succeed");

        // Check entries after before_method
        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after before_method: {}", entries_after_before.len());

        println!("Calling after_method...");
        let result = b"result".to_vec();
        let after_result = facet.after_method(method, &payload, &result).await;
        println!("after_method result: {:?}", after_result);
        assert!(after_result.is_ok(), "after_method should succeed");

        // Flush
        storage.flush().await.unwrap();

        // Check entries after after_method
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after after_method: {}", entries_after.len());
        
        for (i, entry) in entries_after.iter().enumerate() {
            println!("Entry {}: sequence={}, actor_id={}, type={:?}", 
                i, entry.sequence, entry.actor_id, 
                entry.entry.as_ref().map(|e| format!("{:?}", e)));
        }

        assert_eq!(entries_after.len(), 2, "Should have 2 entries (MessageReceived + MessageProcessed)");
    }

    /// Test: Compare replay_on_activation=true vs false
    #[tokio::test]
    async fn test_replay_activation_comparison() {
        let storage1 = create_test_storage().await;
        let storage2 = create_test_storage().await;

        // Test 1: replay_on_activation = false
        println!("=== Test 1: replay_on_activation = false ===");
        let config1 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: false, // Disabled
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet1 = DurabilityFacet::new(storage1.clone(), config1);
        let actor_id1 = "debug-no-replay";

        facet1.on_attach(actor_id1, serde_json::json!({})).await.unwrap();
        facet1.before_method("test", b"test").await.unwrap();
        facet1.after_method("test", b"test", b"result").await.unwrap();
        storage1.flush().await.unwrap();

        let entries1 = storage1.replay_from(actor_id1, 0).await.unwrap();
        println!("Entries with replay_on_activation=false: {}", entries1.len());
        assert_eq!(entries1.len(), 2, "Should have 2 entries");

        // Test 2: replay_on_activation = true
        println!("=== Test 2: replay_on_activation = true ===");
        let config2 = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true, // Enabled
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet2 = DurabilityFacet::new(storage2.clone(), config2);
        let actor_id2 = "debug-with-replay";

        facet2.on_attach(actor_id2, serde_json::json!({})).await.unwrap();
        facet2.before_method("test", b"test").await.unwrap();
        facet2.after_method("test", b"test", b"result").await.unwrap();
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-sequence";

        println!("=== Before attach ===");
        let entries_before_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before attach: {}", entries_before_attach.len());

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        println!("=== After attach, before write ===");
        let entries_after_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after attach: {}", entries_after_attach.len());

        // Write entry
        facet.before_method("test", b"test").await.unwrap();
        
        println!("=== After before_method ===");
        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after before_method: {}", entries_after_before.len());
        if !entries_after_before.is_empty() {
            println!("First entry sequence: {}", entries_after_before[0].sequence);
        }

        facet.after_method("test", b"test", b"result").await.unwrap();
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

        // Test 1: Direct storage write
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
        assert_eq!(entries_direct.len(), 1, "Should have 1 entry from direct write");

        // Test 2: Facet write with replay_on_activation=true
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id2 = "debug-facet";

        facet.on_attach(actor_id2, serde_json::json!({})).await.unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet.after_method("test", b"test", b"result").await.unwrap();
        storage.flush().await.unwrap();

        let entries_facet = storage.replay_from(actor_id2, 0).await.unwrap();
        println!("Entries from facet write: {}", entries_facet.len());
        assert_eq!(entries_facet.len(), 2, "Should have 2 entries from facet write");
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-actor-id";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet.after_method("test", b"test", b"result").await.unwrap();
        storage.flush().await.unwrap();

        // Check all entries in storage (no actor_id filter)
        let all_entries = storage.replay_from("", 0).await;
        println!("All entries query result: {:?}", all_entries);

        // Check with correct actor_id
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries for actor_id '{}': {}", actor_id, entries.len());
        
        for entry in &entries {
            println!("Entry actor_id: '{}', sequence: {}", entry.actor_id, entry.sequence);
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-query";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Try to write
        facet.before_method("test", b"test").await.unwrap();
        facet.after_method("test", b"test", b"result").await.unwrap();
        storage.flush().await.unwrap();

        // Try different queries
        let entries_0 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries with from_sequence=0: {}", entries_0.len());
        
        let entries_1 = storage.replay_from(actor_id, 1).await.unwrap();
        println!("Entries with from_sequence=1: {}", entries_1.len());
        
        // Check if we can query all entries (no actor_id filter - this might fail)
        // But we can check stats
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
        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-step";

        println!("=== Step 1: on_attach ===");
        let attach_result = facet.on_attach(actor_id, serde_json::json!({})).await;
        println!("Attach result: {:?}", attach_result);
        
        // Check checkpoint
        let checkpoint_result = storage.get_latest_checkpoint(actor_id).await;
        println!("Checkpoint result: {:?}", checkpoint_result);

        // Check existing entries
        let existing_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Existing entries: {}", existing_entries.len());

        println!("=== Step 2: before_method ===");
        let before_result = facet.before_method("test", b"test").await;
        println!("Before result: {:?}", before_result);
        
        // Immediately check entries
        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after before_method: {}", entries_after_before.len());

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

