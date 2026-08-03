// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// PlexSpaces is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// PlexSpaces is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with PlexSpaces. If not, see <https://www.gnu.org/licenses/>.

//! Dynamic Facet System for Runtime Behavior Composition
//!
//! Facets allow actors to gain new capabilities at runtime without inheritance or recompilation.
//! This is inspired by the facet pattern where secondary objects (facets) can be dynamically
//! attached to primary objects (actors) to extend their behavior.

// event_emitter and capabilities are declared in lib.rs

use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;

/// Exit reason for actor termination (minimal definition for Facet trait)
///
/// This is a minimal definition to avoid circular dependencies.
/// The full ExitReason is defined in plexspaces-core, but facets only need
/// this interface for the trait signature.
///
/// When facets are used in actors, they receive the full ExitReason from plexspaces-core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExitReason {
    /// Normal termination (not an error)
    Normal,
    /// Shutdown requested (graceful)
    Shutdown,
    /// Killed forcefully
    Killed,
    /// Error with message
    Error(String),
    /// Linked actor died
    Linked {
        /// ID of the linked actor that died
        actor_id: String,
        /// The exit reason from the linked actor
        reason: Box<ExitReason>,
    },
}

impl ExitReason {
    /// Convert from core ExitReason (avoids circular dependency)
    ///
    /// ## Purpose
    /// Converts core ExitReason to facet::ExitReason.
    /// This is needed when calling facet lifecycle hooks from Actor.
    ///
    /// ## Note
    /// This is a manual conversion to avoid circular dependencies.
    /// The facet crate cannot depend on core crate.
    /// Callers should convert core::ExitReason to facet::ExitReason manually.
    pub fn from_core(core_reason: &impl std::fmt::Debug) -> Self {
        // Use string representation to avoid direct dependency
        // This is a workaround - in practice, we'd use a trait or shared enum
        // For now, we'll match on the debug representation
        let debug_str = format!("{:?}", core_reason);

        // Parse from debug string (not ideal, but avoids circular dependency)
        if debug_str.contains("Normal") {
            ExitReason::Normal
        } else if debug_str.contains("Shutdown") {
            ExitReason::Shutdown
        } else if debug_str.contains("Killed") {
            ExitReason::Killed
        } else if debug_str.contains("Linked") {
            // Extract actor_id and reason from debug string
            // Format: "Linked { actor_id: \"...\", reason: ... }"
            let actor_id = if let Some(start) = debug_str.find("actor_id: \"") {
                let start = start + 11;
                if let Some(end) = debug_str[start..].find("\"") {
                    debug_str[start..start + end].to_string()
                } else {
                    "unknown".to_string()
                }
            } else {
                "unknown".to_string()
            };

            // For linked reasons, we'll use a simplified representation
            ExitReason::Linked {
                actor_id,
                reason: Box::new(ExitReason::Error("linked".to_string())),
            }
        } else {
            // Extract error message if present
            if let Some(start) = debug_str.find("Error(\"") {
                let start = start + 7;
                if let Some(end) = debug_str[start..].find("\"") {
                    ExitReason::Error(debug_str[start..start + end].to_string())
                } else {
                    ExitReason::Error(debug_str)
                }
            } else {
                ExitReason::Error(debug_str)
            }
        }
    }
}

/// A facet that can be dynamically attached to actors
#[async_trait]
pub trait Facet: Send + Sync + Any {
    /// Unique identifier for this facet type
    fn facet_type(&self) -> &str;

    /// Get reference to self as Any (for downcasting)
    fn as_any(&self) -> &dyn Any;

    /// Get mutable reference to self as Any (for downcasting)
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Called when facet is attached to an actor
    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError>;

    /// Called when facet is detached from an actor
    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError>;

