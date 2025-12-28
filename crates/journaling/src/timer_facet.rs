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

//! Timer Facet (Non-Durable Timers)
//!
//! ## Purpose
//! Provides non-durable, in-memory timers for actors. Timers are lost on actor
//! deactivation, making them suitable for transient, high-frequency operations.
//!
//! ## Architecture Context
//! Part of Phase 8.5: High Priority Missing Features. Implements Orleans-style
//! timers as an opt-in facet (not default).
//!
//! ## Design Decision
//! Timers are opt-in via facet to maintain simplicity:
//! - Regular actors: No timers (simple, predictable)
//! - Actors with TimerFacet: Can register timers (for heartbeats, polling)
//!
//! ## How It Works
//! ```text
//! 1. Actor attaches TimerFacet
//! 2. Actor registers timer via facet
//! 3. Timer fires → TimerFired message sent to actor
//! 4. Actor deactivates → All timers lost (not persisted)
//! ```

use async_trait::async_trait;
use plexspaces_core::{ActorRef, ActorService};
use plexspaces_facet::{ErrorHandling, Facet, FacetError, InterceptResult};
use plexspaces_mailbox::Message;
use plexspaces_proto::prost_types;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;
use tokio::task::JoinHandle;
use metrics;
use tracing;

// Optional dependency for distributed locking
#[cfg(feature = "locks")]
use plexspaces_locks::{AcquireLockOptions, LockManager, LockResult, ReleaseLockOptions};

// Re-export timer types from proto (TimerRegistration is re-exported in lib.rs)
use plexspaces_proto::timer::v1::TimerRegistration;
// TimerFired is in timer.proto
use plexspaces_proto::timer::v1::TimerFired;

/// Timer Facet for non-durable, in-memory timers
///
/// ## Purpose
/// Implements Orleans-inspired non-durable timers. Timers are in-memory only
/// and lost on actor deactivation.
///
/// ## Multi-Node Support
/// Optional distributed locking prevents duplicate timer execution across nodes.
/// Use `register_with_lock()` or pass `lock_key` in config to enable.
///
/// ## Thread Safety
/// Uses Arc<RwLock<>> for concurrent access to timer state.
pub struct TimerFacet {
    /// Facet configuration (immutable)
    config: Value,
    
    /// Facet priority (immutable)
    priority: i32,
    
    /// Actor ID this facet is attached to
    actor_id: Arc<RwLock<Option<String>>>,
    
    /// Actor reference for sending timer fired messages
    actor_ref: Arc<RwLock<Option<ActorRef>>>,
    
    /// ActorService for sending messages (required since ActorRef is now pure data)
    actor_service: Arc<RwLock<Option<Arc<dyn ActorService>>>>,
    
    /// Active timers: timer_name -> TimerHandle
    timers: Arc<RwLock<HashMap<String, TimerHandle>>>,
    
    /// Optional LockManager for distributed locking (multi-node protection)
    #[cfg(feature = "locks")]
    lock_manager: Option<Arc<dyn LockManager>>,
    
    /// Node ID for lock holder identification
    node_id: Arc<RwLock<Option<String>>>,
}

/// Default priority for TimerFacet
pub const TIMER_FACET_DEFAULT_PRIORITY: i32 = 50;

/// Timer handle for managing timer lifecycle
struct TimerHandle {
    /// Timer registration
    registration: TimerRegistration,
    
    /// Tokio task handle (for cancellation)
    handle: JoinHandle<()>,
}

