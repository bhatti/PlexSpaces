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

//! Reply waiter using async condition variables for high-performance ask pattern
//!
//! ## Purpose
//! Replaces ReplyTracker with async condition variables for better performance
//! and simpler design. Uses tokio::sync::Notify for async-compatible wait/notify.
//!
//! ## Design
//! - Uses Mutex + Notify for async-compatible waiting
//! - Supports timeout via tokio::time::timeout
//! - Single-use (one reply per waiter)
//! - High performance: no channel overhead, direct notification

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{Mutex, Notify, RwLock};
use plexspaces_mailbox::Message;
use crate::service_locator::Service;

/// Reply waiter using async condition variables for high-performance ask pattern
///
/// ## Purpose
/// Replaces ReplyTracker with async condition variables for better performance
/// and simpler design. Uses tokio::sync::Notify for async-compatible wait/notify.
///
/// ## Design
/// - Uses Mutex + Notify for async-compatible waiting
/// - Supports timeout via tokio::time::timeout
/// - Single-use (one reply per waiter)
/// - High performance: no channel overhead, direct notification
#[derive(Clone)]
pub struct ReplyWaiter {
    reply: Arc<Mutex<Option<Message>>>,
    notify: Arc<Notify>,
}

impl ReplyWaiter {
    /// Create a new reply waiter
    pub fn new() -> Self {
        Self {
            reply: Arc::new(Mutex::new(None)),
            notify: Arc::new(Notify::new()),
        }
    }
    
    /// Wait for reply with timeout (async)
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for reply
    ///
    /// ## Returns
    /// - Ok(Message) if reply received within timeout
    /// - Err(ReplyWaiterError::Timeout) if timeout exceeded
    pub async fn wait(&self, timeout: Duration) -> Result<Message, ReplyWaiterError> {
        tracing::debug!(
            "🟣 [REPLY_WAITER] WAIT START: timeout={:?}",
            timeout
        );
        
        let timeout_future = tokio::time::sleep(timeout);
        let wait_future = async {
            loop {
                let mut reply = self.reply.lock().await;
                if let Some(msg) = reply.take() {
                    tracing::debug!(
                        "🟣 [REPLY_WAITER] WAIT SUCCESS: Received reply, sender={:?}, receiver={}, correlation_id={:?}",
                        msg.sender, msg.receiver, msg.correlation_id
                    );
                    return Ok(msg);
                }
                drop(reply); // Release lock before waiting
                
                // Wait for notification
                tracing::debug!("🟣 [REPLY_WAITER] WAITING: Waiting for notification...");
                self.notify.notified().await;
                tracing::debug!("🟣 [REPLY_WAITER] NOTIFIED: Woken up, checking for reply...");
            }
        };
        
        tokio::select! {
            result = wait_future => {
                match &result {
                    Ok(msg) => tracing::debug!(
                        "🟣 [REPLY_WAITER] WAIT COMPLETED: Reply received, sender={:?}, receiver={}, correlation_id={:?}",
                        msg.sender, msg.receiver, msg.correlation_id
                    ),
                    Err(e) => tracing::debug!(
                        "🟣 [REPLY_WAITER] WAIT ERROR: error={:?}",
                        e
                    ),
                }
                result
            },
            _ = timeout_future => {
                tracing::debug!("🟣 [REPLY_WAITER] WAIT TIMEOUT: timeout={:?}", timeout);
                Err(ReplyWaiterError::Timeout)
            },
        }
    }
    
