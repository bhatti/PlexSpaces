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

//! Simplified Behavior System
//!
//! This module provides the single, unified Behavior trait that replaces
//! all the different handler traits with a pure functional approach.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Input to a behavior
#[derive(Clone, Debug)]
pub struct Input {
    /// The message to process
    pub message: Vec<u8>,

    /// Message type hint (for routing)
    pub message_type: String,

    /// Sender information
    pub sender: Option<String>,

    /// Current actor state (read-only)
    pub state: State,

    /// Context (headers, metadata, etc.)
    pub context: Context,
}

/// Output from a behavior
#[derive(Clone, Debug)]
pub struct Output {
    /// Optional response data
    pub response: Option<Vec<u8>>,

    /// Effects to apply (side effects)
    pub effects: Vec<Effect>,

    /// State mutations to apply
    pub state_updates: Vec<StateUpdate>,
}

/// Effect: A side effect to be applied by the runtime
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Effect {
    /// Send a message to another actor
    SendMessage {
        /// Target actor ID
        to: String,
        /// Message payload
        message: Vec<u8>,
        /// Message headers
        headers: HashMap<String, String>,
    },

    /// Reply to the sender
    Reply {
        /// Reply message payload
        message: Vec<u8>,
    },

    /// Set a timer
    SetTimer {
        /// Timer name
        name: String,
        /// Duration in milliseconds
        duration_ms: u64,
        /// Message to send when timer fires
        message: Vec<u8>,
    },

    /// Cancel a timer
    CancelTimer {
        /// Timer name to cancel
        name: String,
    },

    /// Emit an event
    EmitEvent {
        /// Event type identifier
        event_type: String,
        /// Event data
        data: Vec<u8>,
    },

    /// Attach a facet dynamically
    AttachFacet {
        /// Type of facet to attach
        facet_type: String,
        /// Facet priority
        priority: i32,
        /// Facet configuration
        config: HashMap<String, String>,
    },

    /// Detach a facet
    DetachFacet {
        /// Type of facet to detach
        facet_type: String,
    },

    /// Log a message
    Log {
        /// Log level (e.g., "info", "warn", "error")
        level: String,
        /// Log message
        message: String,
    },

    /// Custom effect (for extensions)
    Custom {
        /// Custom effect type
        effect_type: String,
        /// Effect parameters
        params: Vec<u8>,
    },
}

/// State update operation
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum StateUpdate {
    /// Set a key-value pair
    Set {
        /// Key to set
        key: String,
        /// Value to set
        value: Vec<u8>,
    },

    /// Delete a key
    Delete {
        /// Key to delete
        key: String,
    },

    /// Merge with existing value (for CRDTs)
    Merge {
        /// Key to merge
        key: String,
        /// Value to merge
        value: Vec<u8>,
        /// Type of merge operation
        merge_type: String,
    },

    /// Clear all state
    Clear,
}

/// Actor state (simple key-value)
#[derive(Clone, Debug, Default)]
pub struct State {
    data: HashMap<String, Vec<u8>>,
}

impl State {
    /// Get value for key
    pub fn get(&self, key: &str) -> Option<&Vec<u8>> {
        self.data.get(key)
    }

    /// Check if key exists
    pub fn contains(&self, key: &str) -> bool {
        self.data.contains_key(key)
    }
}

/// Context for message processing
#[derive(Clone, Debug, Default)]
pub struct Context {
    /// Actor ID
    pub actor_id: String,

    /// Correlation ID for tracing
    pub correlation_id: Option<String>,

    /// Headers from the message
    pub headers: HashMap<String, String>,

    /// Current timestamp
    pub timestamp: u64,
}

/// The ONE behavior trait that all behaviors implement
#[async_trait]
pub trait Behavior: Send + Sync {
    /// Process a message and return effects
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError>;

    /// Optional: Declare what message types this behavior handles
    fn handles(&self) -> Vec<String> {
        vec!["*".to_string()] // Handle all by default
    }

