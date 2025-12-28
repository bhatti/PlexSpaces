// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Lesser General Public License as published by
// the Free Software Foundation, either version 2.1 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Integration tests for DynamoDB JournalStorage backend.
//!
//! ## TDD Approach
//! These tests are written FIRST before implementation (RED phase).

#[cfg(feature = "ddb-backend")]
mod ddb_tests {
    use plexspaces_journaling::*;
    use plexspaces_proto::journaling::v1::{
        journal_entry::Entry, Checkpoint, JournalEntry, MessageReceived,
    };
    use prost_types;
    use std::time::SystemTime;

    /// Helper to create DynamoDB storage for testing
    async fn create_ddb_storage() -> DynamoDBJournalStorage {
        let endpoint = std::env::var("DYNAMODB_ENDPOINT_URL")
            .or_else(|_| std::env::var("PLEXSPACES_DDB_ENDPOINT_URL"))
            .unwrap_or_else(|_| "http://localhost:8000".to_string());
        
        DynamoDBJournalStorage::new(
            "us-east-1".to_string(),
            "plexspaces-journaling-test".to_string(),
            Some(endpoint),
        )
        .await
        .expect("Failed to create DynamoDB journal storage")
    }

    /// Helper to create unique actor ID for test isolation
    fn unique_actor_id(prefix: &str) -> String {
        use ulid::Ulid;
        format!("{}-{}", prefix, Ulid::new())
    }

    // =========================================================================
    // Journal Entry Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_append_entry() {
        let storage = create_ddb_storage().await;

        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: unique_actor_id("actor"),
            sequence: 1,
            entry: Some(Entry::MessageReceived(MessageReceived {
                message_id: "msg-1".to_string(),
                sender_id: "sender-1".to_string(),
                payload: b"test".to_vec(),
                ..Default::default()
            })),
            timestamp: Some(prost_types::Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            correlation_id: String::new(),
        };

