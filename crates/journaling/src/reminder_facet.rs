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

//! Reminder Facet (Durable Reminders)
//!
//! ## Purpose
//! Provides durable, persistent reminders for actors. Reminders survive actor
//! deactivation and crashes, making them suitable for critical operations like
//! billing, SLA enforcement, and scheduled tasks.
//!
//! ## Architecture Context
//! Part of Phase 8.5: High Priority Missing Features. Implements Orleans-style
//! reminders as an opt-in facet (not default).
//!
//! ## Design Decision
//! Reminders are opt-in via facet to maintain simplicity:
//! - Regular actors: No reminders (simple, predictable)
//! - Actors with ReminderFacet: Can register reminders (for billing, SLA, cron jobs)
//!
//! ## Design Notes
//! ReminderFacet uses `Arc<dyn JournalStorage>` (trait object) instead of generics.
//! This design choice:
//! - Enables SDK annotation support (`facets = ["reminder"]`)
//! - Is consistent with DurabilityFacet and other facets
//! - Uses standard Rust trait object pattern for runtime polymorphism
//! - Allows storage backend to be configured at runtime
//!
//! ## How It Works
//! ```text
//! 1. Actor attaches ReminderFacet
//! 2. Actor registers reminder → Persisted to storage
//! 3. Background task checks for due reminders
//! 4. Reminder fires → ReminderFired message sent to actor
//! 5. If actor deactivated → Auto-activate (via VirtualActorFacet integration)
//! 6. Actor deactivates → Reminders persist (survive deactivation)
//! ```
//!
//! ## Example
//! ```rust,no_run
//! use plexspaces_journaling::*;
//! use plexspaces_actor::JournalStorage;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create storage backend (trait object)
//! let storage: Arc<dyn JournalStorage> = Arc::new(
//!     SqliteJournalStorage::new(":memory:").await?
//! );
//!
//! // Create reminder facet with trait object storage
//! let facet = ReminderFacet::new(storage, serde_json::json!({}), 50);
//!
//! // Attach to actor via spawn_actor(..., facets)
//! # Ok(())
//! # }
//! ```

use async_trait::async_trait;
use plexspaces_common::RequestContextExt;
use plexspaces_facet::{Facet, FacetError};
use plexspaces_proto::common::v1::Message;
use plexspaces_proto::prost_types;
use plexspaces_proto::timer::v1::{ReminderRegistration, ReminderState};
use plexspaces_service_traits::{ActorId, JournalStorage, ServiceLocatorBase};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

// Re-export ReminderFired from proto
pub use plexspaces_proto::timer::v1::ReminderFired;

/// Reminder Facet for durable, persistent reminders
///
/// ## Purpose
/// Implements Orleans-inspired durable reminders. Reminders are persisted to
/// storage and survive actor deactivation and crashes.
///
/// ## Design
/// Uses `Arc<dyn JournalStorage>` (trait object) for storage backend, enabling:
/// - SDK annotation support (`facets = ["reminder"]`)
/// - Runtime storage backend configuration
/// - Consistency with DurabilityFacet pattern
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to reminder state.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_journaling::*;
/// use plexspaces_actor::JournalStorage;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let storage: Arc<dyn JournalStorage> = Arc::new(
///     SqliteJournalStorage::new(":memory:").await?
/// );
/// let facet = ReminderFacet::new(storage, serde_json::json!({}), 50);
/// # Ok(())
/// # }
/// ```
pub struct ReminderFacet {
    /// Facet configuration (immutable)
    config: Value,

    /// Facet priority (immutable)
    priority: i32,

    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,

    /// ServiceLocator for looking up ActorService when sending messages
    service_locator: Arc<dyn ServiceLocatorBase>,

    /// Journal storage backend (trait object for runtime polymorphism)
    storage: Arc<dyn JournalStorage>,

    /// Active reminders: reminder_name -> ReminderState
    reminders: Arc<RwLock<HashMap<String, ReminderState>>>,

    /// Background task handle for checking due reminders
    background_task: Arc<RwLock<Option<JoinHandle<()>>>>,

    /// Shutdown signal for background task
    shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
}

/// Default priority for ReminderFacet
pub const REMINDER_FACET_DEFAULT_PRIORITY: i32 = 50;

