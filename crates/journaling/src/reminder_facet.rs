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
// GNU Lesser Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public License
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
//! ## How It Works
//! ```text
//! 1. Actor attaches ReminderFacet
//! 2. Actor registers reminder → Persisted to storage
//! 3. Background task checks for due reminders
//! 4. Reminder fires → ReminderFired message sent to actor
//! 5. If actor deactivated → Auto-activate (via VirtualActorFacet integration)
//! 6. Actor deactivates → Reminders persist (survive deactivation)
//! ```

use crate::storage::{JournalStorage, ReminderRegistration, ReminderState};
use async_trait::async_trait;
use plexspaces_core::{ActorId, ActorRef, ActorService};
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use plexspaces_mailbox::Message;
use plexspaces_proto::prost_types;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

/// Trait for activating virtual actors (used by ReminderFacet)
///
/// ## Purpose
/// Allows ReminderFacet to trigger actor activation when reminders fire.
/// This enables reminders to wake up deactivated virtual actors.
///
/// ## Design
/// Similar to LinkProvider pattern - decouples ReminderFacet from Node.
#[async_trait]
pub trait ActivationProvider: Send + Sync {
    /// Check if actor is currently active
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to check
    ///
    /// ## Returns
    /// true if actor is active, false if deactivated
    async fn is_actor_active(&self, actor_id: &ActorId) -> bool;
    
    /// Activate a virtual actor
    ///
    /// ## Arguments
    /// * `actor_id` - Actor to activate
    ///
    /// ## Returns
    /// ActorRef if activation successful, error otherwise
    async fn activate_actor(&self, actor_id: &ActorId) -> Result<ActorRef, String>;
}

// Re-export ReminderFired from proto
pub use plexspaces_proto::timer::v1::ReminderFired;

/// Reminder Facet for durable, persistent reminders
///
/// ## Purpose
/// Implements Orleans-inspired durable reminders. Reminders are persisted to
/// storage and survive actor deactivation and crashes.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to reminder state.
pub struct ReminderFacet<S: JournalStorage> {
    /// Facet configuration (immutable)
    config: Value,
    
    /// Facet priority (immutable)
    priority: i32,
    
    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,
    
    /// Actor reference for sending reminder fired messages
    actor_ref: Arc<RwLock<Option<ActorRef>>>,
    
    /// ActorService for sending messages (required since ActorRef is now pure data)
    actor_service: Arc<RwLock<Option<Arc<dyn ActorService>>>>,
    
    /// Journal storage backend (for persistence)
    storage: Arc<S>,
    
    /// Active reminders: reminder_name -> ReminderState
    reminders: Arc<RwLock<HashMap<String, ReminderState>>>,
    
    /// Background task handle for checking due reminders
    background_task: Arc<RwLock<Option<JoinHandle<()>>>>,
    
    /// Shutdown signal for background task
    shutdown_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
    
    /// Optional activation provider for virtual actor integration
    activation_provider: Option<Arc<dyn ActivationProvider>>,
}

/// Default priority for ReminderFacet
pub const REMINDER_FACET_DEFAULT_PRIORITY: i32 = 50;

