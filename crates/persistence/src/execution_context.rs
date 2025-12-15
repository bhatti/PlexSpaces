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

//! Execution context for deterministic replay
//!
//! ## Purpose
//! Provides an execution context for actors that tracks side effects during normal
//! execution and validates their order during replay. This prevents replay bugs
//! caused by executing side effects in the wrong order.
//!
//! ## Why This Exists
//! - **Replay Correctness**: Side effects must execute in the same order during replay
//! - **Deterministic Execution**: Ensures actors produce consistent results across restarts
//! - **Bug Prevention**: Catches order violations early with clear error messages
//!
//! ## Architecture Context
//! This module is part of the PlexSpaces durability infrastructure (Pillar 3: Restate-inspired).
//! It integrates with the Journal system to enable deterministic replay after failures.
//!
//! ## Example Usage
//!
//! ```rust
//! use plexspaces_persistence::execution_context::{ExecutionContext, ExecutionMode};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Normal execution mode
//! let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
//!
//! // Record side effects in order
//! let user_id = ctx.record_side_effect("db_write", || {
//!     Ok("user-123".to_string())
//! }).await?;
//!
//! let email_sent = ctx.record_side_effect("send_email", || {
//!     Ok(true)
//! }).await?;
//!
//! // During replay, side effects return cached results
//! ctx.set_mode(ExecutionMode::Replay);
//! ctx.reset_sequence();
//!
//! // Returns cached "user-123" without executing function
//! let replayed_user_id = ctx.record_side_effect("db_write", || {
//!     panic!("Should not execute during replay")
//! }).await?;
//!
//! assert_eq!(user_id, replayed_user_id);
//! # Ok(())
//! # }
//! ```

use bytes::Bytes;
use std::collections::HashMap;

/// Execution context for deterministic replay
///
/// ## Purpose
/// Tracks side effects during actor execution to enable deterministic replay.
/// Validates side effect ordering to prevent replay bugs.
///
/// ## Side Effect Tracking
/// - **Normal Mode**: Executes side effects and caches results with sequence numbers
/// - **Replay Mode**: Returns cached results and validates execution order
///
/// ## Sequence Validation
/// Each side effect is assigned a monotonically increasing sequence number.
/// During replay, side effects MUST be replayed in the same order (same sequence).
pub struct ExecutionContext {
    /// Actor identifier
    actor_id: String,

    /// Current journal sequence number
    _current_sequence: u64,

    /// Execution mode (Normal or Replay)
    mode: ExecutionMode,

    /// Side effect sequence counter (monotonically increasing)
    side_effect_sequence: u64,

    /// Side effect cache: effect_id -> (sequence, result_bytes)
    side_effect_cache: HashMap<String, (u64, Bytes)>,
}

/// Execution mode for the context
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExecutionMode {
    /// Normal execution - execute side effects and cache results
    Normal,

    /// Replay execution - return cached results and validate order
    Replay,
}

impl ExecutionContext {
    /// Creates a new execution context
    ///
    /// ## Arguments
    /// * `actor_id` - Unique identifier for the actor
    /// * `mode` - Execution mode (Normal or Replay)
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_persistence::execution_context::{ExecutionContext, ExecutionMode};
    ///
    /// let ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
    /// ```
    pub fn new(actor_id: impl Into<String>, mode: ExecutionMode) -> Self {
        Self {
            actor_id: actor_id.into(),
            _current_sequence: 0,
            mode,
            side_effect_sequence: 0,
            side_effect_cache: HashMap::new(),
        }
    }

