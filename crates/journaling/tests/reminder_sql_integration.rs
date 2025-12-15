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

//! SQL Reminder Integration Tests
//!
//! Comprehensive integration tests for reminder persistence using SQLite.
//! Tests cover all reminder operations: register, unregister, load, update, and query_due.

use plexspaces_journaling::{
    JournalStorage, ReminderRegistration, ReminderState,
    sql::SqliteJournalStorage,
};
use plexspaces_proto::prost_types;
use std::time::{Duration, SystemTime};
use tokio::time::sleep;

/// Helper to create a test SQLite storage (in-memory)
async fn create_test_storage() -> SqliteJournalStorage {
    SqliteJournalStorage::new(":memory:").await.unwrap()
}

/// Helper to create a test reminder registration
fn create_test_reminder_registration(
    actor_id: &str,
    reminder_name: &str,
    interval_secs: i64,
    first_fire_secs: i64,
) -> ReminderRegistration {
    ReminderRegistration {
        actor_id: actor_id.to_string(),
        reminder_name: reminder_name.to_string(),
        interval: Some(prost_types::Duration {
            seconds: interval_secs,
            nanos: 0,
        }),
        first_fire_time: Some(prost_types::Timestamp {
            seconds: first_fire_secs,
            nanos: 0,
        }),
        callback_data: b"test-data".to_vec(),
        persist_across_activations: true,
        max_occurrences: 0, // Infinite
    }
}

#[tokio::test]
async fn test_register_reminder() {
    let storage = create_test_storage().await;
    
    let registration = create_test_reminder_registration("actor1@node1", "reminder1", 60, 1000);
    let reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register reminder
    let result = storage.register_reminder(&reminder_state).await;
    if let Err(e) = &result {
        eprintln!("Error registering reminder: {:?}", e);
    }
    assert!(result.is_ok(), "Failed to register reminder: {:?}", result.err());
    
    // Verify it was persisted by loading it
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].registration.as_ref().unwrap().reminder_name, "reminder1");
    assert_eq!(loaded[0].registration.as_ref().unwrap().actor_id, "actor1@node1");
}

#[tokio::test]
async fn test_unregister_reminder() {
    let storage = create_test_storage().await;
    
    let registration = create_test_reminder_registration("actor1@node1", "reminder1", 60, 1000);
    let reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register reminder
    storage.register_reminder(&reminder_state).await.unwrap();
    
    // Verify it exists
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    
    // Unregister reminder
    let result = storage.unregister_reminder("actor1@node1", "reminder1").await;
    assert!(result.is_ok());
    
    // Verify it was removed
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 0);
}

#[tokio::test]
async fn test_load_reminders() {
    let storage = create_test_storage().await;
    
    // Register multiple reminders for same actor
    for i in 1..=3 {
        let registration = create_test_reminder_registration(
            "actor1@node1",
            &format!("reminder{}", i),
            60,
            1000 + i,
        );
        let reminder_state = ReminderState {
            registration: Some(registration.clone()),
            last_fired: None,
            next_fire_time: registration.first_fire_time.clone(),
            fire_count: 0,
            is_active: true,
        };
        storage.register_reminder(&reminder_state).await.unwrap();
    }
    
    // Load all reminders for actor
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 3);
    
    // Verify all reminders are present
    let names: Vec<String> = loaded.iter()
        .map(|r| r.registration.as_ref().unwrap().reminder_name.clone())
        .collect();
    assert!(names.contains(&"reminder1".to_string()));
    assert!(names.contains(&"reminder2".to_string()));
    assert!(names.contains(&"reminder3".to_string()));
}

#[tokio::test]
async fn test_load_reminders_only_active() {
    let storage = create_test_storage().await;
    
    // Register active reminder
    let registration1 = create_test_reminder_registration("actor1@node1", "active", 60, 1000);
    let reminder_state1 = ReminderState {
        registration: Some(registration1.clone()),
        last_fired: None,
        next_fire_time: registration1.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    storage.register_reminder(&reminder_state1).await.unwrap();
    
    // Register inactive reminder
    let registration2 = create_test_reminder_registration("actor1@node1", "inactive", 60, 1000);
    let reminder_state2 = ReminderState {
        registration: Some(registration2.clone()),
        last_fired: None,
        next_fire_time: registration2.first_fire_time.clone(),
        fire_count: 0,
        is_active: false,
    };
    storage.register_reminder(&reminder_state2).await.unwrap();
    
    // Load reminders - should only return active ones
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].registration.as_ref().unwrap().reminder_name, "active");
}