impl TimerFacet {
    /// Create a new timer facet
    ///
    /// ## Arguments
    /// * `config` - Facet configuration (can be empty object `{}` for defaults)
    /// * `priority` - Facet priority (default: 50)
    ///
    /// ## Returns
    /// New TimerFacet ready to attach to an actor
    pub fn new(config: Value, priority: i32) -> Self {
        TimerFacet {
            config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            actor_ref: Arc::new(RwLock::new(None)),
            actor_service: Arc::new(RwLock::new(None)),
            timers: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(feature = "locks")]
            lock_manager: None,
            node_id: Arc::new(RwLock::new(None)),
        }
    }
    
    /// Create a new timer facet with LockManager (for multi-node protection)
    ///
    /// ## Arguments
    /// * `lock_manager` - LockManager for distributed locking (Redis or SQL backend)
    /// * `node_id` - Node ID for lock holder identification
    /// * `config` - Facet configuration
    /// * `priority` - Facet priority
    ///
    /// ## Returns
    /// New TimerFacet with distributed locking support
    #[cfg(feature = "locks")]
    pub fn with_lock_manager(
        lock_manager: Arc<dyn LockManager>, 
        node_id: String,
        config: Value,
        priority: i32,
    ) -> Self {
        TimerFacet {
            config,
            priority,
            actor_id: Arc::new(RwLock::new(None)),
            actor_ref: Arc::new(RwLock::new(None)),
            actor_service: Arc::new(RwLock::new(None)),
            timers: Arc::new(RwLock::new(HashMap::new())),
            lock_manager: Some(lock_manager),
            node_id: Arc::new(RwLock::new(Some(node_id))),
        }
    }
    
    /// Set node ID (called during attachment if not set)
    pub async fn set_node_id(&self, node_id: String) {
        let mut id_guard = self.node_id.write().await;
        if id_guard.is_none() {
            *id_guard = Some(node_id);
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
    
    /// Register a timer
    ///
    /// ## Arguments
    /// * `registration` - Timer registration details
    ///
    /// ## Returns
    /// Timer ID (for unregistration) or error
    pub async fn register_timer(
        &self,
        registration: TimerRegistration,
    ) -> Result<String, TimerError> {
        let actor_id = self.actor_id.read().await
            .clone()
            .ok_or_else(|| TimerError::NotAttached)?;
        
        // Validate registration
        if registration.timer_name.is_empty() {
            return Err(TimerError::InvalidRegistration("timer_name cannot be empty".to_string()));
        }
        
        // Check if timer already exists
        let mut timers = self.timers.write().await;
        if timers.contains_key(&registration.timer_name) {
            return Err(TimerError::TimerExists(registration.timer_name.clone()));
        }
        
        // Convert proto Duration to std::time::Duration
        let due_time = proto_duration_to_std(&registration.due_time)
            .ok_or_else(|| TimerError::InvalidRegistration("invalid due_time".to_string()))?;
        
        let interval = proto_duration_to_std(&registration.interval)
            .ok_or_else(|| TimerError::InvalidRegistration("invalid interval".to_string()))?;
        
        // Validate durations
        if interval.is_zero() && registration.periodic {
            return Err(TimerError::InvalidRegistration("periodic timer must have interval > 0".to_string()));
        }
        
        // Get actor reference and service for sending messages
        let actor_ref_opt = self.actor_ref.read().await.clone();
        let actor_ref = actor_ref_opt.ok_or_else(|| TimerError::NotAttached)?;
        let actor_service_opt = self.actor_service.read().await.clone();
        let actor_service = actor_service_opt.ok_or_else(|| TimerError::NotAttached)?;
        
        // Get lock manager and node ID for distributed locking (if enabled)
        #[cfg(feature = "locks")]
        let (lock_manager, node_id) = {
            let lock_mgr = self.lock_manager.clone();
            let node_id_val = self.node_id.read().await.clone();
            (lock_mgr, node_id_val)
        };
        
        // Create timer task
        let timer_name_for_task = registration.timer_name.clone();
        let timer_name_for_return = registration.timer_name.clone();
        let callback_data = registration.callback_data.clone();
        let periodic = registration.periodic;
        let actor_id_clone = actor_id.clone();
        let actor_ref_clone = actor_ref.clone();
        
        let handle = tokio::spawn(async move {
            #[cfg(feature = "locks")]
            let mut current_lock: Option<plexspaces_locks::Lock> = None;
            
            // Clone actor_id for lock operations (before it's moved into fire_timer)
            #[cfg(feature = "locks")]
            let actor_id_for_lock = actor_id_clone.clone();
            
            // Helper function to fire timer (with lock check if enabled)
            let fire_timer = async move |lock_held: bool| -> bool {
                if !lock_held {
                    return false; // Don't fire if lock not held
                }
                
                // Fire timer - send TimerFired message to actor
                let timer_fired = TimerFired {
                    actor_id: actor_id_clone.clone(),
                    timer_name: timer_name_for_task.clone(),
                    fired_at: Some(prost_types::Timestamp::from(SystemTime::now())),
                    callback_data: callback_data.clone(),
                };
                
                // Encode TimerFired using prost
                let payload = prost::Message::encode_to_vec(&timer_fired);
                
                // Create message with timer type
                let message = Message::new(payload)
                    .with_metadata("type".to_string(), "TimerFired".to_string())
                    .with_metadata("timer_name".to_string(), timer_name_for_task.clone());
                
                // Use ActorService to send message (handles local/remote routing)
                if let Err(e) = actor_service.send(actor_ref_clone.id.as_str(), message).await {
                    tracing::warn!("Failed to send timer message: {}", e);
                }
                true
            };
            
            // Note: TimerRegistration no longer has lock_key field, so locking is disabled
            // Locking for timers would require adding lock_key back to proto or using a different mechanism
            #[cfg(feature = "locks")]
            let mut lock_held = if let (Some(ref lock_mgr), Some(node_id_val)) = (lock_manager.as_ref(), node_id.as_ref()) {
                // Use actor_id as lock key since lock_key was removed from proto
                let ctx = plexspaces_core::RequestContext::internal();
                match lock_mgr.acquire_lock(&ctx, AcquireLockOptions {
                    lock_key: format!("timer:{}", actor_id_for_lock),
                    holder_id: node_id_val.clone(),
                    lease_duration_secs: if periodic { interval.as_secs() as u32 * 2 } else { 60 }, // 2x interval for periodic, 60s for one-time
                    additional_wait_time_ms: 0,
                    refresh_period_ms: 1000,
                    metadata: Default::default(),
                }).await {
                    Ok(lock) => {
                        current_lock = Some(lock);
                        true
                    }
                    Err(_) => {
                        // Lock acquisition failed (held by another node)
                        false
                    }
                }
            } else {
                true // No lock manager or node_id, fire anyway
            };
            
            #[cfg(not(feature = "locks"))]
            let mut lock_held = true; // No locking support, always fire
            
            // Wait for due_time
            if !due_time.is_zero() {
                tokio::time::sleep(due_time).await;
            }
            
            // Fire timer (if lock held)
            if !fire_timer(lock_held).await {
                return; // Lock not held, don't fire
            }
            
            // If periodic, continue firing
            if periodic {
                loop {
                    tokio::time::sleep(interval).await;
                    
                    // Note: TimerRegistration no longer has lock_key field, so locking is disabled
                    #[cfg(feature = "locks")]
                    if false { // Disabled - no lock_key in proto
                        let ctx = plexspaces_core::RequestContext::internal();
                        match lock_manager.as_ref().unwrap().renew_lock(&ctx, plexspaces_locks::RenewLockOptions {
                            lock_key: "".to_string(),
                            holder_id: node_id.as_ref().unwrap().clone(),
                            version: "".to_string(),
                            lease_duration_secs: interval.as_secs() as u32 * 2,
                            metadata: Default::default(),
                        }).await {
                            Ok(renewed_lock) => {
                                current_lock = Some(renewed_lock);
                                lock_held = true;
                            }
                            Err(_) => {
                                // Lock renewal failed (lost lock to another node)
                                lock_held = false;
                            }
                        }
                    }
                    
                    // Fire timer (if lock still held)
                    if !fire_timer(lock_held).await {
                        // Lost lock, stop firing
                        break;
                    }
                }
            }
            
            // Note: TimerRegistration no longer has lock_key field, so locking is disabled
            #[cfg(feature = "locks")]
            if false { // Disabled - no lock_key in proto
                let ctx = plexspaces_core::RequestContext::internal();
                let _ = lock_manager.as_ref().unwrap().release_lock(&ctx, ReleaseLockOptions {
                    lock_key: "".to_string(),
                    holder_id: node_id.as_ref().unwrap().clone(),
                    version: "".to_string(),
                    delete_lock: false,
                }).await;
            }
        });
        
        // Store timer
        timers.insert(
            timer_name_for_return.clone(),
            TimerHandle {
                registration,
                handle,
            },
        );
        
        Ok(timer_name_for_return)
    }
    
    /// Register a timer with distributed lock (opt-in multi-node protection)
    ///
    /// ## Purpose
    /// Registers a timer with distributed locking to prevent duplicate execution
    /// across multiple nodes. Only the node holding the lock will fire the timer.
    ///
    /// ## Usage
    /// ```rust
    /// // With LockManager configured
    /// timer_facet.register_with_lock(
    ///     "heartbeat",
    ///     Duration::from_secs(2),
    ///     true, // periodic
    ///     Some("heartbeat-lock".to_string()), // lock key
    /// ).await?;
    /// ```
    ///
    /// ## Arguments
    /// * `name` - Timer name
    /// * `interval` - Interval for periodic timers, or delay for one-time timers
    /// * `periodic` - Whether timer is periodic
    /// Note: lock_key removed from TimerRegistration proto - locking disabled
    
    /// Unregister a timer
    ///
    /// ## Arguments
    /// * `timer_name` - Name of timer to unregister
    ///
    /// ## Returns
    /// Success or error
    pub async fn unregister_timer(&self, timer_name: &str) -> Result<(), TimerError> {
        let mut timers = self.timers.write().await;
        
        if let Some(timer_handle) = timers.remove(timer_name) {
            // Cancel timer task
            timer_handle.handle.abort();
            Ok(())
        } else {
            Err(TimerError::TimerNotFound(timer_name.to_string()))
        }
    }
    
    /// List all timers for this actor
    ///
    /// ## Returns
    /// Vector of timer registrations
    pub async fn list_timers(&self) -> Vec<TimerRegistration> {
        let timers = self.timers.read().await;
        timers.values()
            .map(|handle| handle.registration.clone())
            .collect()
    }
    
    /// Register a timer with simplified API (convenience method)
    ///
    /// ## Purpose
    /// Simplifies timer registration by using actor_id from facet state
    /// and providing simpler parameters.
    ///
    /// ## Usage
    /// ```rust
    /// // After attaching facet explicitly (A3: explicit)
    /// timer_facet.register_simple("heartbeat", Duration::from_secs(2), true).await?;
    /// ```
    ///
    /// ## Arguments
    /// * `name` - Timer name
    /// * `interval` - Interval for periodic timers, or delay for one-time timers
    /// * `periodic` - Whether timer is periodic
    ///
    /// ## Returns
    /// Timer name (for unregistration) or error
    pub async fn register_simple(
        &self,
        name: &str,
        interval: std::time::Duration,
        periodic: bool,
    ) -> Result<String, TimerError> {
        let actor_id = self.actor_id.read().await
            .clone()
            .ok_or_else(|| TimerError::NotAttached)?;
        
        let registration = TimerRegistration {
            actor_id,
            timer_name: name.to_string(),
            interval: Some(prost_types::Duration {
                seconds: interval.as_secs() as i64,
                nanos: interval.subsec_nanos() as i32,
            }),
            due_time: Some(prost_types::Duration {
                seconds: 0,
                nanos: 0,
            }), // Fire immediately
            callback_data: vec![],
            periodic,
        };
        
        self.register_timer(registration).await
    }
    
    /// Register a one-time timer (convenience method)
    ///
    /// ## Usage
    /// ```rust
    /// timer_facet.register_once("cleanup", Duration::from_secs(5)).await?;
    /// ```
    pub async fn register_once(
        &self,
        name: &str,
        delay: std::time::Duration,
    ) -> Result<String, TimerError> {
        self.register_simple(name, delay, false).await
    }
    
    /// Register a periodic timer (convenience method)
    ///
    /// ## Usage
    /// ```rust
    /// timer_facet.register_periodic("heartbeat", Duration::from_secs(2)).await?;
    /// ```
    pub async fn register_periodic(
        &self,
        name: &str,
        interval: std::time::Duration,
    ) -> Result<String, TimerError> {
        self.register_simple(name, interval, true).await
    }
}

#[async_trait]
impl Facet for TimerFacet {
    fn facet_type(&self) -> &str {
        "timer"
    }
    
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    
    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        // Use stored config, ignore parameter (config is set in constructor)
        let mut id = self.actor_id.write().await;
        *id = Some(actor_id.to_string());
        
        // Try to get actor_ref from config (if provided)
        // TODO: This might need to be set separately via set_actor_ref()
        // For now, we'll require it to be set after attachment
        drop(id);
        Ok(())
    }
    
    async fn on_detach(&mut self, _actor_id: &str) -> Result<(), FacetError> {
        // Cancel all timers
        let mut timers = self.timers.write().await;
        for timer_handle in timers.values() {
            timer_handle.handle.abort();
        }
        timers.clear();
        
        // Clear actor ID and reference
        let mut id = self.actor_id.write().await;
        *id = None;
        drop(id);
        
        let mut ref_guard = self.actor_ref.write().await;
        *ref_guard = None;
        
        Ok(())
    }

    /// Phase 4.3: Handle EXIT signal from linked actor
    ///
    /// ## Purpose
    /// Cancels all timers when actor receives EXIT signal from linked actor.
    /// This prevents timers from firing after the actor is about to terminate.
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
        // Cancel all timers on EXIT
        let mut timers = self.timers.write().await;
        let timer_count = timers.len();
        for (timer_name, handle) in timers.drain() {
            handle.handle.abort();
            tracing::debug!(
                actor_id = %actor_id,
                timer_name = %timer_name,
                "Cancelled timer on EXIT signal"
            );
        }
        
        if timer_count > 0 {
            metrics::counter!("plexspaces_timer_facet_exit_cancelled_total",
                "actor_id" => actor_id.to_string(),
                "timer_count" => timer_count.to_string()
            ).increment(timer_count as u64);
            tracing::info!(
                actor_id = %actor_id,
                timer_count = timer_count,
                "Cancelled all timers on EXIT signal"
            );
        }
        
        Ok(())
    }

    /// Phase 4.3: Handle DOWN notification from monitored actor
    ///
    /// ## Purpose
    /// Logs DOWN notification for observability. TimerFacet doesn't need to
    /// take action on DOWN notifications (timers are actor-specific).
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
            "TimerFacet received DOWN notification (no action needed)"
        );
        
        metrics::counter!("plexspaces_timer_facet_down_total",
            "actor_id" => actor_id.to_string(),
            "monitored_id" => monitored_id.to_string()
        ).increment(1);
        
        Ok(())
    }
    
    fn get_state(&self) -> Result<Value, FacetError> {
        let timers = self.timers.read();
        // TODO: Serialize timer state for persistence (if needed)
        Ok(Value::Null)
    }
    
    fn get_config(&self) -> Value {
        self.config.clone()
    }
    
    fn get_priority(&self) -> i32 {
        self.priority
    }
}