    /// Records a side effect during normal execution or returns cached result during replay
    ///
    /// ## Arguments
    /// * `effect_id` - Unique identifier for this side effect
    /// * `effect_fn` - Function to execute (only called in Normal mode)
    ///
    /// ## Returns
    /// The result of the side effect (executed or cached)
    ///
    /// ## Errors
    /// - [`ContextError::SideEffectOrderViolation`]: Side effect replayed out of order
    /// - [`ContextError::MissingSideEffect`]: Side effect not found in cache during replay
    /// - [`ContextError::SerializationError`]: Failed to serialize/deserialize result
    /// - [`ContextError::SideEffectExecutionError`]: Side effect function returned error
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_persistence::execution_context::{ExecutionContext, ExecutionMode};
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
    ///
    /// // Execute side effect and cache result
    /// let result = ctx.record_side_effect("db_write", || {
    ///     Ok(42u32)
    /// }).await?;
    ///
    /// assert_eq!(result, 42);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn record_side_effect<F, R>(
        &mut self,
        effect_id: &str,
        effect_fn: F,
    ) -> Result<R, ContextError>
    where
        F: FnOnce() -> Result<R, Box<dyn std::error::Error>>,
        R: serde::Serialize + serde::de::DeserializeOwned,
    {
        match self.mode {
            ExecutionMode::Normal => {
                // Execute side effect
                let result = effect_fn().map_err(|e| ContextError::SideEffectExecutionError {
                    effect_id: effect_id.to_string(),
                    error: e.to_string(),
                })?;

                // Serialize result
                let result_bytes = bincode::serialize(&result)
                    .map_err(|e| ContextError::SerializationError(e.to_string()))?;

                // Assign sequence number
                let sequence = self.side_effect_sequence;
                self.side_effect_sequence += 1;

                // Cache result with sequence
                self.side_effect_cache
                    .insert(effect_id.to_string(), (sequence, result_bytes.into()));

                // TODO: Journal side effect with sequence
                // self.journal_side_effect(effect_id, sequence, &result_bytes).await?;

                Ok(result)
            }

            ExecutionMode::Replay => {
                // Retrieve cached result
                let (expected_sequence, result_bytes) = self
                    .side_effect_cache
                    .get(effect_id)
                    .ok_or_else(|| ContextError::MissingSideEffect {
                        effect_id: effect_id.to_string(),
                    })?;

                // VALIDATE: Side effect replayed in correct order
                let actual_sequence = self.side_effect_sequence;
                if actual_sequence != *expected_sequence {
                    return Err(ContextError::SideEffectOrderViolation {
                        effect_id: effect_id.to_string(),
                        expected_sequence: *expected_sequence,
                        actual_sequence,
                    });
                }

                self.side_effect_sequence += 1;

                // Deserialize cached result
                let result: R = bincode::deserialize(result_bytes)
                    .map_err(|e| ContextError::SerializationError(e.to_string()))?;
                Ok(result)
            }
        }
    }

    /// Sets the execution mode
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_persistence::execution_context::{ExecutionContext, ExecutionMode};
    /// let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
    /// ctx.set_mode(ExecutionMode::Replay);
    /// ```
    pub fn set_mode(&mut self, mode: ExecutionMode) {
        self.mode = mode;
    }

    /// Resets the side effect sequence counter
    ///
    /// This is typically called before replay to start from sequence 0.
    ///
    /// ## Example
    /// ```rust
    /// # use plexspaces_persistence::execution_context::{ExecutionContext, ExecutionMode};
    /// let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
    /// // ... record some side effects ...
    /// ctx.reset_sequence();
    /// ```
    pub fn reset_sequence(&mut self) {
        self.side_effect_sequence = 0;
    }

    /// Returns the current execution mode
    pub fn mode(&self) -> ExecutionMode {
        self.mode
    }

    /// Returns the current side effect sequence number
    pub fn side_effect_sequence(&self) -> u64 {
        self.side_effect_sequence
    }

    /// Returns the actor ID
    pub fn actor_id(&self) -> &str {
        &self.actor_id
    }

    /// Returns the number of cached side effects
    pub fn cached_side_effects(&self) -> usize {
        self.side_effect_cache.len()
    }

    /// Clears the side effect cache
    ///
    /// This is typically used when starting a new execution from scratch.
    pub fn clear_cache(&mut self) {
        self.side_effect_cache.clear();
    }
}