impl ReminderFacet {
    /// Create a new reminder facet
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend as trait object
    /// * `config` - Facet configuration (can be empty object `{}` for defaults)
    /// * `priority` - Facet priority (default: 50)
    /// * `service_locator` - ServiceLocator for looking up ActorService when sending messages
    ///
    /// ## Returns
    /// New ReminderFacet ready to attach to an actor
    ///
    /// ## Example
    /// ```rust,no_run
    /// # use plexspaces_journaling::*;
    /// # use plexspaces_actor::{JournalStorage, ServiceLocator};
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let storage: Arc<dyn JournalStorage> = Arc::new(
    ///     SqliteJournalStorage::new(":memory:").await?
    /// );
    /// let facet = ReminderFacet::new(storage, serde_json::json!({}), 50, service_locator);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(
        storage: Arc<dyn JournalStorage>,
        config: Value,
        priority: i32,
        service_locator: Arc<dyn ServiceLocatorBase>,
    ) -> Self {
        ReminderFacet {
            config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            service_locator,
            storage,
            reminders: Arc::new(RwLock::new(HashMap::new())),
            background_task: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
        }
    }

    /// Create a new reminder facet with default configuration
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend as trait object
    /// * `service_locator` - ServiceLocator for looking up ActorService when sending messages
    ///
    /// ## Returns
    /// New ReminderFacet with default priority (50) and empty config
    pub fn with_storage(
        storage: Arc<dyn JournalStorage>,
        service_locator: Arc<dyn ServiceLocatorBase>,
    ) -> Self {
        Self::new(
            storage,
            serde_json::json!({}),
            REMINDER_FACET_DEFAULT_PRIORITY,
            service_locator,
        )
    }

    /// Register a reminder
    ///
    /// ## Arguments
    /// * `registration` - Reminder registration details
    ///
    /// ## Returns
    /// Reminder ID (for unregistration) or error
    pub async fn register_reminder(
        &self,
        registration: ReminderRegistration,
    ) -> Result<String, ReminderError> {
        let _actor_id = self
            .actor_id
            .read()
            .await
            .clone()
            .ok_or(ReminderError::NotAttached)?;

        // Validate registration
        if registration.reminder_name.is_empty() {
            return Err(ReminderError::InvalidRegistration(
                "reminder_name cannot be empty".to_string(),
            ));
        }

        // Check if reminder already exists
        let mut reminders = self.reminders.write().await;
        if reminders.contains_key(&registration.reminder_name) {
            return Err(ReminderError::ReminderExists(
                registration.reminder_name.clone(),
            ));
        }

        // Convert proto Duration to std::time::Duration
        let interval = proto_duration_to_std(&registration.interval)
            .ok_or_else(|| ReminderError::InvalidRegistration("invalid interval".to_string()))?;

        // Validate interval (must be > 0)
        if interval.is_zero() {
            return Err(ReminderError::InvalidRegistration(
                "interval must be > 0".to_string(),
            ));
        }

        // Calculate next fire time
        let now = SystemTime::now();
        let first_fire_time = registration
            .first_fire_time
            .as_ref()
            .map(proto_timestamp_to_system_time)
            .unwrap_or(now);

        // If first_fire_time is in the past, fire immediately
        let next_fire_time = if first_fire_time <= now {
            now
        } else {
            first_fire_time
        };

        // Create reminder state
        let reminder_state = ReminderState {
            registration: Some(registration.clone()),
            last_fired: None,
            next_fire_time: Some(prost_types::Timestamp::from(next_fire_time)),
            fire_count: 0,
            is_active: true,
        };

        // Persist to storage
        self.storage
            .register_reminder(&reminder_state)
            .await
            .map_err(|e| ReminderError::Storage(e.to_string()))?;

        // Store in memory
        reminders.insert(registration.reminder_name.clone(), reminder_state);

        // Start background task if not already running
        self.start_background_task_if_needed().await?;

        Ok(registration.reminder_name)
    }

    /// Unregister a reminder
    ///
    /// ## Arguments
    /// * `reminder_name` - Name of reminder to unregister
    ///
    /// ## Returns
    /// Success or error
    pub async fn unregister_reminder(&self, reminder_name: &str) -> Result<(), ReminderError> {
        let mut reminders = self.reminders.write().await;

        if reminders.contains_key(reminder_name) {
            // Remove from storage
            let actor_id = self
                .actor_id
                .read()
                .await
                .clone()
                .ok_or(ReminderError::NotAttached)?;

            self.storage
                .unregister_reminder(&actor_id, reminder_name)
                .await
                .map_err(|e| ReminderError::Storage(e.to_string()))?;

            // Remove from memory
            reminders.remove(reminder_name);

            Ok(())
        } else {
            Err(ReminderError::ReminderNotFound(reminder_name.to_string()))
        }
    }

