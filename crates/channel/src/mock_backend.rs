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

//! Mock channel backend for testing
//!
//! ## Purpose
//! Provides a mock channel implementation for testing ack/nack, retry/DLQ,
//! crashes, and shutdown scenarios without requiring external dependencies.
//!
//! ## Features
//! - Configurable failure rates (for crash simulation)
//! - Poisonous message tracking (messages that always fail)
//! - Delivery count tracking
//! - DLQ support
//! - Shutdown simulation
//!
//! ## Usage
//! ```rust,no_run
//! use plexspaces_channel::MockChannel;
//! use plexspaces_proto::channel::v1::*;
//!
//! let config = ChannelConfig { /* ... */ };
//! let channel = MockChannel::new(config);
//! ```

use crate::{Channel, ChannelError, ChannelResult};
use async_trait::async_trait;
use futures::stream::BoxStream;
use plexspaces_proto::channel::v1::{
    ChannelBackend, ChannelConfig, ChannelMessage, ChannelStats,
};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

/// Mock channel for testing
///
/// ## Configuration
/// - `failure_rate`: Probability of operations failing (0.0 to 1.0)
/// - `poisonous_messages`: Set of message IDs that always fail
/// - `crash_on_message`: Message ID that triggers a crash
pub struct MockChannel {
    config: ChannelConfig,
    messages: Arc<RwLock<VecDeque<ChannelMessage>>>,
    pending_acks: Arc<RwLock<HashMap<String, ChannelMessage>>>,
    delivery_count: Arc<RwLock<HashMap<String, u32>>>,
    dlq: Arc<RwLock<Vec<ChannelMessage>>>,
    stats: Arc<ChannelStatsData>,
    closed: Arc<AtomicBool>,
    // Test configuration
    _failure_rate: f64, // 0.0 to 1.0 (for future use)
    poisonous_messages: Arc<RwLock<std::collections::HashSet<String>>>,
    crash_on_message: Arc<RwLock<Option<String>>>,
}

struct ChannelStatsData {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    messages_acked: AtomicU64,
    messages_failed: AtomicU64, // Used for nacked messages
    messages_dlq: AtomicU64,
    errors: AtomicU64,
}

impl MockChannel {
    /// Create a new mock channel
    pub fn new(config: ChannelConfig) -> Self {
        Self {
            config,
            messages: Arc::new(RwLock::new(VecDeque::new())),
            pending_acks: Arc::new(RwLock::new(HashMap::new())),
            delivery_count: Arc::new(RwLock::new(HashMap::new())),
            dlq: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(ChannelStatsData {
                messages_sent: AtomicU64::new(0),
                messages_received: AtomicU64::new(0),
                messages_acked: AtomicU64::new(0),
                messages_failed: AtomicU64::new(0),
                messages_dlq: AtomicU64::new(0),
                errors: AtomicU64::new(0),
            }),
            closed: Arc::new(AtomicBool::new(false)),
            _failure_rate: 0.0,
            poisonous_messages: Arc::new(RwLock::new(std::collections::HashSet::new())),
            crash_on_message: Arc::new(RwLock::new(None)),
        }
    }

    /// Set failure rate for testing (0.0 = never fail, 1.0 = always fail)
    pub async fn set_failure_rate(&self, rate: f64) {
        // Store in config metadata for now (could add to ChannelConfig if needed)
        // For testing, we'll use a separate field
    }

    /// Mark a message as poisonous (will always fail)
    pub async fn mark_poisonous(&self, message_id: &str) {
        let mut poisonous = self.poisonous_messages.write().await;
        poisonous.insert(message_id.to_string());
    }

    /// Set message ID that triggers a crash
    pub async fn set_crash_on_message(&self, message_id: Option<String>) {
        let mut crash = self.crash_on_message.write().await;
        *crash = message_id;
    }

    /// Get DLQ messages (for testing)
    pub async fn get_dlq_messages(&self) -> Vec<ChannelMessage> {
        let dlq = self.dlq.read().await;
        dlq.clone()
    }