        let seq = storage.append_entry(&entry).await.unwrap();
        assert_eq!(seq, 1);
    }

    #[tokio::test]
    async fn test_ddb_append_batch() {
        let storage = create_ddb_storage().await;

        let entries = vec![
            JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: unique_actor_id("actor"),
                sequence: 1,
                entry: Some(Entry::MessageReceived(MessageReceived {
                    message_id: "msg-1".to_string(),
                    sender_id: "sender-1".to_string(),
                    payload: b"test1".to_vec(),
                    ..Default::default()
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            },
            JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: unique_actor_id("actor"),
                sequence: 2,
                entry: Some(Entry::MessageReceived(MessageReceived {
                    message_id: "msg-2".to_string(),
                    sender_id: "sender-1".to_string(),
                    payload: b"test2".to_vec(),
                    ..Default::default()
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1001,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            },
        ];

        let (first, last, count) = storage.append_batch(&entries).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(last, 2);
        assert_eq!(count, 2);
    }

    #[tokio::test]
    async fn test_ddb_replay_from() {
        let storage = create_ddb_storage().await;
        let actor_id = unique_actor_id("actor");

        // Append entries
        let entry1 = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence: 1,
            entry: Some(Entry::MessageReceived(MessageReceived {
                message_id: "msg-1".to_string(),
                sender_id: "sender-1".to_string(),
                payload: b"test1".to_vec(),
                ..Default::default()
            })),
            timestamp: Some(prost_types::Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            correlation_id: String::new(),
        };

        let entry2 = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence: 2,
            entry: Some(Entry::MessageReceived(MessageReceived {
                message_id: "msg-2".to_string(),
                sender_id: "sender-1".to_string(),
                payload: b"test2".to_vec(),
                ..Default::default()
            })),
            timestamp: Some(prost_types::Timestamp {
                seconds: 1001,
                nanos: 0,
            }),
            correlation_id: String::new(),
        };

        storage.append_entry(&entry1).await.unwrap();
        storage.append_entry(&entry2).await.unwrap();

        // Replay from sequence 1
        let replayed = storage.replay_from(&actor_id, 1).await.unwrap();
        assert_eq!(replayed.len(), 2);
        assert_eq!(replayed[0].sequence, 1);
        assert_eq!(replayed[1].sequence, 2);

        // Replay from sequence 2
        let replayed = storage.replay_from(&actor_id, 2).await.unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].sequence, 2);
    }

    // =========================================================================
    // Checkpoint Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_save_and_get_checkpoint() {
        let storage = create_ddb_storage().await;

        let checkpoint = Checkpoint {
            actor_id: unique_actor_id("actor"),
            sequence: 100,
            state_data: b"state".to_vec(),
            timestamp: Some(prost_types::Timestamp {
                seconds: 2000,
                nanos: 0,
            }),
            compression: 0, // CompressionType::None
            metadata: std::collections::HashMap::new(),
            state_schema_version: 1,
        };

        let actor_id = checkpoint.actor_id.clone();
        storage.save_checkpoint(&checkpoint).await.unwrap();
        let retrieved = storage.get_latest_checkpoint(&actor_id).await.unwrap();
        assert_eq!(retrieved.actor_id, actor_id);
        assert_eq!(retrieved.sequence, 100);
        assert_eq!(retrieved.state_data, b"state");
    }

    #[tokio::test]
    async fn test_ddb_get_checkpoint_not_found() {
        let storage = create_ddb_storage().await;

        let result = storage.get_latest_checkpoint("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_ddb_truncate_to() {
        let storage = create_ddb_storage().await;

        // Append entries
        for i in 1..=10 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: unique_actor_id("actor"),
                sequence: i,
                entry: Some(Entry::MessageReceived(MessageReceived {
                    message_id: format!("msg-{}", i),
                    sender_id: "sender-1".to_string(),
                    payload: format!("test{}", i).into_bytes(),
                    ..Default::default()
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000 + i as i64,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Truncate to sequence 5
        let actor_id = unique_actor_id("actor");
        // Create entries first
        for i in 1..=10 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.clone(),
                sequence: i,
                entry: Some(Entry::StateChanged(StateChanged {
                    old_state: format!("old{}", i).into_bytes(),
                    new_state: format!("new{}", i).into_bytes(),
                    change_type: format!("change{}", i),
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000 + i as i64,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            };
            storage.append_entry(&entry).await.unwrap();
        }
        let deleted = storage.truncate_to(&actor_id, 5).await.unwrap();
        assert_eq!(deleted, 5);

        // Replay should only return entries after sequence 5
        let replayed = storage.replay_from(&actor_id, 1).await.unwrap();
        assert_eq!(replayed.len(), 5);
        assert_eq!(replayed[0].sequence, 6);
    }

    // =========================================================================
    // Event Sourcing Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_append_event() {
        let storage = create_ddb_storage().await;

        use plexspaces_proto::journaling::v1::StateChanged;
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: unique_actor_id("actor"),
            sequence: 1,
            entry: Some(Entry::StateChanged(StateChanged {
                old_state: b"old".to_vec(),
                new_state: b"new".to_vec(),
                change_type: "state_changed".to_string(),
            })),
            timestamp: Some(prost_types::Timestamp {
                seconds: 1000,
                nanos: 0,
            }),
            correlation_id: String::new(),
        };

        let seq = storage.append_entry(&entry).await.unwrap();
        assert_eq!(seq, 1);
    }

    #[tokio::test]
    async fn test_ddb_replay_events_from() {
        let storage = create_ddb_storage().await;
        let actor_id = unique_actor_id("actor");

        use plexspaces_proto::journaling::v1::StateChanged;
        // Append entries
        for i in 1..=5 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.clone(),
                sequence: i,
                entry: Some(Entry::StateChanged(StateChanged {
                    old_state: format!("old{}", i).into_bytes(),
                    new_state: format!("new{}", i).into_bytes(),
                    change_type: format!("change{}", i),
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000 + i as i64,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Replay from sequence 3
        let entries = storage.replay_from(&actor_id, 3).await.unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].sequence, 3);
    }

    // =========================================================================
    // Stats Tests
    // =========================================================================

    #[tokio::test]
    async fn test_ddb_get_stats() {
        let storage = create_ddb_storage().await;
        let actor_id = unique_actor_id("actor");

        // Append some entries
        for i in 1..=5 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: actor_id.clone(),
                sequence: i,
                entry: Some(Entry::MessageReceived(MessageReceived {
                    message_id: format!("msg-{}", i),
                    sender_id: "sender-1".to_string(),
                    payload: b"test".to_vec(),
                    ..Default::default()
                })),
                timestamp: Some(prost_types::Timestamp {
                    seconds: 1000 + i as i64,
                    nanos: 0,
                }),
                correlation_id: String::new(),
            };
            storage.append_entry(&entry).await.unwrap();
        }
        let stats = storage.get_stats(Some(&actor_id)).await.unwrap();
        assert!(stats.total_entries >= 5);
    }

    #[tokio::test]
    async fn test_ddb_flush() {
        let storage = create_ddb_storage().await;

        // Flush should succeed (may be no-op for some backends)
        storage.flush().await.unwrap();
    }
}

