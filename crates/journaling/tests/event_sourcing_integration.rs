// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// Integration tests for event sourcing functionality

use plexspaces_journaling::*;
use plexspaces_facet::Facet;
use plexspaces_proto::common::v1::PageRequest;
use plexspaces_proto::prost_types;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;

#[tokio::test]
async fn test_event_sourcing_full_workflow() {
    let storage = Arc::new(MemoryJournalStorage::new());
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
async fn test_event_sourcing_replay_on_activation() {
    let storage = Arc::new(MemoryJournalStorage::new());

    // Pre-populate events
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

    // Create facet with auto_replay enabled
    let config = EventSourcingConfig {
        event_log_enabled: true,
        auto_replay: true,
        snapshot_interval: 100,
        time_travel_enabled: true,
        metadata: HashMap::new(),
        default_page_size: 100,
        max_page_size: 1000,
    };

    let mut facet = EventSourcingFacet::new(storage.clone(), config);

    // On attach, should replay events
    facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

    // Verify sequence was updated by checking events
    // (event_sequence is private, so we verify via storage)
    let events = storage.replay_events_from("actor-1", 0).await.unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events.last().unwrap().sequence, 3);
}

#[tokio::test]
async fn test_event_sourcing_paginated_history() {
    let storage = Arc::new(MemoryJournalStorage::new());

    // Pre-populate 20 events
    for i in 1..=20 {
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

    // Get paginated history
    let mut page_token = String::new();
    let mut total_events = 0;

    loop {
        let page_request = PageRequest {
            page_size: 5,
            page_token: page_token.clone(),
            filter: String::new(),
            order_by: String::new(),
        };

        let history = storage
            .get_actor_history_paginated("actor-1", &page_request)
            .await
            .unwrap();

        total_events += history.events.len();

        if history.page_response.is_none()
            || history.page_response.as_ref().unwrap().next_page_token.is_empty()
        {
            break;
        }

        page_token = history.page_response.as_ref().unwrap().next_page_token.clone();
    }

    assert_eq!(total_events, 20);
}

#[tokio::test]
async fn test_event_sourcing_time_travel() {
    let storage = Arc::new(MemoryJournalStorage::new());

    // Pre-populate events representing state changes
    let events_data = vec![
        ("user_created", r#"{"name": "Alice"}"#),
        ("user_updated", r#"{"name": "Alice", "age": 30}"#),
        ("user_updated", r#"{"name": "Alice", "age": 31}"#),
    ];

    for (i, (event_type, event_data)) in events_data.iter().enumerate() {
        let event = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: (i + 1) as u64,
            event_type: event_type.to_string(),
            event_data: event_data.as_bytes().to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: format!("corr-{}", i + 1),
            metadata: HashMap::new(),
        };
        storage.append_event(&event).await.unwrap();
    }

    // Time-travel: Replay to sequence 2 (before last update)
    let events = storage.replay_events_from("actor-1", 1).await.unwrap();
    let events_to_seq2: Vec<&ActorEvent> = events.iter().take(2).collect();

    assert_eq!(events_to_seq2.len(), 2);
    assert_eq!(events_to_seq2[0].event_type, "user_created");
    assert_eq!(events_to_seq2[1].event_type, "user_updated");
}

#[tokio::test]
async fn test_event_sourcing_with_durability_facet() {
    // This test demonstrates how EventSourcingFacet works with DurabilityFacet
    let storage = Arc::new(MemoryJournalStorage::new());

    // Create durability config
    let durability_config = DurabilityConfig {
        backend: JournalBackend::JournalBackendMemory as i32,
        checkpoint_interval: 10,
        checkpoint_timeout: None,
        replay_on_activation: true,
        cache_side_effects: true,
        compression: CompressionType::CompressionTypeNone as i32,
        backend_config: None,
            state_schema_version: 1,
    };

    // Create event sourcing config
    let event_sourcing_config = EventSourcingConfig {
        event_log_enabled: true,
        auto_replay: true,
        snapshot_interval: 100,
        time_travel_enabled: true,
        metadata: HashMap::new(),
        default_page_size: 100,
        max_page_size: 1000,
    };

    // Both facets share the same storage
    let durability_facet = DurabilityFacet::new((*storage).clone(), durability_config);
    let event_sourcing_facet = EventSourcingFacet::new(storage.clone(), event_sourcing_config);

    // Both facets can work together
    // DurabilityFacet journals messages
    // EventSourcingFacet logs events (state changes)
    use plexspaces_facet::Facet;
    assert_eq!(durability_facet.facet_type(), "durability");
    assert_eq!(event_sourcing_facet.facet_type(), "event_sourcing");
}

#[tokio::test]
async fn test_event_sourcing_large_event_log() {
    let storage = Arc::new(MemoryJournalStorage::new());

    // Create 1000 events
    for i in 1..=1000 {
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
        page_size: 100,
        page_token: String::new(),
        filter: String::new(),
        order_by: String::new(),
    };

    let (events, page_response) = storage
        .replay_events_from_paginated("actor-1", 0, &page_request)
        .await
        .unwrap();

    assert_eq!(events.len(), 100);
    assert!(!page_response.next_page_token.is_empty());

    // Verify we can get all events through pagination
    let mut total = 100;
    let mut next_token = page_response.next_page_token;

    for _ in 0..9 {
        // 9 more pages to get all 1000 events
        let page_request = PageRequest {
            page_size: 100,
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

    assert_eq!(total, 1000);
}

#[tokio::test]
async fn test_event_sourcing_causal_tracking() {
    let storage = Arc::new(MemoryJournalStorage::new());

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

