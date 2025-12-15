// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Debug tests for detach/reattach scenarios that are failing

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

    /// Test: Exact scenario from test_replay_missing_checkpoint
    /// This is the failing test - let's debug it step by step
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
        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-4";

        println!("Attaching facet...");
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

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
            println!("  Entry {}: sequence={}, actor_id={}", i, entry.sequence, entry.actor_id);
        }
        
        assert_eq!(
            entries_before.len(),
            10,
            "Should have 10 entries before restart (5 messages * 2 entries), got {}",
            entries_before.len()
        );
        
        println!("=== Phase 3: Detach ===");
        facet.on_detach(actor_id).await.unwrap();
        
        // Verify entries still exist after detach
        let entries_after_detach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after detach: {}", entries_after_detach.len());
        assert_eq!(entries_after_detach.len(), 10, "Entries should persist after detach");

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
        let mut new_facet = DurabilityFacet::new(storage.clone(), restart_config);
        
        println!("Reattaching facet...");
        new_facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        println!("=== Phase 5: Verify entries after reattach ===");
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after reattach: {}", entries_after.len());
        
        for (i, entry) in entries_after.iter().enumerate() {
            println!("  Entry {}: sequence={}, actor_id={}", i, entry.sequence, entry.actor_id);
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

        // Write to storage1
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

        // Read from storage2 (should see the entry since they share the pool)
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

        // Phase 1: Write entries
        let mut facet1 = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-seq";

        facet1.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        
        for i in 1..=3 {
            facet1.before_method("test", &format!("{}", i).into_bytes()).await.unwrap();
            facet1.after_method("test", &[], &format!("result-{}", i).into_bytes()).await.unwrap();
        }
        
        storage.flush().await.unwrap();
        
        let entries1 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after first write: {}", entries1.len());
        assert_eq!(entries1.len(), 6, "Should have 6 entries");

        // Phase 2: Detach and reattach
        facet1.on_detach(actor_id).await.unwrap();
        
        let mut facet2 = DurabilityFacet::new(storage.clone(), config);
        facet2.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Phase 3: Write new entry and verify sequence
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
            
            let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
            facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();
            
            facet.before_method("test", &format!("cycle-{}", cycle).into_bytes()).await.unwrap();
            facet.after_method("test", &[], &format!("result-{}", cycle).into_bytes()).await.unwrap();
            
            storage.flush().await.unwrap();
            
            let entries = storage.replay_from(actor_id, 0).await.unwrap();
            println!("Entries after cycle {}: {}", cycle, entries.len());
            
            facet.on_detach(actor_id).await.unwrap();
        }

        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());
        assert_eq!(final_entries.len(), 6, "Should have 6 entries (3 cycles * 2 entries)");
    }
}

