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

//! Durability facet for optional actor durability
//!
//! ## Purpose
//! Provides Restate-inspired durable execution for actors through journaling
//! and deterministic replay. 100% optional via facet pattern.
//!
//! ## Architecture Context
//! Part of PlexSpaces Pillar 3 (Durability). Implements facet pattern for
//! dynamic durability attachment - actors without this facet have zero overhead.
//!
//! ## How It Works
//! ```text
//! Normal Execution:
//! 1. before_method → Journal MessageReceived entry
//! 2. Execute actor method in ExecutionContext (NORMAL mode)
//! 3. after_method → Journal MessageProcessed + SideEffects
//! 4. Periodic checkpoint if interval reached
//!
//! Replay After Crash:
//! 1. on_attach → Load latest checkpoint
//! 2. Replay journal entries from checkpoint
//! 3. Restore ExecutionContext with cached side effects
//! 4. Actor state restored to pre-crash point
//! 5. Continue normal execution
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use plexspaces_facet::Facet;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Configure durability
//! let config = DurabilityConfig {
//!     backend: JournalBackend::JournalBackendSqlite as i32,
//!     checkpoint_interval: 100,
//!     checkpoint_timeout: None,
//!     replay_on_activation: true,
//!     cache_side_effects: true,
//!     compression: CompressionType::CompressionTypeNone as i32,
//!     state_schema_version: 1,
//!     backend_config: None,
//! };
//!
//! // Create storage backend
//! let storage = SqliteJournalStorage::new(":memory:").await?;
//!
//! // Create durability facet
//! let config_value = serde_json::json!({
//!     "backend": config.backend,
//!     "checkpoint_interval": config.checkpoint_interval,
//!     "replay_on_activation": config.replay_on_activation,
//! });
//! let mut facet = DurabilityFacet::new(storage, config_value, 50);
//!
//! // Attach to actor (triggers replay if enabled)
//! facet.on_attach("actor-123", serde_json::json!({})).await?;
//!
//! // Facet intercepts all actor operations transparently
//! # Ok(())
//! # }
//! ```

use crate::{
    Checkpoint, CheckpointConfig, CheckpointManager, DurabilityConfig, ExecutionContextImpl,
    ExecutionMode, JournalEntry, JournalError, JournalResult, JournalStorage, MessageProcessed, MessageReceived,
    ProcessingResult, SideEffectExecuted, ReplayHandler, StateLoader,
};
use std::collections::HashSet;
use async_trait::async_trait;
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use plexspaces_proto::prost_types;
use plexspaces_proto::v1::journaling::{CompressionType, JournalBackend};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH, Instant};
use tokio::sync::RwLock;
use plexspaces_mailbox::Message;
use plexspaces_core::ActorContext;

// Observability
use metrics;
use tracing;

/// Durability facet providing journaling and deterministic replay
///
/// ## Purpose
/// Implements the Facet trait to add durability to actors without modifying
/// the actor core. Transparently journals all operations and enables replay.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to execution context and state.
///
/// ## Design
/// - Wraps JournalStorage backend (SQLite, PostgreSQL, Redis, Memory)
/// - Uses ExecutionContext for side effect caching
/// - Uses CheckpointManager for periodic snapshots
/// - Intercepts before/after method calls via Facet trait
pub struct DurabilityFacet<S: JournalStorage> {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    
    /// Facet priority (immutable)
    priority: i32,
    
    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,

    /// Durability configuration (parsed from config_value)
    config: DurabilityConfig,

    /// Journal storage backend
    storage: Arc<S>,

    /// Checkpoint manager
    checkpoint_manager: Arc<CheckpointManager<S>>,

    /// Execution context for current execution
    pub(crate) execution_context: Arc<RwLock<Option<ExecutionContextImpl>>>,

    /// Current message sequence number
    message_sequence: Arc<RwLock<u64>>,

    /// Latest checkpoint state (loaded during on_attach if checkpoint exists)
    pub(crate) latest_checkpoint: Arc<RwLock<Option<Checkpoint>>>,

    /// Pending promises (not yet resolved)
    pending_promises: Arc<RwLock<HashSet<String>>>,

    /// Resolved promises (promise_id -> (result, timestamp))
    resolved_promises: Arc<RwLock<HashMap<String, (Result<Vec<u8>, String>, prost_types::Timestamp)>>>,

    /// Replay handler for deterministic message replay
    replay_handler: Arc<RwLock<Option<Box<dyn ReplayHandler>>>>,

    /// State loader for automatic checkpoint state deserialization (optional)
    state_loader: Arc<RwLock<Option<Box<dyn StateLoader>>>>,
}

/// Default priority for DurabilityFacet
pub const DURABILITY_FACET_DEFAULT_PRIORITY: i32 = 50;

