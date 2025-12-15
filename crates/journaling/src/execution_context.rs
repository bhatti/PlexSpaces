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

//! Execution context for deterministic replay (RESTATE-inspired)
//!
//! ## Purpose
//! Provides side effect caching to enable deterministic actor replay.
//! When replaying from journal, cached results are returned instead of
//! re-executing side effects (HTTP calls, database queries, random numbers, etc.).
//!
//! ## Architecture Context
//! Used by DurabilityFacet to wrap actor message processing:
//! - **Normal mode**: Execute side effects, cache results in journal
//! - **Replay mode**: Return cached results, don't re-execute
//!
//! ## RESTATE Pattern
//! Inspired by RESTATE's execution context:
//! - Side effects are wrapped in `record_side_effect()` calls
//! - Results are journaled as `SideEffectExecuted` entries
//! - On replay, cached results are returned without re-execution
//! - Guarantees exactly-once execution semantics
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use prost::Message;
//!
//! # #[derive(Clone, PartialEq, Message)]
//! # struct ExampleData { #[prost(uint32, tag = "1")] pub value: u32 }
//! #
//! # async fn example() -> JournalResult<()> {
//! # use prost::Message; // Need this for decode()
//! // Normal execution - executes side effects and caches results
//! let ctx = ExecutionContextImpl::new("actor-123", ExecutionMode::ExecutionModeNormal);
//!
//! // Record side effect (simulating external API call)
//! let result_bytes: Vec<u8> = ctx.record_side_effect(
//!     "api_call_1",
//!     "external_api",
//!     || async { Ok(ExampleData { value: 42 }) }
//! ).await?;
//!
//! // Side effect executed, result is serialized protobuf bytes
//! let data = ExampleData::decode(result_bytes.as_slice())
//!     .map_err(|e| JournalError::Serialization(e.to_string()))?;
//! assert_eq!(data.value, 42);
//!
//! // In replay mode, the cached result would be returned without
//! // re-executing the closure. The DurabilityFacet handles populating
//! // the cache from journal entries during actor recovery.
//! # Ok(())
//! # }
//! ```

use crate::{ExecutionContext, ExecutionMode, JournalError, JournalResult, SideEffectEntry};
use plexspaces_proto::prost_types;
use prost::Message;
use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;

// Observability
use metrics;
use tracing;

/// Implementation of ExecutionContext with side effect caching
///
/// ## Purpose
/// Wraps actor execution to intercept and cache side effects.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to side effect cache.
///
/// ## Design
/// - Normal mode: Executes closure, stores result in cache
/// - Replay mode: Returns cached result, closure NOT executed
pub struct ExecutionContextImpl {
    /// Proto-defined execution context
    inner: Arc<RwLock<ExecutionContext>>,

    /// Journal entries created during this execution (side effects)
    side_effects: Arc<RwLock<Vec<SideEffectEntry>>>,
}