/// Timer errors
#[derive(Debug, thiserror::Error)]
pub enum TimerError {
    #[error("Timer facet not attached to actor")]
    NotAttached,
    
    #[error("Timer already exists: {0}")]
    TimerExists(String),
    
    #[error("Timer not found: {0}")]
    TimerNotFound(String),
    
    #[error("Invalid registration: {0}")]
    InvalidRegistration(String),
}

/// Convert proto Duration to std::time::Duration
fn proto_duration_to_std(duration: &Option<prost_types::Duration>) -> Option<Duration> {
    duration.as_ref().map(|d| {
        Duration::from_secs(d.seconds as u64) + Duration::from_nanos(d.nanos as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_core::ActorRef;
    use plexspaces_mailbox::{Mailbox, MailboxConfig};
    use prost_types;
    use std::sync::Arc;
    use tokio::sync::mpsc;
    
    async fn create_test_facet() -> (TimerFacet, ActorRef, mpsc::Receiver<Message>) {
        let test_actor_id = "test-actor";
        let mailbox_id = format!("timer-mailbox-{}", test_actor_id);
        let mailbox = Arc::new(
            Mailbox::new(MailboxConfig::default(), mailbox_id)
                .await
                .expect("Failed to create timer mailbox")
        );
        let actor_ref = ActorRef::new("test-actor@test-node".to_string()).unwrap();
        
        // Create receiver for messages sent to mailbox
        // Note: This is a simplified test setup - in real usage, messages go to actor's mailbox
        let (tx, rx) = mpsc::channel(100);
        
        let facet = TimerFacet::new(serde_json::json!({}), 75);
        (facet, actor_ref, rx)
    }
    
    // Create a mock ActorService for tests
    struct MockActorService;
    #[async_trait::async_trait]
    impl ActorService for MockActorService {
        async fn spawn_actor(&self, _actor_id: &str, _actor_type: &str, _initial_state: Vec<u8>) -> Result<plexspaces_core::ActorRef, Box<dyn std::error::Error + Send + Sync>> {
            Err("Not implemented".into())
        }
        async fn send(&self, _actor_id: &str, _message: Message) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
            Ok("msg-id".to_string())
        }
        async fn send_reply(&self, _correlation_id: Option<&str>, _sender_id: &plexspaces_core::ActorId, _target_actor_id: plexspaces_core::ActorId, _reply_message: Message) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
            Ok(())
        }
    }
    
    fn create_test_timer_registration(
        timer_name: &str,
        interval_secs: u64,
        due_time_secs: u64,
        periodic: bool,
    ) -> TimerRegistration {
        TimerRegistration {
            actor_id: "test-actor".to_string(),
            timer_name: timer_name.to_string(),
            interval: Some(prost_types::Duration {
                seconds: interval_secs as i64,
                nanos: 0,
            }),
            due_time: Some(prost_types::Duration {
                seconds: due_time_secs as i64,
                nanos: 0,
            }),
            callback_data: vec![],
            periodic,
        }
    }
    
    #[tokio::test]
    async fn test_timer_facet_creation() {
        let (facet, _actor_ref, _rx) = create_test_facet().await;
        assert_eq!(facet.facet_type(), "timer");
    }
    
    #[tokio::test]
    async fn test_timer_facet_attach() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let actor_id = facet.actor_id.read().await.clone();
        assert_eq!(actor_id, Some("actor-1".to_string()));
    }
    
    #[tokio::test]
    async fn test_register_timer_before_attach_fails() {
        let (facet, _actor_ref, _rx) = create_test_facet().await;
        
        let registration = create_test_timer_registration("timer-1", 1, 0, false);
        let result = facet.register_timer(registration).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimerError::NotAttached));
    }
    
    #[tokio::test]
    async fn test_register_timer_after_attach() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        facet.set_actor_service(Arc::new(MockActorService)).await;
        
        let registration = create_test_timer_registration("timer-1", 1, 0, false);
        let timer_id = facet.register_timer(registration).await.unwrap();
        
        assert_eq!(timer_id, "timer-1");
        
        let timers = facet.list_timers().await;
        assert_eq!(timers.len(), 1);
        assert_eq!(timers[0].timer_name, "timer-1");
    }
    
    #[tokio::test]
    async fn test_register_duplicate_timer_fails() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref.clone()).await;
        facet.set_actor_service(Arc::new(MockActorService)).await;
        
        let registration1 = create_test_timer_registration("timer-1", 1, 0, false);
        facet.register_timer(registration1).await.unwrap();
        
        let registration2 = create_test_timer_registration("timer-1", 2, 0, false);
        let result = facet.register_timer(registration2).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimerError::TimerExists(_)));
    }
    
    #[tokio::test]
    async fn test_unregister_timer() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        facet.set_actor_service(Arc::new(MockActorService)).await;
        
        let registration = create_test_timer_registration("timer-1", 1, 0, false);
        facet.register_timer(registration).await.unwrap();
        
        facet.unregister_timer("timer-1").await.unwrap();
        
        let timers = facet.list_timers().await;
        assert_eq!(timers.len(), 0);
    }
    
    #[tokio::test]
    async fn test_unregister_nonexistent_timer_fails() {
        let (mut facet, _actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        
        let result = facet.unregister_timer("nonexistent").await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimerError::TimerNotFound(_)));
    }
    
    // Note: Timer firing tests require actual message delivery verification
    // These will be implemented after we have a working message delivery mechanism
    // For now, we test the registration/unregistration logic
    
    #[tokio::test]
    async fn test_periodic_timer_zero_interval_fails() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        
        let registration = create_test_timer_registration("timer-1", 0, 0, true);
        let result = facet.register_timer(registration).await;
        
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TimerError::InvalidRegistration(_)));
    }
    
    #[tokio::test]
    async fn test_multiple_timers() {
        let (mut facet, actor_ref, _rx) = create_test_facet().await;
        facet.on_attach("actor-1", serde_json::json!({})).await.unwrap();
        facet.set_actor_ref(actor_ref).await;
        facet.set_actor_service(Arc::new(MockActorService)).await;
        
        let registration1 = create_test_timer_registration("timer-1", 1, 0, false);
        let registration2 = create_test_timer_registration("timer-2", 2, 0, false);
        let registration3 = create_test_timer_registration("timer-3", 3, 0, false);
        
        facet.register_timer(registration1).await.unwrap();
        facet.register_timer(registration2).await.unwrap();
        facet.register_timer(registration3).await.unwrap();
        
        let timers = facet.list_timers().await;
        assert_eq!(timers.len(), 3);
    }
}

