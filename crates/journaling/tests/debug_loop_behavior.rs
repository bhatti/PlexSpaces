// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Debug tests for loop behavior - the failing test processes messages in a loop

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;

    /// Helper to create a test SQLite storage (in-memory)
    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-loop";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        println!("=== Processing 5 messages in loop ===");
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            
            println!("Iteration {}: before_method", i);
            facet.before_method(method, &payload).await.unwrap();
            
            // Check entries after before_method
            let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
            println!("  Entries after before_method: {}", entries_after_before.len());
            
            let result = format!("counter = {}", i).into_bytes();
            println!("Iteration {}: after_method", i);
            facet.after_method(method, &payload, &result).await.unwrap();
            
            // Check entries after after_method
            let entries_after_after = storage.replay_from(actor_id, 0).await.unwrap();
            println!("  Entries after after_method: {}", entries_after_after.len());
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-loop-flush";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
            
            // Flush after each message
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

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "debug-batch";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process messages
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            
            facet.before_method(method, &payload).await.unwrap();
            // before_method uses append_entry (single write, auto-commit)
            
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
            // after_method uses append_batch (transaction, needs commit)
        }

        // Check before flush
        let entries_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush: {}", entries_before_flush.len());

        storage.flush().await.unwrap();

        // Check after flush
        let entries_after_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after flush: {}", entries_after_flush.len());
        
        assert_eq!(entries_after_flush.len(), 10, "Should have 10 entries");
    }
}