    /// List all reminders for this actor
    ///
    /// ## Returns
    /// Vector of reminder states
    pub async fn list_reminders(&self) -> Vec<ReminderState> {
        let reminders = self.reminders.read().await;
        reminders.values().cloned().collect()
    }

    /// Start background task for checking due reminders
    async fn start_background_task_if_needed(&self) -> Result<(), ReminderError> {
        let mut task_guard = self.background_task.write().await;

        if task_guard.is_some() {
            // Already running
            return Ok(());
        }

        let reminders = self.reminders.clone();
        let storage = self.storage.clone();
        let service_locator = self.service_locator.clone();
        let shutdown_tx = self.shutdown_tx.clone();
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        *shutdown_tx.write().await = Some(tx);

        let handle = tokio::spawn(async move {
            loop {
                // Check for shutdown signal
                if rx.try_recv().is_ok() {
                    break;
                }

                let now = SystemTime::now();

                // Get due reminders from storage (more efficient than checking all in memory)
                let due_reminders = storage.query_due_reminders(now).await.unwrap_or_default();

                // Debug: Log if we found due reminders (only in debug mode)
                if !due_reminders.is_empty() && tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!("Found {} due reminders", due_reminders.len());
                }

                // Fire due reminders
                for reminder in due_reminders {
                    // Get registration (unwrap since it should always be Some for active reminders)
                    let reg = match reminder.registration.as_ref() {
                        Some(r) => r,
                        None => {
                            tracing::warn!("Reminder has no registration, skipping");
                            continue;
                        }
                    };

                    let actor_id_str = reg.actor_id.clone();
                    let actor_id = match ActorId::from_canonical(&actor_id_str) {
                        Ok(actor_id) => actor_id,
                        Err(error) => {
                            tracing::warn!(
                                actor_id = %actor_id_str,
                                error = %error,
                                "Skipping reminder with invalid canonical actor ID"
                            );
                            continue;
                        }
                    };

                    if let Some(checker) = service_locator.get_actor_state_checker().await {
                        let is_active = checker.is_actor_state_active(&actor_id).await;
                        if !is_active {
                            if let Some(factory) = service_locator.get_actor_factory().await {
                                let _ = factory.activate_virtual_actor(&actor_id).await;
                            }
                        }
                    }

                    // Fire reminder - get ActorService from ServiceLocator when needed
                    if let Some(actor_service) = service_locator.get_actor_service().await {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            tracing::debug!("Firing reminder: {}", reg.reminder_name);
                        }
                        let reminder_fired = ReminderFired {
                            actor_id: reg.actor_id.clone(),
                            reminder_name: reg.reminder_name.clone(),
                            fired_at: Some(prost_types::Timestamp::from(now)),
                            callback_data: reg.callback_data.clone(),
                        };

                        // Encode ReminderFired using prost
                        let payload = prost::Message::encode_to_vec(&reminder_fired);

                        // Create message with reminder type
                        let mut headers = std::collections::HashMap::new();
                        headers.insert("type".to_string(), "ReminderFired".to_string());
                        headers.insert("reminder_name".to_string(), reg.reminder_name.clone());
                        let message = Message {
                            id: ulid::Ulid::new().to_string(),
                            payload,
                            message_type: "ReminderFired".to_string(),
                            headers,
                            receiver_id: actor_id_str.clone(),
                            sender_id: String::new(),
                            ..Default::default()
                        };

                        // Use ActorService to send message (handles local/remote routing)
                        let ctx = plexspaces_common::RequestContext::new_without_auth(
                            String::new(),
                            String::new(),
                        );
                        if let Err(e) = actor_service.send(&ctx, &actor_id_str, message).await {
                            tracing::warn!("Failed to send reminder message: {}", e);
                        }
                    } else {
                        tracing::warn!(
                            "Skipping reminder {}: ActorService not available",
                            reg.reminder_name
                        );
                    }

                    // Update reminder state
                    let mut updated_reminder = reminder.clone();
                    updated_reminder.fire_count += 1;
                    updated_reminder.last_fired = Some(prost_types::Timestamp::from(now));

                    // Check if max_occurrences reached
                    if let Some(reg) = updated_reminder.registration.as_ref() {
                        if reg.max_occurrences > 0
                            && updated_reminder.fire_count >= reg.max_occurrences
                        {
                            // Auto-delete reminder
                            updated_reminder.is_active = false;
                            storage
                                .unregister_reminder(&reg.actor_id, &reg.reminder_name)
                                .await
                                .unwrap_or_default();

                            // Remove from memory
                            let mut reminders_guard = reminders.write().await;
                            reminders_guard.remove(&reg.reminder_name);
                        } else {
                            // Schedule next fire
                            let interval = proto_duration_to_std(&reg.interval)
                                .unwrap_or(Duration::from_secs(1));
                            updated_reminder.next_fire_time =
                                Some(prost_types::Timestamp::from(now + interval));

                            // Update in storage
                            storage
                                .update_reminder(&updated_reminder)
                                .await
                                .unwrap_or_default();

                            // Update in memory
                            let mut reminders_guard = reminders.write().await;
                            reminders_guard.insert(reg.reminder_name.clone(), updated_reminder);
                        }
                    }
                }

                // Sleep for a short duration before next check
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        });