#[tokio::test]
async fn test_update_reminder() {
    let storage = create_test_storage().await;
    
    let registration = create_test_reminder_registration("actor1@node1", "reminder1", 60, 1000);
    let mut reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register reminder
    storage.register_reminder(&reminder_state).await.unwrap();
    
    // Update reminder state (simulate firing)
    reminder_state.fire_count = 1;
    reminder_state.last_fired = Some(prost_types::Timestamp {
        seconds: 2000,
        nanos: 0,
    });
    reminder_state.next_fire_time = Some(prost_types::Timestamp {
        seconds: 2060,
        nanos: 0,
    });
    
    // Update in storage
    let result = storage.update_reminder(&reminder_state).await;
    assert!(result.is_ok());
    
    // Verify update was persisted
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].fire_count, 1);
    assert!(loaded[0].last_fired.is_some());
    assert_eq!(loaded[0].last_fired.as_ref().unwrap().seconds, 2000);
}

#[tokio::test]
async fn test_query_due_reminders() {
    let storage = create_test_storage().await;
    
    let now = SystemTime::now();
    let now_secs = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    
    // Register reminder that's due (next_fire_time in the past)
    let registration1 = create_test_reminder_registration("actor1@node1", "due1", 60, now_secs - 10);
    let reminder_state1 = ReminderState {
        registration: Some(registration1.clone()),
        last_fired: None,
        next_fire_time: Some(prost_types::Timestamp {
            seconds: now_secs - 10,
            nanos: 0,
        }),
        fire_count: 0,
        is_active: true,
    };
    storage.register_reminder(&reminder_state1).await.unwrap();
    
    // Register reminder that's due (next_fire_time exactly at now)
    let registration2 = create_test_reminder_registration("actor1@node1", "due2", 60, now_secs);
    let reminder_state2 = ReminderState {
        registration: Some(registration2.clone()),
        last_fired: None,
        next_fire_time: Some(prost_types::Timestamp {
            seconds: now_secs,
            nanos: 0,
        }),
        fire_count: 0,
        is_active: true,
    };
    storage.register_reminder(&reminder_state2).await.unwrap();
    
    // Register reminder that's not due (next_fire_time in the future)
    let registration3 = create_test_reminder_registration("actor1@node1", "not_due", 60, now_secs + 100);
    let reminder_state3 = ReminderState {
        registration: Some(registration3.clone()),
        last_fired: None,
        next_fire_time: Some(prost_types::Timestamp {
            seconds: now_secs + 100,
            nanos: 0,
        }),
        fire_count: 0,
        is_active: true,
    };
    storage.register_reminder(&reminder_state3).await.unwrap();
    
    // Query due reminders (before now + 1 second)
    let due = storage.query_due_reminders(now + Duration::from_secs(1)).await.unwrap();
    assert_eq!(due.len(), 2);
    
    let due_names: Vec<String> = due.iter()
        .map(|r| r.registration.as_ref().unwrap().reminder_name.clone())
        .collect();
    assert!(due_names.contains(&"due1".to_string()));
    assert!(due_names.contains(&"due2".to_string()));
    assert!(!due_names.contains(&"not_due".to_string()));
}

#[tokio::test]
async fn test_query_due_reminders_only_active() {
    let storage = create_test_storage().await;
    
    let now = SystemTime::now();
    let now_secs = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    
    // Register active reminder that's due
    let registration1 = create_test_reminder_registration("actor1@node1", "active_due", 60, now_secs - 10);
    let reminder_state1 = ReminderState {
        registration: Some(registration1.clone()),
        last_fired: None,
        next_fire_time: Some(prost_types::Timestamp {
            seconds: now_secs - 10,
            nanos: 0,
        }),
        fire_count: 0,
        is_active: true,
    };
    storage.register_reminder(&reminder_state1).await.unwrap();
    
    // Register inactive reminder that's due
    let registration2 = create_test_reminder_registration("actor1@node1", "inactive_due", 60, now_secs - 10);
    let reminder_state2 = ReminderState {
        registration: Some(registration2.clone()),
        last_fired: None,
        next_fire_time: Some(prost_types::Timestamp {
            seconds: now_secs - 10,
            nanos: 0,
        }),
        fire_count: 0,
        is_active: false,
    };
    storage.register_reminder(&reminder_state2).await.unwrap();
    
    // Query due reminders - should only return active ones
    let due = storage.query_due_reminders(now + Duration::from_secs(1)).await.unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].registration.as_ref().unwrap().reminder_name, "active_due");
}

#[tokio::test]
async fn test_reminder_persistence_across_operations() {
    let storage = create_test_storage().await;
    
    let registration = create_test_reminder_registration("actor1@node1", "persistent", 60, 1000);
    let mut reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register
    storage.register_reminder(&reminder_state).await.unwrap();
    
    // Update (simulate firing)
    reminder_state.fire_count = 1;
    reminder_state.last_fired = Some(prost_types::Timestamp {
        seconds: 2000,
        nanos: 0,
    });
    reminder_state.next_fire_time = Some(prost_types::Timestamp {
        seconds: 2060,
        nanos: 0,
    });
    storage.update_reminder(&reminder_state).await.unwrap();
    
    // Verify persistence
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].fire_count, 1);
    
    // Update again
    reminder_state.fire_count = 2;
    reminder_state.last_fired = Some(prost_types::Timestamp {
        seconds: 3000,
        nanos: 0,
    });
    reminder_state.next_fire_time = Some(prost_types::Timestamp {
        seconds: 3060,
        nanos: 0,
    });
    storage.update_reminder(&reminder_state).await.unwrap();
    
    // Verify persistence
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].fire_count, 2);
}

