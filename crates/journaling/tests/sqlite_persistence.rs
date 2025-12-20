// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// SQLite persistence integration tests - divide and conquer approach
// These tests isolate specific persistence behaviors to identify issues

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

    /// Test 1: Basic append_entry and replay_from
    #[tokio::test]
    async fn test_basic_append_and_replay() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-basic";

        // Create a simple entry
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
                        payload: vec![1, 2, 3],
                        metadata: Default::default(),
                    },
                ),
            ),
        };

        // Append entry
        let sequence = storage.append_entry(&entry).await.unwrap();
        assert_eq!(sequence, 1);

        // Flush to ensure write
        storage.flush().await.unwrap();

        // Replay from sequence 0
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Should have 1 entry");
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[0].actor_id, actor_id);
    }

    /// Test 2: Multiple append_entry calls
    #[tokio::test]
    async fn test_multiple_append_entries() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-multiple";

        // Append 5 entries
        for i in 1..=5 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: vec![i as u8],
                            metadata: Default::default(),
                        },
                    ),
                ),
            };

            let sequence = storage.append_entry(&entry).await.unwrap();
            assert_eq!(sequence, i);
        }

        // Flush
        storage.flush().await.unwrap();

        // Replay all entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 5, "Should have 5 entries");
        
        // Verify sequences are correct
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.sequence, (i + 1) as u64);
        }
    }

    /// Test 3: append_batch with multiple entries
    #[tokio::test]
    async fn test_append_batch() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-batch";

        // Create batch of entries
        let mut entries = Vec::new();
        for i in 1..=3 {
            entries.push(JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: vec![i as u8],
                            metadata: Default::default(),
                        },
                    ),
                ),
            });
        }

        // Append batch
        let (first_seq, last_seq, count) = storage.append_batch(&entries).await.unwrap();
        assert_eq!(first_seq, 1);
        assert_eq!(last_seq, 3);
        assert_eq!(count, 3);

        // Flush
        storage.flush().await.unwrap();

        // Replay all entries
        let replayed = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(replayed.len(), 3, "Should have 3 entries");
    }

    /// Test 4: Replay from specific sequence
    #[tokio::test]
    async fn test_replay_from_sequence() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-sequence";

        // Append 10 entries
        for i in 1..=10 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: vec![i as u8],
                            metadata: Default::default(),
                        },
                    ),
                ),
            };

            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Replay from sequence 5
        let entries = storage.replay_from(actor_id, 5).await.unwrap();
        assert_eq!(entries.len(), 6, "Should have 6 entries (sequences 5-10)");
        assert_eq!(entries[0].sequence, 5);
        assert_eq!(entries[5].sequence, 10);
    }

    /// Test 5: DurabilityFacet before_method writes entry
    #[tokio::test]
    async fn test_facet_before_method_writes() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false, // Disable replay for this test
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "test-actor-facet-before";

        // Attach facet
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Call before_method
        let payload = b"test payload".to_vec();
        facet.before_method("test_method", &payload).await.unwrap();

        // Flush
        storage.flush().await.unwrap();

        // Verify entry was written
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Should have 1 entry after before_method");
        
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        if let Some(Entry::MessageReceived(msg)) = &entries[0].entry {
            assert_eq!(msg.message_type, "test_method");
            assert_eq!(msg.payload, payload);
        } else {
            panic!("Expected MessageReceived entry");
        }
    }

    /// Test 6: DurabilityFacet after_method writes entry
    #[tokio::test]
    async fn test_facet_after_method_writes() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "test-actor-facet-after";

        // Attach facet
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Call before_method first
        facet.before_method("test_method", &[]).await.unwrap();

        // Call after_method
        let result = b"result data".to_vec();
        facet.after_method("test_method", &[], &result).await.unwrap();

        // Flush
        storage.flush().await.unwrap();

        // Verify both entries were written
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 2, "Should have 2 entries (MessageReceived + MessageProcessed)");
        
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        assert!(matches!(entries[0].entry, Some(Entry::MessageReceived(_))));
        assert!(matches!(entries[1].entry, Some(Entry::MessageProcessed(_))));
    }

    /// Test 7: Multiple before/after cycles
    #[tokio::test]
    async fn test_facet_multiple_cycles() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        let actor_id = "test-actor-facet-cycles";

        // Attach facet
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages
        for i in 1..=5 {
            let payload = format!("payload-{}", i).into_bytes();
            facet.before_method("test_method", &payload).await.unwrap();
            let result = format!("result-{}", i).into_bytes();
            facet.after_method("test_method", &payload, &result).await.unwrap();
        }

        // Flush
        storage.flush().await.unwrap();

        // Verify all entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 10, "Should have 10 entries (5 * 2)");
        
        // Verify sequences are monotonic
        for i in 1..entries.len() {
            assert!(entries[i].sequence > entries[i - 1].sequence);
        }
    }

    /// Test 8: on_attach with replay_on_activation=false and existing entries
    #[tokio::test]
    async fn test_facet_attach_no_replay_with_entries() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-attach-no-replay";

        // First, create some entries directly
        for i in 1..=3 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: vec![i as u8],
                            metadata: Default::default(),
                        },
                    ),
                ),
            };

            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Now attach facet (should initialize sequence from existing entries)
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process a new message
        facet.before_method("new_method", &[]).await.unwrap();
        facet.after_method("new_method", &[], &[]).await.unwrap();

        storage.flush().await.unwrap();

        // Verify new entries have correct sequences (4, 5)
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 5, "Should have 5 entries (3 old + 2 new)");
        assert_eq!(entries[3].sequence, 4);
        assert_eq!(entries[4].sequence, 5);
    }

    /// Test 9: on_attach with replay_on_activation=true and existing entries
    #[tokio::test]
    async fn test_facet_attach_with_replay_and_entries() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-attach-replay";

        // First, create some entries directly
        for i in 1..=3 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: vec![i as u8],
                            metadata: Default::default(),
                        },
                    ),
                ),
            };

            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Now attach facet with replay enabled
        let mut facet = DurabilityFacet::new(storage.clone(), config_to_value(&config), 50);
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process a new message
        facet.before_method("new_method", &[]).await.unwrap();
        facet.after_method("new_method", &[], &[]).await.unwrap();

        storage.flush().await.unwrap();

        // Verify new entries have correct sequences (4, 5)
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 5, "Should have 5 entries (3 old + 2 new)");
        assert_eq!(entries[3].sequence, 4);
        assert_eq!(entries[4].sequence, 5);
    }

    /// Test 10: Detach and reattach with existing entries
    #[tokio::test]
    async fn test_facet_detach_reattach() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 0,
            checkpoint_timeout: None,
            replay_on_activation: false,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let actor_id = "test-actor-detach-reattach";

        // First facet: create entries
        let mut facet1 = DurabilityFacet::new(storage.clone(), config.clone());
        facet1.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 3 messages
        for i in 1..=3 {
            let payload = format!("payload-{}", i).into_bytes();
            facet1.before_method("test_method", &payload).await.unwrap();
            facet1.after_method("test_method", &payload, &[]).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Detach
        facet1.on_detach(actor_id).await.unwrap();

        // Verify entries still exist
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 6, "Should have 6 entries after detach");

        // Reattach with new facet
        let mut facet2 = DurabilityFacet::new(storage.clone(), config);
        facet2.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process one more message
        facet2.before_method("new_method", &[]).await.unwrap();
        facet2.after_method("new_method", &[], &[]).await.unwrap();

        storage.flush().await.unwrap();

        // Verify all entries
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(all_entries.len(), 8, "Should have 8 entries (6 old + 2 new)");
        assert_eq!(all_entries[6].sequence, 7);
        assert_eq!(all_entries[7].sequence, 8);
    }

    /// Test 11: Exact scenario from test_replay_missing_checkpoint
    /// This isolates the issue - process 5 messages with replay_on_activation=true
    #[tokio::test]
    async fn test_facet_replay_missing_checkpoint_scenario() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 10,
            checkpoint_timeout: None,
            replay_on_activation: true, // This is the key difference
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-replay-scenario";

        // Attach with replay enabled
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process 5 messages (exactly like the failing test)
        for i in 1..=5 {
            let method = "increment";
            let payload = serde_json::json!({ "value": i }).to_string().into_bytes();
            facet.before_method(method, &payload).await.unwrap();
            let result = format!("counter = {}", i).into_bytes();
            facet.after_method(method, &payload, &result).await.unwrap();
        }

        // Flush to ensure entries are written
        storage.flush().await.unwrap();

        // Verify entries exist (this is where the failing test fails)
        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(
            entries_before.len(),
            10,
            "Should have 10 entries before restart (5 messages * 2 entries), got {}",
            entries_before.len()
        );
    }
}