/// Errors that can occur during execution context operations
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    /// Side effect executed out of order during replay
    #[error("Side effect '{effect_id}' executed out of order: expected sequence {expected_sequence}, got {actual_sequence}")]
    SideEffectOrderViolation {
        /// Side effect identifier
        effect_id: String,
        /// Expected sequence number (from cache)
        expected_sequence: u64,
        /// Actual sequence number (current replay position)
        actual_sequence: u64,
    },

    /// Side effect not found in cache during replay
    #[error("Side effect '{effect_id}' not found in cache during replay")]
    MissingSideEffect {
        /// Side effect identifier
        effect_id: String,
    },

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Side effect execution error
    #[error("Side effect '{effect_id}' execution failed: {error}")]
    SideEffectExecutionError {
        /// Side effect identifier
        effect_id: String,
        /// Error message
        error: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_side_effect_correct_order() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        // Record side effects in order
        let r1 = ctx
            .record_side_effect("effect-1", || Ok("result-1".to_string()))
            .await
            .unwrap();
        let r2 = ctx
            .record_side_effect("effect-2", || Ok("result-2".to_string()))
            .await
            .unwrap();

        assert_eq!(r1, "result-1");
        assert_eq!(r2, "result-2");

        // Verify sequences assigned
        assert_eq!(ctx.side_effect_cache.get("effect-1").unwrap().0, 0);
        assert_eq!(ctx.side_effect_cache.get("effect-2").unwrap().0, 1);
        assert_eq!(ctx.cached_side_effects(), 2);
    }

    #[tokio::test]
    async fn test_side_effect_replay_correct_order() {
        // Setup: Record side effects in normal mode
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
        ctx.record_side_effect("effect-1", || Ok(42u32))
            .await
            .unwrap();
        ctx.record_side_effect("effect-2", || Ok(99u32))
            .await
            .unwrap();

        // Replay in correct order
        ctx.set_mode(ExecutionMode::Replay);
        ctx.reset_sequence();

        let r1: u32 = ctx
            .record_side_effect("effect-1", || panic!("Should not execute"))
            .await
            .unwrap();
        let r2: u32 = ctx
            .record_side_effect("effect-2", || panic!("Should not execute"))
            .await
            .unwrap();

        assert_eq!(r1, 42u32);
        assert_eq!(r2, 99u32);
    }

    #[tokio::test]
    async fn test_side_effect_replay_wrong_order() {
        // Setup: Record side effects in normal mode
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
        ctx.record_side_effect("effect-1", || Ok(42u32))
            .await
            .unwrap();
        ctx.record_side_effect("effect-2", || Ok(99u32))
            .await
            .unwrap();

        // Replay in WRONG order (swap effect-1 and effect-2)
        ctx.set_mode(ExecutionMode::Replay);
        ctx.reset_sequence();

        // This should FAIL with SideEffectOrderViolation
        let result: Result<u32, _> = ctx
            .record_side_effect("effect-2", || panic!("Should not execute"))
            .await;

        assert!(matches!(
            result,
            Err(ContextError::SideEffectOrderViolation {
                effect_id,
                expected_sequence: 1,  // effect-2 was recorded at sequence 1
                actual_sequence: 0,    // But replayed at sequence 0
            }) if effect_id == "effect-2"
        ));
    }

    #[tokio::test]
    async fn test_side_effect_missing_from_cache() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Replay);

        // Replay side effect that was never recorded
        let result = ctx.record_side_effect("unknown-effect", || Ok(42u32)).await;

        assert!(matches!(
            result,
            Err(ContextError::MissingSideEffect { effect_id }) if effect_id == "unknown-effect"
        ));
    }

    #[tokio::test]
    async fn test_context_creation() {
        let ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
        assert_eq!(ctx.actor_id(), "actor-123");
        assert_eq!(ctx.mode(), ExecutionMode::Normal);
        assert_eq!(ctx.side_effect_sequence(), 0);
        assert_eq!(ctx.cached_side_effects(), 0);
    }

    #[tokio::test]
    async fn test_mode_switching() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);
        assert_eq!(ctx.mode(), ExecutionMode::Normal);

        ctx.set_mode(ExecutionMode::Replay);
        assert_eq!(ctx.mode(), ExecutionMode::Replay);
    }

    #[tokio::test]
    async fn test_sequence_reset() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        // Record some side effects
        ctx.record_side_effect("effect-1", || Ok(42u32))
            .await
            .unwrap();
        ctx.record_side_effect("effect-2", || Ok(99u32))
            .await
            .unwrap();

        assert_eq!(ctx.side_effect_sequence(), 2);

        // Reset sequence
        ctx.reset_sequence();
        assert_eq!(ctx.side_effect_sequence(), 0);

        // Cache should still contain effects
        assert_eq!(ctx.cached_side_effects(), 2);
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        // Record some side effects
        ctx.record_side_effect("effect-1", || Ok(42u32))
            .await
            .unwrap();
        ctx.record_side_effect("effect-2", || Ok(99u32))
            .await
            .unwrap();

        assert_eq!(ctx.cached_side_effects(), 2);

        // Clear cache
        ctx.clear_cache();
        assert_eq!(ctx.cached_side_effects(), 0);
    }

    #[tokio::test]
    async fn test_side_effect_execution_error() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        // Side effect that returns error
        let result: Result<u32, _> = ctx
            .record_side_effect("failing-effect", || Err("intentional error".into()))
            .await;

        assert!(matches!(
            result,
            Err(ContextError::SideEffectExecutionError { effect_id, error })
            if effect_id == "failing-effect" && error == "intentional error"
        ));
    }

    #[tokio::test]
    async fn test_complex_type_serialization() {
        #[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
        struct ComplexType {
            id: u64,
            name: String,
            values: Vec<i32>,
        }

        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        let complex_data = ComplexType {
            id: 123,
            name: "test".to_string(),
            values: vec![1, 2, 3, 4, 5],
        };

        // Record complex type
        let result = ctx
            .record_side_effect("complex-effect", || Ok(complex_data.clone()))
            .await
            .unwrap();

        assert_eq!(result, complex_data);

        // Replay and verify deserialization
        ctx.set_mode(ExecutionMode::Replay);
        ctx.reset_sequence();

        let replayed_result: ComplexType = ctx
            .record_side_effect("complex-effect", || panic!("Should not execute"))
            .await
            .unwrap();

        assert_eq!(replayed_result, complex_data);
    }

    #[tokio::test]
    async fn test_multiple_effects_different_types() {
        let mut ctx = ExecutionContext::new("actor-123", ExecutionMode::Normal);

        // Record side effects of different types
        let r1 = ctx
            .record_side_effect("string-effect", || Ok("hello".to_string()))
            .await
            .unwrap();
        let r2 = ctx
            .record_side_effect("number-effect", || Ok(42u32))
            .await
            .unwrap();
        let r3 = ctx
            .record_side_effect("bool-effect", || Ok(true))
            .await
            .unwrap();

        assert_eq!(r1, "hello");
        assert_eq!(r2, 42u32);
        assert_eq!(r3, true);

        // Replay all in order
        ctx.set_mode(ExecutionMode::Replay);
        ctx.reset_sequence();

        let replayed_r1: String = ctx
            .record_side_effect("string-effect", || panic!("Should not execute"))
            .await
            .unwrap();
        let replayed_r2: u32 = ctx
            .record_side_effect("number-effect", || panic!("Should not execute"))
            .await
            .unwrap();
        let replayed_r3: bool = ctx
            .record_side_effect("bool-effect", || panic!("Should not execute"))
            .await
            .unwrap();

        assert_eq!(replayed_r1, "hello");
        assert_eq!(replayed_r2, 42u32);
        assert_eq!(replayed_r3, true);
    }

    #[tokio::test]
    async fn test_execution_mode_equality() {
        assert_eq!(ExecutionMode::Normal, ExecutionMode::Normal);
        assert_eq!(ExecutionMode::Replay, ExecutionMode::Replay);
        assert_ne!(ExecutionMode::Normal, ExecutionMode::Replay);
    }
}
