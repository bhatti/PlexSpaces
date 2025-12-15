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

//! Event sourcing facet for optional actor event sourcing
//!
//! ## Purpose
//! Provides Temporal-inspired event sourcing for actors through event logging
//! and state reconstruction. 100% optional via facet pattern, builds on DurabilityFacet.
//!
//! ## Architecture Context
//! Part of PlexSpaces Pillar 3 (Durability). Implements facet pattern for
//! dynamic event sourcing attachment - actors without this facet have zero overhead.
//!
//! ## How It Works
//! ```text
//! Normal Execution:
//! 1. DurabilityFacet journals MessageReceived entry
//! 2. Actor processes message and changes state
//! 3. EventSourcingFacet logs ActorEvent (state change)
//! 4. DurabilityFacet journals MessageProcessed entry
//! 5. Periodic snapshot if interval reached
//!
//! Replay After Crash:
//! 1. DurabilityFacet replays journal entries
//! 2. EventSourcingFacet replays events to reconstruct state
//! 3. Actor state restored to pre-crash point
//! 4. Continue normal execution
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use plexspaces_facet::Facet;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure event sourcing
//! let config = EventSourcingConfig {
//!     event_log_enabled: true,
//!     auto_replay: true,
//!     snapshot_interval: 100,
//!     time_travel_enabled: true,
//!     metadata: Default::default(),
//!     default_page_size: 100,
//!     max_page_size: 1000,
//! };
//!
//! // Create event sourcing facet (requires DurabilityFacet)
//! let mut facet = EventSourcingFacet::new(config);
//!
//! // Attach to actor (triggers replay if enabled)
//! facet.on_attach("actor-123", serde_json::json!({})).await?;
//!
//! // Facet intercepts state changes and logs events transparently
//! # Ok(())
//! # }
//! ```

use crate::{ActorEvent, ActorHistory, EventSourcingConfig, JournalResult, JournalStorage};
use async_trait::async_trait;
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use plexspaces_proto::prost_types;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

/// Event sourcing facet providing event logging and state reconstruction
///
/// ## Purpose
/// Implements the Facet trait to add event sourcing to actors without modifying
/// the actor core. Transparently logs state changes as events and enables replay.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to state.
///
/// ## Design
/// - Builds on DurabilityFacet (requires it to be present)
/// - Logs events when state changes occur
/// - Supports replay from event log
/// - Supports pagination for efficient retrieval
/// - Shares snapshots with DurabilityFacet (efficiency)
pub struct EventSourcingFacet<S: JournalStorage> {
    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,

    /// Event sourcing configuration
    config: EventSourcingConfig,

    /// Journal storage backend (shared with DurabilityFacet)
    storage: Arc<S>,

    /// Current event sequence number
    event_sequence: Arc<RwLock<u64>>,
}

impl<S: JournalStorage + Clone + 'static> EventSourcingFacet<S> {
    /// Create a new event sourcing facet
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend (shared with DurabilityFacet)
    /// * `config` - Event sourcing configuration from proto
    ///
    /// ## Returns
    /// New EventSourcingFacet ready to attach to an actor
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example() -> JournalResult<()> {
    /// let storage = MemoryJournalStorage::new();
    /// let config = EventSourcingConfig {
    ///     event_log_enabled: true,
    ///     auto_replay: true,
    ///     snapshot_interval: 100,
    ///     time_travel_enabled: true,
    ///     metadata: Default::default(),
    ///     default_page_size: 100,
    ///     max_page_size: 1000,
    /// };
    ///
    /// let facet = EventSourcingFacet::new(Arc::new(storage), config);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(storage: Arc<S>, config: EventSourcingConfig) -> Self {
        Self {
            actor_id: Arc::new(RwLock::new(None)),
            config,
            storage,
            event_sequence: Arc::new(RwLock::new(0)),
        }
    }

    /// Log an event to the event log
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `event_type` - Type of event (e.g., "counter_incremented")
    /// * `event_data` - Serialized event payload
    /// * `caused_by` - Correlation ID linking to journal entry
    ///
    /// ## Returns
    /// Sequence number assigned to the event
    ///
    /// ## Errors
    /// - Journal storage errors
    async fn log_event(
        &self,
        actor_id: &str,
        event_type: &str,
        event_data: &[u8],
        caused_by: &str,
    ) -> JournalResult<u64> {
        if !self.config.event_log_enabled {
            return Ok(0); // Event logging disabled
        }

        // Get next sequence number
        let mut seq = self.event_sequence.write().await;
        *seq += 1;
        let sequence = *seq;

        // Create event
        let event = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.to_string(),
            sequence,
            event_type: event_type.to_string(),
            event_data: event_data.to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: caused_by.to_string(),
            metadata: HashMap::new(),
        };

        // Append to storage
        self.storage.append_event(&event).await?;

        Ok(sequence)
    }

    /// Replay events to reconstruct state
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay events for
    /// * `from_sequence` - Start sequence (0 for full replay)
    ///
    /// ## Returns
    /// Vec of events replayed
    ///
    /// ## Errors
    /// - Journal storage errors
    async fn replay_events(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        if !self.config.auto_replay {
            return Ok(Vec::new()); // Auto-replay disabled
        }

        // Replay events from storage
        let events = self.storage.replay_events_from(actor_id, from_sequence).await?;

        // Update sequence to last replayed event
        if let Some(last_event) = events.last() {
            let mut seq = self.event_sequence.write().await;
            *seq = last_event.sequence;
        }

        Ok(events)
    }
}

