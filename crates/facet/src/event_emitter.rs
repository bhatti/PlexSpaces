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

//! Event Emitter Facet - Replaces GenEventBehavior with a more flexible facet
//!
//! This demonstrates how to migrate from a behavior to a facet for optional capabilities.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::{Facet, FacetError, InterceptResult};

/// Event emitter facet - adds event emission capability to any actor
pub struct EventEmitterFacet {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    /// Facet priority (immutable)
    priority: i32,
    /// Event handlers/subscribers
    subscribers: Arc<RwLock<Vec<EventSubscriber>>>,
    /// Event filters
    filters: Vec<EventFilter>,
    /// Configuration (parsed from config_value)
    config: EventEmitterConfig,
}

/// Default priority for EventEmitterFacet
pub const EVENT_EMITTER_FACET_DEFAULT_PRIORITY: i32 = 10;

/// Configuration for event emitter facet
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventEmitterConfig {
    /// Whether to emit events asynchronously
    pub async_emit: bool,
    /// Maximum subscribers
    pub max_subscribers: usize,
    /// Event buffer size
    pub buffer_size: usize,
    /// Whether to log events
    pub log_events: bool,
}

impl Default for EventEmitterConfig {
    fn default() -> Self {
        EventEmitterConfig {
            async_emit: true,
            max_subscribers: 100,
            buffer_size: 1000,
            log_events: false,
        }
    }
}

/// Event subscriber
#[derive(Clone)]
pub struct EventSubscriber {
    /// Subscriber ID
    pub id: String,
    /// Actor reference to send events to
    pub actor_ref: String, // In real implementation, would be ActorRef
    /// Event types to subscribe to (None = all events)
    pub event_types: Option<Vec<String>>,
    /// Whether this subscription is active
    pub active: bool,
}

/// Event filter
#[derive(Clone)]
pub enum EventFilter {
    /// Only emit events of specific types
    TypeFilter(Vec<String>),
    /// Only emit events matching a pattern
    PatternFilter(String),
    /// Custom filter function
    Custom(Arc<dyn Fn(&Event) -> bool + Send + Sync>),
}

impl std::fmt::Debug for EventFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EventFilter::TypeFilter(types) => f.debug_tuple("TypeFilter").field(types).finish(),
            EventFilter::PatternFilter(pattern) => {
                f.debug_tuple("PatternFilter").field(pattern).finish()
            }
            EventFilter::Custom(_) => f.debug_tuple("Custom").field(&"<function>").finish(),
        }
    }
}

/// Event structure
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Event {
    /// Event type
    pub event_type: String,
    /// Event data
    pub data: Value,
    /// Timestamp
    pub timestamp: u64,
    /// Source actor
    pub source: String,
    /// Metadata
    pub metadata: HashMap<String, String>,
}

impl Default for EventEmitterFacet {
    fn default() -> Self {
        Self::new(serde_json::json!({}), EVENT_EMITTER_FACET_DEFAULT_PRIORITY)
    }
}

impl EventEmitterFacet {
    /// Create a new event emitter facet
    pub fn new(config: Value, priority: i32) -> Self {
        let mut emitter_config = EventEmitterConfig::default();
        // Parse config values manually (EventEmitterConfig doesn't implement Deserialize)
        if let Some(max_subscribers) = config.get("max_subscribers").and_then(|v| v.as_u64()) {
            emitter_config.max_subscribers = max_subscribers as usize;
        }
        if let Some(async_emit) = config.get("async_emit").and_then(|v| v.as_bool()) {
            emitter_config.async_emit = async_emit;
        }
        if let Some(buffer_size) = config.get("buffer_size").and_then(|v| v.as_u64()) {
            emitter_config.buffer_size = buffer_size as usize;
        }
        if let Some(log_events) = config.get("log_events").and_then(|v| v.as_bool()) {
            emitter_config.log_events = log_events;
        }
        EventEmitterFacet {
            config_value: config,
            priority,
            subscribers: Arc::new(RwLock::new(Vec::new())),
            filters: Vec::new(),
            config: emitter_config,
        }
    }

    /// Add a subscriber
    pub async fn add_subscriber(&self, subscriber: EventSubscriber) -> Result<(), FacetError> {
        let mut subs = self.subscribers.write().await;

        if subs.len() >= self.config.max_subscribers {
            return Err(FacetError::InvalidConfig(format!(
                "Maximum subscribers ({}) reached",
                self.config.max_subscribers
            )));
        }

        // Check if already subscribed
        if subs.iter().any(|s| s.id == subscriber.id) {
            return Err(FacetError::AlreadyAttached(subscriber.id));
        }

        subs.push(subscriber);
        Ok(())
    }

