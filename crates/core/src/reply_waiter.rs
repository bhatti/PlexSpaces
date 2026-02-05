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
//! **ReplyWaiter is used ONLY for async waiting, NOT for routing.**
//!
//! ReplyWaiter provides the async waiting mechanism for the `ask()` pattern (request-reply).
//! It allows the caller of `ask()` to wait asynchronously for a reply message with timeout support.
//!
//! ## Important: ReplyWaiter vs Routing
//!
//! **ReplyWaiter is NOT responsible for routing replies.** Routing is handled by:
//! - **Local sender**: `ActorService::send_reply()` looks up the sender's ActorRef in the registry
//!   and calls `ActorRef::tell()` on it. `tell()` then routes the reply to ReplyWaiter if a
//!   correlation_id matches.
//! - **Remote sender**: `ActorService::send_reply()` uses gRPC to send the reply to the remote node.
//!   The remote `ActorRef::tell()` then routes the reply to ReplyWaiter if a correlation_id matches.
//!
//! **ReplyWaiter's role**: Once a reply message arrives at `ActorRef::tell()` with a matching
//! correlation_id, `tell()` routes it to the ReplyWaiter, which wakes up the waiting `ask()` caller.
//!
//! ## Design
//! - Uses Mutex + Notify for async-compatible waiting
//! - Supports timeout via tokio::time::timeout
//! - Single-use (one reply per waiter)
//! - High performance: no channel overhead, direct notification
//!
//! ## Reply Routing Flow
//!
//! ### Local Sender (Same Node)
//! ```
//! 1. Actor calls ctx.send_reply()
//!    └─> ActorService::send_reply() looks up sender's ActorRef in registry
//!    └─> Calls sender_ref.tell(reply_message) with correlation_id
//!    └─> ActorRef::tell() checks for ReplyWaiter with correlation_id
//!        └─> Routes reply to ReplyWaiter (bypasses mailbox)
//!            └─> ReplyWaiter.notify() wakes up waiting ask() caller
//! ```
//!
//! ### Remote Sender (Different Node)
//! ```
//! 1. Actor calls ctx.send_reply()
//!    └─> ActorService::send_reply() detects remote sender
//!    └─> Uses gRPC to send reply to remote node
//!    └─> Reply arrives at remote ActorRef::tell() with correlation_id
//!        └─> ActorRef::tell() checks for ReplyWaiter with correlation_id
//!            └─> Routes reply to ReplyWaiter (bypasses mailbox)
//!                └─> ReplyWaiter.notify() wakes up waiting ask() caller
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;
use tokio::sync::{Mutex, Notify, RwLock};
use plexspaces_proto::common::v1::Message;
use crate::Service;