    /// Intercept a method call before it reaches the actor.
    ///
    /// `headers` carries the message headers map (same `headers` field from `Message`).
    /// Facets can read `headers["memo_key"]` for an explicit cache key, `correlation_id`
    /// for idempotency tracking, or any other routing metadata.
    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        Ok(InterceptResult::Continue)
    }

    /// Intercept a method call after the actor processes it.
    ///
    /// `headers` is the same headers map passed to `before_method`.
    async fn after_method(
        &self,
        _method: &str,
        _args: &[u8],
        _result: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        Ok(InterceptResult::Continue)
    }

    /// Handle errors from the actor
    async fn on_error(&self, _method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        Ok(ErrorHandling::Propagate)
    }

    /// Get current facet state (for persistence/migration)
    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(Value::Null)
    }

    /// Restore facet state
    fn set_state(&mut self, _state: Value) -> Result<(), FacetError> {
        Ok(())
    }

    /// Get facet configuration (immutable)
    fn get_config(&self) -> Value;

    /// Get facet priority
    fn get_priority(&self) -> i32;

    /// Called when actor receives EXIT signal from linked actor
    /// Only called if actor has trap_exit=true
    ///
    /// ## Purpose
    /// Allows facets to handle EXIT signals from linked actors.
    /// For example, TimerFacet may cancel timers, ReminderFacet may pause reminders.
    ///
    /// ## When Called
    /// - Only if `ActorContext.trap_exit = true`
    /// - After `Actor::handle_exit()` is called
    /// - Before actor terminates (if ExitAction::Propagate)
    ///
    /// ## Default
    /// Does nothing - most facets don't need to handle EXIT signals
    async fn on_exit(
        &mut self,
        _actor_id: &str,
        _from: &str,
        _reason: &crate::ExitReason,
    ) -> Result<(), FacetError> {
        Ok(())
    }

    /// Called when actor receives DOWN notification from monitored actor
    ///
    /// ## Purpose
    /// Allows facets to handle DOWN notifications from monitored actors.
    /// For example, facets may update state or trigger cleanup.
    ///
    /// ## When Called
    /// - After actor receives DOWN notification
    /// - Actor continues running (DOWN is informational, not fatal)
    ///
    /// ## Default
    /// Does nothing - most facets don't need to handle DOWN notifications
    async fn on_down(
        &mut self,
        _actor_id: &str,
        _monitored_id: &str,
        _reason: &crate::ExitReason,
    ) -> Result<(), FacetError> {
        Ok(())
    }

    /// Called after actor.init() completes (for post-init setup)
    ///
    /// ## Purpose
    /// Allows facets to perform setup that requires actor to be initialized.
    /// For example, facets may need to access actor state after init().
    ///
    /// ## When Called
    /// - After `Actor::init()` completes successfully
    /// - Before actor enters message loop
    ///
    /// ## Default
    /// Does nothing - most facets don't need post-init setup
    async fn on_init_complete(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        Ok(())
    }

    /// Called before actor.terminate() (for pre-terminate cleanup)
    ///
    /// ## Purpose
    /// Allows facets to perform cleanup before actor terminates.
    /// For example, TimerFacet may cancel timers, DurabilityFacet may flush journal.
    ///
    /// ## When Called
    /// - Before `Actor::terminate()` is called
    /// - Actor is still active (can access state)
    ///
    /// ## Default
    /// Does nothing - most facets clean up in on_detach()
    async fn on_terminate_start(
        &mut self,
        _actor_id: &str,
        _reason: &crate::ExitReason,
    ) -> Result<(), FacetError> {
        Ok(())
    }
}

/// Result of intercepting a method
#[derive(Debug)]
pub enum InterceptResult {
    /// Continue to next facet or actor
    Continue,
    /// Replace the arguments
    ReplaceArgs(Vec<u8>),
    /// Replace the result
    ReplaceResult(Vec<u8>),
    /// Stop processing and return early
    ShortCircuit(Vec<u8>),
}

/// Outcome of before-method interception
///
/// ## Purpose
/// Clearly indicates what the caller should do after interception.
/// This replaces the hacky `(Vec<u8>, bool)` tuple with explicit intent.
///
/// ## Design
/// - `CallActor(args)`: Call the actor's handle_message with these args (may have been modified by ReplaceArgs)
/// - `ShortCircuit(result)`: Don't call the actor, return this result directly
#[derive(Debug, Clone)]
pub enum BeforeInterceptOutcome {
    /// Call the actor with these arguments (may have been modified by facets)
    CallActor(Vec<u8>),
    /// Don't call the actor, return this result directly (facet handled the message)
    ShortCircuit(Vec<u8>),
}

/// How to handle errors
#[derive(Debug)]
pub enum ErrorHandling {
    /// Propagate the error
    Propagate,
    /// Retry the operation with maximum attempts
    Retry {
        /// Maximum number of retry attempts
        max_attempts: u32,
    },
    /// Replace with default value
    Default(Vec<u8>),
    /// Transform the error
    Transform(String),
}