#[tokio::test]
async fn test_multiple_actors_reminders() {
    let storage = create_test_storage().await;
    
    // Register reminders for different actors
    for actor_num in 1..=3 {
        let actor_id = format!("actor{}@node1", actor_num);
        for reminder_num in 1..=2 {
            let registration = create_test_reminder_registration(
                &actor_id,
                &format!("reminder{}", reminder_num),
                60,
                1000,
            );
            let reminder_state = ReminderState {
                registration: Some(registration.clone()),
                last_fired: None,
                next_fire_time: registration.first_fire_time.clone(),
                fire_count: 0,
                is_active: true,
            };
            storage.register_reminder(&reminder_state).await.unwrap();
        }
    }
    
    // Verify each actor has their own reminders
    for actor_num in 1..=3 {
        let actor_id = format!("actor{}@node1", actor_num);
        let loaded = storage.load_reminders(&actor_id).await.unwrap();
        assert_eq!(loaded.len(), 2);
    }
}

#[tokio::test]
async fn test_reminder_upsert_on_register() {
    let storage = create_test_storage().await;
    
    let registration = create_test_reminder_registration("actor1@node1", "reminder1", 60, 1000);
    let reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register first time
    storage.register_reminder(&reminder_state).await.unwrap();
    
    // Register again (should upsert, not fail)
    let mut updated_state = reminder_state.clone();
    updated_state.fire_count = 5;
    storage.register_reminder(&updated_state).await.unwrap();
    
    // Verify it was updated (not duplicated)
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].fire_count, 5);
}

#[tokio::test]
async fn test_query_due_reminders_ordering() {
    let storage = create_test_storage().await;
    
    let now = SystemTime::now();
    let now_secs = now.duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    
    // Register reminders with different fire times
    let fire_times = vec![
        (now_secs - 30, "reminder3"), // Earliest
        (now_secs - 20, "reminder2"),
        (now_secs - 10, "reminder1"), // Latest
    ];
    
    for (fire_secs, name) in fire_times {
        let registration = create_test_reminder_registration("actor1@node1", name, 60, fire_secs);
        let reminder_state = ReminderState {
            registration: Some(registration.clone()),
            last_fired: None,
            next_fire_time: Some(prost_types::Timestamp {
                seconds: fire_secs,
                nanos: 0,
            }),
            fire_count: 0,
            is_active: true,
        };
        storage.register_reminder(&reminder_state).await.unwrap();
    }
    
    // Query due reminders - should be ordered by next_fire_time ASC
    let due = storage.query_due_reminders(now + Duration::from_secs(1)).await.unwrap();
    assert_eq!(due.len(), 3);
    
    // Verify ordering (earliest first)
    assert_eq!(due[0].registration.as_ref().unwrap().reminder_name, "reminder3");
    assert_eq!(due[1].registration.as_ref().unwrap().reminder_name, "reminder2");
    assert_eq!(due[2].registration.as_ref().unwrap().reminder_name, "reminder1");
}

#[tokio::test]
async fn test_reminder_with_nanos_precision() {
    let storage = create_test_storage().await;
    
    let registration = ReminderRegistration {
        actor_id: "actor1@node1".to_string(),
        reminder_name: "nanos_reminder".to_string(),
        interval: Some(prost_types::Duration {
            seconds: 60,
            nanos: 500_000_000, // 0.5 seconds
        }),
        first_fire_time: Some(prost_types::Timestamp {
            seconds: 1000,
            nanos: 250_000_000, // 0.25 seconds
        }),
        callback_data: b"test".to_vec(),
        persist_across_activations: true,
        max_occurrences: 0,
    };
    
    let reminder_state = ReminderState {
        registration: Some(registration.clone()),
        last_fired: None,
        next_fire_time: registration.first_fire_time.clone(),
        fire_count: 0,
        is_active: true,
    };
    
    // Register
    storage.register_reminder(&reminder_state).await.unwrap();
    
    // Load and verify nanos are preserved
    let loaded = storage.load_reminders("actor1@node1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].registration.as_ref().unwrap().interval.as_ref().unwrap().nanos, 500_000_000);
    assert_eq!(loaded[0].registration.as_ref().unwrap().first_fire_time.as_ref().unwrap().nanos, 250_000_000);
    assert_eq!(loaded[0].next_fire_time.as_ref().unwrap().nanos, 250_000_000);
}

