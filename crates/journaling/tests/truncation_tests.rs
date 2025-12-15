// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Tests for journal truncation behavior with auto_truncate enabled

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
    use plexspaces_journaling::sql::SqliteJournalStorage;
    use plexspaces_facet::Facet;
    use std::collections::HashMap;

    /// Helper to create a test SQLite storage (in-memory)
    async fn create_test_storage() -> SqliteJournalStorage {
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    use plexspaces_proto::prost_types;
    use std::time::SystemTime;

    /// Test: Truncation preserves checkpointed entry
    /// When checkpoint is created at sequence N, entries <= N-1 are truncated
    #[tokio::test]
    async fn test_truncation_preserves_checkpointed_entry() {
        let storage = create_test_storage().await;
        let actor_id = "test-truncate-preserve";

        let checkpoint_config = CheckpointConfig {
            enabled: true,
            entry_interval: 10u64,
            time_interval: None,
            compression: CompressionType::CompressionTypeNone as i32,
            retention_count: 2,
            auto_truncate: true,
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let checkpoint_manager = CheckpointManager::new(
            storage.clone(),
            checkpoint_config,
            1,
        );

        // Create entries 1-10
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
                            sender_id: "sender".to_string(),
                            message_type: "test".to_string(),
                            payload: b"test".to_vec(),
                            metadata: Default::default(),
                        },
                    ),
                ),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        storage.flush().await.unwrap();

        // Trigger checkpoint at sequence 10
        // This should truncate entries <= 9, keeping entry 10
        let state_data = b"state".to_vec();
        checkpoint_manager.maybe_checkpoint(actor_id, 10, state_data).await.unwrap();

        storage.flush().await.unwrap();

        // Verify: entries 1-9 truncated, entry 10 remains
        let entries = storage.replay_from(actor_id, 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Should have 1 entry (sequence 10)");
        assert_eq!(entries[0].sequence, 10, "Remaining entry should be sequence 10");
    }
}