        *task_guard = Some(handle);
        Ok(())
    }

    /// Stop background task
    async fn stop_background_task(&self) {
        // Send shutdown signal
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }

        // Wait for task to complete
        if let Some(handle) = self.background_task.write().await.take() {
            let _ = handle.await;
        }
    }
}

#[async_trait]
impl Facet for ReminderFacet {
    fn facet_type(&self) -> &str {
        "reminder"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        let mut id = self.actor_id.write().await;
        *id = Some(actor_id.to_string());
        drop(id);

        // Load existing reminders from storage
        let loaded_reminders = self
            .storage
            .load_reminders(actor_id)
            .await
            .unwrap_or_default();

        // Restore reminders to memory
        let mut reminders = self.reminders.write().await;
        for reminder in loaded_reminders {
            if reminder.is_active {
                if let Some(reg) = reminder.registration.as_ref() {
                    reminders.insert(reg.reminder_name.clone(), reminder);
                }
            }
        }

        // Start background task
        self.start_background_task_if_needed()
            .await
            .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

        Ok(())
    }

    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Stop background task
        self.stop_background_task().await;

        // Persist reminders to storage (save all active reminders)
        let reminders_to_save = {
            let reminders_guard = self.reminders.read().await;
            reminders_guard
                .values()
                .filter(|r| {
                    r.is_active
                        && r.registration
                            .as_ref()
                            .map(|reg| reg.persist_across_activations)
                            .unwrap_or(false)
                })
                .cloned()
                .collect::<Vec<_>>()
        };

        for reminder in reminders_to_save {
            let _ = self.storage.update_reminder(&reminder).await;
        }

        // Clear reminders
        let mut reminders = self.reminders.write().await;
        reminders.clear();

        // Clear actor ID
        let mut id = self.actor_id.write().await;
        *id = None;