/// Error type for facet operations
#[derive(Debug, thiserror::Error)]
pub enum FacetError {
    /// Facet with given type not found
    #[error("Facet not found: {0}")]
    NotFound(String),

    /// Configuration error (e.g., failed to downcast types)
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// Facet of this type already attached
    #[error("Facet already attached: {0}")]
    AlreadyAttached(String),

    /// Invalid facet configuration provided
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Failed to attach facet to actor
    #[error("Attachment failed: {0}")]
    AttachmentFailed(String),

    /// Failed to detach facet from actor
    #[error("Detachment failed: {0}")]
    DetachmentFailed(String),

    /// Message interception processing failed
    #[error("Interception failed: {0}")]
    InterceptionFailed(String),
}

/// Container for facets attached to an actor
pub struct FacetContainer {
    facets: Vec<Arc<RwLock<Box<dyn Facet>>>>,
    metadata: HashMap<String, FacetMetadata>,
}

/// Metadata about a facet attached to an actor
#[derive(Clone, Debug)]
pub struct FacetMetadata {
    /// Type identifier of the facet (e.g., "durability", "timer")
    pub facet_type: String,
    /// Priority for facet execution order (higher = earlier)
    pub priority: i32,
    /// When the facet was attached
    pub attached_at: std::time::Instant,
    /// Configuration for the facet
    pub config: Value,
}

