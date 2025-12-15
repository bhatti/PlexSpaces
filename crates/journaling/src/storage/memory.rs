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

//! In-memory journal storage
//!
//! ## Purpose
//! Provides a HashMap-based journal storage for testing and development.
//! **NOT for production use** - data is not persisted across restarts.
//!
//! ## Design
//! - HashMap of actor_id -> Vec<JournalEntry> sorted by sequence
//! - HashMap of actor_id -> Checkpoint (latest only)
//! - Thread-safe using RwLock
//! - No durability (in-memory only)
//!
//! ## Use Cases
//! - Unit tests for actor behaviors
//! - Integration tests for journaling logic
//! - Local development without database setup
//! - Benchmarking journal overhead without I/O

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{Checkpoint, JournalEntry, JournalError, JournalResult, JournalStats, JournalStorage, ActorEvent, ActorHistory};
use crate::storage::{ReminderState, ReminderRegistration};
use plexspaces_proto::prost_types;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use std::time::SystemTime;

/// In-memory journal storage (testing only)
///
/// ## Purpose
/// HashMap-based journal for unit/integration tests. Data is lost on restart.
///
/// ## Design Notes
/// - Entries stored in Vec, sorted by sequence
/// - One checkpoint per actor (latest only)
/// - RwLock for thread-safe concurrent access
/// - No persistence, no compression
///
/// ## Example
/// ```rust
/// use plexspaces_journaling::*;
///
/// # async fn example() -> JournalResult<()> {
/// let storage = MemoryJournalStorage::new();
///
/// // Append entry
/// let entry = JournalEntry {
///     id: ulid::Ulid::new().to_string(),
///     actor_id: "actor-123".to_string(),
///     sequence: 1,
///     timestamp: None,
///     correlation_id: String::new(),
///     entry: None,
/// };
/// storage.append_entry(&entry).await?;
///
/// // Replay
/// let entries = storage.replay_from("actor-123", 0).await?;
/// assert_eq!(entries.len(), 1);
/// # Ok(())
/// # }
/// ```
#[derive(Default, Clone)]
pub struct MemoryJournalStorage {
    /// Journal entries by actor_id
    ///
    /// ## Structure
    /// actor_id -> Vec<JournalEntry> sorted by sequence
    ///
    /// ## Design Notes
    /// - Vec maintains insertion order (sequence order)
    /// - Lookups are O(log n) binary search on sequence
    /// - Replay is O(n) scan from sequence
    entries: Arc<RwLock<HashMap<String, Vec<JournalEntry>>>>,

    /// Checkpoints by actor_id (latest only)
    ///
    /// ## Structure
    /// actor_id -> Checkpoint (most recent)
    ///
    /// ## Design Notes
    /// - Only one checkpoint per actor (latest)
    /// - Older checkpoints are discarded
    /// - Lookup is O(1) HashMap access
    checkpoints: Arc<RwLock<HashMap<String, Checkpoint>>>,

    /// Sequence counters by actor_id
    ///
    /// ## Design Notes
    /// - Tracks next sequence number for each actor
    /// - Starts at 1 (not 0)
    /// - Monotonically increasing per actor
    sequences: Arc<RwLock<HashMap<String, u64>>>,

    /// Event log by actor_id (for event sourcing)
    ///
    /// ## Structure
    /// actor_id -> Vec<ActorEvent> sorted by sequence
    ///
    /// ## Design Notes
    /// - Events are separate from journal entries
    /// - Events represent state changes (not messages)
    /// - Vec maintains insertion order (sequence order)
    events: Arc<RwLock<HashMap<String, Vec<ActorEvent>>>>,

    /// Event sequence counters by actor_id
    ///
    /// ## Design Notes
    /// - Separate from journal entry sequences
    /// - Tracks next event sequence number for each actor
    /// - Starts at 1 (not 0)
    event_sequences: Arc<RwLock<HashMap<String, u64>>>,

    /// Reminders by actor_id
    ///
    /// ## Structure
    /// actor_id -> HashMap<reminder_name, ReminderState>
    ///
    /// ## Design Notes
    /// - Reminders are persisted to survive actor deactivation
    /// - HashMap allows O(1) lookup by reminder name
    /// - Only active reminders are stored
    reminders: Arc<RwLock<HashMap<String, HashMap<String, ReminderState>>>>,
}