    /// Remove a subscriber
    pub async fn remove_subscriber(&self, subscriber_id: &str) -> Result<(), FacetError> {
        let mut subs = self.subscribers.write().await;
        subs.retain(|s| s.id != subscriber_id);
        Ok(())
    }

    /// Emit an event to all subscribers
    async fn emit_event(&self, event: Event) -> Result<(), FacetError> {
        // Apply filters
        for filter in &self.filters {
            if !self.should_emit(&event, filter) {
                return Ok(()); // Event filtered out
            }
        }

        if self.config.log_events {
            println!("Event emitted: {:?}", event);
        }

        let subs = self.subscribers.read().await;

        for subscriber in subs.iter().filter(|s| s.active) {
            // Check if subscriber wants this event type
            if let Some(ref types) = subscriber.event_types {
                if !types.contains(&event.event_type) {
                    continue;
                }
            }

            // In real implementation, send to actor
            if self.config.async_emit {
                let event_clone = event.clone();
                let subscriber_id = subscriber.id.clone();
                tokio::spawn(async move {
                    // Send event to subscriber
                    println!(
                        "Sending event to subscriber {}: {:?}",
                        subscriber_id, event_clone
                    );
                });
            } else {
                println!("Sending event to subscriber {}: {:?}", subscriber.id, event);
            }
        }

        Ok(())
    }

    fn should_emit(&self, event: &Event, filter: &EventFilter) -> bool {
        match filter {
            EventFilter::TypeFilter(types) => types.contains(&event.event_type),
            EventFilter::PatternFilter(pattern) => event.event_type.contains(pattern),
            EventFilter::Custom(f) => f(event),
        }
    }
}

#[async_trait]
impl Facet for EventEmitterFacet {
    fn facet_type(&self) -> &str {
        "event_emitter"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        println!("Event emitter facet attached to actor: {}", actor_id);
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        // Clear all subscribers
        self.subscribers.write().await.clear();
        println!("Event emitter facet detached from actor: {}", actor_id);
        Ok(())
    }

    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        _result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Emit an event after method execution
        // This is where GenEventBehavior functionality is replicated

        // Check if this method should emit an event
        if method.starts_with("event_") || method.ends_with("_completed") {
            let event = Event {
                event_type: format!("method.{}", method),
                data: Value::String(format!("Method {} completed", method)),
                timestamp: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs(),
                source: "actor".to_string(), // Would be actual actor ID
                metadata: HashMap::new(),
            };

            self.emit_event(event).await?;
        }

        Ok(InterceptResult::Continue)
    }
    
    fn get_config(&self) -> Value {
        self.config_value.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Migration helper to convert from GenEventBehavior
pub fn migrate_from_gen_event(/* gen_event: GenEventBehavior */) -> EventEmitterFacet {
    // This would convert GenEventBehavior handlers to EventEmitterFacet subscribers

    // Migration logic would go here:
    // 1. Extract handlers from GenEventBehavior
    // 2. Convert to EventSubscribers
    // 3. Add to facet

    EventEmitterFacet::new(serde_json::json!({}), EVENT_EMITTER_FACET_DEFAULT_PRIORITY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_event_emitter_facet() {
        let mut facet = EventEmitterFacet::new(serde_json::json!({}), 50);

        // Attach to actor
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // Add subscriber
        let subscriber = EventSubscriber {
            id: "sub-1".to_string(),
            actor_ref: "subscriber-actor".to_string(),
            event_types: Some(vec!["test.event".to_string()]),
            active: true,
        };

        facet.add_subscriber(subscriber).await.unwrap();

        // Trigger after_method which should emit event
        let result = facet
            .after_method("event_test", b"args", b"result")
            .await
            .unwrap();

        assert!(matches!(result, InterceptResult::Continue));
    }

    #[tokio::test]
    async fn test_max_subscribers() {
        let config_value = serde_json::json!({
            "max_subscribers": 2,
        });
        let facet = EventEmitterFacet::new(config_value, 50);

        // Add first subscriber
        facet
            .add_subscriber(EventSubscriber {
                id: "sub-1".to_string(),
                actor_ref: "actor-1".to_string(),
                event_types: None,
                active: true,
            })
            .await
            .unwrap();

        // Add second subscriber
        facet
            .add_subscriber(EventSubscriber {
                id: "sub-2".to_string(),
                actor_ref: "actor-2".to_string(),
                event_types: None,
                active: true,
            })
            .await
            .unwrap();

        // Third should fail
        let result = facet
            .add_subscriber(EventSubscriber {
                id: "sub-3".to_string(),
                actor_ref: "actor-3".to_string(),
                event_types: None,
                active: true,
            })
            .await;

        assert!(result.is_err());
    }
}