    /// Optional: Initialize behavior
    async fn init(&mut self, _config: HashMap<String, String>) -> Result<(), BehaviorError> {
        Ok(())
    }

    /// Optional: Cleanup behavior
    async fn cleanup(&mut self) -> Result<(), BehaviorError> {
        Ok(())
    }
}

/// Behavior errors
#[derive(Debug, thiserror::Error)]
pub enum BehaviorError {
    /// Invalid input provided to behavior
    #[error("Invalid input: {0}")]
    InvalidInput(String),

    /// Processing failed during execution
    #[error("Processing failed: {0}")]
    ProcessingFailed(String),

    /// Message not handled by behavior
    #[error("Not handled")]
    NotHandled,
}

// ============================================================================
// EXAMPLE BEHAVIORS (showing the simplification)
// ============================================================================

/// Simple counter behavior
pub struct CounterBehavior {
    // No state! State is managed by the actor runtime
}

#[async_trait]
impl Behavior for CounterBehavior {
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError> {
        let current = input
            .state
            .get("count")
            .and_then(|bytes| serde_json::from_slice::<i64>(bytes).ok())
            .unwrap_or(0);

        let mut effects = vec![];
        let mut state_updates = vec![];

        match input.message_type.as_str() {
            "increment" => {
                let new_count = current + 1;
                state_updates.push(StateUpdate::Set {
                    key: "count".to_string(),
                    value: serde_json::to_vec(&new_count).unwrap(),
                });

                effects.push(Effect::Log {
                    level: "info".to_string(),
                    message: format!("Count incremented to {}", new_count),
                });
            }
            "decrement" => {
                let new_count = current - 1;
                state_updates.push(StateUpdate::Set {
                    key: "count".to_string(),
                    value: serde_json::to_vec(&new_count).unwrap(),
                });
            }
            "get" => {
                return Ok(Output {
                    response: Some(serde_json::to_vec(&current).unwrap()),
                    effects: vec![],
                    state_updates: vec![],
                });
            }
            _ => return Err(BehaviorError::NotHandled),
        }

        Ok(Output {
            response: None,
            effects,
            state_updates,
        })
    }

    fn handles(&self) -> Vec<String> {
        vec![
            "increment".to_string(),
            "decrement".to_string(),
            "get".to_string(),
        ]
    }
}

/// Request-Reply behavior (replaces GenServer)
pub struct RequestReplyBehavior<H> {
    handler: H,
}

impl<H> RequestReplyBehavior<H> {
    pub fn new(handler: H) -> Self {
        RequestReplyBehavior { handler }
    }
}

#[async_trait]
impl<H> Behavior for RequestReplyBehavior<H>
where
    H: Fn(Vec<u8>) -> Vec<u8> + Send + Sync,
{
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError> {
        // Process the request
        let response = (self.handler)(input.message);

        // Create reply effect if we have a sender
        let mut effects = vec![];
        if input.sender.is_some() {
            effects.push(Effect::Reply {
                message: response.clone(),
            });
        }

        Ok(Output {
            response: Some(response),
            effects,
            state_updates: vec![],
        })
    }
}

/// State Machine behavior (simplified)
pub struct StateMachineBehavior {
    transitions: HashMap<(String, String), String>, // (state, event) -> new_state
}