impl<S: JournalStorage + Clone + 'static> ReminderFacet<S> {
    /// Create a new reminder facet
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend for persistence
    /// * `config` - Facet configuration (can be empty object `{}` for defaults)
    /// * `priority` - Facet priority (default: 50)
    ///
    /// ## Returns
    /// New ReminderFacet ready to attach to an actor
    pub fn new(storage: Arc<S>, config: Value, priority: i32) -> Self {
        ReminderFacet {
            config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            actor_ref: Arc::new(RwLock::new(None)),
            actor_service: Arc::new(RwLock::new(None)),
            storage,
            reminders: Arc::new(RwLock::new(HashMap::new())),
            background_task: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            activation_provider: None,
        }
    }
    
    /// Create a new reminder facet with activation provider
    ///
    /// ## Arguments
    /// * `storage` - Journal storage backend for persistence
    /// * `activation_provider` - Provider for activating virtual actors
    /// * `config` - Facet configuration
    /// * `priority` - Facet priority
    ///
    /// ## Returns
    /// New ReminderFacet ready to attach to an actor
    ///
    /// ## Design Notes
    /// When activation_provider is provided, reminders will trigger
    /// actor activation if the actor is deactivated (VirtualActorFacet integration).
    pub fn with_activation_provider(
        storage: Arc<S>,
        activation_provider: Arc<dyn ActivationProvider>,
        config: Value,
        priority: i32,
    ) -> Self {
        ReminderFacet {
            config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            actor_ref: Arc::new(RwLock::new(None)),
            actor_service: Arc::new(RwLock::new(None)),
            storage,
            reminders: Arc::new(RwLock::new(HashMap::new())),
            background_task: Arc::new(RwLock::new(None)),
            shutdown_tx: Arc::new(RwLock::new(None)),
            activation_provider: Some(activation_provider),
        }
    }
    
    /// Set actor reference (called during attachment)
    ///
    /// ## Arguments
    /// * `actor_ref` - Reference to the actor for sending messages
    pub async fn set_actor_ref(&self, actor_ref: ActorRef) {
        let mut ref_guard = self.actor_ref.write().await;
        *ref_guard = Some(actor_ref);
    }
    
    /// Set ActorService (called during attachment)
    ///
    /// ## Arguments
    /// * `actor_service` - ActorService for sending messages
    pub async fn set_actor_service(&self, actor_service: Arc<dyn ActorService>) {
        let mut service_guard = self.actor_service.write().await;
        *service_guard = Some(actor_service);
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
        let actor_id = self.actor_id.read().await
            .clone()
            .ok_or_else(|| ReminderError::NotAttached)?;
        
        // Validate registration
        if registration.reminder_name.is_empty() {
            return Err(ReminderError::InvalidRegistration("reminder_name cannot be empty".to_string()));
        }
        
        // Check if reminder already exists
        let mut reminders = self.reminders.write().await;
        if reminders.contains_key(&registration.reminder_name) {
            return Err(ReminderError::ReminderExists(registration.reminder_name.clone()));
        }
        
        // Convert proto Duration to std::time::Duration
        let interval = proto_duration_to_std(&registration.interval)
            .ok_or_else(|| ReminderError::InvalidRegistration("invalid interval".to_string()))?;
        
        // Validate interval (must be > 0)
        if interval.is_zero() {
            return Err(ReminderError::InvalidRegistration("interval must be > 0".to_string()));
        }
        
        // Calculate next fire time
        let now = SystemTime::now();
        let first_fire_time = registration.first_fire_time
            .as_ref()
            .map(|t| proto_timestamp_to_system_time(t))
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
        self.storage.register_reminder(&reminder_state).await
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
            let actor_id = self.actor_id.read().await
                .clone()
                .ok_or_else(|| ReminderError::NotAttached)?;
            
            self.storage.unregister_reminder(&actor_id, reminder_name).await
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
        let actor_ref = self.actor_ref.clone();
        let actor_service = self.actor_service.clone();
        let shutdown_tx = self.shutdown_tx.clone();
        let activation_provider_clone = self.activation_provider.clone();
        
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
                let due_reminders = storage.query_due_reminders(now).await
                    .unwrap_or_default();
                
                // Debug: Log if we found due reminders (only in debug mode)
                if !due_reminders.is_empty() {
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
                    let actor_id = ActorId::from(actor_id_str.clone());
                    
                    // Check if actor is active (VirtualActorFacet integration)
                    let should_activate = if let Some(provider) = &activation_provider_clone {
                        !provider.is_actor_active(&actor_id).await
                    } else {
                        false
                    };
                    
                    // If actor is deactivated and we have an activation provider, activate it
                    if should_activate {
                        if let Some(provider) = &activation_provider_clone {
                            if let Ok(activated_ref) = provider.activate_actor(&actor_id).await {
                                // Update actor_ref with activated reference
                                *actor_ref.write().await = Some(activated_ref);
                            }
                        }
                    }
                    
                    // Fire reminder
                    let actor_ref_opt = actor_ref.read().await.clone();
                    let actor_service_opt = actor_service.read().await.clone();
                    if let (Some(ref_guard), Some(service_guard)) = (actor_ref_opt.as_ref(), actor_service_opt.as_ref()) {
                        tracing::debug!("Firing reminder: {}", reg.reminder_name);
                        let reminder_fired = ReminderFired {
                            actor_id: reg.actor_id.clone(),
                            reminder_name: reg.reminder_name.clone(),
                            fired_at: Some(prost_types::Timestamp::from(now)),
                            callback_data: reg.callback_data.clone(),
                        };
                        
                        // Encode ReminderFired using prost
                        let payload = prost::Message::encode_to_vec(&reminder_fired);
                        
                        // Create message with reminder type
                        let message = Message::new(payload)
                            .with_metadata("type".to_string(), "ReminderFired".to_string())
                            .with_metadata("reminder_name".to_string(), reg.reminder_name.clone());
                        
                        // Use ActorService to send message (handles local/remote routing)
                        if let Err(e) = service_guard.send(ref_guard.id.as_str(), message).await {
                            tracing::warn!("Failed to send reminder message: {}", e);
                        }
                    } else {
                        tracing::warn!("Skipping reminder {}: actor_ref or actor_service not set", reg.reminder_name);
                    }
                    
                    // Update reminder state
                    let mut updated_reminder = reminder.clone();
                    updated_reminder.fire_count += 1;
                    updated_reminder.last_fired = Some(prost_types::Timestamp::from(now));
                    
                    // Check if max_occurrences reached
                    if let Some(reg) = updated_reminder.registration.as_ref() {
                        if reg.max_occurrences > 0 && updated_reminder.fire_count >= reg.max_occurrences as i32 {
                            // Auto-delete reminder
                            updated_reminder.is_active = false;
                            storage.unregister_reminder(&reg.actor_id, &reg.reminder_name).await
                                .unwrap_or_default();
                            
                            // Remove from memory
                            let mut reminders_guard = reminders.write().await;
                            reminders_guard.remove(&reg.reminder_name);
                        } else {
                            // Schedule next fire
                            let interval = proto_duration_to_std(&reg.interval).unwrap_or(Duration::from_secs(1));
                            updated_reminder.next_fire_time = Some(prost_types::Timestamp::from(now + interval));
                            
                            // Update in storage
                            storage.update_reminder(&updated_reminder).await
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
impl<S: JournalStorage + Clone + 'static> Facet for ReminderFacet<S> {
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
        let loaded_reminders = self.storage.load_reminders(actor_id).await
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
        self.start_background_task_if_needed().await
            .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;
        
        Ok(())
    }
    
    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Stop background task
        self.stop_background_task().await;
        
        // Persist reminders to storage (save all active reminders)
        let reminders_to_save = {
            let reminders_guard = self.reminders.read().await;
            reminders_guard.values()
                .filter(|r| r.is_active && r.registration.as_ref().map(|reg| reg.persist_across_activations).unwrap_or(false))
                .cloned()
                .collect::<Vec<_>>()
        };
        
        for reminder in reminders_to_save {
            let _ = self.storage.update_reminder(&reminder).await;
        }
        
        // Clear reminders
        let mut reminders = self.reminders.write().await;
        reminders.clear();
        
        // Clear actor ID and reference
        let mut id = self.actor_id.write().await;
        *id = None;
        drop(id);
        
        let mut ref_guard = self.actor_ref.write().await;
        *ref_guard = None;
        
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
    #[error("Reminder facet not attached to actor")]
    NotAttached,
    
    #[error("Reminder already exists: {0}")]
    ReminderExists(String),
    
    #[error("Reminder not found: {0}")]
    ReminderNotFound(String),
    
    #[error("Invalid registration: {0}")]
    InvalidRegistration(String),
    
    #[error("Storage error: {0}")]
    Storage(String),
}

/// Convert proto Duration to std::time::Duration
fn proto_duration_to_std(duration: &Option<prost_types::Duration>) -> Option<Duration> {
    duration.as_ref().map(|d| {
        Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64)
    })
}

/// Convert proto Timestamp to SystemTime
fn proto_timestamp_to_system_time(timestamp: &prost_types::Timestamp) -> SystemTime {
    SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp.seconds as u64) + Duration::from_nanos(timestamp.nanos as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::MemoryJournalStorage;
    use plexspaces_core::ActorRef;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};
    use prost_types;
    use std::sync::Arc;
    
    fn create_test_facet() -> (ReminderFacet<MemoryJournalStorage>, ActorRef) {
        let storage = Arc::new(MemoryJournalStorage::new());
        let facet = ReminderFacet::new(storage, serde_json::json!({}), 75);
        
        let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
        
        (facet, actor_ref)
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
            actor_id: "test-actor".to_string(),
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
        let (facet, _actor_ref) = create_test_facet();
        assert_eq!(facet.facet_type(), "reminder");
    }
    
    #[tokio::test]
    async fn test_reminder_facet_attach() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let actor_id = facet.actor_id.read().await.clone();
        assert_eq!(actor_id, Some("actor-1".to_string()));
    }
    
    #[tokio::test]
    async fn test_register_reminder_before_attach_fails() {
        let (facet, _actor_ref) = create_test_facet();
        
        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        let result = facet.register_reminder(registration).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReminderError::NotAttached));
    }
    
    #[tokio::test]
    async fn test_register_reminder_after_attach() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        let reminder_id = facet.register_reminder(registration).await.unwrap();
        
        assert_eq!(reminder_id, "reminder-1");
        
        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 1);
        assert_eq!(reminders[0].registration.as_ref().unwrap().reminder_name, "reminder-1");
    }
    
    #[tokio::test]
    async fn test_register_duplicate_reminder_fails() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref.clone()).await;
        
        let registration1 = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration1).await.unwrap();
        
        let registration2 = create_test_reminder_registration("reminder-1", 2, 0, 0);
        let result = facet.register_reminder(registration2).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReminderError::ReminderExists(_)));
    }
    
    #[tokio::test]
    async fn test_unregister_reminder() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration).await.unwrap();
        
        facet.unregister_reminder("reminder-1").await.unwrap();
        
        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 0);
    }
    
    #[tokio::test]
    async fn test_unregister_nonexistent_reminder_fails() {
        let (mut facet, _actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        
        let result = facet.unregister_reminder("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReminderError::ReminderNotFound(_)));
    }
    
    #[tokio::test]
    async fn test_reminder_with_max_occurrences() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let registration = create_test_reminder_registration("reminder-1", 1, 0, 3);
        facet.register_reminder(registration).await.unwrap();
        
        // Wait for reminders to fire (background task will fire them)
        tokio::time::sleep(Duration::from_secs(4)).await;
        
        // Reminder should be auto-deleted after 3 fires
        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 0);
    }
    
    #[tokio::test]
    async fn test_multiple_reminders() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
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
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let mut registration = create_test_reminder_registration("reminder-1", 0, 0, 0);
        registration.interval = Some(prost_types::Duration { seconds: 0, nanos: 0 });
        let result = facet.register_reminder(registration).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), ReminderError::InvalidRegistration(_)));
    }
    
    #[tokio::test]
    async fn test_reminder_detach_stops_background_task() {
        let (mut facet, actor_ref) = create_test_facet();
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let registration = create_test_reminder_registration("reminder-1", 1, 0, 0);
        facet.register_reminder(registration).await.unwrap();
        
        // Detach should stop background task
        facet.on_detach("actor-1").await.unwrap();
        
        // Reminders should be cleared
        let reminders = facet.list_reminders().await;
        assert_eq!(reminders.len(), 0);
    }
}