impl ExecutionContextImpl {
    /// Create a new execution context
    ///
    /// ## Arguments
    /// * `actor_id` - Actor this context belongs to
    /// * `mode` - Execution mode (NORMAL or REPLAY)
    ///
    /// ## Returns
    /// New ExecutionContext ready for use
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_journaling::*;
    /// let ctx = ExecutionContextImpl::new("actor-123", ExecutionMode::ExecutionModeNormal);
    /// ```
    pub fn new(actor_id: impl Into<String>, mode: ExecutionMode) -> Self {
        Self {
            inner: Arc::new(RwLock::new(ExecutionContext {
                actor_id: actor_id.into(),
                current_sequence: 0,
                mode: mode.into(),
                side_effect_cache: HashMap::new(),
                metadata: HashMap::new(),
            })),
            side_effects: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set the current sequence number being executed
    ///
    /// ## Arguments
    /// * `sequence` - Sequence number from journal entry
    ///
    /// ## Purpose
    /// Tracks which journal entry is currently being processed.
    /// Used for debugging and side effect correlation.
    pub async fn set_sequence(&self, sequence: u64) {
        let mut inner = self.inner.write().await;
        inner.current_sequence = sequence;
    }

    /// Get the current sequence number
    pub async fn sequence(&self) -> u64 {
        let inner = self.inner.read().await;
        inner.current_sequence
    }

    /// Get the execution mode
    pub async fn mode(&self) -> ExecutionMode {
        let inner = self.inner.read().await;
        ExecutionMode::try_from(inner.mode).unwrap_or(ExecutionMode::ExecutionModeUnspecified)
    }

    /// Set the execution mode
    ///
    /// ## Purpose
    /// Switches execution mode between NORMAL and REPLAY.
    /// Used after replaying journal to switch back to normal execution.
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_journaling::*;
    /// # async fn example() {
    /// let ctx = ExecutionContextImpl::new("actor-123", ExecutionMode::ExecutionModeReplay);
    /// // ... replay journal ...
    /// ctx.set_mode(ExecutionMode::ExecutionModeNormal).await;  // Switch to normal mode
    /// # }
    /// ```
    pub async fn set_mode(&self, mode: ExecutionMode) {
        let mut inner = self.inner.write().await;
        inner.mode = mode as i32;
    }

    /// Load side effects from journal entries (for replay mode)
    ///
    /// ## Arguments
    /// * `entries` - Side effect entries from journal replay
    ///
    /// ## Purpose
    /// Populates the side effect cache with results from previous execution.
    /// Must be called before replaying actor logic.
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example(journal_entries: Vec<SideEffectEntry>) {
    /// let mut ctx = ExecutionContextImpl::new("actor-123", ExecutionMode::ExecutionModeReplay);
    /// ctx.load_side_effects(journal_entries).await;
    /// // Now ctx.record_side_effect() will return cached results
    /// # }
    /// ```
    pub async fn load_side_effects(&self, entries: Vec<SideEffectEntry>) {
        let mut inner = self.inner.write().await;

        for entry in entries {
            inner
                .side_effect_cache
                .insert(entry.side_effect_id.clone(), entry.output_data.clone());
        }
    }

    /// Record a side effect with caching for deterministic replay
    ///
    /// ## Arguments
    /// * `side_effect_id` - Unique ID for this side effect (e.g., "http_get_1")
    /// * `side_effect_type` - Type of side effect (e.g., "http_request")
    /// * `executor` - Async closure that executes the side effect
    ///
    /// ## Returns
    /// Result of the side effect (from cache in REPLAY mode, from execution in NORMAL mode)
    ///
    /// ## Behavior
    /// - **NORMAL mode**: Execute closure, cache result, return result
    /// - **REPLAY mode**: Return cached result, closure NOT executed
    ///
    /// ## Errors
    /// - `JournalError::SideEffectNotFound` if replay mode and side effect not in cache
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # use prost::Message;
    /// #
    /// # #[derive(Clone, PartialEq, Message)]
    /// # struct UserData { #[prost(string, tag = "1")] pub name: String }
    /// #
    /// # async fn example(ctx: &ExecutionContextImpl) -> JournalResult<()> {
    /// # use prost::Message; // Need this for decode()
    /// // Record side effect with closure returning future
    /// let user_bytes = ctx.record_side_effect(
    ///     "fetch_user_1",
    ///     "http_request",
    ///     || async {
    ///         // This closure only executes in NORMAL mode
    ///         // In REPLAY mode, cached result is returned
    ///         Ok(UserData { name: "Alice".to_string() })
    ///     }
    /// ).await?;
    ///
    /// // Deserialize result using prost::Message trait
    /// let user = UserData::decode(user_bytes.as_slice())
    ///     .map_err(|e| JournalError::Serialization(e.to_string()))?;
    /// assert_eq!(user.name, "Alice");
    /// Ok(())
    /// # }
    /// ```
    pub async fn record_side_effect<F, Fut, T>(
        &self,
        side_effect_id: impl Into<String>,
        side_effect_type: impl Into<String>,
        executor: F,
    ) -> JournalResult<Vec<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<T, Box<dyn std::error::Error + Send + Sync>>>,
        T: Message + Default,
    {
        let side_effect_id = side_effect_id.into();
        let side_effect_type = side_effect_type.into();

        let mode = self.mode().await;

        match mode {
            ExecutionMode::ExecutionModeReplay => {
                // REPLAY mode: Return cached result
                let inner = self.inner.read().await;

                inner
                    .side_effect_cache
                    .get(&side_effect_id)
                    .cloned()
                    .ok_or_else(|| {
                        JournalError::Serialization(format!(
                            "Side effect not found in cache during replay: {}",
                            side_effect_id
                        ))
                    })
            }

            ExecutionMode::ExecutionModeNormal | ExecutionMode::ExecutionModeUnspecified => {
                // NORMAL mode: Execute side effect and cache result

                // Execute the side effect
                let start = std::time::Instant::now();
                let result = executor()
                    .await
                    .map_err(|e| JournalError::Serialization(e.to_string()))?;
                let duration = start.elapsed();

                // Serialize result to bytes
                let mut output_bytes = Vec::new();
                result
                    .encode(&mut output_bytes)
                    .map_err(|e| JournalError::Serialization(e.to_string()))?;

                // Store in cache
                {
                    let mut inner = self.inner.write().await;
                    inner
                        .side_effect_cache
                        .insert(side_effect_id.clone(), output_bytes.clone());
                    
                    // Record metrics
                    metrics::counter!("plexspaces_journaling_side_effects_executed_total",
                        "side_effect_type" => side_effect_type.clone(),
                        "actor_id" => inner.actor_id.clone()
                    ).increment(1);
                    metrics::histogram!("plexspaces_journaling_side_effect_execution_duration_seconds",
                        "side_effect_type" => side_effect_type.clone(),
                        "actor_id" => inner.actor_id.clone()
                    ).record(duration.as_secs_f64());
                    tracing::debug!(
                        actor_id = %inner.actor_id,
                        side_effect_id = %side_effect_id,
                        side_effect_type = %side_effect_type,
                        duration_ms = duration.as_millis(),
                        "Side effect executed and cached"
                    );
                }

                // Create side effect entry for journal
                let entry = SideEffectEntry {
                    side_effect_id: side_effect_id.clone(),
                    side_effect_type: side_effect_type.clone(),
                    input_data: vec![], // Could serialize input if needed
                    output_data: output_bytes.clone(),
                    executed_at: Some(prost_types::Timestamp {
                        seconds: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64,
                        nanos: 0,
                    }),
                    metadata: HashMap::new(),
                };

                // Record for later journaling
                {
                    let mut side_effects = self.side_effects.write().await;
                    side_effects.push(entry);
                }

                Ok(output_bytes)
            }
        }
    }

    /// Record a side effect with raw bytes (no protobuf serialization)
    ///
    /// This is a simpler version of `record_side_effect` that works with raw bytes
    /// instead of requiring protobuf Message types.
    ///
    /// ## Arguments
    /// * `side_effect_id` - Unique identifier for deterministic replay
    /// * `side_effect_type` - Type/category of side effect
    /// * `executor` - Async function that performs the side effect and returns bytes
    ///
    /// ## Returns
    /// Cached bytes in REPLAY mode, or executed bytes in NORMAL mode
    ///
    /// ## Example
    /// ```ignore
    /// let result = ctx.record_side_effect_raw(
    ///     "api_call_1",
    ///     "http_request",
    ///     || async {
    ///         let response = http_client.get("https://api.example.com").await?;
    ///         serde_json::to_vec(&response)
    ///             .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
    ///     }
    /// ).await?;
    /// ```
    pub async fn record_side_effect_raw<F, Fut>(
        &self,
        side_effect_id: impl Into<String>,
        side_effect_type: impl Into<String>,
        executor: F,
    ) -> JournalResult<Vec<u8>>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>>>,
    {
        let side_effect_id = side_effect_id.into();
        let side_effect_type = side_effect_type.into();

        let mode = self.mode().await;

        match mode {
            ExecutionMode::ExecutionModeReplay => {
                // REPLAY mode: Return cached result
                let inner = self.inner.read().await;

                inner
                    .side_effect_cache
                    .get(&side_effect_id)
                    .cloned()
                    .ok_or_else(|| {
                        JournalError::Serialization(format!(
                            "Side effect not found in cache during replay: {}",
                            side_effect_id
                        ))
                    })
            }

            ExecutionMode::ExecutionModeNormal | ExecutionMode::ExecutionModeUnspecified => {
                // NORMAL mode: Execute side effect and cache result

                // Execute the side effect (returns bytes directly)
                let output_bytes = executor()
                    .await
                    .map_err(|e| JournalError::Serialization(e.to_string()))?;

                // Store in cache
                {
                    let mut inner = self.inner.write().await;
                    inner
                        .side_effect_cache
                        .insert(side_effect_id.clone(), output_bytes.clone());
                }

                // Create side effect entry for journal
                let entry = SideEffectEntry {
                    side_effect_id: side_effect_id.clone(),
                    side_effect_type: side_effect_type.clone(),
                    input_data: vec![], // Could serialize input if needed
                    output_data: output_bytes.clone(),
                    executed_at: Some(prost_types::Timestamp {
                        seconds: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs() as i64,
                        nanos: 0,
                    }),
                    metadata: HashMap::new(),
                };

                // Record for later journaling
                {
                    let mut side_effects = self.side_effects.write().await;
                    side_effects.push(entry);
                }

                Ok(output_bytes)
            }
        }
    }

    /// Get all side effect entries created during this execution
    ///
    /// ## Returns
    /// Vec of side effect entries to be written to journal
    ///
    /// ## Purpose
    /// Called by DurabilityFacet to write side effects to journal.
    pub async fn take_side_effects(&self) -> Vec<SideEffectEntry> {
        let mut side_effects = self.side_effects.write().await;
        std::mem::take(&mut *side_effects)
    }

    /// Clear all cached side effects (for testing)
    #[cfg(test)]
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.side_effect_cache.clear();

        let mut side_effects = self.side_effects.write().await;
        side_effects.clear();
    }

    /// Get number of cached side effects (for testing)
    #[cfg(test)]
    pub async fn cache_size(&self) -> usize {
        let inner = self.inner.read().await;
        inner.side_effect_cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, PartialEq, prost::Message)]
    struct TestResult {
        #[prost(string, tag = "1")]
        value: String,
    }

    #[tokio::test]
    async fn test_normal_mode_executes_side_effect() {
        let ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeNormal);

        let result = ctx
            .record_side_effect("test_1", "test", || async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: "executed".to_string(),
                })
            })
            .await
            .unwrap();

        // Verify result was returned
        let decoded = TestResult::decode(&result[..]).unwrap();
        assert_eq!(decoded.value, "executed");

        // Verify side effect was cached
        assert_eq!(ctx.cache_size().await, 1);

        // Verify side effect entry was created
        let side_effects = ctx.take_side_effects().await;
        assert_eq!(side_effects.len(), 1);
        assert_eq!(side_effects[0].side_effect_id, "test_1");
        assert_eq!(side_effects[0].side_effect_type, "test");
    }

    #[tokio::test]
    async fn test_replay_mode_returns_cached_result() {
        let ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeReplay);

        // Simulate cached side effect from journal
        let expected = TestResult {
            value: "cached".to_string(),
        };
        let mut expected_bytes = Vec::new();
        expected.encode(&mut expected_bytes).unwrap();

        let cached_entry = SideEffectEntry {
            side_effect_id: "test_1".to_string(),
            side_effect_type: "test".to_string(),
            input_data: vec![],
            output_data: expected_bytes.clone(),
            executed_at: None,
            metadata: HashMap::new(),
        };

        ctx.load_side_effects(vec![cached_entry]).await;

        // Record side effect - should NOT execute closure, return cached
        let mut executed = false;
        let result = ctx
            .record_side_effect("test_1", "test", || async {
                executed = true; // Should NOT be set
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: "should_not_execute".to_string(),
                })
            })
            .await
            .unwrap();

        // Verify closure was NOT executed
        assert!(!executed, "Side effect should not execute in REPLAY mode");

        // Verify cached result was returned
        let decoded = TestResult::decode(&result[..]).unwrap();
        assert_eq!(decoded.value, "cached");
    }

    #[tokio::test]
    async fn test_replay_mode_missing_cache_entry() {
        let ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeReplay);

        // No cached entries loaded

        // Try to record side effect - should fail
        let result = ctx
            .record_side_effect("test_1", "test", || async {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: "value".to_string(),
                })
            })
            .await;

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Side effect not found"));
    }

    #[tokio::test]
    async fn test_multiple_side_effects() {
        let ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeNormal);

        // Record multiple side effects
        for i in 1..=3 {
            ctx.record_side_effect(format!("test_{}", i), "test", || async move {
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: format!("result_{}", i),
                })
            })
            .await
            .unwrap();
        }

        // Verify all cached
        assert_eq!(ctx.cache_size().await, 3);

        // Verify all side effect entries created
        let side_effects = ctx.take_side_effects().await;
        assert_eq!(side_effects.len(), 3);
        assert_eq!(side_effects[0].side_effect_id, "test_1");
        assert_eq!(side_effects[1].side_effect_id, "test_2");
        assert_eq!(side_effects[2].side_effect_id, "test_3");
    }

    #[tokio::test]
    async fn test_sequence_tracking() {
        let ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeNormal);

        assert_eq!(ctx.sequence().await, 0);

        ctx.set_sequence(42).await;
        assert_eq!(ctx.sequence().await, 42);

        ctx.set_sequence(100).await;
        assert_eq!(ctx.sequence().await, 100);
    }

    #[tokio::test]
    async fn test_deterministic_replay() {
        // Normal execution
        let normal_ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeNormal);

        let normal_result = normal_ctx
            .record_side_effect("random_1", "random", || async {
                // Simulate non-deterministic operation
                let random_value = std::time::SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
                    .to_string();

                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: random_value,
                })
            })
            .await
            .unwrap();

        let side_effects = normal_ctx.take_side_effects().await;

        // Replay execution
        let replay_ctx = ExecutionContextImpl::new("actor-1", ExecutionMode::ExecutionModeReplay);
        replay_ctx.load_side_effects(side_effects).await;

        let replay_result = replay_ctx
            .record_side_effect("random_1", "random", || async {
                // Different random value - should NOT be used
                Ok::<_, Box<dyn std::error::Error + Send + Sync>>(TestResult {
                    value: "different_value".to_string(),
                })
            })
            .await
            .unwrap();

        // Results should be identical (deterministic)
        assert_eq!(normal_result, replay_result);
    }
}