    /// Notify waiter that reply has arrived (async)
    ///
    /// ## Arguments
    /// * `reply` - The reply message
    ///
    /// ## Returns
    /// - Ok(()) if notification sent successfully
    /// - Err if reply already set
    pub async fn notify(&self, reply: Message) -> Result<(), ReplyWaiterError> {
        tracing::debug!(
            "🟣 [REPLY_WAITER] NOTIFY START: reply_sender={:?}, reply_receiver={}, correlation_id={:?}",
            reply.sender, reply.receiver, reply.correlation_id
        );
        
        let mut stored_reply = self.reply.lock().await;
        
        if stored_reply.is_some() {
            tracing::warn!(
                "🟣 [REPLY_WAITER] NOTIFY FAILED: Reply already set, correlation_id={:?}",
                reply.correlation_id
            );
            return Err(ReplyWaiterError::AlreadySet);
        }
        
        *stored_reply = Some(reply.clone());
        drop(stored_reply); // Release lock before notifying
        
        tracing::debug!(
            "🟣 [REPLY_WAITER] NOTIFYING: correlation_id={:?}, reply_sender={:?}, reply_receiver={}",
            reply.correlation_id, reply.sender, reply.receiver
        );
        self.notify.notify_one();
        
        tracing::debug!(
            "🟣 [REPLY_WAITER] NOTIFY SUCCESS: correlation_id={:?}",
            reply.correlation_id
        );
        Ok(())
    }
}

impl Default for ReplyWaiter {
    fn default() -> Self {
        Self::new()
    }
}

/// Registry for managing reply waiters by correlation_id
///
/// ## Purpose
/// Stores ReplyWaiter instances keyed by correlation_id for routing
/// replies to waiting ask() callers.
#[derive(Clone)]
pub struct ReplyWaiterRegistry {
    waiters: Arc<RwLock<HashMap<String, ReplyWaiter>>>,
}

impl ReplyWaiterRegistry {
    /// Create a new ReplyWaiterRegistry
    pub fn new() -> Self {
        Self {
            waiters: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    /// Register a waiter for a correlation_id
    ///
    /// ## Arguments
    /// * `correlation_id` - Unique correlation ID for this request
    /// * `waiter` - The reply waiter to register
    pub async fn register(&self, correlation_id: String, waiter: ReplyWaiter) {
        tracing::debug!("ReplyWaiterRegistry::register: correlation_id={}", correlation_id);
        let mut waiters = self.waiters.write().await;
        waiters.insert(correlation_id, waiter);
    }
    
    /// Notify a waiter that reply has arrived
    ///
    /// ## Arguments
    /// * `correlation_id` - Correlation ID from the reply message
    /// * `reply` - The reply message
    ///
    /// ## Returns
    /// - true if waiter was found and notified
    /// - false if no waiter found for this correlation_id
    pub async fn notify(&self, correlation_id: &str, reply: Message) -> bool {
        tracing::debug!("ReplyWaiterRegistry::notify: correlation_id={}", correlation_id);
        
        let mut waiters = self.waiters.write().await;
        if let Some(waiter) = waiters.remove(correlation_id) {
            drop(waiters); // Release lock before notifying
            if waiter.notify(reply).await.is_ok() {
                tracing::debug!("ReplyWaiterRegistry::notify: Waiter notified successfully");
                return true;
            } else {
                tracing::warn!("ReplyWaiterRegistry::notify: Failed to notify waiter");
            }
        } else {
            tracing::debug!("ReplyWaiterRegistry::notify: No waiter found for correlation_id={}", correlation_id);
        }
        false
    }
    
    /// Remove a waiter (for cleanup on timeout/error)
    ///
    /// ## Arguments
    /// * `correlation_id` - Correlation ID to remove
    pub async fn remove(&self, correlation_id: &str) {
        tracing::debug!("ReplyWaiterRegistry::remove: correlation_id={}", correlation_id);
        let mut waiters = self.waiters.write().await;
        waiters.remove(correlation_id);
    }
}

impl Default for ReplyWaiterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// Implement Service trait for ServiceLocator
impl Service for ReplyWaiterRegistry {}

#[derive(Debug, Error)]
pub enum ReplyWaiterError {
    #[error("Timeout waiting for reply")]
    Timeout,
    #[error("No reply received")]
    NoReply,
    #[error("Reply already set")]
    AlreadySet,
}