impl Default for FacetContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl FacetContainer {
    /// Creates a new empty facet container
    pub fn new() -> Self {
        FacetContainer {
            facets: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    /// Attach a facet (config and priority are extracted from facet)
    pub async fn attach(
        &mut self,
        facet: Box<dyn Facet>,
        actor_id: &str,
    ) -> Result<String, FacetError> {
        self.attach_with_tenant_context(facet, actor_id, None, None)
            .await
    }

    /// Attach a facet with tenant_id/namespace from API request
    /// This allows facets (like RegistryFacet) to use API-provided tenant_id/namespace
    pub async fn attach_with_tenant_context(
        &mut self,
        mut facet: Box<dyn Facet>,
        actor_id: &str,
        tenant_id: Option<String>,
        namespace: Option<String>,
    ) -> Result<String, FacetError> {
        let facet_type = facet.facet_type().to_string();
        metrics::counter!("plexspaces_facet_attach_attempts_total", "facet_type" => facet_type.clone()).increment(1);

        // Check if already attached
        if self.metadata.contains_key(&facet_type) {
            metrics::counter!("plexspaces_facet_attach_errors_total", "facet_type" => facet_type.clone(), "error" => "already_attached").increment(1);
            tracing::warn!(facet_type = %facet_type, "Facet already attached");
            return Err(FacetError::AlreadyAttached(facet_type));
        }

        // Extract config and priority from facet
        let mut config = facet.get_config();
        let priority = facet.get_priority();

        // Inject tenant_id/namespace into every facet's config so they can build
        // a properly-scoped RequestContext without hard-coding actor_id as tenant.
        if let Some(config_obj) = config.as_object_mut() {
            if let Some(tid) = tenant_id {
                config_obj.insert("_tenant_id".to_string(), serde_json::Value::String(tid));
            }
            if let Some(ns) = namespace {
                config_obj.insert("_namespace".to_string(), serde_json::Value::String(ns));
            }
        }

        facet.on_attach(actor_id, config.clone()).await?;

        // Store metadata FIRST (before creating Arc, so we can use it for sorting)
        let facet_type_clone = facet_type.clone();
        let priority_clone = priority;

        // Create locked facet
        let facet_arc = Arc::new(RwLock::new(facet));

        // Store metadata
        self.metadata.insert(
            facet_type_clone.clone(),
            FacetMetadata {
                facet_type: facet_type_clone.clone(),
                priority: priority_clone,
                attached_at: std::time::Instant::now(),
                config: config.clone(),
            },
        );

        // Insert based on priority (higher priority first)
        // Use metadata to find insertion position
        let insert_pos = self
            .facets
            .iter()
            .position(|f| {
                // Get priority from metadata (faster than locking facet)
                let facet_type = {
                    // Try to read facet type (non-blocking)
                    if let Ok(guard) = f.try_read() {
                        guard.facet_type().to_string()
                    } else {
                        return false; // Skip if locked
                    }
                };

                // Check priority from metadata
                if let Some(existing_metadata) = self.metadata.get(&facet_type) {
                    existing_metadata.priority < priority_clone
                } else {
                    false
                }
            })
            .unwrap_or(self.facets.len());

        self.facets.insert(insert_pos, facet_arc);

        // Sort facets by priority (descending) to ensure correct order
        // This is a safety measure in case metadata is out of sync
        self.facets.sort_by(|a, b| {
            let priority_a = {
                if let Ok(guard) = a.try_read() {
                    self.metadata
                        .get(guard.facet_type())
                        .map(|m| m.priority)
                        .unwrap_or(0)
                } else {
                    0
                }
            };
            let priority_b = {
                if let Ok(guard) = b.try_read() {
                    self.metadata
                        .get(guard.facet_type())
                        .map(|m| m.priority)
                        .unwrap_or(0)
                } else {
                    0
                }
            };
            priority_b.cmp(&priority_a) // Descending order (high priority first)
        });

        // Store metadata
        self.metadata.insert(
            facet_type.clone(),
            FacetMetadata {
                facet_type: facet_type.clone(),
                priority,
                attached_at: std::time::Instant::now(),
                config,
            },
        );

        metrics::counter!("plexspaces_facet_attached_total", "facet_type" => facet_type.clone())
            .increment(1);
        metrics::gauge!("plexspaces_facet_active_total", "facet_type" => facet_type.clone())
            .increment(1.0);
        Ok(facet_type)
    }

    /// Detach a facet
    pub async fn detach(&mut self, facet_type: &str, actor_id: &str) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_detach_attempts_total", "facet_type" => facet_type.to_string()).increment(1);
        let start = std::time::Instant::now();

        // Find and remove facet
        if self.metadata.remove(facet_type).is_none() {
            metrics::counter!("plexspaces_facet_detach_errors_total", "facet_type" => facet_type.to_string(), "error" => "not_found").increment(1);
            tracing::warn!(facet_type = %facet_type, "Facet not found for detach");
            return Err(FacetError::NotFound(facet_type.to_string()));
        }

        // Find facet in list (use try_read to avoid blocking)
        let pos = self.facets.iter().position(|f| {
            // Use try_read to avoid blocking the async runtime
            if let Ok(guard) = f.try_read() {
                guard.facet_type() == facet_type
            } else {
                // If locked, we can't check - this is rare and we'll handle it gracefully
                false
            }
        });

        if let Some(pos) = pos {
            let facet = self.facets.remove(pos);
            let mut facet = facet.write().await;
            facet.on_detach(actor_id).await?;
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_detach_duration_seconds", "facet_type" => facet_type.to_string()).record(duration.as_secs_f64());
        metrics::counter!("plexspaces_facet_detached_total", "facet_type" => facet_type.to_string()).increment(1);
        metrics::gauge!("plexspaces_facet_active_total", "facet_type" => facet_type.to_string())
            .decrement(1.0);
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(facet_type = %facet_type, actor_id = %actor_id, "Facet detached");
        }

        Ok(())
    }

    /// Execute before interceptors
    ///
    /// ## Returns
    /// `BeforeInterceptOutcome` enum that clearly indicates what the caller should do:
    /// - `CallActor(args)`: Call the actor's handle_message with these args (may have been modified)
    /// - `ShortCircuit(result)`: Don't call the actor, return this result directly
    ///
    /// ## Design
    /// This method returns an explicit enum instead of a hacky `(Vec<u8>, bool)` tuple.
    /// The enum clearly communicates intent and eliminates ambiguity about what should happen next.
    pub async fn intercept_before(
        &self,
        method: &str,
        args: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<BeforeInterceptOutcome, FacetError> {
        let span =
            tracing::span!(tracing::Level::TRACE, "facet.intercept_before", method = %method);
        let _enter = span.enter();

        metrics::counter!("plexspaces_facet_intercept_before_total", "method" => method.to_string()).increment(1);
        let start = std::time::Instant::now();

        let facet_count = self.get_facet_count();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(method = %method, facet_count = facet_count, "FacetContainer: Checking facets for interception");
        }

        let mut current_args = args.to_vec();

        for facet in &self.facets {
            let facet = facet.read().await;
            let facet_type = facet.facet_type();
            match facet.before_method(method, &current_args, headers).await? {
                InterceptResult::Continue => {
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(method = %method, facet_type = %facet_type, "FacetContainer: Facet continued (no interception)");
                    }
                }
                InterceptResult::ReplaceArgs(new_args) => {
                    current_args = new_args;
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(facet_type = %facet_type, "Facet replaced args");
                    }
                }
                InterceptResult::ShortCircuit(result) => {
                    let duration = start.elapsed();
                    metrics::histogram!("plexspaces_facet_intercept_before_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());
                    metrics::counter!("plexspaces_facet_intercept_shortcircuit_total", "method" => method.to_string(), "facet_type" => facet_type.to_string()).increment(1);
                    info!(method = %method, facet_type = %facet_type, duration_ms = duration.as_millis(), "🎯 FacetContainer: Facet short-circuited message");
                    return Ok(BeforeInterceptOutcome::ShortCircuit(result));
                }
                _ => {}
            }
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_intercept_before_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());
        let facet_count = self.get_facet_count();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(method = %method, facet_count = facet_count, duration_ms = duration.as_millis(), "FacetContainer: No facet intercepted, passing to actor");
        }

