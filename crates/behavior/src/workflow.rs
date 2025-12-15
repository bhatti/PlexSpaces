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

//! Workflow execution context for durable execution
//!
//! Implements Restate-inspired durable execution with:
//! - ExecutionContext with ctx.run() and ctx.sleep() patterns
//! - Deterministic replay
//! - Durable promises
//! - Side effect recording
//!
//! Note: WorkflowBehavior itself is defined in mod.rs

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use plexspaces_core::{ActorId, BehaviorError};
use plexspaces_persistence::{
    Journal, JournalEntry, JournalError, PromiseMetadata, PromiseResult, SideEffect,
};

/// Execution context for durable workflows (Restate-inspired)
///
/// Provides ctx.run(), ctx.sleep(), and promise operations
/// All operations are journaled for deterministic replay
pub struct ExecutionContext {
    actor_id: ActorId,
    journal: Arc<dyn Journal>,
    /// Replay mode - true during recovery/replay
    replay_mode: bool,
    /// Current sequence in replay
    replay_sequence: u64,
    /// Cached side effects from journal (for deterministic replay)
    cached_effects: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(actor_id: ActorId, journal: Arc<dyn Journal>) -> Self {
        ExecutionContext {
            actor_id,
            journal,
            replay_mode: false,
            replay_sequence: 0,
            cached_effects: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Enter replay mode with cached effects
    pub async fn enter_replay_mode(&mut self, from_sequence: u64) -> Result<(), JournalError> {
        self.replay_mode = true;
        self.replay_sequence = from_sequence;

        // Load cached effects from journal
        let entries = self
            .journal
            .replay_from(&self.actor_id, from_sequence)
            .await?;
        let mut effects = self.cached_effects.write().await;

        for entry in entries {
            if let JournalEntry::SideEffectRecorded { effect, .. } = entry {
                match effect {
                    SideEffect::ExternalCall {
                        service,
                        method,
                        response: Some(resp),
                        ..
                    } => {
                        // Match the key format used in ctx.run()
                        let key = if service == "ctx" {
                            format!("run:{}", method)
                        } else {
                            format!("{}:{}", service, method)
                        };
                        effects.insert(key, resp);
                    }
                    SideEffect::ExternalCall { .. } => {} // No response
                    SideEffect::RandomGenerated { value } => {
                        effects.insert("random".to_string(), value);
                    }
                    SideEffect::TimeAccessed { timestamp } => {
                        let time_bytes = timestamp.timestamp().to_le_bytes().to_vec();
                        effects.insert("time".to_string(), time_bytes);
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }

    /// Exit replay mode
    pub fn exit_replay_mode(&mut self) {
        self.replay_mode = false;
        self.replay_sequence = 0;
    }

    /// ctx.run() - Execute an idempotent operation
    ///
    /// During normal execution: runs the operation and journals the result
    /// During replay: returns the cached result from journal
    ///
    /// This is the core pattern from Restate for making any operation durable
    pub async fn run<F, T, E>(&self, name: &str, operation: F) -> Result<T, BehaviorError>
    where
        F: FnOnce() -> Result<T, E> + Send,
        T: Serialize + for<'de> Deserialize<'de> + Send,
        E: std::fmt::Display,
    {
        let key = format!("run:{}", name);

        // Check if we're in replay mode
        if self.replay_mode {
            let effects = self.cached_effects.read().await;
            if let Some(cached) = effects.get(&key) {
                // Return cached result
                let result: T = serde_json::from_slice(cached).map_err(|e| {
                    BehaviorError::ProcessingError(format!(
                        "Failed to deserialize cached result: {}",
                        e
                    ))
                })?;
                return Ok(result);
            }
        }

        // Execute the operation
        let result = operation()
            .map_err(|e| BehaviorError::ProcessingError(format!("Operation failed: {}", e)))?;

        // Journal the result
        let result_bytes = serde_json::to_vec(&result).map_err(|e| {
            BehaviorError::ProcessingError(format!("Failed to serialize result: {}", e))
        })?;

        self.journal
            .record_side_effect(
                &self.actor_id,
                SideEffect::ExternalCall {
                    service: "ctx".to_string(),
                    method: name.to_string(),
                    request: vec![],
                    response: Some(result_bytes.clone()),
                },
            )
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to journal result: {}", e))
            })?;

        // Cache for this execution
        self.cached_effects.write().await.insert(key, result_bytes);

        Ok(result)
    }

    /// ctx.sleep() - Durable delay
    ///
    /// During normal execution: schedules a timer and journals it
    /// During replay: skips the actual sleep but honors the journal entry
    pub async fn sleep(&self, duration: Duration) -> Result<(), BehaviorError> {
        let duration_ms = duration.num_milliseconds() as u64;

        // Journal the sleep
        self.journal
            .record_side_effect(&self.actor_id, SideEffect::Sleep { duration_ms })
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to journal sleep: {}", e))
            })?;

        // Only actually sleep if not in replay mode
        if !self.replay_mode {
            tokio::time::sleep(tokio::time::Duration::from_millis(duration_ms)).await;
        }

        Ok(())
    }

    /// Get current time (deterministic during replay)
    pub async fn now(&self) -> Result<DateTime<Utc>, BehaviorError> {
        if self.replay_mode {
            // Return cached time from journal
            let effects = self.cached_effects.read().await;
            if let Some(time_bytes) = effects.get("time") {
                let timestamp = i64::from_le_bytes(time_bytes[..8].try_into().map_err(|e| {
                    BehaviorError::ProcessingError(format!("Invalid timestamp: {:?}", e))
                })?);
                return Ok(DateTime::from_timestamp(timestamp, 0).unwrap_or_else(Utc::now));
            }
        }

        let now = Utc::now();

        // Journal the time access
        self.journal
            .record_side_effect(&self.actor_id, SideEffect::TimeAccessed { timestamp: now })
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to journal time: {}", e))
            })?;

        Ok(now)
    }