#[async_trait]
impl Behavior for StateMachineBehavior {
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError> {
        // Get current state
        let current_state = input
            .state
            .get("fsm_state")
            .and_then(|bytes| String::from_utf8(bytes.clone()).ok())
            .unwrap_or_else(|| "initial".to_string());

        // Check for transition
        let event = input.message_type.clone();
        let key = (current_state.clone(), event.clone());

        if let Some(new_state) = self.transitions.get(&key) {
            // Transition to new state
            let state_updates = vec![StateUpdate::Set {
                key: "fsm_state".to_string(),
                value: new_state.as_bytes().to_vec(),
            }];

            let effects = vec![
                Effect::Log {
                    level: "info".to_string(),
                    message: format!("FSM: {} + {} -> {}", current_state, event, new_state),
                },
                Effect::EmitEvent {
                    event_type: "state_changed".to_string(),
                    data: new_state.as_bytes().to_vec(),
                },
            ];

            Ok(Output {
                response: None,
                effects,
                state_updates,
            })
        } else {
            Err(BehaviorError::InvalidInput(format!(
                "No transition from {} on event {}",
                current_state, event
            )))
        }
    }
}

/// Composite behavior (combines multiple behaviors)
pub struct CompositeBehavior {
    behaviors: Vec<Box<dyn Behavior>>,
}

impl Default for CompositeBehavior {
    fn default() -> Self {
        Self::new()
    }
}

impl CompositeBehavior {
    /// Creates a new empty composite behavior
    pub fn new() -> Self {
        CompositeBehavior { behaviors: vec![] }
    }

    /// Adds a behavior to the composite (builder pattern)
    #[allow(clippy::should_implement_trait)]
    pub fn add(mut self, behavior: Box<dyn Behavior>) -> Self {
        self.behaviors.push(behavior);
        self
    }
}

#[async_trait]
impl Behavior for CompositeBehavior {
    async fn process(&mut self, input: Input) -> Result<Output, BehaviorError> {
        let mut all_effects = vec![];
        let mut all_state_updates = vec![];
        let mut response = None;

        // Try each behavior in order
        for behavior in &mut self.behaviors {
            if behavior.handles().contains(&input.message_type)
                || behavior.handles().contains(&"*".to_string())
            {
                match behavior.process(input.clone()).await {
                    Ok(output) => {
                        all_effects.extend(output.effects);
                        all_state_updates.extend(output.state_updates);
                        if output.response.is_some() {
                            response = output.response;
                        }
                    }
                    Err(BehaviorError::NotHandled) => continue,
                    Err(e) => return Err(e),
                }
            }
        }

        if all_effects.is_empty() && all_state_updates.is_empty() && response.is_none() {
            return Err(BehaviorError::NotHandled);
        }

        Ok(Output {
            response,
            effects: all_effects,
            state_updates: all_state_updates,
        })
    }

    fn handles(&self) -> Vec<String> {
        self.behaviors.iter().flat_map(|b| b.handles()).collect()
    }
}

// ============================================================================
// BENEFITS OF THIS DESIGN
// ============================================================================

// 1. ONE trait to implement (not 5-10 different handler traits)
// 2. Pure functions - behaviors return effects, not execute them
// 3. Testable - can test behaviors without runtime
// 4. Composable - behaviors can be combined easily
// 5. Extensible - new effects can be added without changing behaviors

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_counter_behavior() {
        let mut behavior = CounterBehavior {};

        // Test increment
        let input = Input {
            message: vec![],
            message_type: "increment".to_string(),
            sender: None,
            state: State::default(),
            context: Context::default(),
        };

        let output = behavior.process(input).await.unwrap();
        assert_eq!(output.state_updates.len(), 1);
        assert!(matches!(
            &output.state_updates[0],
            StateUpdate::Set { key, .. } if key == "count"
        ));
    }

    #[tokio::test]
    async fn test_effects_not_side_effects() {
        // This test demonstrates that behaviors don't execute side effects,
        // they just return them as data

        let mut behavior = CounterBehavior {};

        let input = Input {
            message: vec![],
            message_type: "increment".to_string(),
            sender: Some("other-actor".to_string()),
            state: State::default(),
            context: Context::default(),
        };

        let output = behavior.process(input).await.unwrap();

        // The behavior returns effects, but doesn't execute them
        // This makes behaviors pure and testable
        assert!(output
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Log { .. })));

        // No actual logging happened - just returned as data
        // The runtime will execute these effects
    }
}
