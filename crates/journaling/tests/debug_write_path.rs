// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Debug tests to trace the exact write path

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;

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
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        
        // Check entries immediately after attach
        let entries_after_attach = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after attach: {}", entries_after_attach.len());

        println!("=== Step 2: before_method ===");
        let before_result = facet.before_method("test", b"test").await;
        println!("before_method returned: {:?}", before_result);
        
        // Check entries immediately after before_method (before flush)
        let entries_after_before = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after before_method (before flush): {}", entries_after_before.len());
        
        if entries_after_before.is_empty() {
            println!("WARNING: No entries found after before_method!");
            // Try to query directly from SQLite to see if entry exists
            // This would require raw SQL access, but let's check if flush helps
        }

        println!("=== Step 3: Flush after before_method ===");
        storage.flush().await.unwrap();
        let entries_after_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after before_method (after flush): {}", entries_after_before_flush.len());

        println!("=== Step 4: after_method ===");
        let after_result = facet.after_method("test", b"test", b"result").await;
        println!("after_method returned: {:?}", after_result);
        
        let entries_after_after = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after after_method (before flush): {}", entries_after_after.len());

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
        
        // Create facet with storage1
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

        let mut facet = DurabilityFacet::new(storage1.clone(), config);
        let actor_id = "debug-instance";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        facet.before_method("test", b"test").await.unwrap();
        facet.after_method("test", b"test", b"result").await.unwrap();
        
        // Use storage1 directly (not cloned)
        storage1.flush().await.unwrap();
        
        // Query using storage1
        let entries1 = storage1.replay_from(actor_id, 0).await.unwrap();
        println!("Entries using storage1: {}", entries1.len());
        
        // Query using cloned storage
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
            checkpoint_interval: 10, // Same as failing test
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "debug-transaction";

        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Write one message
        facet.before_method("test", b"test").await.unwrap();
        
        // Check immediately (before flush)
        let entries_before_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush: {}", entries_before_flush.len());
        
        // Flush
        storage.flush().await.unwrap();
        
        // Check after flush
        let entries_after_flush = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries after flush: {}", entries_after_flush.len());
        
        // Write second part
        facet.after_method("test", b"test", b"result").await.unwrap();
        
        // Check before flush
        let entries_before_flush2 = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Entries before flush2: {}", entries_before_flush2.len());
        
        // Flush again
        storage.flush().await.unwrap();
        
        // Final check
        let final_entries = storage.replay_from(actor_id, 0).await.unwrap();
        println!("Final entries: {}", final_entries.len());
        
        assert_eq!(final_entries.len(), 2, "Should have 2 entries");
    }

    /// Test: Compare checkpoint_interval=10 vs 1000
    #[tokio::test]
    async fn test_checkpoint_interval_effect() {
        let storage1 = create_test_storage().await;
        let storage2 = create_test_storage().await;

        // Test with checkpoint_interval=10 (like failing test)
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

        let mut facet1 = DurabilityFacet::new(storage1.clone(), config1);
        let actor_id1 = "debug-checkpoint-10";

        facet1.on_attach(actor_id1, serde_json::json!({})).await.unwrap();
        facet1.before_method("test", b"test").await.unwrap();
        facet1.after_method("test", b"test", b"result").await.unwrap();
        storage1.flush().await.unwrap();

        let entries1 = storage1.replay_from(actor_id1, 0).await.unwrap();
        println!("Entries with checkpoint_interval=10: {}", entries1.len());

        // Test with checkpoint_interval=1000
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

        let mut facet2 = DurabilityFacet::new(storage2.clone(), config2);
        let actor_id2 = "debug-checkpoint-1000";

        facet2.on_attach(actor_id2, serde_json::json!({})).await.unwrap();
        facet2.before_method("test", b"test").await.unwrap();
        facet2.after_method("test", b"test", b"result").await.unwrap();
        storage2.flush().await.unwrap();

        let entries2 = storage2.replay_from(actor_id2, 0).await.unwrap();
        println!("Entries with checkpoint_interval=1000: {}", entries2.len());

        assert_eq!(entries1.len(), 2, "checkpoint_interval=10 should work");
        assert_eq!(entries2.len(), 2, "checkpoint_interval=1000 should work");
    }
}