        Ok(BeforeInterceptOutcome::CallActor(current_args))
    }

    /// Execute after interceptors
    pub async fn intercept_after(
        &self,
        method: &str,
        args: &[u8],
        result: &[u8],
        headers: &std::collections::HashMap<String, String>,
    ) -> Result<Vec<u8>, FacetError> {
        let span = tracing::span!(tracing::Level::TRACE, "facet.intercept_after", method = %method);
        let _enter = span.enter();

        metrics::counter!("plexspaces_facet_intercept_after_total", "method" => method.to_string())
            .increment(1);
        let start = std::time::Instant::now();

        let mut current_result = result.to_vec();

        // Run in reverse order for after interceptors
        for facet in self.facets.iter().rev() {
            let facet = facet.read().await;
            match facet
                .after_method(method, args, &current_result, headers)
                .await?
            {
                InterceptResult::Continue => {}
                InterceptResult::ReplaceResult(new_result) => {
                    current_result = new_result;
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(facet_type = %facet.facet_type(), "Facet replaced result");
                    }
                }
                _ => {}
            }
        }

        let duration = start.elapsed();
        metrics::histogram!("plexspaces_facet_intercept_after_duration_seconds", "method" => method.to_string()).record(duration.as_secs_f64());

        Ok(current_result)
    }

    /// List attached facets
    /// Get the number of facets attached
    pub fn get_facet_count(&self) -> usize {
        self.facets.len()
    }

    /// List all attached facet types
    pub fn list_facets(&self) -> Vec<String> {
        self.metadata.keys().cloned().collect()
    }

    /// Get a facet by type (for FacetService - Option B)
    ///
    /// ## Arguments
    /// * `facet_type` - Facet type identifier
    ///
    /// ## Returns
    /// Arc to the facet if found, None otherwise
    pub fn get_facet(&self, facet_type: &str) -> Option<Arc<RwLock<Box<dyn Facet>>>> {
        // Find facet by type
        for facet_arc in &self.facets {
            // Try to read facet type (non-blocking check)
            // Note: This is a bit inefficient, but necessary for type matching
            if let Ok(facet_guard) = facet_arc.try_read() {
                if facet_guard.facet_type() == facet_type {
                    return Some(facet_arc.clone());
                }
            }
        }
        None
    }

    /// Get all facets (in priority order, high priority first)
    ///
    /// ## Returns
    /// Reference to facets vector (sorted by priority, descending)
    pub fn get_all_facets(&self) -> &[Arc<RwLock<Box<dyn Facet>>>] {
        &self.facets
    }

    /// Get metadata for all facets
    ///
    /// ## Returns
    /// Reference to metadata HashMap (facet_type -> FacetMetadata)
    pub fn get_metadata(&self) -> &HashMap<String, FacetMetadata> {
        &self.metadata
    }

    /// Detach all facets (for actor termination)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Result with any errors encountered (continues detaching even if one fails)
    ///
    /// ## Note
    /// Detaches in reverse priority order (low priority first, reverse of attach)
    pub async fn detach_all(&mut self, actor_id: &str) -> Vec<FacetError> {
        let mut errors = Vec::new();

        // Detach in reverse priority order (ascending priority, reverse of attach)
        // Sort by priority ascending for reverse order
        let mut facets_to_detach: Vec<(String, i32)> = self
            .metadata
            .iter()
            .map(|(facet_type, metadata)| (facet_type.clone(), metadata.priority))
            .collect();
        facets_to_detach.sort_by_key(|(_, priority)| *priority); // Ascending order

        for (facet_type, _) in facets_to_detach {
            if let Err(e) = self.detach(&facet_type, actor_id).await {
                errors.push(e);
            }
        }

        errors
    }

    /// Call on_exit() on all facets (in priority order, descending)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `from` - ID of linked actor that exited
    /// * `reason` - Exit reason
    ///
    /// ## Returns
    /// Result with any errors encountered (continues calling even if one fails)
    pub async fn call_on_exit(
        &mut self,
        actor_id: &str,
        from: &str,
        reason: &ExitReason,
    ) -> Vec<FacetError> {
        let mut errors = Vec::new();

        // Call on all facets in priority order (high priority first)
        for facet_arc in &self.facets {
            let mut facet = facet_arc.write().await;
            if let Err(e) = facet.on_exit(actor_id, from, reason).await {
                errors.push(e);
                // Continue with other facets even if one fails
            }
        }

        errors
    }

    /// Call on_down() on all facets (in priority order, descending)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `monitored_id` - ID of monitored actor that terminated
    /// * `reason` - Exit reason
    ///
    /// ## Returns
    /// Result with any errors encountered (continues calling even if one fails)
    pub async fn call_on_down(
        &mut self,
        actor_id: &str,
        monitored_id: &str,
        reason: &ExitReason,
    ) -> Vec<FacetError> {
        let mut errors = Vec::new();

        // Call on all facets in priority order (high priority first)
        for facet_arc in &self.facets {
            let mut facet = facet_arc.write().await;
            if let Err(e) = facet.on_down(actor_id, monitored_id, reason).await {
                errors.push(e);
                // Continue with other facets even if one fails
            }
        }

        errors
    }

    /// Call on_init_complete() on all facets (in priority order, descending)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    ///
    /// ## Returns
    /// Result with any errors encountered (continues calling even if one fails)
    pub async fn call_on_init_complete(&mut self, actor_id: &str) -> Vec<FacetError> {
        let mut errors = Vec::new();

        // Call on all facets in priority order (high priority first)
        for facet_arc in &self.facets {
            let mut facet = facet_arc.write().await;
            if let Err(e) = facet.on_init_complete(actor_id).await {
                errors.push(e);
                // Continue with other facets even if one fails
            }
        }

        errors
    }

    /// Call on_terminate_start() on all facets (in priority order, descending)
    ///
    /// ## Arguments
    /// * `actor_id` - Actor ID
    /// * `reason` - Exit reason
    ///
    /// ## Returns
    /// Result with any errors encountered (continues calling even if one fails)
    pub async fn call_on_terminate_start(
        &mut self,
        actor_id: &str,
        reason: &ExitReason,
    ) -> Vec<FacetError> {
        let mut errors = Vec::new();

        // Call on all facets in priority order (high priority first)
        for facet_arc in &self.facets {
            let mut facet = facet_arc.write().await;
            if let Err(e) = facet.on_terminate_start(actor_id, reason).await {
                errors.push(e);
                // Continue with other facets even if one fails
            }
        }

        errors
    }
}

