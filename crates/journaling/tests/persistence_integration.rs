// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for journal persistence with SQLite
// These tests isolate specific persistence issues to help debug failures

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

    /// Test 1: Write a single entry and read it back immediately
    #[tokio::test]
    async fn test_single_entry_write_read() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-single";

        // Create a simple journal entry
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
                        payload: b"test payload".to_vec(),
                        metadata: Default::default(),
                    },
                ),
            ),
        };

        // Write entry
        let sequence = storage.append_entry(&entry).await.unwrap();
        assert_eq!(sequence, 1);

        // Flush to ensure write is visible
        storage.flush().await.unwrap();

        // Read back immediately
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Should have 1 entry");
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[0].actor_id, actor_id);
    }

    /// Test 2: Write multiple entries and read them back
    #[tokio::test]
    async fn test_multiple_entries_write_read() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-multiple";

        // Write 5 entries
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
                            payload: format!("payload-{}", i).into_bytes(),
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

        // Read back all entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 5, "Should have 5 entries");
        
        // Verify sequences are correct
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(entry.sequence, (i + 1) as u64);
        }
    }

    /// Test 3: Write entries using batch append
    #[tokio::test]
    async fn test_batch_append_persistence() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-batch";

        // Create batch of entries
        let mut batch = Vec::new();
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
                            payload: format!("payload-{}", i).into_bytes(),
                            metadata: Default::default(),
                        },
                    ),
                ),
            };
            batch.push(entry);
        }

        // Write batch
        let (first_seq, last_seq, count) = storage.append_batch(&batch).await.unwrap();
        assert_eq!(first_seq, 1);
        assert_eq!(last_seq, 3);
        assert_eq!(count, 3);

        // Flush
        storage.flush().await.unwrap();

        // Read back
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 3, "Should have 3 entries");
    }

    /// Test 4: Write entries, create new storage instance, verify persistence
    /// This tests that in-memory database is shared correctly
    #[tokio::test]
    async fn test_persistence_across_storage_instances() {
        let storage1 = create_test_storage().await;
        let actor_id = "test-actor-shared";

        // Write entry using first storage instance
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

        // Clone storage (should share same pool)
        let storage2 = storage1.clone();

        // Read using second storage instance
        let entries = storage2.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Should have 1 entry when reading from cloned storage");
    }

    /// Test 5: Test sequence number assignment when entry.sequence = 0
    #[tokio::test]
    async fn test_auto_sequence_assignment() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-auto-seq";

        // Write entry with sequence = 0 (should be auto-assigned)
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence: 0, // Should be auto-assigned
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

        let sequence = storage.append_entry(&entry).await.unwrap();
        assert_eq!(sequence, 1, "First entry should get sequence 1");

        storage.flush().await.unwrap();

        // Write another entry with sequence = 0
        let entry2 = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence: 0,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                    MessageReceived {
                        message_id: "msg-2".to_string(),
                        sender_id: "sender-1".to_string(),
                        message_type: "test".to_string(),
                        payload: b"test2".to_vec(),
                        metadata: Default::default(),
                    },
                ),
            ),
        };

        let sequence2 = storage.append_entry(&entry2).await.unwrap();
        assert_eq!(sequence2, 2, "Second entry should get sequence 2");

        storage.flush().await.unwrap();

        // Verify both entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 2, "Should have 2 entries");
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);
    }

    /// Test 6: Test DurabilityFacet write and read cycle
    #[tokio::test]
    async fn test_durability_facet_write_read() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000, // No checkpointing
            checkpoint_timeout: None,
            replay_on_activation: false, // Disable replay for this test
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet = DurabilityFacet::new(storage.clone(), config);
        let actor_id = "test-actor-facet";

        // Attach facet
        facet.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Process one message
        let method = "test_method";
        let payload = b"test payload".to_vec();
        facet.before_method(method, &payload).await.unwrap();
        let result = b"result".to_vec();
        facet.after_method(method, &payload, &result).await.unwrap();

        // Flush
        storage.flush().await.unwrap();

        // Read back entries
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 2, "Should have 2 entries (MessageReceived + MessageProcessed)");
        
        // Verify first entry is MessageReceived
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        assert!(matches!(entries[0].entry, Some(Entry::MessageReceived(_))));
        assert!(matches!(entries[1].entry, Some(Entry::MessageProcessed(_))));
    }

    /// Test 7: Test DurabilityFacet with replay_on_activation = true
    #[tokio::test]
    async fn test_durability_facet_replay_activation() {
        let storage = create_test_storage().await;
        let config = DurabilityConfig {
            backend: JournalBackend::JournalBackendSqlite as i32,
            checkpoint_interval: 1000,
            checkpoint_timeout: None,
            replay_on_activation: true, // Enable replay
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            backend_config: None,
            state_schema_version: 1,
        };

        let mut facet1 = DurabilityFacet::new(storage.clone(), config.clone());
        let actor_id = "test-actor-replay";

        // First attach and write entries
        facet1.on_attach(actor_id, serde_json::json!({})).await.unwrap();
        
        // Process 2 messages
        for i in 1..=2 {
            let method = "test_method";
            let payload = format!("payload-{}", i).into_bytes();
            facet1.before_method(method, &payload).await.unwrap();
            let result = format!("result-{}", i).into_bytes();
            facet1.after_method(method, &payload, &result).await.unwrap();
        }

        // Flush
        storage.flush().await.unwrap();

        // Verify entries were written
        let entries_before = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries_before.len(), 4, "Should have 4 entries (2 messages * 2 entries each)");

        // Detach
        facet1.on_detach(actor_id).await.unwrap();

        // Create new facet with replay enabled
        let mut facet2 = DurabilityFacet::new(storage.clone(), config);
        
        // Attach should trigger replay
        facet2.on_attach(actor_id, serde_json::json!({})).await.unwrap();

        // Verify entries still exist after replay
        let entries_after = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries_after.len(), 4, "Should still have 4 entries after replay");

        // Verify sequence number was restored by writing a new entry
        // If sequence was restored correctly, new entry should have sequence 5
        facet2.before_method("test_method", b"new").await.unwrap();
        storage.flush().await.unwrap();
        let entries_after_new = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries_after_new.len(), 5, "Should have 5 entries after writing new one");
        assert_eq!(entries_after_new[4].sequence, 5, "New entry should have sequence 5");
    }

    /// Test 8: Test checkpoint persistence
    #[tokio::test]
    async fn test_checkpoint_persistence() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-checkpoint";

        // Create a checkpoint
        let checkpoint = Checkpoint {
            actor_id: actor_id.to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            state_data: b"state data".to_vec(),
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: Default::default(),
            state_schema_version: 1,
        };

        storage.save_checkpoint(&checkpoint).await.unwrap();
        storage.flush().await.unwrap();

        // Read back checkpoint
        let retrieved = storage.get_latest_checkpoint(actor_id).await.unwrap();
        assert_eq!(retrieved.sequence, 10);
        assert_eq!(retrieved.actor_id, actor_id);
        assert_eq!(retrieved.state_data, b"state data");
    }

    /// Test 9: Test replay_from with different sequence numbers
    #[tokio::test]
    async fn test_replay_from_sequence() {
        let storage = create_test_storage().await;
        let actor_id = "test-actor-replay-seq";

        // Write 5 entries
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
                            payload: format!("payload-{}", i).into_bytes(),
                            metadata: Default::default(),
                        },
                    ),
                ),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Test replay_from(0) - should get all entries
        let all_entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(all_entries.len(), 5);

        // Test replay_from(3) - should get entries 3, 4, 5
        let entries_from_3 = storage.replay_from(actor_id, 3).await.unwrap();
        assert_eq!(entries_from_3.len(), 3);
        assert_eq!(entries_from_3[0].sequence, 3);
        assert_eq!(entries_from_3[1].sequence, 4);
        assert_eq!(entries_from_3[2].sequence, 5);

        // Test replay_from(6) - should get no entries
        let entries_from_6 = storage.replay_from(actor_id, 6).await.unwrap();
        assert_eq!(entries_from_6.len(), 0);
    }

    /// Test 10: Test multiple actors with separate journals
    #[tokio::test]
    async fn test_multiple_actors_isolation() {
        let storage = create_test_storage().await;
        let actor1_id = "actor-1";
        let actor2_id = "actor-2";

        // Write entries for actor 1
        for i in 1..=3 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor1_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-1".to_string(),
                            message_type: "test".to_string(),
                            payload: b"payload".to_vec(),
                            metadata: Default::default(),
                        },
                    ),
                ),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Write entries for actor 2
        for i in 1..=2 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor2_id.to_string(),
                sequence: i,
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                correlation_id: String::new(),
                entry: Some(
                    plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                        MessageReceived {
                            message_id: format!("msg-{}", i),
                            sender_id: "sender-2".to_string(),
                            message_type: "test".to_string(),
                            payload: b"payload".to_vec(),
                            metadata: Default::default(),
                        },
                    ),
                ),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Verify isolation
        let actor1_entries = storage.replay_from(actor1_id, 0).await.unwrap();
        assert_eq!(actor1_entries.len(), 3, "Actor 1 should have 3 entries");

        let actor2_entries = storage.replay_from(actor2_id, 0).await.unwrap();
        assert_eq!(actor2_entries.len(), 2, "Actor 2 should have 2 entries");
    }
}