impl MemoryJournalStorage {
    /// Create a new in-memory journal storage
    ///
    /// ## Returns
    /// New instance with empty HashMaps
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_journaling::*;
    ///
    /// let storage = MemoryJournalStorage::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Get next sequence number for actor
    async fn next_sequence(&self, actor_id: &str) -> u64 {
        let mut sequences = self.sequences.write().await;
        let seq = sequences.entry(actor_id.to_string()).or_insert(1);
        let current = *seq;
        *seq += 1;
        current
    }

    /// Get next event sequence number for actor
    async fn next_event_sequence(&self, actor_id: &str) -> u64 {
        let mut sequences = self.event_sequences.write().await;
        let seq = sequences.entry(actor_id.to_string()).or_insert(1);
        let current = *seq;
        *seq += 1;
        current
    }
}

#[async_trait]
impl JournalStorage for MemoryJournalStorage {
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64> {
        let mut entries = self.entries.write().await;

        // Get or create entry vec for this actor
        let actor_entries = entries
            .entry(entry.actor_id.clone())
            .or_insert_with(Vec::new);

        // Assign sequence if not set
        let mut entry = entry.clone();
        if entry.sequence == 0 {
            entry.sequence = self.next_sequence(&entry.actor_id).await;
        }

        let sequence = entry.sequence;

        // Append entry (maintains sequence order if appended in order)
        actor_entries.push(entry);

        Ok(sequence)
    }

    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)> {
        if entries.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut all_entries = self.entries.write().await;

        let first_sequence = entries[0].sequence;
        let last_sequence = entries[entries.len() - 1].sequence;
        let count = entries.len();

        for entry in entries {
            let actor_entries = all_entries
                .entry(entry.actor_id.clone())
                .or_insert_with(Vec::new);

            let mut entry = entry.clone();
            if entry.sequence == 0 {
                entry.sequence = self.next_sequence(&entry.actor_id).await;
            }

            actor_entries.push(entry);
        }

        Ok((first_sequence, last_sequence, count))
    }

    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>> {
        let entries = self.entries.read().await;

        if let Some(actor_entries) = entries.get(actor_id) {
            // Filter entries >= from_sequence
            let filtered: Vec<JournalEntry> = actor_entries
                .iter()
                .filter(|e| e.sequence >= from_sequence)
                .cloned()
                .collect();

            Ok(filtered)
        } else {
            // No entries for this actor
            Ok(Vec::new())
        }
    }

    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        let checkpoints = self.checkpoints.read().await;

        checkpoints
            .get(actor_id)
            .cloned()
            .ok_or_else(|| JournalError::CheckpointNotFound(actor_id.to_string()))
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        let mut checkpoints = self.checkpoints.write().await;

        // Replace existing checkpoint (only keep latest)
        checkpoints.insert(checkpoint.actor_id.clone(), checkpoint.clone());

        Ok(())
    }

    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64> {
        let mut entries = self.entries.write().await;

        if let Some(actor_entries) = entries.get_mut(actor_id) {
            // Count entries to be removed
            let before_len = actor_entries.len();

            // Keep only entries > sequence
            actor_entries.retain(|e| e.sequence > sequence);

            let after_len = actor_entries.len();
            let deleted = (before_len - after_len) as u64;

            Ok(deleted)
        } else {
            // No entries for this actor
            Ok(0)
        }
    }

    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats> {
        let entries = self.entries.read().await;
        let checkpoints = self.checkpoints.read().await;

        let mut stats = JournalStats {
            total_entries: 0,
            total_checkpoints: checkpoints.len() as u64,
            storage_bytes: 0, // Not meaningful for in-memory
            entries_by_actor: HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        };

        if let Some(aid) = actor_id {
            // Stats for specific actor
            if let Some(actor_entries) = entries.get(aid) {
                stats.total_entries = actor_entries.len() as u64;
                stats
                    .entries_by_actor
                    .insert(aid.to_string(), actor_entries.len() as u64);

                if let Some(first) = actor_entries.first() {
                    stats.oldest_entry = first.timestamp.clone();
                }
                if let Some(last) = actor_entries.last() {
                    stats.newest_entry = last.timestamp.clone();
                }
            }
        } else {
            // Global stats
            for (aid, actor_entries) in entries.iter() {
                stats.total_entries += actor_entries.len() as u64;
                stats
                    .entries_by_actor
                    .insert(aid.clone(), actor_entries.len() as u64);

                // Track oldest/newest across all actors
                // Note: Timestamp doesn't implement PartialOrd, so we just take the first/last seen
                if let Some(first) = actor_entries.first() {
                    if stats.oldest_entry.is_none() {
                        stats.oldest_entry = first.timestamp.clone();
                    }
                }
                if let Some(last) = actor_entries.last() {
                    stats.newest_entry = last.timestamp.clone();
                }
            }
        }

        Ok(stats)
    }

    async fn flush(&self) -> JournalResult<()> {
        // No-op for in-memory storage (always "durable")
        Ok(())
    }

    // ==================== Event Sourcing Methods ====================

    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64> {
        let mut events = self.events.write().await;

        // Get or create event vec for this actor
        let actor_events = events
            .entry(event.actor_id.clone())
            .or_insert_with(Vec::new);

        // Assign sequence if not set
        let mut event = event.clone();
        if event.sequence == 0 {
            event.sequence = self.next_event_sequence(&event.actor_id).await;
        }

        let sequence = event.sequence;

        // Append event (maintains sequence order if appended in order)
        actor_events.push(event);

        Ok(sequence)
    }

    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)> {
        if events.is_empty() {
            return Ok((0, 0, 0));
        }

        let mut all_events = self.events.write().await;

        let first_sequence = events[0].sequence;
        let last_sequence = events[events.len() - 1].sequence;
        let count = events.len();

        for event in events {
            let actor_events = all_events
                .entry(event.actor_id.clone())
                .or_insert_with(Vec::new);

            let mut event = event.clone();
            if event.sequence == 0 {
                event.sequence = self.next_event_sequence(&event.actor_id).await;
            }

            actor_events.push(event);
        }

        Ok((first_sequence, last_sequence, count))
    }

    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        let events = self.events.read().await;

        if let Some(actor_events) = events.get(actor_id) {
            // Filter events >= from_sequence
            let filtered: Vec<ActorEvent> = actor_events
                .iter()
                .filter(|e| e.sequence >= from_sequence)
                .cloned()
                .collect();

            Ok(filtered)
        } else {
            // No events for this actor
            Ok(Vec::new())
        }
    }

    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory> {
        let events = self.events.read().await;

        if let Some(actor_events) = events.get(actor_id) {
            let latest_sequence = actor_events
                .last()
                .map(|e| e.sequence)
                .unwrap_or(0);

            let created_at = actor_events
                .first()
                .and_then(|e| e.timestamp.clone())
                .or_else(|| Some(prost_types::Timestamp::from(std::time::SystemTime::now())));

            let updated_at = actor_events
                .last()
                .and_then(|e| e.timestamp.clone())
                .or_else(|| Some(prost_types::Timestamp::from(std::time::SystemTime::now())));

            Ok(ActorHistory {
                actor_id: actor_id.to_string(),
                events: actor_events.clone(),
                latest_sequence,
                created_at,
                updated_at,
                metadata: HashMap::new(),
                page_response: None,
            })
        } else {
            // No events for this actor - return empty history
            Ok(ActorHistory {
                actor_id: actor_id.to_string(),
                events: Vec::new(),
                latest_sequence: 0,
                created_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                updated_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                metadata: HashMap::new(),
                page_response: None,
            })
        }
    }

    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)> {
        let events = self.events.read().await;

        // Decode page_token to get starting sequence (cursor-based pagination)
        // page_token format: sequence number as string (e.g., "123")
        let start_sequence = if page_request.page_token.is_empty() {
            from_sequence
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(from_sequence)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as usize;

        if let Some(actor_events) = events.get(actor_id) {
            // Binary search for start position (O(log n))
            let start_idx = actor_events
                .binary_search_by_key(&start_sequence, |e| e.sequence)
                .unwrap_or_else(|idx| idx);

            // Filter events >= start_sequence and >= from_sequence
            let filtered: Vec<&ActorEvent> = actor_events
                .iter()
                .skip(start_idx)
                .filter(|e| e.sequence >= start_sequence.max(from_sequence))
                .take(page_size + 1) // Fetch one extra to check if there's more
                .collect();

            let has_more = filtered.len() > page_size;
            let events_to_return: Vec<ActorEvent> = filtered
                .iter()
                .take(page_size)
                .cloned()
                .cloned()
                .collect();

            // Generate next_page_token if there are more events
            // page_token format: sequence number as string (e.g., "123")
            let next_page_token = if has_more && !events_to_return.is_empty() {
                // Use last sequence number + 1 as cursor (exclusive start for next page)
                let last_sequence = events_to_return.last().unwrap().sequence;
                (last_sequence + 1).to_string()
            } else {
                String::new()
            };

            let page_response = PageResponse {
                next_page_token,
                total_size: 0, // Total size not available without full scan (expensive)
            };

            Ok((events_to_return, page_response))
        } else {
            // No events for this actor
            Ok((
                Vec::new(),
                PageResponse {
                    next_page_token: String::new(),
                    total_size: 0,
                },
            ))
        }
    }

    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory> {
        let events = self.events.read().await;

        // Decode page_token to get starting sequence (cursor-based pagination)
        // page_token format: sequence number as string (e.g., "123")
        let start_sequence = if page_request.page_token.is_empty() {
            0
        } else {
            page_request.page_token.parse::<u64>().unwrap_or(0)
        };

        // Validate and clamp page_size (1-1000)
        let page_size = page_request.page_size.max(1).min(1000) as usize;

        if let Some(actor_events) = events.get(actor_id) {
            let latest_sequence = actor_events
                .last()
                .map(|e| e.sequence)
                .unwrap_or(0);

            let created_at = actor_events
                .first()
                .and_then(|e| e.timestamp.clone())
                .or_else(|| Some(prost_types::Timestamp::from(std::time::SystemTime::now())));

            let updated_at = actor_events
                .last()
                .and_then(|e| e.timestamp.clone())
                .or_else(|| Some(prost_types::Timestamp::from(std::time::SystemTime::now())));

            // Binary search for start position (O(log n))
            let start_idx = actor_events
                .binary_search_by_key(&start_sequence, |e| e.sequence)
                .unwrap_or_else(|idx| idx);

            // Fetch page_size + 1 to check if there's more
            let filtered: Vec<&ActorEvent> = actor_events
                .iter()
                .skip(start_idx)
                .take(page_size + 1)
                .collect();

            let has_more = filtered.len() > page_size;
            let events_to_return: Vec<ActorEvent> = filtered
                .iter()
                .take(page_size)
                .cloned()
                .cloned()
                .collect();

            // Generate next_page_token if there are more events
            // page_token format: sequence number as string (e.g., "123")
            let next_page_token = if has_more && !events_to_return.is_empty() {
                // Use last sequence number + 1 as cursor (exclusive start for next page)
                let last_sequence = events_to_return.last().unwrap().sequence;
                (last_sequence + 1).to_string()
            } else {
                String::new()
            };

            let page_response = PageResponse {
                next_page_token,
                total_size: 0, // Total size not available without full scan (expensive)
            };

            Ok(ActorHistory {
                actor_id: actor_id.to_string(),
                events: events_to_return,
                latest_sequence,
                created_at,
                updated_at,
                metadata: HashMap::new(),
                page_response: Some(page_response),
            })
        } else {
            // No events for this actor - return empty history
            Ok(ActorHistory {
                actor_id: actor_id.to_string(),
                events: Vec::new(),
                latest_sequence: 0,
                created_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                updated_at: Some(prost_types::Timestamp::from(std::time::SystemTime::now())),
                metadata: HashMap::new(),
                page_response: Some(PageResponse {
                    next_page_token: String::new(),
                    total_size: 0,
                }),
            })
        }
    }

    // ==================== Reminder Methods ====================

    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        let mut reminders = self.reminders.write().await;
        let actor_reminders = reminders
            .entry(reg.actor_id.clone())
            .or_insert_with(HashMap::new);
        actor_reminders.insert(
            reg.reminder_name.clone(),
            reminder_state.clone(),
        );
        Ok(())
    }

    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()> {
        let mut reminders = self.reminders.write().await;
        if let Some(actor_reminders) = reminders.get_mut(actor_id) {
            actor_reminders.remove(reminder_name);
            if actor_reminders.is_empty() {
                reminders.remove(actor_id);
            }
        }
        Ok(())
    }

    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>> {
        let reminders = self.reminders.read().await;
        if let Some(actor_reminders) = reminders.get(actor_id) {
            Ok(actor_reminders
                .values()
                .filter(|r| r.is_active)
                .cloned()
                .collect())
        } else {
            Ok(Vec::new())
        }
    }

    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let reg = reminder_state.registration.as_ref().ok_or_else(|| {
            JournalError::Configuration("ReminderState must have registration".to_string())
        })?;
        let mut reminders = self.reminders.write().await;
        let actor_reminders = reminders
            .entry(reg.actor_id.clone())
            .or_insert_with(HashMap::new);
        actor_reminders.insert(
            reg.reminder_name.clone(),
            reminder_state.clone(),
        );
        Ok(())
    }

    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>> {
        let reminders = self.reminders.read().await;
        let mut due_reminders = Vec::new();

        for actor_reminders in reminders.values() {
            for reminder in actor_reminders.values() {
                if reminder.is_active {
                    if let Some(next_fire_time) = &reminder.next_fire_time {
                        let fire_time = SystemTime::UNIX_EPOCH
                            + std::time::Duration::from_secs(next_fire_time.seconds as u64)
                            + std::time::Duration::from_nanos(next_fire_time.nanos as u64);
                        if fire_time <= before_time {
                            due_reminders.push(reminder.clone());
                        }
                    }
                }
            }
        }

        Ok(due_reminders)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_replay() {
        let storage = MemoryJournalStorage::new();

        // Append entries
        let entry1 = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 1,
            timestamp: None,
            correlation_id: String::new(),
            entry: None,
        };

        let entry2 = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 2,
            timestamp: None,
            correlation_id: String::new(),
            entry: None,
        };

        storage.append_entry(&entry1).await.unwrap();
        storage.append_entry(&entry2).await.unwrap();

        // Replay all
        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].sequence, 1);
        assert_eq!(entries[1].sequence, 2);

        // Replay from sequence 2
        let entries = storage.replay_from("actor-1", 2).await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].sequence, 2);

        // Replay for non-existent actor
        let entries = storage.replay_from("actor-999", 1).await.unwrap();
        assert_eq!(entries.len(), 0);
    }

    #[tokio::test]
    async fn test_append_batch() {
        let storage = MemoryJournalStorage::new();

        let entries = vec![
            JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: 1,
                timestamp: None,
                correlation_id: String::new(),
                entry: None,
            },
            JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: 2,
                timestamp: None,
                correlation_id: String::new(),
                entry: None,
            },
        ];

        let (first, last, count) = storage.append_batch(&entries).await.unwrap();
        assert_eq!(first, 1);
        assert_eq!(last, 2);
        assert_eq!(count, 2);

        // Verify replay
        let replay = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(replay.len(), 2);
    }

    #[tokio::test]
    async fn test_checkpoint() {
        let storage = MemoryJournalStorage::new();

        // Save checkpoint
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 100,
            timestamp: None,
            state_data: vec![1, 2, 3],
            compression: 0,
            metadata: HashMap::new(),
            state_schema_version: 0,
        };

        storage.save_checkpoint(&checkpoint).await.unwrap();

        // Get checkpoint
        let loaded = storage.get_latest_checkpoint("actor-1").await.unwrap();
        assert_eq!(loaded.actor_id, "actor-1");
        assert_eq!(loaded.sequence, 100);
        assert_eq!(loaded.state_data, vec![1, 2, 3]);

        // Get non-existent checkpoint
        let result = storage.get_latest_checkpoint("actor-999").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_truncate() {
        let storage = MemoryJournalStorage::new();

        // Append 10 entries
        for i in 1..=10 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                timestamp: None,
                correlation_id: String::new(),
                entry: None,
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Truncate to sequence 5 (removes 1-5, keeps 6-10)
        let deleted = storage.truncate_to("actor-1", 5).await.unwrap();
        assert_eq!(deleted, 5);

        // Verify remaining entries
        let entries = storage.replay_from("actor-1", 1).await.unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].sequence, 6);
        assert_eq!(entries[4].sequence, 10);
    }

    #[tokio::test]
    async fn test_stats() {
        let storage = MemoryJournalStorage::new();

        // Append entries for actor-1
        for i in 1..=5 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-1".to_string(),
                sequence: i,
                timestamp: None,
                correlation_id: String::new(),
                entry: None,
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Append entries for actor-2
        for i in 1..=3 {
            let entry = JournalEntry {
                id: ulid::Ulid::new().to_string(),
                actor_id: "actor-2".to_string(),
                sequence: i,
                timestamp: None,
                correlation_id: String::new(),
                entry: None,
            };
            storage.append_entry(&entry).await.unwrap();
        }

        // Global stats
        let stats = storage.get_stats(None).await.unwrap();
        assert_eq!(stats.total_entries, 8);
        assert_eq!(stats.entries_by_actor.get("actor-1"), Some(&5));
        assert_eq!(stats.entries_by_actor.get("actor-2"), Some(&3));

        // Actor-specific stats
        let stats = storage.get_stats(Some("actor-1")).await.unwrap();
        assert_eq!(stats.total_entries, 5);
        assert_eq!(stats.entries_by_actor.get("actor-1"), Some(&5));
    }

    #[tokio::test]
    async fn test_flush() {
        let storage = MemoryJournalStorage::new();

        // Flush should always succeed (no-op)
        storage.flush().await.unwrap();
    }

    #[tokio::test]
    async fn test_clone_shares_data() {
        // CRITICAL TEST: Verify that cloning MemoryJournalStorage shares the Arc-wrapped HashMaps
        let storage1 = MemoryJournalStorage::new();
        let storage2 = storage1.clone();

        // Write to storage1
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: "test-actor".to_string(),
            sequence: 1,
            timestamp: None,
            correlation_id: String::new(),
            entry: None,
        };
        storage1.append_entry(&entry).await.unwrap();

        // Read from storage2 - should see the entry
        let entries = storage2.replay_from("test-actor", 0).await.unwrap();
        assert_eq!(entries.len(), 1, "Clone should share data via Arc");
        assert_eq!(entries[0].actor_id, "test-actor");
    }

    // ==================== Event Sourcing Tests ====================

    #[tokio::test]
    async fn test_append_event() {
        let storage = MemoryJournalStorage::new();

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
    }

    #[tokio::test]
    async fn test_append_events_batch() {
        let storage = MemoryJournalStorage::new();

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
    async fn test_replay_events_from() {
        let storage = MemoryJournalStorage::new();

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
    async fn test_get_actor_history() {
        let storage = MemoryJournalStorage::new();

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
    async fn test_replay_events_from_paginated() {
        let storage = MemoryJournalStorage::new();

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
            page_size: 3,
            page_token: String::new(),
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, page_response) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].sequence, 1);
        assert!(!page_response.next_page_token.is_empty());

        // Second page
        let page_request2 = PageRequest {
            page_size: 3,
            page_token: page_response.next_page_token,
            filter: String::new(),
            order_by: String::new(),
        };

        let (events2, page_response2) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request2)
            .await
            .unwrap();

        assert_eq!(events2.len(), 3);
        assert_eq!(events2[0].sequence, 4);
        assert!(!page_response2.next_page_token.is_empty());

        // Last page
        let page_request3 = PageRequest {
            page_size: 3,
            page_token: page_response2.next_page_token,
            filter: String::new(),
            order_by: String::new(),
        };

        let (events3, page_response3) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request3)
            .await
            .unwrap();

        assert_eq!(events3.len(), 3);
        assert_eq!(events3[0].sequence, 7);
        // Should have more pages
        assert!(!page_response3.next_page_token.is_empty());
    }

    #[tokio::test]
    async fn test_get_actor_history_paginated() {
        let storage = MemoryJournalStorage::new();

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
            page_size: 2,
            page_token: String::new(),
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
        assert!(!history.page_response.as_ref().unwrap().next_page_token.is_empty());

        // Second page
        let page_request2 = PageRequest {
            page_size: 2,
            page_token: history.page_response.as_ref().unwrap().next_page_token.clone(),
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
    async fn test_pagination_empty_event_log() {
        let storage = MemoryJournalStorage::new();

        let page_request = PageRequest {
            page_size: 10,
            page_token: String::new(),
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, page_response) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 0);
        assert!(page_response.next_page_token.is_empty());
    }

    #[tokio::test]
    async fn test_pagination_page_size_clamping() {
        let storage = MemoryJournalStorage::new();

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

        // Test page_size > max (should clamp to 1000)
        let page_request = PageRequest {
            page_size: 2000, // Exceeds max
            page_token: String::new(),
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, _) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 5); // All events, but clamped to reasonable size
    }

    #[tokio::test]
    async fn test_pagination_from_sequence() {
        let storage = MemoryJournalStorage::new();

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
            page_size: 3,
            page_token: String::new(),
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
}