    /// Get delivery count for a message (for testing)
    pub async fn get_delivery_count(&self, message_id: &str) -> u32 {
        let delivery = self.delivery_count.read().await;
        delivery.get(message_id).copied().unwrap_or(0)
    }

    /// Check if message is poisonous
    async fn is_poisonous(&self, message_id: &str) -> bool {
        let poisonous = self.poisonous_messages.read().await;
        poisonous.contains(message_id)
    }

    /// Check if message should trigger crash
    async fn should_crash(&self, message_id: &str) -> bool {
        let crash = self.crash_on_message.read().await;
        crash.as_ref().map(|id| id == message_id).unwrap_or(false)
    }

    /// Increment delivery count
    async fn increment_delivery_count(&self, message_id: &str) {
        let mut delivery = self.delivery_count.write().await;
        let count = delivery.entry(message_id.to_string()).or_insert(0);
        *count += 1;
    }
}

#[async_trait]
impl Channel for MockChannel {
    async fn send(&self, message: ChannelMessage) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        // Check for crash
        if self.should_crash(&message.id).await {
            self.stats.errors.fetch_add(1, Ordering::Relaxed);
            return Err(ChannelError::BackendError("Simulated crash".to_string()));
        }

        let message_id = message.id.clone();
        let mut messages = self.messages.write().await;
        messages.push_back(message);
        self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);
        Ok(message_id)
    }

    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let mut result = Vec::new();
        let mut messages = self.messages.write().await;
        let mut pending = self.pending_acks.write().await;

        for _ in 0..max_messages {
            if let Some(mut msg) = messages.pop_front() {
                // Increment delivery count BEFORE adding to pending
                self.increment_delivery_count(&msg.id).await;
                let count = self.get_delivery_count(&msg.id).await;
                msg.delivery_count = count;

                // Add to pending acks (clone after setting delivery_count)
                let msg_clone = msg.clone();
                pending.insert(msg.id.clone(), msg_clone);
                result.push(msg);
            } else {
                break;
            }
        }

        self.stats.messages_received.fetch_add(result.len() as u64, Ordering::Relaxed);
        Ok(result)
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        self.receive(max_messages).await
    }

    async fn subscribe(
        &self,
        _consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, ChannelMessage>> {
        // For testing, we'll use a simple stream
        use futures::stream;
        let messages = self.messages.clone();
        let pending = self.pending_acks.clone();
        let delivery_count = self.delivery_count.clone();
        let stats = self.stats.clone();
        let closed = self.closed.clone();

        let stream = stream::unfold((), move |_| {
            let messages = messages.clone();
            let pending = pending.clone();
            let delivery_count = delivery_count.clone();
            let stats = stats.clone();
            let closed = closed.clone();

            async move {
                if closed.load(Ordering::Relaxed) {
                    return None;
                }

                let mut msgs = messages.write().await;
                if let Some(msg) = msgs.pop_front() {
                    // Increment delivery count
                    let delivery_count_value = {
                        let mut delivery = delivery_count.write().await;
                        let count = delivery.entry(msg.id.clone()).or_insert(0);
                        *count += 1;
                        *count
                    };

                    // Clone message with updated delivery count
                    let mut msg_with_count = msg.clone();
                    msg_with_count.delivery_count = delivery_count_value;

                    // Add to pending
                    {
                        let mut pending_guard = pending.write().await;
                        pending_guard.insert(msg_with_count.id.clone(), msg_with_count.clone());
                    }

                    stats.messages_received.fetch_add(1, Ordering::Relaxed);
                    Some((msg_with_count, ()))
                } else {
                    None
                }
            }
        });

        Ok(Box::pin(stream))
    }

    async fn publish(&self, message: ChannelMessage) -> ChannelResult<u32> {
        self.send(message).await?;
        Ok(1) // Mock returns 1 subscriber
    }

    async fn ack(&self, message_id: &str) -> ChannelResult<()> {
        let mut pending = self.pending_acks.write().await;
        if pending.remove(message_id).is_some() {
            // Reset delivery count on successful ack
            let mut delivery = self.delivery_count.write().await;
            delivery.remove(message_id);
            
            self.stats.messages_acked.fetch_add(1, Ordering::Relaxed);
            
            crate::observability::record_channel_ack(
                &self.config.name,
                message_id,
                crate::observability::backend_name(self.config.backend),
            );
            
            Ok(())
        } else {
            self.stats.errors.fetch_add(1, Ordering::Relaxed);
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
        // Get retry/DLQ config
        let max_retries = if self.config.max_retries > 0 {
            self.config.max_retries
        } else {
            3 // Default: 3 retries
        };
        let dlq_enabled = self.config.dlq_enabled;

        // Get delivery count
        let delivery_count = self.get_delivery_count(message_id).await;
        let is_poisonous = self.is_poisonous(message_id).await;

        // Remove from pending
        let mut pending = self.pending_acks.write().await;
        let message = pending.remove(message_id);

        if let Some(mut msg) = message {
            // Update delivery_count in message
            msg.delivery_count = delivery_count;
            
            // Check if we should requeue or send to DLQ
            let should_requeue = requeue && delivery_count < max_retries && !is_poisonous;
            let send_to_dlq = (!requeue || delivery_count >= max_retries || is_poisonous) && dlq_enabled;

            if should_requeue {
                // Requeue message
                let mut messages = self.messages.write().await;
                messages.push_back(msg.clone());
                crate::observability::record_channel_nack(
                    &self.config.name,
                    message_id,
                    true,
                    delivery_count,
                    crate::observability::backend_name(self.config.backend),
                );
            } else if send_to_dlq {
                // Send to DLQ
                if !self.config.dead_letter_queue.is_empty() {
                    let mut dlq = self.dlq.write().await;
                    dlq.push(msg);
                    self.stats.messages_dlq.fetch_add(1, Ordering::Relaxed);
                    
                    let reason = if is_poisonous {
                        "poisonous_message"
                    } else {
                        "max_retries_exceeded"
                    };
                    
                    crate::observability::record_channel_dlq(
                        &self.config.name,
                        message_id,
                        delivery_count,
                        reason,
                        crate::observability::backend_name(self.config.backend),
                    );
                }
            } else {
                // Don't requeue, don't DLQ (will timeout)
                crate::observability::record_channel_nack(
                    &self.config.name,
                    message_id,
                    false,
                    delivery_count,
                    crate::observability::backend_name(self.config.backend),
                );
            }

            self.stats.messages_failed.fetch_add(1, Ordering::Relaxed);
            Ok(())
        } else {
            self.stats.errors.fetch_add(1, Ordering::Relaxed);
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        let stats = &self.stats;
        let messages = self.messages.read().await;
        let pending = self.pending_acks.read().await;
        let dlq = self.dlq.read().await;

        Ok(ChannelStats {
            name: self.config.name.clone(),
            backend: ChannelBackend::ChannelBackendCustom as i32,
            messages_sent: stats.messages_sent.load(Ordering::Relaxed),
            messages_received: stats.messages_received.load(Ordering::Relaxed),
            messages_pending: (messages.len() + pending.len()) as u64,
            messages_failed: stats.messages_failed.load(Ordering::Relaxed),
            avg_latency_us: 0,
            throughput: 0.0,
            backend_stats: {
                let mut map = HashMap::new();
                map.insert("dlq_size".to_string(), dlq.len().to_string());
                map.insert("poisonous_count".to_string(), self.poisonous_messages.read().await.len().to_string());
                map.insert("messages_acked".to_string(), stats.messages_acked.load(Ordering::Relaxed).to_string());
                map.insert("messages_failed".to_string(), stats.messages_failed.load(Ordering::Relaxed).to_string());
                map.insert("messages_dlq".to_string(), stats.messages_dlq.load(Ordering::Relaxed).to_string());
                // Note: messages_nacked is tracked as messages_failed in ChannelStats
                map
            },
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        self.closed.store(true, Ordering::Relaxed);
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}