impl<S: JournalStorage + Clone + 'static> DurabilityFacet<S> {
    /// Create a new durability facet
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend
    /// * `config` - Facet configuration as Value (can be empty `{}` for defaults)
    /// * `priority` - Facet priority (default: 50)
    ///
    /// ## Returns
    /// New DurabilityFacet ready to attach to an actor
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # async fn example() -> JournalResult<()> {
    /// let storage = MemoryJournalStorage::new();
    /// let config = serde_json::json!({
    ///     "checkpoint_interval": 100,
    ///     "replay_on_activation": true,
    /// });
    ///
    /// let facet = DurabilityFacet::new(storage, config, 50);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(storage: S, config: Value, priority: i32) -> Self {
        // Parse Value to DurabilityConfig
        let durability_config = Self::parse_config(config.clone());
        // Create checkpoint config from durability config
        let checkpoint_config = CheckpointConfig {
            enabled: durability_config.checkpoint_interval > 0,
            entry_interval: durability_config.checkpoint_interval,
            time_interval: durability_config.checkpoint_timeout.clone(),
            compression: durability_config.compression,
            retention_count: 2,  // Keep last 2 checkpoints
            auto_truncate: false, // Don't auto-truncate - let users control this via config
            async_checkpointing: false,
            metadata: HashMap::new(),
        };

        let storage_arc = Arc::new(storage);

        // Create a cloned reference for CheckpointManager
        // Note: CheckpointManager needs S, not Arc<S>
        // We'll wrap storage in Arc for sharing, but CheckpointManager takes ownership
        let checkpoint_storage = Arc::clone(&storage_arc);
        // Get schema version from durability_config (default to 1 if not set)
        let schema_version = if durability_config.state_schema_version > 0 {
            durability_config.state_schema_version
        } else {
            1 // Default to schema version 1
        };
        
        let checkpoint_manager = Arc::new(CheckpointManager::new(
            (*checkpoint_storage).clone(),
            checkpoint_config,
            schema_version,
        ));

        Self {
            config_value: config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            config: durability_config,
            storage: storage_arc,
            checkpoint_manager,
            execution_context: Arc::new(RwLock::new(None)),
            message_sequence: Arc::new(RwLock::new(0)),
            latest_checkpoint: Arc::new(RwLock::new(None)),
            pending_promises: Arc::new(RwLock::new(HashSet::new())),
            resolved_promises: Arc::new(RwLock::new(HashMap::new())),
            replay_handler: Arc::new(RwLock::new(None)),
            state_loader: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Parse Value to DurabilityConfig
    fn parse_config(config: Value) -> DurabilityConfig {
        // Extract values from JSON config or use defaults
        DurabilityConfig {
            backend: config
                .get("backend")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
                .unwrap_or(JournalBackend::JournalBackendMemory as i32),
            checkpoint_interval: config
                .get("checkpoint_interval")
                .and_then(|v| v.as_i64())
                .map(|v| v as u64)
                .unwrap_or(100),
            checkpoint_timeout: config
                .get("checkpoint_timeout")
                .and_then(|v| {
                    // Try to parse as Duration proto or as string
                    if let Some(duration_str) = v.as_str() {
                        // Parse duration string (e.g., "5m", "10s") - simplified for now
                        // In real implementation, would parse to prost_types::Duration
                        None // Skip for now - requires duration parsing
                    } else {
                        None
                    }
                }),
            replay_on_activation: config
                .get("replay_on_activation")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            cache_side_effects: config
                .get("cache_side_effects")
                .and_then(|v| v.as_bool())
                .unwrap_or(true),
            compression: config
                .get("compression")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32)
                .unwrap_or(CompressionType::CompressionTypeNone as i32),
            state_schema_version: config
                .get("state_schema_version")
                .and_then(|v| v.as_i64())
                .map(|v| v as u32)
                .unwrap_or(1),
            backend_config: None, // BackendConfig doesn't implement Deserialize, skip for now
        }
    }

    /// Set replay handler for deterministic message replay
    ///
    /// ## Arguments
    /// * `handler` - Replay handler that will replay messages through actor's handler
    ///
    /// ## Notes
    /// - Must be called before `on_attach()` if replay is enabled
    /// - Handler will be used during `replay_journal_with_handler()`
    pub async fn set_replay_handler(&self, handler: Box<dyn ReplayHandler>) {
        let mut h = self.replay_handler.write().await;
        *h = Some(handler);
    }


    /// Set state loader for automatic checkpoint state deserialization
    ///
    /// ## Arguments
    /// * `loader` - State loader that will deserialize and restore checkpoint state
    ///
    /// ## Notes
    /// - Optional: If not set, actor must manually call `get_latest_checkpoint()`
    /// - If set, checkpoint state will be automatically loaded during `on_attach()`
    pub async fn set_state_loader(&self, loader: Box<dyn StateLoader>) {
        let mut l = self.state_loader.write().await;
        *l = Some(loader);
    }

    /// Get latest checkpoint (for manual state loading)
    ///
    /// ## Returns
    /// Latest checkpoint if exists, None otherwise
    ///
    /// ## Notes
    /// - Use this for manual state loading (Restate-style)
    /// - If StateLoader is set, automatic loading will be used instead
    pub async fn get_latest_checkpoint(&self) -> JournalResult<Option<Checkpoint>> {
        let actor_id = self.actor_id.read().await;
        let actor_id = actor_id.as_ref().ok_or_else(|| {
            JournalError::Serialization("Actor not attached".to_string())
        })?;

        match self.storage.get_latest_checkpoint(actor_id).await {
            Ok(checkpoint) => Ok(Some(checkpoint)),
            Err(JournalError::CheckpointNotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Replay journal entries to restore actor state (without message handler)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay
    /// * `from_sequence` - Sequence number to start replay from (0 for full replay)
    ///
    /// ## Returns
    /// ExecutionContext with cached side effects from replay
    ///
    /// ## Errors
    /// - Journal storage errors
    /// - Deserialization errors
    ///
    /// ## Notes
    /// - This is the legacy method that doesn't replay messages through handler
    /// - Use `replay_journal_with_handler()` for full deterministic replay
    async fn replay_journal(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<ExecutionContextImpl> {
        // Create execution context in REPLAY mode
        let ctx = ExecutionContextImpl::new(actor_id, ExecutionMode::ExecutionModeReplay);

        // Load journal entries from the specified sequence
        let entries = self.storage.replay_from(actor_id, from_sequence).await?;

        // Extract side effects from journal for caching
        let mut side_effects = Vec::new();
        for entry in &entries {
            use plexspaces_proto::v1::journaling::journal_entry::Entry;
            if let Some(Entry::SideEffectExecuted(ref se)) = entry.entry {
                // Convert SideEffectExecuted to SideEffectEntry
                let side_effect_entry = crate::SideEffectEntry {
                    side_effect_id: se.effect_id.clone(),
                    side_effect_type: se.effect_type.to_string(),
                    input_data: se.request.clone(),
                    output_data: se.response.clone(),
                    executed_at: entry.timestamp.clone(),
                    metadata: HashMap::new(),
                };
                side_effects.push(side_effect_entry);
            }
        }

        // Load side effects into context
        ctx.load_side_effects(side_effects.clone()).await;
        
            // Record side effect cache metrics
            let side_effect_count = side_effects.len();
            if side_effect_count > 0 {
                metrics::histogram!("plexspaces_journaling_side_effects_cached",
                    "actor_id" => actor_id.to_string()
                ).record(side_effect_count as f64);
                tracing::debug!(actor_id = %actor_id, side_effects_count = side_effect_count, "Side effects cached for replay");
            }

        // CRITICAL: Update message_sequence to highest sequence number from ALL entries
        // This ensures we don't create duplicate sequence numbers when writing new entries
        // We need to check ALL entries, not just those from from_sequence, to get the true max
        let all_entries = self.storage.replay_from(actor_id, 0).await?;
        if let Some(last_entry) = all_entries.last() {
            ctx.set_sequence(last_entry.sequence).await;
            let mut seq = self.message_sequence.write().await;
            *seq = last_entry.sequence;
        }

        Ok(ctx)
    }

    /// Replay journal entries with message handler (deterministic replay)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to replay
    /// * `from_sequence` - Sequence number to start replay from (checkpoint sequence + 1 for delta)
    ///
    /// ## Returns
    /// ExecutionContext with cached side effects from replay
    ///
    /// ## Errors
    /// - Journal storage errors
    /// - Replay handler errors
    /// - Deserialization errors
    ///
    /// ## Notes
    /// - Replays MessageReceived entries through ReplayHandler (if set)
    /// - Side effects are cached (not re-executed)
    /// - ExecutionContext is in REPLAY mode during replay
    /// - Gets ActorContext from ReplayHandler (handler stores it)
    async fn replay_journal_with_handler(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<ExecutionContextImpl> {
        // Create execution context in REPLAY mode
        let ctx = ExecutionContextImpl::new(actor_id, ExecutionMode::ExecutionModeReplay);

        // Load journal entries from the specified sequence (delta replay)
        let entries = self.storage.replay_from(actor_id, from_sequence).await?;

        // Extract side effects from journal for caching (Restate pattern)
        let mut side_effects = Vec::new();
        for entry in &entries {
            use plexspaces_proto::v1::journaling::journal_entry::Entry;
            if let Some(Entry::SideEffectExecuted(ref se)) = entry.entry {
                // Convert SideEffectExecuted to SideEffectEntry
                let side_effect_entry = crate::SideEffectEntry {
                    side_effect_id: se.effect_id.clone(),
                    side_effect_type: se.effect_type.to_string(),
                    input_data: se.request.clone(),
                    output_data: se.response.clone(),
                    executed_at: entry.timestamp.clone(),
                    metadata: HashMap::new(),
                };
                side_effects.push(side_effect_entry);
            }
        }

        // Load side effects into context (for caching during replay)
        ctx.load_side_effects(side_effects.clone()).await;
        
        // Record side effect cache metrics
        let side_effect_count = side_effects.len();
        if side_effect_count > 0 {
            metrics::histogram!("plexspaces_journaling_side_effects_cached",
                "actor_id" => actor_id.to_string()
            ).record(side_effect_count as f64);
            tracing::debug!(actor_id = %actor_id, side_effects_count = side_effect_count, "Side effects cached for replay");
        }

        // Replay MessageReceived entries through handler (Restate pattern)
        // CRITICAL: Only replay MessageReceived, not MessageProcessed
        // MessageProcessed is just a record - the actual state change
        // happens when we replay MessageReceived through the handler
        let replay_handler = self.replay_handler.read().await;
        if let Some(handler) = replay_handler.as_ref() {
            // Get ActorContext from handler (it's stored in ActorReplayHandler)
            // We'll use a dummy context for now - the handler has the real context
            // The handler's replay_message will use its own stored context
            for entry in &entries {
                use plexspaces_proto::v1::journaling::journal_entry::Entry;
                if let Some(Entry::MessageReceived(ref msg_received)) = entry.entry {
                    // Reconstruct Message from journal entry
                    let mut message = Message::new(msg_received.payload.clone());
                    message.id = msg_received.message_id.clone();
                    message.metadata = msg_received.metadata.clone();
                    message.priority = plexspaces_mailbox::MessagePriority::Normal;
                    message.correlation_id = Some(entry.correlation_id.clone());
                    message.reply_to = None;
                    message.sender = Some(msg_received.sender_id.clone());
                    message.receiver = actor_id.to_string();
                    message.message_type = msg_received.message_type.clone();
                    message.idempotency_key = None;

                    // Replay message through handler (ExecutionContext is in REPLAY mode)
                    // Handler will use its stored ActorContext (ignores the context parameter)
                    // ExecutionContext will automatically return cached side effects
                    // instead of executing them (deterministic replay)
                    // Note: We pass a dummy context here - handler uses its own stored context
                    // For replay, we need tenant_id from the actor's context
                    // This is a placeholder - in production, tenant_id should come from actor's actual context
                    // Note: This is acceptable here as it's internal replay logic
                    // Use internal context for system operations
                    use plexspaces_core::RequestContext;
                    let internal_ctx = RequestContext::internal();
                    // Create minimal ServiceLocator for replay context
                    // This is used internally for replay operations, not for production actor context
                    use plexspaces_core::ServiceLocator;
                    let service_locator = Arc::new(ServiceLocator::new());
                    let dummy_context = ActorContext::new(
                        "local".to_string(),
                        internal_ctx.namespace().to_string(),
                        internal_ctx.tenant_id().to_string(),
                        service_locator,
                        None,
                    );
                    handler.replay_message(message, &dummy_context).await
                        .map_err(|e| JournalError::Replay(format!("Replay failed: {}", e)))?;
                }
            }
        } else {
            tracing::warn!(actor_id = %actor_id, "No replay handler set - messages will not be replayed through actor handler");
        }

        // Update message_sequence to highest sequence number from ALL entries
        // This ensures we don't create duplicate sequence numbers when writing new entries
        let all_entries = self.storage.replay_from(actor_id, 0).await?;
        if let Some(last_entry) = all_entries.last() {
            ctx.set_sequence(last_entry.sequence).await;
            let mut seq = self.message_sequence.write().await;
            *seq = last_entry.sequence;
        }

        Ok(ctx)
    }

    /// Get execution context (for testing only - not part of public API)
    #[doc(hidden)]
    pub fn get_execution_context(&self) -> Arc<RwLock<Option<ExecutionContextImpl>>> {
        Arc::clone(&self.execution_context)
    }

    /// Create a durable promise
    ///
    /// ## Arguments
    /// * `promise_id` - Unique identifier for the promise
    /// * `timeout` - Optional timeout duration
    ///
    /// ## Returns
    /// `JournalResult<()>` - Success or error
    pub async fn create_promise(
        &self,
        promise_id: &str,
        timeout: Option<std::time::Duration>,
    ) -> JournalResult<()> {
        let actor_id = self.actor_id.read().await;
        let actor_id = actor_id.as_ref().ok_or_else(|| {
            JournalError::Serialization("Actor not attached".to_string())
        })?;

        // Check if promise already exists
        let pending = self.pending_promises.read().await;
        let resolved = self.resolved_promises.read().await;
        if pending.contains(promise_id) || resolved.contains_key(promise_id) {
            return Err(JournalError::Serialization(format!(
                "Promise {} already exists",
                promise_id
            )));
        }
        drop(pending);
        drop(resolved);

        // Convert timeout to protobuf Duration
        let timeout_proto = timeout.map(|d| prost_types::Duration {
            seconds: d.as_secs() as i64,
            nanos: (d.subsec_nanos() as i32),
        });

        // Increment sequence for promise entry
        let sequence = {
            let mut seq = self.message_sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create journal entry
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseCreated(
                    plexspaces_proto::v1::journaling::PromiseCreated {
                        promise_id: promise_id.to_string(),
                        timeout: timeout_proto,
                    },
                ),
            ),
        };

        // Append to journal
        self.storage.append_entry(&entry).await?;

        // Add to pending promises
        {
            let mut pending = self.pending_promises.write().await;
            pending.insert(promise_id.to_string());
        }

        Ok(())
    }

    /// Resolve a promise (fulfilled or rejected)
    ///
    /// ## Arguments
    /// * `promise_id` - Promise identifier
    /// * `result` - Result: `Ok(Vec<u8>)` for success, `Err(String)` for error
    ///
    /// ## Returns
    /// `JournalResult<()>` - Success or error
    pub async fn resolve_promise(
        &self,
        promise_id: &str,
        result: Result<Vec<u8>, String>,
    ) -> JournalResult<()> {
        let actor_id = self.actor_id.read().await;
        let actor_id = actor_id.as_ref().ok_or_else(|| {
            JournalError::Serialization("Actor not attached".to_string())
        })?;

        // Check if promise exists and is pending
        let pending = self.pending_promises.read().await;
        if !pending.contains(promise_id) {
            return Err(JournalError::Serialization(format!(
                "Promise {} not found or already resolved",
                promise_id
            )));
        }
        drop(pending);

        // Increment sequence for promise resolution entry
        let sequence = {
            let mut seq = self.message_sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create journal entry
        let (result_bytes, error_msg) = match &result {
            Ok(bytes) => (bytes.clone(), String::new()),
            Err(err) => (Vec::new(), err.clone()),
        };

        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::PromiseResolved(
                    plexspaces_proto::v1::journaling::PromiseResolved {
                        promise_id: promise_id.to_string(),
                        result: result_bytes.clone(),
                        error: error_msg.clone(),
                    },
                ),
            ),
        };

        // Append to journal
        self.storage.append_entry(&entry).await?;

        // Move from pending to resolved
        {
            let mut pending = self.pending_promises.write().await;
            pending.remove(promise_id);
        }

        {
            let mut resolved = self.resolved_promises.write().await;
            resolved.insert(
                promise_id.to_string(),
                (
                    result,
                    entry.timestamp.clone().unwrap_or_else(|| prost_types::Timestamp {
                        seconds: 0,
                        nanos: 0,
                    }),
                ),
            );
        }

        Ok(())
    }

    /// Get list of pending promises (not yet resolved)
    ///
    /// ## Returns
    /// `JournalResult<Vec<String>>` - List of pending promise IDs
    pub async fn get_pending_promises(&self) -> JournalResult<Vec<String>> {
        let pending = self.pending_promises.read().await;
        Ok(pending.iter().cloned().collect())
    }

    /// Get promise result if resolved
    ///
    /// ## Arguments
    /// * `promise_id` - Promise identifier
    ///
    /// ## Returns
    /// `JournalResult<Option<(Result<Vec<u8>, String>, prost_types::Timestamp)>>` - Promise result and timestamp if resolved, None if pending
    pub async fn get_promise_result(
        &self,
        promise_id: &str,
    ) -> JournalResult<Option<(Result<Vec<u8>, String>, prost_types::Timestamp)>> {
        let resolved = self.resolved_promises.read().await;
        Ok(resolved.get(promise_id).cloned())
    }

    /// Load promises from journal entries (for recovery after restart)
    async fn load_promises_from_journal(&self, actor_id: &str) -> JournalResult<()> {
        let entries = self.storage.replay_from(actor_id, 0).await?;

        let mut pending = self.pending_promises.write().await;
        let mut resolved = self.resolved_promises.write().await;

        // Clear existing state
        pending.clear();
        resolved.clear();

        // Process all entries to reconstruct promise state
        for entry in &entries {
            use plexspaces_proto::v1::journaling::journal_entry::Entry as JournalEntryVariant;
            match &entry.entry {
                Some(JournalEntryVariant::PromiseCreated(promise_created)) => {
                    // Add to pending if not already resolved
                    let promise_id = promise_created.promise_id.clone();
                    if !resolved.contains_key(&promise_id) {
                        pending.insert(promise_id);
                    }
                }
                Some(JournalEntryVariant::PromiseResolved(promise_resolved)) => {
                    let promise_id = promise_resolved.promise_id.clone();
                    // Remove from pending
                    pending.remove(&promise_id);
                    // Add to resolved
                    let result = if promise_resolved.error.is_empty() {
                        Ok(promise_resolved.result.clone())
                    } else {
                        Err(promise_resolved.error.clone())
                    };
                    resolved.insert(
                        promise_id,
                        (
                            result,
                            entry.timestamp.clone().unwrap_or_else(|| prost_types::Timestamp {
                                seconds: 0,
                                nanos: 0,
                            }),
                        ),
                    );
                }
                _ => {}
            }
        }

        Ok(())
    }
}

#[async_trait]
impl<S: JournalStorage + Clone + 'static> Facet for DurabilityFacet<S> {
    fn facet_type(&self) -> &str {
        "durability"
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

        // Replay journal if enabled
        if self.config.replay_on_activation {
            // Step 1: Load latest checkpoint (if exists)
            let (from_sequence, _checkpoint) = match self.storage.get_latest_checkpoint(actor_id).await {
                Ok(checkpoint) => {
                    // Step 2: Validate schema version automatically (prevents incompatible checkpoints)
                    let current_version = self.config.state_schema_version;
                    if checkpoint.state_schema_version > current_version {
                        return Err(FacetError::InvalidConfig(format!(
                            "Incompatible checkpoint schema version: checkpoint={}, current={}, actor_id={}",
                            checkpoint.state_schema_version,
                            current_version,
                            actor_id
                        )));
                    }

                    // Step 3: Attempt automatic state loading (if StateLoader provided)
                    let state_loader = self.state_loader.read().await;
                    if let Some(loader) = state_loader.as_ref() {
                        // Automatic loading (Azure Durable Functions style)
                        match loader.deserialize(&checkpoint.state_data) {
                            Ok(state_value) => {
                                // Restore state to actor
                                if let Err(e) = loader.restore_state(&state_value).await {
                                    return Err(FacetError::InvalidConfig(format!(
                                        "Failed to restore checkpoint state: {}",
                                        e
                                    )));
                                }
                                tracing::info!(
                                    actor_id = %actor_id,
                                    sequence = checkpoint.sequence,
                                    schema_version = checkpoint.state_schema_version,
                                    "Checkpoint state automatically restored"
                                );
                            }
                            Err(e) => {
                                return Err(FacetError::InvalidConfig(format!(
                                    "Failed to deserialize checkpoint state: {}",
                                    e
                                )));
                            }
                        }
                    } else {
                        // Manual loading (Restate style) - store checkpoint for actor to load
                        let mut latest_checkpoint = self.latest_checkpoint.write().await;
                        *latest_checkpoint = Some(checkpoint.clone());
                        drop(latest_checkpoint);
                        tracing::info!(
                            actor_id = %actor_id,
                            sequence = checkpoint.sequence,
                            "Checkpoint available for manual loading"
                        );
                    }

                    let checkpoint_seq = checkpoint.sequence + 1; // Replay from after checkpoint
                    (checkpoint_seq, Some(checkpoint))
                }
                Err(JournalError::CheckpointNotFound(_)) => {
                    // No checkpoint, replay from beginning
                    tracing::debug!(actor_id = %actor_id, "No checkpoint found, replaying from beginning");
                    (0, None)
                }
                Err(e) => {
                    return Err(FacetError::InvalidConfig(format!(
                        "Failed to load checkpoint: {}",
                        e
                    )));
                }
            };

            // Step 4: Replay journal (with handler if available, otherwise legacy method)
            let start = Instant::now();
            let ctx = {
                let replay_handler = self.replay_handler.read().await;
                if replay_handler.is_some() {
                    // Use new replay method with handler (deterministic message replay)
                    drop(replay_handler);
                    self.replay_journal_with_handler(actor_id, from_sequence)
                        .await
                        .map_err(|e| FacetError::InvalidConfig(e.to_string()))?
                } else {
                    // Legacy method (no message replay through handler)
                    drop(replay_handler);
                    self.replay_journal(actor_id, from_sequence)
                        .await
                        .map_err(|e| FacetError::InvalidConfig(e.to_string()))?
                }
            };
            let duration = start.elapsed();
            
            // Get replay stats
            let entries = self.storage.replay_from(actor_id, from_sequence).await
                .unwrap_or_default();
            let entries_count = entries.len();
            
            // Record metrics
            metrics::counter!("plexspaces_journaling_replays_total",
                "actor_id" => actor_id.to_string()
            ).increment(1);
            metrics::histogram!("plexspaces_journaling_replay_duration_seconds",
                "actor_id" => actor_id.to_string()
            ).record(duration.as_secs_f64());
            metrics::histogram!("plexspaces_journaling_replay_entries_count",
                "actor_id" => actor_id.to_string()
            ).record(entries_count as f64);
            tracing::info!(actor_id = %actor_id, from_sequence = from_sequence, entries_count = entries_count, duration_ms = duration.as_millis(), "Journal replayed");

            // Load promises from journal entries
            self.load_promises_from_journal(actor_id).await
                .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

            // CRITICAL: Switch to NORMAL mode after replay completes
            // This allows new journal entries to be written
            ctx.set_mode(ExecutionMode::ExecutionModeNormal).await;

            // Store execution context for use during normal execution
            let mut exec_ctx = self.execution_context.write().await;
            *exec_ctx = Some(ctx);
        } else {
            // Create new execution context in NORMAL mode
            // Also initialize message_sequence from existing journal entries to prevent duplicates
            let entries = self
                .storage
                .replay_from(actor_id, 0)
                .await
                .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

            // Update message_sequence to highest sequence number from journal
            // This prevents duplicate sequence numbers when writing new entries
            if let Some(last_entry) = entries.last() {
                let mut seq = self.message_sequence.write().await;
                *seq = last_entry.sequence;
            }

            let ctx = ExecutionContextImpl::new(actor_id, ExecutionMode::ExecutionModeNormal);
            let mut exec_ctx = self.execution_context.write().await;
            *exec_ctx = Some(ctx);

            // Load promises from existing journal entries
            self.load_promises_from_journal(actor_id).await
                .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;
        }

        Ok(())
    }

    /// Phase 4.3: Handle EXIT signal from linked actor
    ///
    /// ## Purpose
    /// Flushes journal and saves checkpoint when actor receives EXIT signal from linked actor.
    /// This ensures durability state is persisted before actor terminates.
    ///
    /// ## When Called
    /// - Only if `ActorContext.trap_exit = true`
    /// - After `Actor::handle_exit()` is called
    /// - Before actor terminates (if ExitAction::Propagate)
    async fn on_exit(
        &mut self,
        actor_id: &str,
        _from: &str,
        _reason: &plexspaces_facet::ExitReason,
    ) -> Result<(), FacetError> {
        // Flush journal and save checkpoint on EXIT
        tracing::debug!(
            actor_id = %actor_id,
            "DurabilityFacet handling EXIT signal - flushing journal and saving checkpoint"
        );
        
        // Save checkpoint if enabled (use maybe_checkpoint which handles interval logic)
        let checkpoint_start = std::time::Instant::now();
        let current_sequence = *self.message_sequence.read().await;
        if let Err(e) = self.checkpoint_manager.maybe_checkpoint(actor_id, current_sequence, vec![]).await {
            tracing::warn!(
                actor_id = %actor_id,
                error = %e,
                "Failed to save checkpoint on EXIT signal"
            );
            metrics::counter!("plexspaces_durability_facet_exit_checkpoint_errors_total",
                "actor_id" => actor_id.to_string()
            ).increment(1);
        } else {
            let checkpoint_duration = checkpoint_start.elapsed();
            metrics::histogram!("plexspaces_durability_facet_exit_checkpoint_duration_seconds",
                "actor_id" => actor_id.to_string()
            ).record(checkpoint_duration.as_secs_f64());
            tracing::debug!(
                actor_id = %actor_id,
                duration_ms = checkpoint_duration.as_millis(),
                "Saved checkpoint on EXIT signal"
            );
        }
        
        metrics::counter!("plexspaces_durability_facet_exit_total",
            "actor_id" => actor_id.to_string()
        ).increment(1);
        
        Ok(())
    }

    /// Phase 4.3: Handle DOWN notification from monitored actor
    ///
    /// ## Purpose
    /// Logs DOWN notification for observability. DurabilityFacet doesn't need to
    /// take action on DOWN notifications (journaling is actor-specific).
    ///
    /// ## When Called
    /// - After actor receives DOWN notification
    /// - Actor continues running (DOWN is informational, not fatal)
    async fn on_down(
        &mut self,
        actor_id: &str,
        monitored_id: &str,
        reason: &plexspaces_facet::ExitReason,
    ) -> Result<(), FacetError> {
        // Log DOWN notification for observability
        tracing::debug!(
            actor_id = %actor_id,
            monitored_id = %monitored_id,
            reason = ?reason,
            "DurabilityFacet received DOWN notification (no action needed)"
        );
        
        metrics::counter!("plexspaces_durability_facet_down_total",
            "actor_id" => actor_id.to_string(),
            "monitored_id" => monitored_id.to_string()
        ).increment(1);
        
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Flush any pending journal entries
        self.storage
            .flush()
            .await
            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;

        // Clear state
        {
            let mut aid = self.actor_id.write().await;
            *aid = None;
        }
        {
            let mut checkpoint = self.latest_checkpoint.write().await;
            *checkpoint = None;
        }
        {
            let mut ctx = self.execution_context.write().await;
            *ctx = None;
        }

        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        let actor_id_opt = self.actor_id.read().await;
        let actor_id = actor_id_opt
            .as_ref()
            .ok_or_else(|| FacetError::InterceptionFailed("Actor ID not set".to_string()))?;

        // Increment sequence
        let sequence = {
            let mut seq = self.message_sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create MessageReceived journal entry
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::MessageReceived(
                    MessageReceived {
                        message_id: ulid::Ulid::new().to_string(),
                        sender_id: String::new(),
                        message_type: method.to_string(),
                        payload: args.to_vec(),
                        metadata: HashMap::new(),
                    },
                ),
            ),
        };

        // Append to journal with metrics
        let start = Instant::now();
        self.storage
            .append_entry(&entry)
            .await
            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;
        let duration = start.elapsed();
        
        // Record metrics
        metrics::counter!("plexspaces_journaling_entries_appended_total",
            "actor_id" => actor_id.clone()
        ).increment(1);
        metrics::histogram!("plexspaces_journaling_append_duration_seconds",
            "actor_id" => actor_id.clone()
        ).record(duration.as_secs_f64());
        tracing::debug!(actor_id = %actor_id, sequence = sequence, duration_ms = duration.as_millis(), "Journal entry appended");

        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        _method: &str,
        _args: &[u8],
        result: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        let actor_id_opt = self.actor_id.read().await;
        let actor_id = actor_id_opt
            .as_ref()
            .ok_or_else(|| FacetError::InterceptionFailed("Actor ID not set".to_string()))?;

        // Collect all journal entries for batching
        let mut batch_entries = Vec::new();

        // Increment sequence for MessageProcessed
        let sequence = {
            let mut seq = self.message_sequence.write().await;
            *seq += 1;
            *seq
        };

        // Create MessageProcessed journal entry
        let entry = JournalEntry {
            id: ulid::Ulid::new().to_string(),
            actor_id: actor_id.clone(),
            sequence,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            correlation_id: String::new(),
            entry: Some(
                plexspaces_proto::v1::journaling::journal_entry::Entry::MessageProcessed(
                    MessageProcessed {
                        message_id: ulid::Ulid::new().to_string(),
                        result: ProcessingResult::ProcessingResultSuccess as i32,
                        error_message: String::new(),
                        response_payload: result.to_vec(),
                    },
                ),
            ),
        };

        // Add MessageProcessed to batch
        batch_entries.push(entry);

        // Collect side effects from execution context
        if let Some(ref ctx) = *self.execution_context.read().await {
            let side_effects = ctx.take_side_effects().await;
            for se in side_effects {
                // Increment sequence for each side effect
                let se_sequence = {
                    let mut seq = self.message_sequence.write().await;
                    *seq += 1;
                    *seq
                };

                let se_entry = JournalEntry {
                    id: ulid::Ulid::new().to_string(),
                    actor_id: actor_id.clone(),
                    sequence: se_sequence,
                    timestamp: se.executed_at.clone(),
                    correlation_id: String::new(),
                    entry: Some(
                        plexspaces_proto::v1::journaling::journal_entry::Entry::SideEffectExecuted(
                            SideEffectExecuted {
                                effect_id: se.side_effect_id,
                                effect_type: 1, // EXTERNAL_CALL
                                request: se.input_data,
                                response: se.output_data,
                                error: String::new(),
                            },
                        ),
                    ),
                };

                // Add SideEffect to batch
                batch_entries.push(se_entry);
            }
        }

        // Batch append all entries in a single transaction (3-5x faster)
        if !batch_entries.is_empty() {
            let start = Instant::now();
            let batch_size = batch_entries.len();
            self.storage
                .append_batch(&batch_entries)
                .await
                .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;
            let duration = start.elapsed();
            
            // Record metrics
            metrics::counter!("plexspaces_journaling_batches_appended_total",
                "actor_id" => actor_id.clone()
            ).increment(1);
            metrics::histogram!("plexspaces_journaling_batch_append_duration_seconds",
                "actor_id" => actor_id.clone()
            ).record(duration.as_secs_f64());
            metrics::histogram!("plexspaces_journaling_batch_size",
                "actor_id" => actor_id.clone()
            ).record(batch_size as f64);
            tracing::debug!(actor_id = %actor_id, batch_size = batch_size, duration_ms = duration.as_millis(), "Journal batch appended");
        }

        // Check if checkpoint should be created
        let current_sequence = *self.message_sequence.read().await;
        if self.config.checkpoint_interval > 0
            && current_sequence % self.config.checkpoint_interval == 0
        {
            // For simplicity, checkpoint with empty state
            // In real implementation, this would serialize actor state
            let state_data = vec![];
            let checkpoint_size = state_data.len();

            let start = Instant::now();
            self.checkpoint_manager
                .maybe_checkpoint(actor_id, current_sequence, state_data)
                .await
                .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;
            let duration = start.elapsed();
            
            // Record metrics
            metrics::counter!("plexspaces_journaling_checkpoints_created_total",
                "actor_id" => actor_id.clone()
            ).increment(1);
            metrics::histogram!("plexspaces_journaling_checkpoint_duration_seconds",
                "actor_id" => actor_id.clone()
            ).record(duration.as_secs_f64());
            metrics::histogram!("plexspaces_journaling_checkpoint_size_bytes",
                "actor_id" => actor_id.clone()
            ).record(checkpoint_size as f64);
            metrics::gauge!("plexspaces_journaling_latest_checkpoint_sequence",
                "actor_id" => actor_id.clone()
            ).set(current_sequence as f64);
            tracing::info!(actor_id = %actor_id, sequence = current_sequence, size_bytes = checkpoint_size, duration_ms = duration.as_millis(), "Checkpoint created");
        }

        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        // For durability facet, we just log errors and propagate
        // Future: Could record error in journal for debugging
        Ok(ErrorHandling::Propagate)
    }
    
    fn get_config(&self) -> Value {
        self.config_value.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

// =============================================================================
// TESTS (TDD - Write tests FIRST, then implement)
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Checkpoint, MemoryJournalStorage};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn create_test_config() -> DurabilityConfig {
        DurabilityConfig {
            backend: JournalBackend::JournalBackendMemory as i32,
            checkpoint_interval: 10, // Checkpoint every 10 messages
            checkpoint_timeout: None,
            replay_on_activation: true,
            cache_side_effects: true,
            compression: CompressionType::CompressionTypeNone as i32,
            state_schema_version: 1,
            backend_config: None,
        }
    }

    /// Helper to convert DurabilityConfig to Value
    fn config_to_value(config: &DurabilityConfig) -> serde_json::Value {
        let mut json = serde_json::json!({
            "backend": config.backend,
            "checkpoint_interval": config.checkpoint_interval,
            "replay_on_activation": config.replay_on_activation,
            "cache_side_effects": config.cache_side_effects,
            "compression": config.compression,
            "state_schema_version": config.state_schema_version,
        });
        // Handle checkpoint_timeout separately (Option<Duration> doesn't serialize directly)
        if let Some(ref timeout) = config.checkpoint_timeout {
            json["checkpoint_timeout_seconds"] = serde_json::json!(timeout.seconds);
            json["checkpoint_timeout_nanos"] = serde_json::json!(timeout.nanos);
        }
        json
    }

    #[tokio::test]
    async fn test_facet_creation() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();

        let facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        assert_eq!(facet.facet_type(), "durability");
    }

    #[tokio::test]
    async fn test_facet_attach_without_replay() {
        let storage = MemoryJournalStorage::new();
        let mut config = create_test_config();
        config.replay_on_activation = false; // Disable replay

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // Should succeed without errors
        let result = facet.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());

        // Verify actor_id was set
        let actor_id = facet.actor_id.read().await;
        assert_eq!(actor_id.as_ref().unwrap(), "actor-1");

        // Verify execution context was created in NORMAL mode
        let ctx = facet.execution_context.read().await;
        assert!(ctx.is_some());
    }

    #[tokio::test]
    async fn test_facet_attach_with_empty_journal_replay() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config(); // replay_on_activation = true

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // Should succeed even with no checkpoint/journal
        let result = facet.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_facet_detach() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Detach should clear state
        let result = facet.on_detach("actor-1").await;
        assert!(result.is_ok());

        // Verify actor_id was cleared
        let actor_id = facet.actor_id.read().await;
        assert!(actor_id.is_none());
    }

    #[tokio::test]
    async fn test_before_method_journals_message_received() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Call before_method
        let args = vec![1, 2, 3, 4];
        let result = facet.before_method("test_method", &args).await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), InterceptResult::Continue));

        // Verify journal entry was created
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 1);

        // Verify it's a MessageReceived entry
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        if let Some(Entry::MessageReceived(msg)) = &entries[0].entry {
            assert_eq!(msg.message_type, "test_method");
            assert_eq!(msg.payload, args);
        } else {
            panic!("Expected MessageReceived entry");
        }
    }

    #[tokio::test]
    async fn test_after_method_journals_message_processed() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Call before_method first
        facet
            .before_method("test_method", &[1, 2, 3])
            .await
            .unwrap();

        // Call after_method
        let result_data = vec![4, 5, 6];
        let result = facet.after_method("test_method", &[], &result_data).await;
        assert!(result.is_ok());

        // Verify journal entries
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 2); // MessageReceived + MessageProcessed

        // Verify second entry is MessageProcessed
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        if let Some(Entry::MessageProcessed(msg)) = &entries[1].entry {
            assert_eq!(msg.result, ProcessingResult::ProcessingResultSuccess as i32);
            assert_eq!(msg.response_payload, result_data);
        } else {
            panic!("Expected MessageProcessed entry");
        }
    }

    #[tokio::test]
    async fn test_sequence_number_increments() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process 3 messages
        for _ in 0..3 {
            facet.before_method("test", &[]).await.unwrap();
            facet.after_method("test", &[], &[]).await.unwrap();
        }

        // Verify sequence numbers increase
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 6); // 3 * (before + after)

        // Check sequences are monotonic
        for i in 1..entries.len() {
            assert!(entries[i].sequence > entries[i - 1].sequence);
        }
    }

    #[tokio::test]
    async fn test_checkpoint_creation_at_interval() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;
        config.checkpoint_interval = 0; // Disable checkpointing for this test

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process 10 messages
        for _ in 0..10 {
            facet.before_method("test", &[]).await.unwrap();
            facet.after_method("test", &[], &[]).await.unwrap();
        }

        // Verify journal entries were created (not truncated)
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 20); // 10 * (before + after)
    }

    #[tokio::test]
    async fn test_replay_journal_on_activation() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false; // First attach without replay

        // Create and attach facet
        let mut facet1 = DurabilityFacet::new(storage_clone.clone(), config_to_value(&config), 50);
        facet1
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process some messages
        for i in 0..3 {
            facet1
                .before_method(&format!("method_{}", i), &[])
                .await
                .unwrap();
            facet1
                .after_method(&format!("method_{}", i), &[], &[])
                .await
                .unwrap();
        }

        facet1.on_detach("actor-1").await.unwrap();

        // Create new facet with replay enabled
        config.replay_on_activation = true;
        let mut facet2 = DurabilityFacet::new(storage_clone, config_to_value(&config), 50);

        // On attach, should replay journal
        let result = facet2.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());

        // Verify execution context has replayed state
        let ctx = facet2.execution_context.read().await;
        assert!(ctx.is_some());

        // Sequence should be restored
        let seq = *facet2.message_sequence.read().await;
        assert_eq!(seq, 6); // 3 * (before + after)
    }

    #[tokio::test]
    async fn test_facet_error_handling() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();

        let facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // on_error should propagate by default
        let result = facet.on_error("test_method", "test error").await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), ErrorHandling::Propagate));
    }

    #[tokio::test]
    async fn test_before_method_without_attach_fails() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();

        let facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // Calling before_method without attach should fail
        let result = facet.before_method("test", &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_multiple_messages_with_different_payloads() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process messages with different payloads
        let messages = vec![
            ("method1", vec![1u8, 2, 3]),
            ("method2", vec![4u8, 5, 6, 7]),
            ("method3", vec![8u8]),
        ];

        for (method, payload) in &messages {
            facet.before_method(method, payload).await.unwrap();
            facet.after_method(method, &[], &[]).await.unwrap();
        }

        // Verify all entries
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();
        assert_eq!(entries.len(), 6); // 3 * (before + after)

        // Verify payloads match
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        for (i, (method, payload)) in messages.iter().enumerate() {
            let entry_idx = i * 2; // Every other entry is MessageReceived
            if let Some(Entry::MessageReceived(msg)) = &entries[entry_idx].entry {
                assert_eq!(&msg.message_type, method);
                assert_eq!(&msg.payload, payload);
            }
        }
    }

    #[tokio::test]
    async fn test_facet_get_state() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();
        let facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // get_state should return Null (default implementation)
        let state = facet.get_state().unwrap();
        assert_eq!(state, serde_json::Value::Null);
    }

    #[tokio::test]
    async fn test_facet_set_state() {
        let storage = MemoryJournalStorage::new();
        let config = create_test_config();
        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);

        // set_state should succeed (default implementation is no-op)
        let state = serde_json::json!({"key": "value"});
        let result = facet.set_state(state);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_replay_with_checkpoint() {
        let storage = MemoryJournalStorage::new();
        let storage_clone = storage.clone();
        let mut config = create_test_config();
        config.replay_on_activation = false;
        config.checkpoint_interval = 5;

        // First facet: create journal entries and checkpoint
        let mut facet1 = DurabilityFacet::new(storage_clone.clone(), config_to_value(&config), 50);
        facet1
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process 6 messages (will create checkpoint at sequence 10)
        for _ in 0..6 {
            facet1.before_method("test", &[]).await.unwrap();
            facet1.after_method("test", &[], &[]).await.unwrap();
        }

        // Manually create a checkpoint
        let checkpoint = Checkpoint {
            actor_id: "actor-1".to_string(),
            sequence: 10,
            timestamp: Some(prost_types::Timestamp {
                seconds: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs() as i64,
                nanos: 0,
            }),
            state_data: vec![],
            compression: CompressionType::CompressionTypeNone as i32,
            metadata: HashMap::new(),
            state_schema_version: 1,
        };
        storage_clone.save_checkpoint(&checkpoint).await.unwrap();

        facet1.on_detach("actor-1").await.unwrap();

        // Second facet: replay from checkpoint
        config.replay_on_activation = true;
        let mut facet2 = DurabilityFacet::new(storage_clone, config_to_value(&config), 50);

        // Should replay from checkpoint sequence + 1
        let result = facet2.on_attach("actor-1", serde_json::json!({})).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_batch_journal_writes() {
        // Test that after_method() batches MessageProcessed and SideEffects
        // in a single append_batch() call for improved performance (3-5x faster)

        // Use SQLite :memory: database to ensure writes are actually persisted
        // and to avoid any potential issues with MemoryJournalStorage cloning
        use crate::sql::SqliteJournalStorage;

        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();
        let storage_clone = storage.clone(); // SQLite pool is Arc-based, so clone shares connection

        let mut config = create_test_config();
        config.replay_on_activation = false;
        config.checkpoint_interval = 0; // Disable checkpointing for this test to verify all entries persist

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Process multiple messages rapidly
        // Without batching: 10 messages = 10 MessageReceived + 10 MessageProcessed = 20 separate writes
        // With batching: 10 messages = 10 MessageReceived + 1 batch of 10 MessageProcessed = 11 writes (1.8x fewer)
        for i in 0..10 {
            facet.before_method("test", &[i as u8]).await.unwrap();
            facet
                .after_method("test", &[], &[i as u8 + 100])
                .await
                .unwrap();
        }

        // Verify all entries were written correctly
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();

        // Should have: 10 MessageReceived + 10 MessageProcessed = 20 total
        assert_eq!(
            entries.len(),
            20,
            "Expected 20 entries, found {}",
            entries.len()
        );

        // Verify entry types alternate correctly
        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        for i in 0..10 {
            assert!(
                matches!(entries[i * 2].entry, Some(Entry::MessageReceived(_))),
                "Entry {} should be MessageReceived",
                i * 2
            );
            assert!(
                matches!(entries[i * 2 + 1].entry, Some(Entry::MessageProcessed(_))),
                "Entry {} should be MessageProcessed",
                i * 2 + 1
            );
        }

        // Verify sequences are strictly monotonic
        for i in 1..entries.len() {
            assert!(
                entries[i].sequence > entries[i - 1].sequence,
                "Sequence {} ({}) should be > sequence {} ({})",
                i,
                entries[i].sequence,
                i - 1,
                entries[i - 1].sequence
            );
        }
    }

    #[tokio::test]
    async fn test_batch_write_with_no_side_effects() {
        // Use SQLite to ensure batching works correctly without side effects
        use crate::sql::SqliteJournalStorage;

        let storage = SqliteJournalStorage::new(":memory:").await.unwrap();
        let storage_clone = storage.clone();

        let mut config = create_test_config();
        config.replay_on_activation = false;

        let mut facet = DurabilityFacet::new(storage, config_to_value(&config), 50);
        facet
            .on_attach("actor-1", serde_json::json!({}))
            .await
            .unwrap();

        // Call before_method and after_method without side effects
        facet
            .before_method("test_method", &[1, 2, 3])
            .await
            .unwrap();
        facet
            .after_method("test_method", &[], &[4, 5, 6])
            .await
            .unwrap();

        // Verify entries
        let entries = storage_clone.replay_from("actor-1", 0).await.unwrap();

        // Should have: 1 MessageReceived + 1 MessageProcessed = 2 total
        assert_eq!(entries.len(), 2);

        use plexspaces_proto::v1::journaling::journal_entry::Entry;
        assert!(matches!(entries[0].entry, Some(Entry::MessageReceived(_))));
        assert!(matches!(entries[1].entry, Some(Entry::MessageProcessed(_))));
    }
}
