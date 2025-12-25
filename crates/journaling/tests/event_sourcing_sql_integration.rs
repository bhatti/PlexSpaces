// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// SQL integration tests for event sourcing functionality

#[cfg(feature = "sqlite-backend")]
mod sqlite_tests {
    use plexspaces_journaling::*;
use plexspaces_facet::Facet;
    use plexspaces_proto::common::v1::PageRequest;
    use plexspaces_proto::prost_types;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::SystemTime;

    async fn create_sqlite_storage() -> SqliteJournalStorage {
        // Use in-memory SQLite for tests
        SqliteJournalStorage::new(":memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_sqlite_append_event() {
        let storage = create_sqlite_storage().await;

        let event = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 1,
            event_type: "counter_incremented".to_string(),
            event_data: b"{\"amount\": 5}".to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: "corr-1".to_string(),
            metadata: HashMap::new(),
        };

        let sequence = storage.append_event(&event).await.unwrap();
        assert_eq!(sequence, 1);

        // Verify event was stored
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "counter_incremented");
        assert_eq!(events[0].sequence, 1);
    }

    #[tokio::test]
    async fn test_sqlite_append_events_batch() {
        let storage = create_sqlite_storage().await;

        let events = vec![
            ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: 1,
                event_type: "counter_incremented".to_string(),
                event_data: b"{\"amount\": 5}".to_vec(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: "corr-1".to_string(),
                metadata: HashMap::new(),
            },
            ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: 2,
                event_type: "counter_incremented".to_string(),
                event_data: b"{\"amount\": 10}".to_vec(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: "corr-2".to_string(),
                metadata: HashMap::new(),
            },
        ];

        let (first, last, count) = storage.append_events_batch(&events).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(last, 2);
        assert_eq!(count, 2);

        // Verify events were stored
        let replayed = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(replayed.len(), 2);
    }

    #[tokio::test]
    async fn test_sqlite_replay_events_from() {
        let storage = create_sqlite_storage().await;

        // Append 5 events
        for i in 1..=5 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // Replay from sequence 3
        let events = storage.replay_events_from("actor-1", 3).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 3);
        assert_eq!(events[1].sequence, 4);
        assert_eq!(events[2].sequence, 5);
    }

    #[tokio::test]
    async fn test_sqlite_get_actor_history() {
        let storage = create_sqlite_storage().await;

        // Append 3 events
        for i in 1..=3 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        let history = storage.get_actor_history("actor-1").await.unwrap();
        assert_eq!(history.events.len(), 3);
        assert_eq!(history.latest_sequence, 3);
        assert_eq!(history.actor_id, "actor-1");
    }

    #[tokio::test]
    async fn test_sqlite_replay_events_from_paginated() {
        let storage = create_sqlite_storage().await;

        // Append 10 events
        for i in 1..=10 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // First page
        let page_request = PageRequest {
            offset: 0,
            limit: 3,
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, page_response) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 1);
        assert!(page_response.has_next);

        // Second page
        let page_request2 = PageRequest {
            offset: page_response.offset + page_response.limit,
            limit: 3,
            filter: String::new(),
            order_by: String::new(),
        };

        let (events2, page_response2) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request2)
            .await
            .unwrap();

        assert_eq!(events2.len(), 3);
        assert_eq!(events2[0].sequence, 4);
        assert!(page_response2.has_next);
    }

    #[tokio::test]
    async fn test_sqlite_get_actor_history_paginated() {
        let storage = create_sqlite_storage().await;

        // Append 5 events
        for i in 1..=5 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // First page
        let page_request = PageRequest {
            offset: 0,
            limit: 2,
            filter: String::new(),
            order_by: String::new(),
        };

        let history = storage
            .get_actor_history_paginated("actor-1", &page_request)
            .await
            .unwrap();

        assert_eq!(history.events.len(), 2);
        assert_eq!(history.latest_sequence, 5);
        assert!(history.page_response.is_some());
        assert!(history.page_response.as_ref().unwrap().has_next);

        // Second page
        let page_response = history.page_response.as_ref().unwrap();
        let page_request2 = PageRequest {
            offset: page_response.offset + page_response.limit,
            limit: 2,
            filter: String::new(),
            order_by: String::new(),
        };

        let history2 = storage
            .get_actor_history_paginated("actor-1", &page_request2)
            .await
            .unwrap();

        assert_eq!(history2.events.len(), 2);
        assert_eq!(history2.latest_sequence, 5);
    }

    #[tokio::test]
    async fn test_sqlite_event_sourcing_facet_integration() {
        let storage = Arc::new(create_sqlite_storage().await);
        let config = EventSourcingConfig {
            event_log_enabled: true,
            auto_replay: true,
            snapshot_interval: 100,
            time_travel_enabled: true,
            metadata: HashMap::new(),
            default_page_size: 100,
            max_page_size: 1000,
        };

        // Create facet
        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Process multiple state changes
        for i in 1..=5 {
            facet
                .after_method(
                    "increment",
                    &[],
                    &format!("{{\"new_value\": {}}}", i).into_bytes(),
                )
                .await
                .unwrap();
        }

        // Verify all events were logged
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 5);

        // Verify event sequences are sequential
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.sequence, (i + 1) as u64);
            assert_eq!(event.event_type, "increment");
        }
    }

    #[tokio::test]
    async fn test_sqlite_pagination_large_event_log() {
        let storage = create_sqlite_storage().await;

        // Create 100 events
        for i in 1..=100 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // Test pagination with large event log
        let page_request = PageRequest {
            page_size: 20,
            page_token: String::new(),
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, page_response) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 20);
        assert!(!page_response.next_page_token.is_empty());

        // Verify we can get all events through pagination
        let mut total = 20;
        let mut next_token = page_response.next_page_token;

        loop {
            let page_request = PageRequest {
                page_size: 20,
                page_token: next_token,
                filter: String::new(),
                order_by: String::new(),
            };

            let (events, page_response) = storage
                .replay_events_from_paginated("actor-1", 0, &page_request)
                .await
                .unwrap();

            total += events.len();
            if page_response.next_page_token.is_empty() {
                break;
            }
            next_token = page_response.next_page_token;
        }

        assert_eq!(total, 100);
    }

    #[tokio::test]
    async fn test_sqlite_event_metadata() {
        let storage = create_sqlite_storage().await;

        let mut metadata = HashMap::new();
        metadata.insert("version".to_string(), "1.0".to_string());
        metadata.insert("source".to_string(), "test".to_string());

        let event = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 1,
            event_type: "counter_incremented".to_string(),
            event_data: b"{\"amount\": 5}".to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: "corr-1".to_string(),
            metadata: metadata.clone(),
        };

        storage.append_event(&event).await.unwrap();

        // Verify metadata was stored
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metadata.get("version"), Some(&"1.0".to_string()));
        assert_eq!(events[0].metadata.get("source"), Some(&"test".to_string()));
    }

    #[tokio::test]
    async fn test_sqlite_event_causal_tracking() {
        let storage = create_sqlite_storage().await;

        // Create events with causal tracking
        let correlation_ids = vec!["corr-1", "corr-2", "corr-3"];

        for (i, corr_id) in correlation_ids.iter().enumerate() {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: (i + 1) as u64,
                event_type: "state_changed".to_string(),
                event_data: format!("{{\"change\": {}}}", i + 1).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: corr_id.to_string(),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // Verify causal tracking
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 3);
        assert_eq!(events[0].caused_by, "corr-1");
        assert_eq!(events[1].caused_by, "corr-2");
        assert_eq!(events[2].caused_by, "corr-3");
    }

    #[tokio::test]
    async fn test_sqlite_pagination_from_sequence() {
        let storage = create_sqlite_storage().await;

        // Append 10 events
        for i in 1..=10 {
            let event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };
            storage.append_event(&event).await.unwrap();
        }

        // Replay from sequence 5 with pagination
        let page_request = PageRequest {
            offset: 0,
            limit: 3,
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, _) = storage
            .replay_events_from_paginated("actor-1", 5, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 5);
        assert_eq!(events[1].sequence, 6);
        assert_eq!(events[2].sequence, 7);
    }

    #[tokio::test]
    async fn test_sqlite_empty_event_log() {
        let storage = create_sqlite_storage().await;

        // Test with empty event log
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 0);

        let history = storage.get_actor_history("actor-1").await.unwrap();
        assert_eq!(history.events.len(), 0);
        assert_eq!(history.latest_sequence, 0);
    }

    #[tokio::test]
    async fn test_sqlite_multiple_actors() {
        let storage = create_sqlite_storage().await;

        // Append events for multiple actors
        for actor_id in ["actor-1", "actor-2", "actor-3"] {
            for i in 1..=3 {
                let event = ActorEvent {
                    id: ulid::Ulid::new().to_string(),
                    actor_id: actor_id.to_string(),
                    sequence: i,
                    event_type: "counter_incremented".to_string(),
                    event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                    timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                    caused_by: format!("corr-{}", i),
                    metadata: HashMap::new(),
                };
                storage.append_event(&event).await.unwrap();
            }
        }

        // Verify each actor has separate event log
        for actor_id in ["actor-1", "actor-2", "actor-3"] {
            let events = storage.replay_events_from(actor_id, 0).await.unwrap();
            assert_eq!(events.len(), 3);
            assert_eq!(events[0].actor_id, actor_id);
        }
    }

    #[tokio::test]
    async fn test_sqlite_sequence_auto_increment() {
        let storage = create_sqlite_storage().await;

        // Append events without sequence (should auto-increment)
        for i in 1..=5 {
            let mut event = ActorEvent {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: 0, // Will be auto-assigned
                event_type: "counter_incremented".to_string(),
                event_data: format!("{{\"amount\": {}}}", i).into_bytes(),
                timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
                caused_by: format!("corr-{}", i),
                metadata: HashMap::new(),
            };

            let sequence = storage.append_event(&event).await.unwrap();
            assert_eq!(sequence, i);
        }

        // Verify sequences are sequential
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.sequence, (i + 1) as u64);
        }
    }
}