/// Reply waiter using async condition variables for high-performance ask pattern
///
/// ## Purpose
/// **ReplyWaiter is used ONLY for async waiting, NOT for routing.**
///
/// ReplyWaiter provides the async waiting mechanism for the `ask()` pattern. It allows the
/// caller of `ask()` to wait asynchronously for a reply message with timeout support.
///
/// ## Important: ReplyWaiter vs Routing
///
/// **ReplyWaiter is NOT responsible for routing replies.** Routing is handled by:
/// - **Local sender**: `ActorService::send_reply()` → `ActorRef::tell()` → ReplyWaiter
/// - **Remote sender**: `ActorService::send_reply()` → gRPC → remote `ActorRef::tell()` → ReplyWaiter
///
/// **ReplyWaiter's role**: Once a reply message arrives at `ActorRef::tell()` with a matching
/// correlation_id, `tell()` routes it to the ReplyWaiter, which wakes up the waiting `ask()` caller.
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
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER] WAIT START: timeout={:?}",
                timeout
            );
        }
        
        let timeout_future = tokio::time::sleep(timeout);
        let wait_future = async {
            loop {
                let mut reply = self.reply.lock().await;
                if let Some(msg) = reply.take() {
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            "[REPLY_WAITER] WAIT SUCCESS: Received reply, sender={}, receiver={}, correlation_id={}",
                            msg.sender_id, msg.receiver_id, msg.correlation_id
                        );
                    }
                    return Ok(msg);
                }
                drop(reply); // Release lock before waiting
                
                // Wait for notification
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!("[REPLY_WAITER] WAITING: Waiting for notification...");
                }
                self.notify.notified().await;
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!("[REPLY_WAITER] NOTIFIED: Woken up, checking for reply...");
                }
            }
        };
        
        tokio::select! {
            result = wait_future => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    match &result {
                        Ok(msg) => {
                            tracing::trace!(
                                "[REPLY_WAITER] WAIT COMPLETED: Reply received, sender={}, receiver={}, correlation_id={}",
                                msg.sender_id, msg.receiver_id, msg.correlation_id
                            );
                        }
                        Err(e) => {
                            tracing::trace!(
                                "[REPLY_WAITER] WAIT ERROR: error={:?}",
                                e
                            );
                        }
                    }
                }
                result
            },
            _ = timeout_future => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!("[REPLY_WAITER] WAIT TIMEOUT: timeout={:?}", timeout);
                }
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
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER] NOTIFY START: reply_sender={}, reply_receiver={}, correlation_id={}",
                reply.sender_id, reply.receiver_id, reply.correlation_id
            );
        }
        
        let mut stored_reply = self.reply.lock().await;
        
        if stored_reply.is_some() {
            return Err(ReplyWaiterError::AlreadySet);
        }
        
        *stored_reply = Some(reply.clone());
        drop(stored_reply); // Release lock before notifying
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER] NOTIFYING: correlation_id={}, reply_sender={}, reply_receiver={}",
                reply.correlation_id, reply.sender_id, reply.receiver_id
            );
        }
        self.notify.notify_one();
        
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER] NOTIFY SUCCESS: correlation_id={:?}",
                reply.correlation_id
            );
        }
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
/// Stores ReplyWaiter instances keyed by correlation_id. When a reply arrives with a matching
/// correlation_id, `ActorRef::tell()` looks up the ReplyWaiter here and notifies it to wake up
/// the waiting `ask()` caller. **Note**: This registry is for async waiting, not for routing.
/// Routing is handled by `ActorService::send_reply()`.
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
        let mut waiters = self.waiters.write().await;
        waiters.insert(correlation_id.clone(), waiter);
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER_REGISTRY] Registered waiter: correlation_id={}, total_waiters={}",
                correlation_id,
                waiters.len()
            );
        }
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
        let waiters_count = self.waiters.read().await.len();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                "[REPLY_WAITER_REGISTRY] notify called: correlation_id={}, waiters_count={}",
                correlation_id,
                waiters_count
            );
        }
        
        let mut waiters = self.waiters.write().await;
        if let Some(waiter) = waiters.remove(correlation_id) {
            drop(waiters); // Release lock before notifying
            if tracing::enabled!(tracing::Level::TRACE) {
                tracing::trace!("[REPLY_WAITER_REGISTRY] Found waiter for correlation_id={}, notifying...", correlation_id);
            }
            match waiter.notify(reply).await {
                Ok(()) => {
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!("[REPLY_WAITER_REGISTRY] Waiter notified successfully: correlation_id={}", correlation_id);
                    }
                    return true;
                }
                Err(e) => {
                    tracing::warn!(
                        "[REPLY_WAITER_REGISTRY] Failed to notify waiter: correlation_id={}, reason={}",
                        correlation_id, e
                    );
                }
            }
        } else {
            tracing::warn!(
                "[REPLY_WAITER_REGISTRY] No waiter found for correlation_id={}, available_ids={:?}",
                correlation_id,
                waiters.keys().collect::<Vec<_>>()
            );
        }
        false
    }
    
    /// Remove a waiter (for cleanup on timeout/error)
    ///
    /// ## Arguments
    /// * `correlation_id` - Correlation ID to remove
    pub async fn remove(&self, correlation_id: &str) {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!("ReplyWaiterRegistry::remove: correlation_id={}", correlation_id);
        }
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
impl Service for ReplyWaiterRegistry {
    fn service_name(&self) -> String {
        crate::service_names::REPLY_WAITER_REGISTRY.to_string()
    }
}

/// Errors that can occur when waiting for a reply
#[derive(Debug, Error)]
pub enum ReplyWaiterError {
    /// Timed out waiting for a reply
    #[error("Timeout waiting for reply")]
    Timeout,
    /// No reply was received (sender dropped)
    #[error("No reply received")]
    NoReply,
    /// Reply was already set (duplicate reply)
    #[error("Reply already set")]
    AlreadySet,
}