#[async_trait]
impl<S: JournalStorage + Clone + 'static> Facet for EventSourcingFacet<S> {
    fn facet_type(&self) -> &str {
        "event_sourcing"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Store actor ID
        {
            let mut aid = self.actor_id.write().await;
            *aid = Some(actor_id.to_string());
        }

        // Replay events if enabled
        if self.config.auto_replay {
            let events = self
                .replay_events(actor_id, 0)
                .await
                .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

            // Update sequence to last replayed event
            if let Some(last_event) = events.last() {
                let mut seq = self.event_sequence.write().await;
                *seq = last_event.sequence;
            }
        }

        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Clear actor ID
        {
            let mut aid = self.actor_id.write().await;
            *aid = None;
        }

        // Reset sequence
        {
            let mut seq = self.event_sequence.write().await;
            *seq = 0;
        }

        Ok(())
    }

    async fn before_method(
        &self,
        _method_name: &str,
        _args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Event sourcing doesn't intercept before_method
        // Events are logged in after_method when state changes
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method_name: &str,
        _args: &[u8],
        result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Check if event logging is enabled
        if !self.config.event_log_enabled {
            return Ok(InterceptResult::Continue);
        }

        // Get actor ID
        let actor_id = {
            let aid = self.actor_id.read().await;
            aid.clone().ok_or_else(|| {
                FacetError::InvalidConfig("Actor ID not set".to_string())
            })?
        };

        // Log event for state change
        // Use method_name as event_type, result as event_data
        // Use empty string for caused_by (will be set by DurabilityFacet correlation)
        let _sequence = self
            .log_event(&actor_id, method_name, result, "")
            .await
            .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

        Ok(InterceptResult::Continue)
    }

    async fn on_error(
        &self,
        _method_name: &str,
        _error: &str,
    ) -> Result<ErrorHandling, FacetError> {
        // Event sourcing doesn't handle errors
        Ok(ErrorHandling::Propagate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MemoryJournalStorage;
    use plexspaces_proto::common::v1::PageRequest;

    fn create_test_config() -> EventSourcingConfig {
        EventSourcingConfig {
            event_log_enabled: true,
            auto_replay: true,
            snapshot_interval: 100,
            time_travel_enabled: true,
            metadata: HashMap::new(),
            default_page_size: 100,
            max_page_size: 1000,
        }
    }

    #[tokio::test]
    async fn test_facet_creation() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let facet = EventSourcingFacet::new(storage, config);

        assert_eq!(facet.facet_type(), "event_sourcing");
    }

    #[tokio::test]
    async fn test_facet_attach_without_replay() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let mut config = create_test_config();
        config.auto_replay = false; // Disable replay

        let mut facet = EventSourcingFacet::new(storage, config);

        // Should succeed without errors
        let result = facet.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());

        // Verify actor_id was set
        let actor_id = facet.actor_id.read().await;
        assert_eq!(actor_id.as_ref().unwrap(), "actor-1");
    }

    #[tokio::test]
    async fn test_facet_attach_with_empty_event_log_replay() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let mut facet = EventSourcingFacet::new(storage, config);

        // Should succeed even with empty event log
        let result = facet.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_log_event() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Log an event
        let sequence = facet
            .log_event("actor-1", "counter_incremented", b"{\"amount\": 5}", "corr-1")
            .await
            .unwrap();

        assert_eq!(sequence, 1);

        // Verify event was stored
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "counter_incremented");
        assert_eq!(events[0].sequence, 1);
    }

    #[tokio::test]
    async fn test_log_event_disabled() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let mut config = create_test_config();
        config.event_log_enabled = false; // Disable event logging

        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Log an event (should return 0, no event logged)
        let sequence = facet
            .log_event("actor-1", "counter_incremented", b"{\"amount\": 5}", "corr-1")
            .await
            .unwrap();

        assert_eq!(sequence, 0);

        // Verify no event was stored
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 0);
    }

    #[tokio::test]
    async fn test_replay_events() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        // Pre-populate events
        let event1 = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 1,
            event_type: "counter_incremented".to_string(),
            event_data: b"{\"amount\": 5}".to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: "corr-1".to_string(),
            metadata: HashMap::new(),
        };

        let event2 = ActorEvent {
            id: ulid::Ulid::new().to_string(),
            actor_id: "actor-1".to_string(),
            sequence: 2,
            event_type: "counter_incremented".to_string(),
            event_data: b"{\"amount\": 10}".to_vec(),
            timestamp: Some(prost_types::Timestamp::from(SystemTime::now())),
            caused_by: "corr-2".to_string(),
            metadata: HashMap::new(),
        };

        storage.append_event(&event1).await.unwrap();
        storage.append_event(&event2).await.unwrap();

        let facet = EventSourcingFacet::new(storage, config);

        // Replay events
        let events = facet.replay_events("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[1].sequence, 2);

        // Verify sequence was updated
        let seq = *facet.event_sequence.read().await;
        assert_eq!(seq, 2);
    }

    #[tokio::test]
    async fn test_replay_events_disabled() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let mut config = create_test_config();
        config.auto_replay = false; // Disable auto-replay

        let facet = EventSourcingFacet::new(storage, config);

        // Replay events (should return empty vec)
        let events = facet.replay_events("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 0);
    }

    #[tokio::test]
    async fn test_after_method_logs_event() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Call after_method (should log event)
        let result = facet
            .after_method("increment", &[], b"{\"new_value\": 5}")
            .await
            .unwrap();

        assert!(matches!(result, InterceptResult::Continue));

        // Verify event was logged
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "increment");
        assert_eq!(events[0].event_data, b"{\"new_value\": 5}");
    }

    #[tokio::test]
    async fn test_after_method_with_event_logging_disabled() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let mut config = create_test_config();
        config.event_log_enabled = false; // Disable event logging

        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Call after_method (should not log event)
        let result = facet
            .after_method("increment", &[], b"{\"new_value\": 5}")
            .await
            .unwrap();

        assert!(matches!(result, InterceptResult::Continue));

        // Verify no event was logged
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 0);
    }

    #[tokio::test]
    async fn test_after_method_without_attach_fails() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let facet = EventSourcingFacet::new(storage, config);

        // Calling after_method without attach should fail
        let result = facet.after_method("test", &[], &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_facet_detach() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let mut facet = EventSourcingFacet::new(storage, config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Detach
        let result = facet.on_detach("actor-1").await;
        assert!(result.is_ok());

        // Verify actor_id was cleared
        let actor_id = facet.actor_id.read().await;
        assert!(actor_id.is_none());

        // Verify sequence was reset
        let seq = *facet.event_sequence.read().await;
        assert_eq!(seq, 0);
    }

    #[tokio::test]
    async fn test_multiple_events() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let mut facet = EventSourcingFacet::new(storage.clone(), config);
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();

        // Log multiple events
        for i in 1..=5 {
            facet
                .log_event(
                    "actor-1",
                    "counter_incremented",
                    &format!("{{\"amount\": {}}}", i).into_bytes(),
                    &format!("corr-{}", i),
                )
                .await
                .unwrap();
        }

        // Verify all events were stored
        let events = storage.replay_events_from("actor-1", 0).await.unwrap();
        assert_eq!(events.len(), 5);

        // Verify sequences are sequential
        for (i, event) in events.iter().enumerate() {
            assert_eq!(event.sequence, (i + 1) as u64);
        }
    }

    #[tokio::test]
    async fn test_replay_events_from_sequence() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        // Pre-populate events
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

        let facet = EventSourcingFacet::new(storage, config);

        // Replay from sequence 3
        let events = facet.replay_events("actor-1", 3).await.unwrap();
        assert_eq!(events.len(), 3); // Sequences 3, 4, 5
        assert_eq!(events[0].sequence, 3);
        assert_eq!(events[1].sequence, 4);
        assert_eq!(events[2].sequence, 5);

        // Verify sequence was updated to last replayed event
        let seq = *facet.event_sequence.read().await;
        assert_eq!(seq, 5);
    }

    #[tokio::test]
    async fn test_paginated_event_replay() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let _config = create_test_config();

        // Pre-populate 10 events
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

        // Test paginated replay
        let page_request = PageRequest {
            page_size: 3,
            page_token: String::new(), // First page
            filter: String::new(),
            order_by: String::new(),
        };

        let (events, page_response) = storage
            .replay_events_from_paginated("actor-1", 0, &page_request)
            .await
            .unwrap();

        assert_eq!(events.len(), 3);
        assert!(!page_response.next_page_token.is_empty());

        // Get next page
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
        assert!(!page_response2.next_page_token.is_empty());
    }

    #[tokio::test]
    async fn test_get_actor_history_paginated() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let _config = create_test_config();

        // Pre-populate 5 events
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

        // Get paginated history
        let page_request = PageRequest {
            page_size: 2,
            page_token: String::new(), // First page
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
    }

    #[tokio::test]
    async fn test_facet_error_handling() {
        let storage = Arc::new(MemoryJournalStorage::new());
        let config = create_test_config();

        let facet = EventSourcingFacet::new(storage, config);

        // on_error should propagate by default
        let result = facet.on_error("test_method", "test error").await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ErrorHandling::Propagate));
    }
}