/// Registry for available facet implementations
pub struct FacetRegistry {
    factories: HashMap<String, Arc<dyn FacetFactory>>,
}

/// Factory for creating facet instances
#[async_trait]
pub trait FacetFactory: Send + Sync {
    /// Create a new facet instance
    async fn create(&self, config: Value) -> Result<Box<dyn Facet>, FacetError>;

    /// Get facet metadata
    fn metadata(&self) -> FacetMetadata;
}

impl Default for FacetRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl FacetRegistry {
    /// Creates a new empty facet registry
    pub fn new() -> Self {
        FacetRegistry {
            factories: HashMap::new(),
        }
    }

    /// Register a facet factory
    pub fn register(&mut self, facet_type: String, factory: Arc<dyn FacetFactory>) {
        self.factories.insert(facet_type, factory);
    }

    /// Create a facet instance
    pub async fn create_facet(
        &self,
        facet_type: &str,
        config: Value,
    ) -> Result<Box<dyn Facet>, FacetError> {
        let factory = self
            .factories
            .get(facet_type)
            .ok_or_else(|| FacetError::NotFound(facet_type.to_string()))?;

        factory.create(config).await
    }

    /// List available facet types
    pub fn list_types(&self) -> Vec<String> {
        self.factories.keys().cloned().collect()
    }
}