    /// Create a durable promise
    pub async fn promise(
        &self,
        promise_id: &str,
        timeout_ms: Option<u64>,
    ) -> Result<(), BehaviorError> {
        self.journal
            .record_promise_created(
                promise_id,
                PromiseMetadata {
                    creator_id: self.actor_id.clone(),
                    timeout_ms,
                    idempotency_key: None,
                },
            )
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to create promise: {}", e))
            })?;

        Ok(())
    }

    /// Resolve a promise
    pub async fn resolve_promise(
        &self,
        promise_id: &str,
        result: PromiseResult,
    ) -> Result<(), BehaviorError> {
        self.journal
            .record_promise_resolved(promise_id, result)
            .await
            .map_err(|e| {
                BehaviorError::ProcessingError(format!("Failed to resolve promise: {}", e))
            })?;

        Ok(())
    }
}

// Note: Workflow (previously WorkflowBehavior), WorkflowRunHandler, WorkflowSignalHandler, and WorkflowQueryHandler
// are defined in ../mod.rs to avoid duplication.
// This module focuses on ExecutionContext and durable execution patterns.

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_persistence::MemoryJournal;

    #[tokio::test]
    async fn test_workflow_execution_context() {
        let actor_id = "test-workflow".to_string();
        let journal = Arc::new(MemoryJournal::new());
        let ctx = ExecutionContext::new(actor_id.clone(), journal.clone());

        // Test ctx.run() pattern
        let result = ctx
            .run("test_op", || -> Result<String, String> {
                Ok("hello".to_string())
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "hello");

        // Verify journal entry
        let entries = journal.get_entries().await.unwrap();
        assert!(!entries.is_empty());
    }

    #[tokio::test]
    async fn test_workflow_deterministic_replay() {
        let actor_id = "test-workflow".to_string();
        let journal = Arc::new(MemoryJournal::new());

        // First execution
        {
            let ctx = ExecutionContext::new(actor_id.clone(), journal.clone());
            let _result = ctx
                .run("random_op", || -> Result<u32, String> {
                    Ok(42) // Use deterministic value for testing
                })
                .await
                .unwrap();
        }

        // Replay execution
        {
            let mut ctx = ExecutionContext::new(actor_id.clone(), journal.clone());
            ctx.enter_replay_mode(1).await.unwrap();

            let result = ctx
                .run("random_op", || -> Result<u32, String> {
                    Ok(99) // This won't actually run during replay
                })
                .await
                .unwrap();

            // Should get the cached result (42), not the new value (99)
            assert_eq!(result, 42);
        }
    }

    #[tokio::test]
    async fn test_workflow_promise() {
        let actor_id = "test-workflow".to_string();
        let journal = Arc::new(MemoryJournal::new());
        let ctx = ExecutionContext::new(actor_id.clone(), journal.clone());

        // Create promise
        ctx.promise("test-promise", Some(5000)).await.unwrap();

        // Resolve promise
        ctx.resolve_promise("test-promise", PromiseResult::Fulfilled(b"result".to_vec()))
            .await
            .unwrap();

        // Verify journal entries
        let entries = journal.get_entries().await.unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_ctx_sleep() {
        let actor_id = "test-workflow".to_string();
        let journal = Arc::new(MemoryJournal::new());
        let ctx = ExecutionContext::new(actor_id.clone(), journal.clone());

        // Test sleep in replay mode (should not actually sleep)
        let mut ctx_replay = ExecutionContext::new(actor_id.clone(), journal.clone());
        ctx_replay.replay_mode = true;

        ctx_replay
            .sleep(Duration::milliseconds(1000))
            .await
            .unwrap();

        // Verify sleep was journaled
        let entries = journal.get_entries().await.unwrap();
        assert!(!entries.is_empty());
    }

    #[tokio::test]
    async fn test_ctx_now() {
        let actor_id = "test-workflow".to_string();
        let journal = Arc::new(MemoryJournal::new());
        let ctx = ExecutionContext::new(actor_id.clone(), journal.clone());

        // Get current time
        let time1 = ctx.now().await.unwrap();
        assert!(time1.timestamp() > 0);

        // Verify time access was journaled
        let entries = journal.get_entries().await.unwrap();
        assert!(!entries.is_empty());
    }
}