        Ok(())
    }

    /// Phase 4.3: Handle EXIT signal from linked actor
    ///
    /// ## Purpose
    /// Pauses all reminders when actor receives EXIT signal from linked actor.
    /// This prevents reminders from firing while the actor is terminating.
    /// Reminders remain persisted and can be resumed after restart.
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
        // Pause all reminders on EXIT (mark as inactive)
        let mut reminders = self.reminders.write().await;
        let mut paused_count = 0;

        for (reminder_name, reminder_state) in reminders.iter_mut() {
            if reminder_state.is_active {
                reminder_state.is_active = false;
                paused_count += 1;

                // Persist paused state to storage
                if let Err(e) = self.storage.update_reminder(reminder_state).await {
                    tracing::warn!(
                        actor_id = %actor_id,
                        reminder_name = %reminder_name,
                        error = %e,
                        "Failed to persist paused reminder state"
                    );
                } else if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        actor_id = %actor_id,
                        reminder_name = %reminder_name,
                        "Paused reminder on EXIT signal"
                    );
                }
            }
        }

        if paused_count > 0 {
            metrics::counter!(
                "plexspaces_reminder_facet_exit_paused_total",
                "actor_id" => actor_id.to_string(),
                "reminder_count" => paused_count.to_string()
            )
            .increment(paused_count as u64);
            tracing::info!(
                actor_id = %actor_id,
                reminder_count = paused_count,
                "Paused all reminders on EXIT signal"
            );
        }

        Ok(())
    }

    /// Phase 4.3: Handle DOWN notification from monitored actor
    ///
    /// ## Purpose
    /// Logs DOWN notification for observability. ReminderFacet doesn't need to
    /// take action on DOWN notifications (reminders are actor-specific).
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
        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %actor_id,
                monitored_id = %monitored_id,
                reason = ?reason,
                "ReminderFacet received DOWN notification (no action needed)"
            );
        }

        metrics::counter!(
            "plexspaces_reminder_facet_down_total",
            "actor_id" => actor_id.to_string(),
            "monitored_id" => monitored_id.to_string()
        )
        .increment(1);

        Ok(())
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        let _reminders = self.reminders.read();
        // TODO: Serialize reminder state for persistence (if needed)
        Ok(Value::Null)
    }

    fn get_config(&self) -> Value {
        self.config.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Reminder errors
#[derive(Debug, thiserror::Error)]
pub enum ReminderError {
    /// Facet not attached to an actor
    #[error("Reminder facet not attached to actor")]
    NotAttached,

    /// Reminder with this name already exists
    #[error("Reminder already exists: {0}")]
    ReminderExists(String),

    /// Reminder not found
    #[error("Reminder not found: {0}")]
    ReminderNotFound(String),

    /// Invalid registration parameters
    #[error("Invalid registration: {0}")]
    InvalidRegistration(String),

    /// Storage backend error
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Convert proto Duration to std::time::Duration
fn proto_duration_to_std(duration: &Option<prost_types::Duration>) -> Option<Duration> {
    duration
        .as_ref()
        .map(|d| Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64))
}

/// Convert proto Timestamp to SystemTime
fn proto_timestamp_to_system_time(timestamp: &prost_types::Timestamp) -> SystemTime {
    SystemTime::UNIX_EPOCH
        + Duration::from_secs(timestamp.seconds as u64)
        + Duration::from_nanos(timestamp.nanos as u64)
}

#[cfg(all(test, feature = "sqlite-backend"))]
mod tests {
    use super::*;
    use crate::SqliteJournalStorage;
    use plexspaces_service_traits::{ActorId, ActorRef, ActorService};
    use plexspaces_services::ServiceLocatorImpl;
    use prost_types;
    use std::sync::Arc;

    struct MockActorService;

    #[async_trait::async_trait]
    impl ActorService for MockActorService {
        async fn spawn_actor(
            &self,
            _ctx: &plexspaces_common::RequestContext,
            _spec: &plexspaces_proto::actor::v1::ActorSpawnSpec,
        ) -> Result<ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented for tests".into())
        }

        async fn send(
            &self,
            _ctx: &plexspaces_common::RequestContext,
            _actor_id: &str,
            _message: Message,
        ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("message-id".to_string())
        }
    }

    async fn create_test_service_locator() -> Arc<dyn ServiceLocatorBase> {
        let service_locator = Arc::new(ServiceLocatorImpl::new());
        service_locator
            .register_actor_service(Arc::new(MockActorService))
            .await;
        service_locator
    }

    /// Canonical [`ActorId`] string shared by reminder tests (`on_attach` + registration).
    fn reminder_tests_actor_id() -> String {
        ActorId::new("actor-1", "reminder_actor", "default", "test-node")
            .expect("valid canonical actor id for reminder tests")
            .to_string()
    }

    /// Creates a test facet with SQLite :memory: backend.
    /// Uses in-memory SQLite for fast, isolated test execution.
    async fn create_test_facet() -> ReminderFacet {
        let storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
        let service_locator = create_test_service_locator().await;
        let facet = ReminderFacet::new(storage, serde_json::json!({}), 75, service_locator);

        facet
    }

    fn create_test_reminder_registration(
        reminder_name: &str,
        interval_secs: u64,
        first_fire_secs: u64,
        max_occurrences: i32,
    ) -> ReminderRegistration {
        let now = SystemTime::now();
        let first_fire_time = now + Duration::from_secs(first_fire_secs);

        ReminderRegistration {
            actor_id: reminder_tests_actor_id(),
            reminder_name: reminder_name.to_string(),
            interval: Some(prost_types::Duration {
                seconds: interval_secs as i64,
                nanos: 0,
            }),
            first_fire_time: Some(prost_types::Timestamp::from(first_fire_time)),
            callback_data: vec![],
            persist_across_activations: true,
            max_occurrences,
        }
    }

    #[tokio::test]
    async fn test_reminder_facet_creation() {
        let facet = create_test_facet().await;
        assert_eq!(facet.facet_type(), "reminder");
    }

    #[tokio::test]
    async fn test_reminder_facet_with_storage() {
        let storage: Arc<dyn JournalStorage> =
            Arc::new(SqliteJournalStorage::new(":memory:").await.unwrap());
        let facet = ReminderFacet::with_storage(storage, create_test_service_locator().await);
        assert_eq!(facet.facet_type(), "reminder");
        assert_eq!(facet.get_priority(), REMINDER_FACET_DEFAULT_PRIORITY);
    }

    #[tokio::test]
    async fn test_reminder_facet_attach() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let actor_id = facet.actor_id.read().await.clone();
        assert_eq!(actor_id, Some(reminder_tests_actor_id()));
    }

    #[tokio::test]
    async fn test_register_reminder_before_attach_fails() {
        let facet = create_test_facet().await;

        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        let result = facet.register_reminder(registration).await;

        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReminderError::NotAttached));
    }

    #[tokio::test]
    async fn test_register_reminder_after_attach() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        let reminder_id = facet.register_reminder(registration).await.unwrap();

        assert_eq!(reminder_id, "reminder-1");

        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 1);
        assert_eq!(
            reminders[0].registration.as_ref().unwrap().reminder_name,
            "reminder-1"
        );
    }

    #[tokio::test]
    async fn test_register_duplicate_reminder_fails() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration1 = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration1).await.unwrap();

        let registration2 = create_test_reminder_registration("reminder-1", 2, 0, 0);
        let result = facet.register_reminder(registration2).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ReminderError::ReminderExists(_)
        ));
    }

    #[tokio::test]
    async fn test_unregister_reminder() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration).await.unwrap();

        facet.unregister_reminder("reminder-1").await.unwrap();

        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 0);
    }

    #[tokio::test]
    async fn test_unregister_nonexistent_reminder_fails() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let result = facet.unregister_reminder("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ReminderError::ReminderNotFound(_)
        ));
    }

    #[tokio::test]
    async fn test_reminder_with_max_occurrences() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration = create_test_reminder_registration("reminder-1", 1, 0, 3);
        facet.register_reminder(registration).await.unwrap();

        // Poll until max_occurrences removes the reminder (background task + 1s interval × 3).
        let poll = Duration::from_millis(50);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        loop {
            if facet.list_reminders().await.is_empty() {
                break;
            }
            if tokio::time::Instant::now() >= deadline {
                panic!(
                    "expected reminder removed after max_occurrences; still {:?}",
                    facet.list_reminders().await
                );
            }
            tokio::time::sleep(poll).await;
        }
    }

    #[tokio::test]
    async fn test_multiple_reminders() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration1 = create_test_reminder_registration("reminder-1", 1, 0, 0);
        let registration2 = create_test_reminder_registration("reminder-2", 2, 0, 0);
        let registration3 = create_test_reminder_registration("reminder-3", 3, 0, 0);

        facet.register_reminder(registration1).await.unwrap();
        facet.register_reminder(registration2).await.unwrap();
        facet.register_reminder(registration3).await.unwrap();

        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 3);
    }

    #[tokio::test]
    async fn test_reminder_zero_interval_fails() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let mut registration = create_test_reminder_registration("reminder-1", 0, 0, 0);
        registration.interval = Some(prost_types::Duration {
            seconds: 0,
            nanos: 0,
        });
        let result = facet.register_reminder(registration).await;

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ReminderError::InvalidRegistration(_)
        ));
    }

    #[tokio::test]
    async fn test_reminder_detach_stops_background_task() {
        let mut facet = create_test_facet().await;
        facet
            .on_attach(&reminder_tests_actor_id(), serde_json::json!({}))
            .await
            .unwrap();

        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration).await.unwrap();

        // Detach should stop background task
        facet.on_detach("actor-1").await.unwrap();

        // Reminders should be cleared
        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 0);
    }
}