// FacetRegistry doesn't implement Service here - core provides a wrapper if needed

// Example facets demonstrating the pattern

/// Logging facet - logs all method calls
pub struct LoggingFacet {
    config: Value,
    priority: i32,
    level: String,
}

/// Default priority for LoggingFacet
pub const LOGGING_FACET_DEFAULT_PRIORITY: i32 = 900;

impl LoggingFacet {
    /// Create a new logging facet
    pub fn new(config: Value, priority: i32) -> Self {
        let level = config
            .get("level")
            .and_then(|v| v.as_str())
            .unwrap_or("INFO")
            .to_string();
        LoggingFacet {
            config,
            priority,
            level,
        }
    }
}

#[async_trait]
impl Facet for LoggingFacet {
    fn facet_type(&self) -> &str {
        "logging"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(actor_id = %actor_id, "Logging facet attached to actor");
        }
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        tracing::debug!(actor_id = %actor_id, "Logging facet detached from actor");
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                level = %self.level,
                method = %method,
                args_bytes = args.len(),
                "Calling method"
            );
        }
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        _result: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        tracing::debug!(
            level = %self.level,
            method = %method,
            result_bytes = _result.len(),
            "Method returned"
        );
        Ok(InterceptResult::Continue)
    }

    fn get_config(&self) -> Value {
        self.config.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// LRU cache entry: value bytes and insertion timestamp
struct LruEntry {
    value: Vec<u8>,
    inserted_at: std::time::Instant,
}

/// Bounded LRU cache with TTL expiry. Interior-mutable via `Mutex` so `&self` methods can write.
struct LruCache {
    /// Ordered from oldest (front) to newest (back)
    order: std::collections::VecDeque<String>,
    entries: HashMap<String, LruEntry>,
    capacity: usize,
}

impl LruCache {
    fn new(capacity: usize) -> Self {
        LruCache {
            order: std::collections::VecDeque::with_capacity(capacity),
            entries: HashMap::with_capacity(capacity),
            capacity,
        }
    }

    fn get(&mut self, key: &str, ttl: std::time::Duration) -> Option<Vec<u8>> {
        let entry = self.entries.get(key)?;
        if entry.inserted_at.elapsed() >= ttl {
            self.remove(key);
            return None;
        }
        // Move to back (most recently used)
        self.order.retain(|k| k != key);
        self.order.push_back(key.to_string());
        Some(entry.value.clone())
    }

    fn insert(&mut self, key: String, value: Vec<u8>) {
        if self.entries.contains_key(&key) {
            self.order.retain(|k| k != &key);
        } else if self.entries.len() >= self.capacity {
            // Evict LRU (oldest)
            if let Some(lru_key) = self.order.pop_front() {
                self.entries.remove(&lru_key);
            }
        }
        self.order.push_back(key.clone());
        self.entries.insert(
            key,
            LruEntry {
                value,
                inserted_at: std::time::Instant::now(),
            },
        );
    }

    fn remove(&mut self, key: &str) {
        self.entries.remove(key);
        self.order.retain(|k| k != key);
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }
}

/// Caching facet - LRU cache with TTL-based expiry and capacity-based eviction
pub struct CachingFacet {
    config: Value,
    priority: i32,
    /// Interior mutability so `&self` methods (`before_method`, `after_method`) can write.
    cache: std::sync::Mutex<LruCache>,
    ttl: std::time::Duration,
}

/// Default priority for CachingFacet
pub const CACHING_FACET_DEFAULT_PRIORITY: i32 = 40;

/// Default maximum entries in CachingFacet LRU cache
pub const CACHING_FACET_DEFAULT_CAPACITY: usize = 1000;

impl CachingFacet {
    /// Create a new caching facet
    pub fn new(config: Value, priority: i32) -> Self {
        let ttl = config
            .get("ttl_seconds")
            .and_then(|v| v.as_u64())
            .map(std::time::Duration::from_secs)
            .unwrap_or(std::time::Duration::from_secs(300));
        let capacity = config
            .get("max_entries")
            .and_then(|v| v.as_u64())
            .map(|n| n as usize)
            .unwrap_or(CACHING_FACET_DEFAULT_CAPACITY);
        CachingFacet {
            config,
            priority,
            cache: std::sync::Mutex::new(LruCache::new(capacity)),
            ttl,
        }
    }
}

#[async_trait]
impl Facet for CachingFacet {
    fn facet_type(&self) -> &str {
        "caching"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        self.cache.lock().unwrap_or_else(|p| p.into_inner()).clear();
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        let key = format!("{}:{}", method, hex::encode(args));
        if let Some(value) = self
            .cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .get(&key, self.ttl)
        {
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!(method = %method, "Cache hit");
            }
            return Ok(InterceptResult::ShortCircuit(value));
        }
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        args: &[u8],
        result: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        let key = format!("{}:{}", method, hex::encode(args));
        self.cache
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(key, result.to_vec());
        Ok(InterceptResult::Continue)
    }

    fn get_config(&self) -> Value {
        self.config.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Metrics facet - tracks method performance
pub struct MetricsFacet {
    config: Value,
    priority: i32,
    metrics: HashMap<String, MethodMetrics>,
}

/// Default priority for MetricsFacet
pub const METRICS_FACET_DEFAULT_PRIORITY: i32 = 800;

impl MetricsFacet {
    /// Create a new metrics facet
    pub fn new(config: Value, priority: i32) -> Self {
        MetricsFacet {
            config,
            priority,
            metrics: HashMap::new(),
        }
    }
}

#[derive(Default)]
struct MethodMetrics {
    count: u64,
    total_time_ms: u64,
    errors: u64,
}

#[async_trait]
impl Facet for MetricsFacet {
    fn facet_type(&self) -> &str {
        "metrics"
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    async fn on_attach(&mut self, _actor_id: &str, _config: Value) -> Result<(), FacetError> {
        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Log final metrics
        for (method, metrics) in &self.metrics {
            let avg_time = if metrics.count > 0 {
                metrics.total_time_ms / metrics.count
            } else {
                0
            };
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    method = %method,
                    calls = metrics.count,
                    avg_ms = avg_time,
                    errors = metrics.errors,
                    "Method metrics on detach"
                );
            }
        }
        Ok(())
    }

    async fn before_method(
        &self,
        _method: &str,
        _args: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        // Start timing (would need to store start time somewhere)
        Ok(InterceptResult::Continue)
    }

    async fn after_method(
        &self,
        method: &str,
        _args: &[u8],
        _result: &[u8],
        _headers: &std::collections::HashMap<String, String>,
    ) -> Result<InterceptResult, FacetError> {
        // Update metrics (would need mutable self)
        tracing::trace!(method = %method, "Recording metrics");
        Ok(InterceptResult::Continue)
    }

    async fn on_error(&self, method: &str, _error: &str) -> Result<ErrorHandling, FacetError> {
        // Increment error count
        tracing::debug!(method = %method, "Error in method");
        Ok(ErrorHandling::Propagate)
    }

    fn get_config(&self) -> Value {
        self.config.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_facet_container() {
        let mut container = FacetContainer::new();

        // Create and attach a logging facet
        let config = serde_json::json!({
            "level": "DEBUG"
        });
        let facet = Box::new(LoggingFacet::new(config.clone(), 10));

        let facet_id = container.attach(facet, "test_actor").await.unwrap();

        assert_eq!(facet_id, "logging");
        assert_eq!(container.list_facets(), vec!["logging"]);

        // Test interception
        let args = b"test args";
        let outcome = container
            .intercept_before("test_method", args, &std::collections::HashMap::new())
            .await
            .unwrap();
        match outcome {
            BeforeInterceptOutcome::CallActor(result) => {
                assert_eq!(
                    result, args,
                    "Should return original args when no facets intercept"
                );
            }
            BeforeInterceptOutcome::ShortCircuit(_) => {
                panic!("Should not be short-circuited when no facets intercept");
            }
        }
    }

    #[tokio::test]
    async fn test_facet_registry() {
        let mut registry = FacetRegistry::new();

        // Register a factory (would need to implement FacetFactory)
        // registry.register("logging", Arc::new(LoggingFacetFactory));

        let types = registry.list_types();
        assert_eq!(types.len(), 0); // No factories registered in this test
    }
}
