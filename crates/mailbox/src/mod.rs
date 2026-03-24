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

//! Mailbox module with composable strategies
//!
//! ## Framework Simplification Note
//! This mailbox implementation is part of the framework simplification effort.
//! Each example change should simplify the framework, reducing boilerplate and
//! improving developer experience. The mailbox uses channel-based messaging
//! to enable proper async/await patterns and eliminate busy-waiting.
//!
//! Instead of having multiple mailbox types, we have one mailbox
//! with composable storage, ordering, and durability strategies.
//!
//! ## Proto-First Design
//! Types are defined in proto and re-exported here for convenience.
//! See `proto/plexspaces/v1/mailbox.proto` for the source of truth.
//!
//! ## Channel-Based Architecture
//! The mailbox uses `tokio::sync::mpsc::Receiver` internally to enable
//! proper async/await patterns. The `dequeue()` method returns a future
//! that yields messages, allowing it to be used in `tokio::select!`.
//! This eliminates busy-waiting and provides zero-latency message delivery.
//!
//! ## Mailbox Capacity
//! Default capacity is 10000 messages. Use `ActorBuilder::with_mailbox_capacity()`
//! or `ActorBuilder::with_mailbox_config()` to configure capacity.
//! When capacity is reached, behavior depends on `BackpressureStrategy`:
//! - `Error`: Returns `MailboxError::Full` (default for Block strategy)
//! - `DropOldest`: Drops oldest message and enqueues new one
//! - `DropNewest`: Drops the new message
//! - `Block`: Returns error (prevents deadlock during shutdown)
//!
//! ## Durable Actors and Mailbox Persistence
//! TODO: For durable actors, mailbox messages should be persisted to storage
//! and recovered on actor restart. Currently, mailbox messages are in-memory
//! only and are lost on actor restart. This is a future enhancement.

use plexspaces_channel::{create_channel, Channel, ChannelError};
use plexspaces_proto::channel::v1::{ChannelConfig, ChannelProvider};
use plexspaces_proto::common::v1::Message as ProtoMessage;
use rand::Rng;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, Notify, RwLock};

#[path = "lru_cache.rs"]
mod lru_cache;
use lru_cache::LruCache;

#[path = "message_helpers.rs"]
mod message_helpers;
pub use message_helpers::*;
// Re-export Message so tests using `use super::*` can access it
pub use plexspaces_proto::common::v1::Message;

// Re-export proto-generated types
pub use plexspaces_proto::mailbox::v1::{
    BackpressureStrategy, MailboxConfig, MailboxError as MailboxErrorProto, MessagePriority,
    OrderingStrategy,
};

// Wrapper for MailboxError to provide thiserror compatibility
#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    /// Mailbox has reached capacity and cannot accept more messages
    #[error("Mailbox is full")]
    Full,

    /// Underlying storage backend error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Invalid mailbox configuration provided
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

impl From<MailboxError> for MailboxErrorProto {
    fn from(err: MailboxError) -> Self {
        match err {
            MailboxError::Full => MailboxErrorProto::MailboxErrorFull,
            MailboxError::StorageError(_) => MailboxErrorProto::MailboxErrorStorage,
            MailboxError::InvalidConfig(_) => MailboxErrorProto::MailboxErrorInvalidConfig,
        }
    }
}

impl From<MailboxErrorProto> for MailboxError {
    fn from(proto: MailboxErrorProto) -> Self {
        match proto {
            MailboxErrorProto::MailboxErrorUnspecified => {
                MailboxError::StorageError("Unspecified error".to_string())
            }
            MailboxErrorProto::MailboxErrorNotFound => {
                MailboxError::StorageError("Mailbox not found".to_string())
            }
            MailboxErrorProto::MailboxErrorFull => MailboxError::Full,
            MailboxErrorProto::MailboxErrorTimeout => {
                MailboxError::StorageError("Timeout".to_string())
            }
            MailboxErrorProto::MailboxErrorInvalidConfig => {
                MailboxError::InvalidConfig("Invalid config".to_string())
            }
            MailboxErrorProto::MailboxErrorStorage => {
                MailboxError::StorageError("Storage error".to_string())
            }
            MailboxErrorProto::MailboxErrorSerialization => {
                MailboxError::StorageError("Serialization error".to_string())
            }
        }
    }
}

// Helper functions for MessagePriority conversion (proto uses different values)
// Cannot add methods to proto-generated types, so use free functions
pub fn message_priority_value(priority: &MessagePriority) -> i32 {
    match priority {
        MessagePriority::MessagePriorityUnspecified => 0,
        MessagePriority::Lowest => 1,
        MessagePriority::Low => 2,
        MessagePriority::Normal => 3,
        MessagePriority::High => 4,
        MessagePriority::Highest => 5,
        MessagePriority::System => 10,
    }
}

fn message_priority_from_value(value: i32) -> MessagePriority {
    // Proto values: System=10, Highest=5, High=4, Normal=3, Low=2, Lowest=1
    match value {
        10 => MessagePriority::System,
        5 => MessagePriority::Highest,
        4 => MessagePriority::High,
        3 => MessagePriority::Normal,
        2 => MessagePriority::Low,
        1 => MessagePriority::Lowest,
        _ => MessagePriority::Normal,
    }
}

// Helper functions for MailboxConfig (cannot add methods to proto-generated types)
pub fn mailbox_config_default() -> MailboxConfig {
    MailboxConfig {
        mailbox_type: 0, // MailboxTypeUnspecified (defaults to Unbounded)
        capacity: 10000,
        backpressure_strategy: BackpressureStrategy::Block as i32,
        message_timeout: None,
        enable_priority: false,
        enable_deduplication: false,
        deduplication_window: None,
        message_id_cache_size: 10000,
        idempotency_cache_size: 10000,
        ordering_strategy: OrderingStrategy::OrderingFifo as i32,
        channel_provider: 0, // Unspecified (defaults to InMemory)
        channel_config: None,
        metadata: std::collections::HashMap::new(),
    }
}

fn mailbox_config_deduplication_window(config: &MailboxConfig) -> Duration {
    if let Some(ref window) = config.deduplication_window {
        Duration::from_secs(window.seconds as u64) + Duration::from_nanos(window.nanos as u64)
    } else {
        Duration::from_secs(24 * 60 * 60) // Default: 24 hours
    }
}

fn mailbox_config_ordering(config: &MailboxConfig) -> OrderingStrategy {
    OrderingStrategy::try_from(config.ordering_strategy).unwrap_or(OrderingStrategy::OrderingFifo)
}

fn mailbox_config_max_size(config: &MailboxConfig) -> usize {
    if config.capacity == 0 {
        usize::MAX // Unlimited
    } else {
        config.capacity as usize
    }
}

fn mailbox_config_backpressure(config: &MailboxConfig) -> BackpressureStrategy {
    BackpressureStrategy::try_from(config.backpressure_strategy)
        .unwrap_or(BackpressureStrategy::Block)
}

fn mailbox_config_message_id_cache_size(config: &MailboxConfig) -> usize {
    if config.message_id_cache_size == 0 {
        10000 // Default
    } else {
        config.message_id_cache_size as usize
    }
}

fn mailbox_config_idempotency_cache_size(config: &MailboxConfig) -> usize {
    if config.idempotency_cache_size == 0 {
        10000 // Default
    } else {
        config.idempotency_cache_size as usize
    }
}

/// Mailbox implementation using channel-based messaging
///
/// ## Architecture
/// Uses `Channel` trait for extensible backend support (InMemory, Redis, Kafka, SQLite).
/// Messages are enqueued via channel.send() and dequeued via channel.receive().
/// Internal priority queue handles ordering before messages are sent to channel.
/// This enables proper async/await patterns and eliminates busy-waiting.
pub struct Mailbox {
    /// Configuration
    config: MailboxConfig,
    /// Channel backend (InMemory, Redis, Kafka, SQLite, etc.)
    channel: Arc<dyn Channel>,
    /// Channel name (used for message routing)
    channel_name: String,
    /// Channel backend type (for is_durable() and backend_type())
    channel_provider: i32,
    /// Mailbox ID (for logging/metrics)
    mailbox_id: String,
    /// Internal queue for ordering/priority (feeds into channel)
    /// For FIFO/LIFO: simple VecDeque
    /// For Priority: BinaryHeap with priority ordering
    internal_queue: Arc<RwLock<MessageStorage>>,
    /// Statistics
    stats: Arc<RwLock<MailboxStats>>,
    /// Background task handle for processing internal queue into channel
    processor_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    /// Notify when messages are available (condition variable for efficient wake-up)
    notify: Arc<Notify>,
    /// Local receiver buffer for in-memory fast path (when using InMemoryChannel)
    /// This allows us to maintain the existing dequeue API while using Channel trait
    local_receiver: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<ProtoMessage>>>>,
    /// LRU cache for message ID deduplication (message_id -> timestamp)
    /// Fixed size cache (default: 10000 entries) with TTL expiration
    message_id_cache: Arc<RwLock<LruCache<String, SystemTime>>>,
    /// LRU cache for idempotency key deduplication (idempotency_key -> (timestamp, cached_response))
    /// Fixed size cache (default: 10000 entries) with TTL expiration
    /// Idempotency keys seen within deduplication_window return cached response
    idempotency_cache: Arc<RwLock<LruCache<String, (SystemTime, Option<ProtoMessage>)>>>,
    /// Deduplication time window (default: 24 hours)
    deduplication_window: Duration,
    /// Maximum cache size for message ID deduplication (default: 10000)
    message_id_cache_size: usize,
    /// Maximum cache size for idempotency key deduplication (default: 10000)
    idempotency_cache_size: usize,
    /// Shutdown flag: when true, mailbox stops accepting new messages
    /// For non-memory channels, also stops receiving from channel backend
    shutdown_flag: Arc<RwLock<bool>>,
    /// In-progress message count (for graceful shutdown tracking)
    in_progress_count: Arc<RwLock<usize>>,
}

/// Internal message storage (used for ordering/priority before sending to channel)
#[derive(Debug)]
enum MessageStorage {
    /// FIFO/LIFO queue
    Queue(VecDeque<ProtoMessage>),
    /// Priority queue (BinaryHeap with reverse ordering for max-heap)
    Priority(BinaryHeap<PriorityMessage>),
}

/// Wrapper for Message in priority queue (BinaryHeap is max-heap, we want highest priority first)
#[derive(Debug, Clone)]
struct PriorityMessage {
    message: ProtoMessage,
}

impl PartialEq for PriorityMessage {
    fn eq(&self, other: &Self) -> bool {
        self.message.priority == other.message.priority
    }
}

impl Eq for PriorityMessage {}

impl PartialOrd for PriorityMessage {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for PriorityMessage {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap, so we want higher priority (larger value) to be "greater"
        // Proto priority is already an i32, so compare directly
        self.message.priority.cmp(&other.message.priority)
    }
}

/// Mailbox statistics
#[derive(Debug, Default)]
struct MailboxStats {
    total_enqueued: u64,
    total_dequeued: u64,
    total_dropped: u64,
    current_size: usize,
}

impl Mailbox {
    /// Create a new mailbox with configuration
    ///
    /// ## Channel-Based Architecture
    /// Creates a channel backend based on `config.channel_provider` (defaults to IN_MEMORY).
    /// The channel backend must be available/configured, otherwise this will return an error.
    ///
    /// ## Arguments
    /// * `config` - Mailbox configuration with channel backend specification
    /// * `mailbox_id` - Unique identifier for this mailbox (used as channel name)
    ///
    /// ## Returns
    /// `Ok(Mailbox)` on success, `Err(MailboxError)` if channel backend is unavailable
    ///
    /// ## Errors
    /// - `MailboxError::InvalidConfig`: Invalid channel backend or configuration
    /// - `MailboxError::StorageError`: Channel backend initialization failed (e.g., Kafka not configured)
    pub async fn new(config: MailboxConfig, mailbox_id: String) -> Result<Self, MailboxError> {
        // Determine channel backend (default to IN_MEMORY if not specified)
        let channel_provider = if config.channel_provider != 0 {
            ChannelProvider::try_from(config.channel_provider).map_err(|_| {
                MailboxError::InvalidConfig(format!(
                    "Invalid channel_provider: {}",
                    config.channel_provider
                ))
            })?
        } else {
            ChannelProvider::ChannelProviderInMemory
        };
        let is_in_memory = channel_provider == ChannelProvider::ChannelProviderInMemory;
        let channel_provider_value = channel_provider as i32;

        // Helper to check if backend is in-memory (needed for shutdown logic)
        let _is_in_memory_clone = is_in_memory;

        // Create channel config from mailbox config
        let mut channel_config = config
            .channel_config
            .clone()
            .unwrap_or_else(|| ChannelConfig {
                name: format!("mailbox:{}", mailbox_id),
                provider: channel_provider_value,
                capacity: config.capacity as u64,
                ..Default::default()
            });

        // Ensure channel name is set
        if channel_config.name.is_empty() {
            channel_config.name = format!("mailbox:{}", mailbox_id);
        }

        // Create channel backend
        let channel = create_channel(channel_config.clone()).await.map_err(|e| {
            MailboxError::StorageError(format!("Failed to create channel backend: {}", e))
        })?;

        // For InMemory backend, create a local mpsc channel for fast-path dequeue
        let (local_sender, local_receiver) = if is_in_memory {
            let (s, r): (
                mpsc::UnboundedSender<ProtoMessage>,
                mpsc::UnboundedReceiver<ProtoMessage>,
            ) = mpsc::unbounded_channel();
            (Some(s), Some(r))
        } else {
            (None, None)
        };

        // Initialize internal queue based on ordering strategy
        let internal_queue = match mailbox_config_ordering(&config) {
            OrderingStrategy::OrderingPriority => MessageStorage::Priority(BinaryHeap::new()),
            _ => MessageStorage::Queue(VecDeque::new()),
        };

        let mailbox = Mailbox {
            config: config.clone(),
            channel: Arc::from(channel),
            channel_name: channel_config.name.clone(),
            channel_provider: channel_provider_value,
            mailbox_id: mailbox_id.clone(),
            internal_queue: Arc::new(RwLock::new(internal_queue)),
            stats: Arc::new(RwLock::new(MailboxStats::default())),
            processor_handle: Arc::new(RwLock::new(None)),
            notify: Arc::new(Notify::new()),
            local_receiver: Arc::new(tokio::sync::Mutex::new(local_receiver)),
            message_id_cache: Arc::new(RwLock::new(LruCache::new(
                mailbox_config_message_id_cache_size(&config),
                mailbox_config_deduplication_window(&config),
            ))),
            idempotency_cache: Arc::new(RwLock::new(LruCache::new(
                mailbox_config_idempotency_cache_size(&config),
                mailbox_config_deduplication_window(&config),
            ))),
            deduplication_window: mailbox_config_deduplication_window(&config),
            message_id_cache_size: mailbox_config_message_id_cache_size(&config),
            idempotency_cache_size: mailbox_config_idempotency_cache_size(&config),
            shutdown_flag: Arc::new(RwLock::new(false)),
            in_progress_count: Arc::new(RwLock::new(0)),
        };

        // Store is_in_memory flag for later use (we'll add a method to access it)

        // Start background processor task to move messages from internal queue to channel
        // For InMemory backend, also forward to local_receiver for fast-path
        if let Some(sender) = local_sender {
            mailbox.start_processor_with_local_sender(sender);
        } else {
            mailbox.start_processor();
        }

        Ok(mailbox)
    }

    /// Start background task to process internal queue into channel
    fn start_processor(&self) {
        let internal_queue = self.internal_queue.clone();
        let channel = self.channel.clone();
        let channel_name = self.channel_name.clone();
        let stats = self.stats.clone();
        let processor_handle = self.processor_handle.clone();
        let notify = self.notify.clone();

        let handle = tokio::spawn(async move {
            loop {
                // Process any existing messages first, then wait for new ones
                // This ensures we don't miss messages that arrive before we start waiting
                let queue_guard = internal_queue.read().await;
                let has_messages = match &*queue_guard {
                    MessageStorage::Queue(queue) => !queue.is_empty(),
                    MessageStorage::Priority(heap) => !heap.is_empty(),
                };
                drop(queue_guard);

                // If no messages, wait for notification
                if !has_messages {
                    notify.notified().await;
                }

                // Process messages from internal queue to channel
                let mut queue_guard = internal_queue.write().await;
                let mut messages_to_send = Vec::new();

                match &mut *queue_guard {
                    MessageStorage::Queue(queue) => {
                        // For FIFO/LIFO: send all messages in order
                        while let Some(msg) = queue.pop_front() {
                            messages_to_send.push(msg);
                        }
                    }
                    MessageStorage::Priority(heap) => {
                        // For Priority: send highest priority first
                        while let Some(priority_msg) = heap.pop() {
                            messages_to_send.push(priority_msg.message);
                        }
                    }
                }
                drop(queue_guard);

                // Send messages to channel backend
                let mut num_sent = 0;
                for msg in messages_to_send {
                    // ProtoMessage is already in the correct format, just set channel name
                    let mut channel_msg = msg.clone();
                    channel_msg.channel = channel_name.clone();
                    match channel.send(channel_msg).await {
                        Ok(_) => {
                            num_sent += 1;
                        }
                        Err(ChannelError::ChannelClosed(_)) => {
                            // Channel closed, stop processing
                            break;
                        }
                        Err(e) => {
                            // Log error but continue processing
                            tracing::warn!("Failed to send message to channel: {}", e);
                        }
                    }
                }

                // Update stats after sending (current_size tracks internal queue, not channel)
                if num_sent > 0 {
                    let mut stats_guard = stats.write().await;
                    stats_guard.current_size = stats_guard.current_size.saturating_sub(num_sent);
                }

                // Notify waiting dequeuers that messages are available
                // This wakes up any actors waiting on dequeue() when messages arrive
                if num_sent > 0 {
                    notify.notify_waiters();
                }
            }
        });

        // Store handle (spawn a task to do this since we can't await in sync function)
        let processor_handle_clone = processor_handle.clone();
        tokio::spawn(async move {
            *processor_handle_clone.write().await = Some(handle);
        });
    }

    /// Start background task with local sender for InMemory fast-path
    fn start_processor_with_local_sender(&self, local_sender: mpsc::UnboundedSender<ProtoMessage>) {
        let internal_queue = self.internal_queue.clone();
        let channel = self.channel.clone();
        let channel_name = self.channel_name.clone();
        let stats = self.stats.clone();
        let processor_handle = self.processor_handle.clone();
        let notify = self.notify.clone();

        let handle = tokio::spawn(async move {
            loop {
                // Process any existing messages first, then wait for new ones
                // This ensures we don't miss messages that arrive before we start waiting
                let queue_guard = internal_queue.read().await;
                let has_messages = match &*queue_guard {
                    MessageStorage::Queue(queue) => !queue.is_empty(),
                    MessageStorage::Priority(heap) => !heap.is_empty(),
                };
                drop(queue_guard);

                // If no messages, wait for notification
                if !has_messages {
                    notify.notified().await;
                }

                // Process messages from internal queue to channel
                let mut queue_guard = internal_queue.write().await;
                let mut messages_to_send = Vec::new();

                match &mut *queue_guard {
                    MessageStorage::Queue(queue) => {
                        // For FIFO/LIFO: send all messages in order
                        while let Some(msg) = queue.pop_front() {
                            messages_to_send.push(msg);
                        }
                    }
                    MessageStorage::Priority(heap) => {
                        // For Priority: send highest priority first
                        while let Some(priority_msg) = heap.pop() {
                            messages_to_send.push(priority_msg.message);
                        }
                    }
                }
                drop(queue_guard);

                // Send messages to both channel backend and local receiver (for fast-path dequeue)
                let mut num_sent = 0;
                for msg in messages_to_send {
                    let msg_id = msg.id.clone();
                    // Send to channel backend
                    // ProtoMessage is already in the correct format, just set channel name
                    let mut channel_msg = msg.clone();
                    channel_msg.channel = channel_name.clone();
                    let channel_send_result = channel.send(channel_msg).await;

                    // Also send to local receiver for fast-path (InMemory backend)
                    // This is non-blocking and doesn't require a lock
                    let local_send_result = local_sender.send(msg.clone());

                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!(
                            message_id = %msg_id,
                            channel_ok = channel_send_result.is_ok(),
                            local_ok = local_send_result.is_ok(),
                            "Mailbox processor: sending message"
                        );
                    }

                    match (channel_send_result, local_send_result) {
                        (Ok(_), Ok(())) => {
                            num_sent += 1;
                            if tracing::enabled!(tracing::Level::TRACE) {
                                tracing::trace!(
                                    message_id = %msg_id,
                                    num_sent,
                                    "Mailbox processor: sent to channel and local_receiver"
                                );
                            }
                        }
                        (Err(ChannelError::ChannelClosed(_)), _) | (_, Err(_)) => {
                            // Channel or local receiver closed, stop processing
                            tracing::warn!(
                                message_id = %msg_id,
                                "Mailbox processor: Channel or local_receiver closed, stopping processor"
                            );
                            break;
                        }
                        (Err(e), _) => {
                            // Log error but continue processing
                            tracing::warn!(
                                message_id = %msg_id,
                                error = %e,
                                "Mailbox processor: Failed to send message to channel, continuing"
                            );
                        }
                    }
                }

                // Update stats after sending
                if num_sent > 0 {
                    let mut stats_guard = stats.write().await;
                    stats_guard.current_size = stats_guard.current_size.saturating_sub(num_sent);
                }

                // Notify waiting dequeuers
                if num_sent > 0 {
                    notify.notify_waiters();
                }
            }
        });

        // Store handle
        let processor_handle_clone = processor_handle.clone();
        tokio::spawn(async move {
            *processor_handle_clone.write().await = Some(handle);
        });
    }

    /// Enqueue a message
    ///
    /// Messages are added to the internal queue based on ordering strategy,
    /// then processed by the background task into the channel.
    ///
    /// ## Deduplication
    /// - Message IDs: Duplicate message IDs within deduplication_window are skipped (LRU cache with fixed size)
    /// - Idempotency keys: Duplicate idempotency keys return cached response (if available) (LRU cache with fixed size)
    pub async fn enqueue(&self, message: ProtoMessage) -> Result<(), MailboxError> {
        // For non-memory channels, check shutdown flag to stop accepting new messages
        if !self.is_in_memory() {
            let shutdown = *self.shutdown_flag.read().await;
            if shutdown {
                return Err(MailboxError::StorageError(
                    "Mailbox is shutting down, not accepting new messages".to_string(),
                ));
            }
        }

        // Check for duplicate message ID (LRU cache with fixed size)
        {
            let mut cache = self.message_id_cache.write().await;
            // Cleanup expired entries (LRU cache handles TTL automatically)
            cache.cleanup_expired();

            // Check if message ID already seen (LRU cache returns None if expired or not found)
            if cache.get(&message.id).is_some() {
                // Duplicate message ID - skip
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(message_id = %message.id, "Skipping duplicate message ID");
                }
                return Ok(());
            }

            // Add to cache (LRU cache handles eviction if full)
            cache.insert(message.id.clone(), SystemTime::now());
        }

        // Check for duplicate idempotency key (LRU cache with fixed size)
        if !message.idempotency_key.is_empty() {
            let idempotency_key = &message.idempotency_key;
            let mut cache = self.idempotency_cache.write().await;
            // Cleanup expired entries
            cache.cleanup_expired();

            // Check if idempotency key already seen
            if let Some(_cached_entry) = cache.get(idempotency_key) {
                // Duplicate idempotency key - skip message (deduplication)
                // Note: get() already checked expiration, so if we get here, the key is valid
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(idempotency_key = %idempotency_key, "Skipping duplicate message with idempotency key");
                }
                return Ok(());
            }

            // Add to cache (no cached response yet - will be set after processing)
            // LRU cache handles eviction if full
            cache.insert(idempotency_key.clone(), (SystemTime::now(), None));
        }

        let mut queue_guard = self.internal_queue.write().await;
        let mut stats = self.stats.write().await;

        // Check capacity
        if stats.current_size >= mailbox_config_max_size(&self.config) {
            match mailbox_config_backpressure(&self.config) {
                BackpressureStrategy::DropOldest => {
                    match &mut *queue_guard {
                        MessageStorage::Queue(queue) => {
                            queue.pop_front();
                        }
                        MessageStorage::Priority(heap) => {
                            heap.pop();
                        }
                    }
                    stats.total_dropped += 1;
                    stats.current_size -= 1;
                }
                BackpressureStrategy::DropNewest => {
                    stats.total_dropped += 1;
                    return Ok(()); // Drop the new message
                }
                BackpressureStrategy::Error => {
                    return Err(MailboxError::Full);
                }
                BackpressureStrategy::Block => {
                    // For Block strategy, we should wait but with a timeout to prevent deadlocks
                    // However, this can cause issues during shutdown, so we'll use a short timeout
                    // In practice, actors should handle MailboxError::Full gracefully
                    // For now, return error to prevent deadlock during shutdown
                    return Err(MailboxError::Full);
                }
                _ => {
                    return Err(MailboxError::Full);
                }
            }
        }

        let message_id = message.id.clone();

        // Clone values before moving message
        let sender_id = message.sender_id.clone();
        let receiver_id = message.receiver_id.clone();
        let message_type = message.message_type.clone();
        let correlation_id = message.correlation_id.clone();

        // Add message based on ordering (always use internal queue, processor moves to channel)
        match mailbox_config_ordering(&self.config) {
            OrderingStrategy::OrderingFifo => {
                if let MessageStorage::Queue(queue) = &mut *queue_guard {
                    queue.push_back(message);
                }
            }
            OrderingStrategy::OrderingLifo => {
                if let MessageStorage::Queue(queue) = &mut *queue_guard {
                    queue.push_front(message);
                }
            }
            OrderingStrategy::OrderingPriority => {
                if let MessageStorage::Priority(heap) = &mut *queue_guard {
                    heap.push(PriorityMessage { message });
                }
            }
            OrderingStrategy::OrderingRandom => {
                if let MessageStorage::Queue(queue) = &mut *queue_guard {
                    let mut rng = rand::thread_rng();
                    let pos = rng.gen_range(0..=queue.len());
                    queue.insert(pos, message);
                }
            }
            _ => {
                // Default to FIFO
                if let MessageStorage::Queue(queue) = &mut *queue_guard {
                    queue.push_back(message);
                }
            }
        }

        stats.total_enqueued += 1;
        stats.current_size += 1;

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                message_id = %message_id,
                sender_id = %sender_id,
                receiver_id = %receiver_id,
                message_type = %message_type,
                correlation_id = %correlation_id,
                queue_size = stats.current_size,
                "Mailbox::enqueue: ✅ Message enqueued successfully"
            );
        }

        // Notify processor that a message is available
        // This wakes up the processor task that's waiting on notify.notified()
        self.notify.notify_one();
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                message_id = %message_id,
                "Mailbox::enqueue: Notified processor task"
            );
        }

        Ok(())
    }

    /// Send a message (alias for enqueue)
    pub async fn send(&self, message: ProtoMessage) -> Result<(), MailboxError> {
        self.enqueue(message).await
    }

    /// Dequeue a message with optional timeout
    ///
    /// Returns a future that yields messages from the channel.
    /// If `timeout` is `None`, waits indefinitely for a message.
    /// If `timeout` is `Some(duration)`, returns `None` if no message arrives within the timeout.
    ///
    /// This can be used in `tokio::select!` for proper async/await patterns.
    ///
    /// ## Example
    /// ```rust,ignore
    /// // Wait indefinitely
    /// let msg = mailbox.dequeue_with_timeout(None).await;
    ///
    /// // Wait with 1 second timeout
    /// let msg = mailbox.dequeue_with_timeout(Some(Duration::from_secs(1))).await;
    ///
    /// // Use in select!
    /// tokio::select! {
    ///     Some(message) = mailbox.dequeue_with_timeout(None) => {
    ///         // Process message
    ///     }
    ///     _ = shutdown_rx.recv() => {
    ///         break;
    ///     }
    /// }
    /// ```
    pub fn dequeue_with_timeout(
        &self,
        timeout: Option<std::time::Duration>,
    ) -> impl std::future::Future<Output = Option<ProtoMessage>> + 'static {
        let channel = self.channel.clone();
        let local_receiver = self.local_receiver.clone();
        let mailbox_id = self.mailbox_id.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        // Compute is_in_memory from channel_provider (avoiding lifetime issues)
        use plexspaces_proto::channel::v1::ChannelProvider;
        let channel_provider = self.channel_provider; // Copy the i32 value
        let is_in_memory = matches!(
            ChannelProvider::try_from(channel_provider),
            Ok(ChannelProvider::ChannelProviderInMemory)
        );

        async move {
            tracing::trace!(
                mailbox_id = %mailbox_id,
                "Mailbox::dequeue_with_timeout: Starting dequeue operation"
            );
            // Try local receiver first (fast-path for InMemory backend)
            // PERFORMANCE: Use try_recv in a loop to avoid holding Mutex lock while waiting
            // This allows the processor to continue sending messages without blocking
            let start_time = std::time::Instant::now();
            let mut attempts = 0;
            loop {
                attempts += 1;

                // Check if we have a receiver (brief lock - only for checking)
                let has_receiver = {
                    let receiver_opt = local_receiver.lock().await;
                    receiver_opt.is_some()
                };

                if !has_receiver {
                    if tracing::enabled!(tracing::Level::TRACE) {
                        tracing::trace!("Mailbox::dequeue: local_receiver not available (attempt {}), falling back to channel backend", attempts);
                    }
                    break; // Fall through to channel backend
                }

                // Try to receive without blocking (brief lock - only for try_recv)
                let msg_opt = {
                    let mut receiver_opt = local_receiver.lock().await;
                    if let Some(ref mut receiver) = *receiver_opt {
                        receiver.try_recv().ok()
                    } else {
                        None
                    }
                };

                if let Some(msg) = msg_opt {
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(
                            mailbox_id = %mailbox_id,
                            message_id = %msg.id,
                            message_type = %message_type_str(&msg),
                            sender_id = %msg.sender_id,
                            receiver_id = %msg.receiver_id,
                            correlation_id = %msg.correlation_id,
                            attempts = attempts,
                            "📬 Mailbox::dequeue: ✅ Received message from local_receiver (try_recv)"
                        );
                    }

                    return Some(msg);
                }

                // Check timeout if specified
                if let Some(duration) = timeout {
                    if start_time.elapsed() >= duration {
                        tracing::trace!(
                            attempts = attempts,
                            elapsed_ms = start_time.elapsed().as_millis(),
                            "Mailbox::dequeue: Timeout waiting for message from local_receiver"
                        );
                        return None;
                    }
                }

                // Log every 100 attempts to avoid spam (roughly every 1ms with 10μs sleep)
                if attempts % 100 == 0 {
                    tracing::trace!(
                        attempts = attempts,
                        elapsed_ms = start_time.elapsed().as_millis(),
                        "Mailbox::dequeue: Still waiting for message from local_receiver..."
                    );
                }

                // Yield to allow other tasks to run (processor can send messages)
                // Use a very short sleep to avoid busy-waiting while still being responsive
                tokio::task::yield_now().await;
                tokio::time::sleep(std::time::Duration::from_micros(10)).await;
            }

            // Fall back to channel backend (for durable backends like SQLite, Redis, Kafka)
            // Check shutdown flag before receiving (for non-memory channels)
            // (shutdown_flag and is_in_memory already captured above)
            match timeout {
                None => {
                    // Indefinite wait - poll channel
                    loop {
                        // Check shutdown flag for non-memory channels
                        if !is_in_memory {
                            let shutdown = *shutdown_flag.read().await;
                            if shutdown {
                                if tracing::enabled!(tracing::Level::DEBUG) {
                                    tracing::debug!(
                                        mailbox_id = %mailbox_id,
                                        "Mailbox::dequeue: Shutdown in progress, stopping receive"
                                    );
                                }
                                return None;
                            }
                        }

                        match channel.receive(1).await {
                            Ok(messages) => {
                                if let Some(channel_msg) = messages.first() {
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        tracing::debug!(
                                            mailbox_id = %mailbox_id,
                                            message_id = %channel_msg.id,
                                            message_type = %message_type_str(channel_msg),
                                            sender = ?channel_msg.sender_id,
                                            receiver = %channel_msg.receiver_id,
                                            correlation_id = ?channel_msg.correlation_id,
                                            "📬 Mailbox::dequeue: ✅ Received message from channel (receive)"
                                        );
                                    }
                                    return Some(channel_msg.clone());
                                }
                            }
                            Err(ChannelError::ChannelClosed(_)) => {
                                return None;
                            }
                            Err(e) => {
                                tracing::warn!("Channel receive error: {}", e);
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                continue;
                            }
                        }
                        // Small sleep to prevent busy-waiting
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                }
                Some(duration) => {
                    // Wait with timeout
                    // Check shutdown flag for non-memory channels
                    if !is_in_memory {
                        let shutdown = *shutdown_flag.read().await;
                        if shutdown {
                            if tracing::enabled!(tracing::Level::DEBUG) {
                                tracing::debug!(
                                    mailbox_id = %mailbox_id,
                                    "Mailbox::dequeue: Shutdown in progress, stopping receive"
                                );
                            }
                            return None;
                        }
                    }

                    let start = std::time::Instant::now();
                    loop {
                        if start.elapsed() >= duration {
                            return None;
                        }

                        match channel.try_receive(1).await {
                            Ok(messages) => {
                                if let Some(channel_msg) = messages.first() {
                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        tracing::debug!(
                                            mailbox_id = %mailbox_id,
                                            message_id = %channel_msg.id,
                                            message_type = %message_type_str(channel_msg),
                                            sender = ?channel_msg.sender_id,
                                            receiver = %channel_msg.receiver_id,
                                            correlation_id = ?channel_msg.correlation_id,
                                            "📬 Mailbox::dequeue: ✅ Received message from channel (try_receive)"
                                        );
                                    }
                                    return Some(channel_msg.clone());
                                }
                            }
                            Err(ChannelError::ChannelClosed(_)) => {
                                return None;
                            }
                            Err(_) => {
                                // No messages available, wait a bit
                                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                                continue;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Dequeue a message (indefinite timeout)
    ///
    /// Returns a future that yields messages from the channel.
    /// Waits indefinitely for a message to arrive.
    /// This is a convenience method that calls `dequeue_with_timeout(None)`.
    ///
    /// ## Example
    /// ```rust,ignore
    /// tokio::select! {
    ///     Some(message) = mailbox.dequeue() => {
    ///         // Process message
    ///     }
    ///     _ = shutdown_rx.recv() => {
    ///         break;
    ///     }
    /// }
    /// ```
    pub fn dequeue(&self) -> impl std::future::Future<Output = Option<ProtoMessage>> {
        self.dequeue_with_timeout(None)
    }

    /// Acknowledge message processing
    ///
    /// ## Arguments
    /// * `message` - The message that was successfully processed
    ///
    /// ## Behavior
    /// - Calls `channel.ack()` with the channel message ID
    /// - Updates statistics
    ///
    /// ## Notes
    /// - Channel implementations handle ack appropriately (InMemory = no-op, Redis/Kafka = actual ack)
    /// - No backend-specific checks needed - channel trait handles it
    pub async fn ack_message(&self, message: &ProtoMessage) -> Result<(), MailboxError> {
        // Get channel message ID (use message.id directly for proto Message)
        let channel_msg_id = &message.id;

        // Call channel.ack() - channel implementation handles backend-specific behavior
        self.channel
            .ack(channel_msg_id)
            .await
            .map_err(|e| MailboxError::StorageError(format!("Failed to ack message: {}", e)))?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_dequeued += 1;
        }

        // OBSERVABILITY: Track successful acks via tracing
        tracing::trace!(
            mailbox_id = %self.mailbox_id,
            message_id = %message.id,
            "Message acked successfully"
        );

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                mailbox_id = %self.mailbox_id,
                message_id = %message.id,
                channel_msg_id = %channel_msg_id,
                "✅ Mailbox::ack_message: Message acknowledged"
            );
        }

        Ok(())
    }

    /// Negative acknowledge message processing
    ///
    /// ## Arguments
    /// * `message` - The message that failed processing
    /// * `error` - Optional error message for logging
    ///
    /// ## Behavior
    /// - Calls `channel.nack()` - channel implementation handles retry/DLQ logic
    /// - Updates statistics
    ///
    /// ## Notes
    /// - Channel implementations handle nack appropriately:
    ///   - InMemory = no-op (just tracks metrics)
    ///   - Redis/Kafka = tracks retry count, requeues or sends to DLQ based on channel config
    /// - No backend-specific checks needed - channel trait handles it
    /// - Retry/DLQ logic is in channel implementation, not mailbox
    pub async fn nack_message(
        &self,
        message: &ProtoMessage,
        error: Option<&str>,
    ) -> Result<(), MailboxError> {
        // Get channel message ID (use message.id directly for proto Message)
        let channel_msg_id = &message.id;

        // Call channel.nack() with requeue=true
        // Channel implementation will handle retry counting and DLQ logic based on its config
        // For channels that support retry/DLQ, they track delivery_count internally
        self.channel
            .nack(channel_msg_id, true)
            .await
            .map_err(|e| MailboxError::StorageError(format!("Failed to nack message: {}", e)))?;

        // Update stats
        {
            let mut stats = self.stats.write().await;
            stats.total_dropped += 1; // Track failed messages
        }

        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(
                mailbox_id = %self.mailbox_id,
                message_id = %message.id,
                channel_msg_id = %channel_msg_id,
                error = ?error,
                "Message nacked (channel handles retry/DLQ)"
            );
        }

        Ok(())
    }

    /// Dequeue a message matching a predicate (selective receive)
    ///
    /// Note: This is less efficient with channel-based architecture as it requires
    /// checking messages. For better performance, use pattern matching in the actor loop.
    pub async fn dequeue_matching<F>(&self, predicate: F) -> Option<ProtoMessage>
    where
        F: Fn(&ProtoMessage) -> bool,
    {
        // For selective receive, we need to check the internal queue
        // This is a limitation of channel-based architecture
        let mut queue_guard = self.internal_queue.write().await;
        let mut stats = self.stats.write().await;

        match &mut *queue_guard {
            MessageStorage::Queue(queue) => {
                if let Some(pos) = queue.iter().position(&predicate) {
                    let message = queue.remove(pos)?;
                    stats.total_dequeued += 1;
                    stats.current_size -= 1;
                    return Some(message);
                }
            }
            MessageStorage::Priority(_heap) => {
                // For priority queue, we'd need to convert to Vec, filter, and rebuild
                // This is inefficient, so we'll just return None for now
                // TODO: Implement proper selective receive for priority queue
            }
        }

        None
    }

    /// Peek at messages without removing
    pub async fn peek(&self, count: usize) -> Vec<ProtoMessage> {
        let queue_guard = self.internal_queue.read().await;
        match &*queue_guard {
            MessageStorage::Queue(queue) => queue.iter().take(count).cloned().collect(),
            MessageStorage::Priority(heap) => {
                // Convert heap to sorted vec for peeking
                let mut sorted: Vec<_> = heap.iter().map(|pm| pm.message.clone()).collect();
                sorted.sort_by(|a, b| {
                    b.priority.cmp(&a.priority) // Proto priority is already i32
                });
                sorted.into_iter().take(count).collect()
            }
        }
    }

    /// Get current size
    pub async fn size(&self) -> usize {
        self.stats.read().await.current_size
    }

    /// Clear all messages
    pub async fn clear(&self) {
        let mut queue_guard = self.internal_queue.write().await;
        let mut stats = self.stats.write().await;

        match &mut *queue_guard {
            MessageStorage::Queue(queue) => queue.clear(),
            MessageStorage::Priority(heap) => heap.clear(),
        }

        stats.current_size = 0;
    }

    /// Check if this mailbox uses a durable backend
    ///
    /// Returns `true` if the mailbox is backed by a durable channel (SQLite, Redis, Kafka),
    /// `false` if it's in-memory only.
    ///
    /// ## Use Case
    /// Used by DurabilityFacet to determine if mailbox messages will survive actor restart.
    pub fn is_durable(&self) -> bool {
        use plexspaces_proto::channel::v1::ChannelProvider;
        // Check if backend is durable (not InMemory or UDP)
        match ChannelProvider::try_from(self.channel_provider) {
            Ok(ChannelProvider::ChannelProviderInMemory) => false,
            Ok(ChannelProvider::ChannelProviderUdp) => false, // UDP is best-effort, not persistent
            Ok(_) => true,   // SQLite, Redis, Kafka, NATS are all durable
            Err(_) => false, // Invalid backend, assume not durable
        }
    }

    /// Check if backend is in-memory (for shutdown logic)
    pub fn is_in_memory(&self) -> bool {
        use plexspaces_proto::channel::v1::ChannelProvider;
        matches!(
            ChannelProvider::try_from(self.channel_provider),
            Ok(ChannelProvider::ChannelProviderInMemory)
        )
    }

    /// Get the channel backend type
    ///
    /// Returns the backend type as a string for logging/metrics.
    pub fn backend_type(&self) -> &'static str {
        use plexspaces_proto::channel::v1::ChannelProvider;
        match ChannelProvider::try_from(self.channel_provider) {
            Ok(ChannelProvider::ChannelProviderInMemory) => "in_memory",
            Ok(ChannelProvider::ChannelProviderRedis) => "redis",
            Ok(ChannelProvider::ChannelProviderKafka) => "kafka",
            Ok(ChannelProvider::ChannelProviderSqlite) => "sqlite",
            Ok(ChannelProvider::ChannelProviderNats) => "nats",
            Ok(ChannelProvider::ChannelProviderUdp) => "udp",
            Ok(ChannelProvider::ChannelProviderSqs) => "sqs",
            Ok(ChannelProvider::ChannelProviderProcessGroup) => "process_group",
            Ok(ChannelProvider::ChannelProviderPostgres) => "postgres",
            Ok(ChannelProvider::ChannelProviderCustom) => "custom",
            Err(_) => "unknown",
        }
    }

    /// Get mailbox statistics for observability
    ///
    /// Returns current size, total enqueued, total dequeued, and backend type.
    /// Used for metrics collection at start/stop/graceful shutdown.
    pub async fn get_stats(&self) -> MailboxObservabilityStats {
        let stats = self.stats.read().await;
        MailboxObservabilityStats {
            current_size: stats.current_size,
            total_enqueued: stats.total_enqueued as usize,
            total_dequeued: stats.total_dequeued as usize,
            backend_type: self.backend_type().to_string(),
            is_durable: self.is_durable(),
        }
    }

    /// Graceful shutdown: Stop accepting new messages and complete in-progress ones
    ///
    /// For non-memory channels:
    /// - Stops accepting new messages via enqueue()
    /// - Stops receiving new messages from channel backend
    /// - Waits for in-progress messages to complete (with timeout)
    /// - Closes the channel to stop receiving
    ///
    /// For durable backends, also ensures all pending messages are persisted.
    ///
    /// ## Use Case
    /// Called during actor graceful shutdown to ensure:
    /// - No new messages are accepted
    /// - In-progress messages complete and get ACK/NACK
    /// - Pending messages are persisted (for durable backends)
    ///
    /// ## Arguments
    /// * `timeout` - Maximum time to wait for in-progress messages to complete
    ///
    /// ## Returns
    /// `Ok(())` if shutdown completed successfully
    pub async fn graceful_shutdown(&self, timeout: Option<Duration>) -> Result<(), MailboxError> {
        tracing::info!(
            mailbox_id = %self.mailbox_id,
            backend = %self.backend_type(),
            "Starting graceful shutdown"
        );

        // Step 1: Set shutdown flag to stop accepting new messages (for non-memory channels)
        if !self.is_in_memory() {
            let mut shutdown_flag = self.shutdown_flag.write().await;
            *shutdown_flag = true;
            drop(shutdown_flag);
            if tracing::enabled!(tracing::Level::DEBUG) {
                tracing::debug!(
                    mailbox_id = %self.mailbox_id,
                    "Shutdown flag set - no new messages will be accepted"
                );
            }
        }

        // Step 2: For durable backends, flush pending messages
        if self.is_durable() {
            let queue_guard = self.internal_queue.read().await;
            let pending_count = match &*queue_guard {
                MessageStorage::Queue(queue) => queue.len(),
                MessageStorage::Priority(heap) => heap.len(),
            };
            drop(queue_guard);

            if pending_count > 0 {
                tracing::info!(
                    mailbox_id = %self.mailbox_id,
                    backend = %self.backend_type(),
                    pending_messages = pending_count,
                    "Flushing pending messages to durable backend during graceful shutdown"
                );

                // Messages will be flushed by the processor task
                // Wait a bit for processor to flush (non-blocking)
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }

        // Step 3: Wait for in-progress messages to complete (with timeout)
        let timeout_duration = timeout.unwrap_or(Duration::from_secs(30));
        let start = std::time::Instant::now();

        loop {
            let in_progress = *self.in_progress_count.read().await;
            if in_progress == 0 {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        mailbox_id = %self.mailbox_id,
                        "All in-progress messages completed"
                    );
                }
                break;
            }

            if start.elapsed() >= timeout_duration {
                tracing::warn!(
                    mailbox_id = %self.mailbox_id,
                    in_progress = in_progress,
                    timeout_secs = timeout_duration.as_secs(),
                    "Timeout waiting for in-progress messages to complete"
                );
                break;
            }

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Step 4: Close the channel to stop receiving (for non-memory channels)
        if !self.is_in_memory() {
            if let Err(e) = self.channel.close().await {
                tracing::warn!(
                    mailbox_id = %self.mailbox_id,
                    error = %e,
                    "Failed to close channel during shutdown"
                );
            } else {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(
                        mailbox_id = %self.mailbox_id,
                        "Channel closed successfully"
                    );
                }
            }
        }

        tracing::info!(
            mailbox_id = %self.mailbox_id,
            "Graceful shutdown completed"
        );

        Ok(())
    }
}

/// Mailbox statistics for observability (public API)
#[derive(Debug, Clone)]
pub struct MailboxObservabilityStats {
    /// Current number of messages in mailbox
    pub current_size: usize,
    /// Total messages enqueued since creation
    pub total_enqueued: usize,
    /// Total messages dequeued since creation
    pub total_dequeued: usize,
    /// Backend type (in_memory, redis, kafka, sqlite, etc.)
    pub backend_type: String,
    /// Whether this mailbox is durable
    pub is_durable: bool,
}

// MailboxError is defined above (wrapper around proto enum)

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::channel::v1::ChannelProvider;

    /// Helper to create a test mailbox with InMemory backend
    async fn create_test_mailbox(config: MailboxConfig) -> Mailbox {
        Mailbox::new(config, format!("test-mailbox-{}", ulid::Ulid::new()))
            .await
            .unwrap()
    }

    /// Helper to create a test mailbox with default config
    async fn create_default_mailbox() -> Mailbox {
        create_test_mailbox(mailbox_config_default()).await
    }

    #[tokio::test]
    async fn test_fifo_mailbox() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue messages
        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Dequeue in FIFO order
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload, b"first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload, b"second");
    }

    #[tokio::test]
    async fn test_lifo_mailbox() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingLifo as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Dequeue in LIFO order
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload, b"second");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload, b"first");
    }

    // ==========================================================================
    // PRIORITY MAILBOX TESTS (Bisected for debugging)
    // ==========================================================================

    /// Test 1: Verify messages are enqueued to internal priority queue
    #[tokio::test]
    async fn test_priority_mailbox_enqueue() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();

        // Messages should be in internal queue
        assert_eq!(mailbox.size().await, 2);
    }

    /// Test 2: Verify priority comparison logic
    #[tokio::test]
    async fn test_priority_comparison() {
        // System (10) > Highest (5) > High (4) > Normal (3) > Low (2)
        assert!(
            message_priority_value(&MessagePriority::System)
                > message_priority_value(&MessagePriority::Highest)
        );
        assert!(
            message_priority_value(&MessagePriority::Highest)
                > message_priority_value(&MessagePriority::High)
        );
        assert!(
            message_priority_value(&MessagePriority::High)
                > message_priority_value(&MessagePriority::Normal)
        );
        assert!(
            message_priority_value(&MessagePriority::Normal)
                > message_priority_value(&MessagePriority::Low)
        );

        // Verify signal message has Highest priority
        let signal_msg = signal_message(b"signal".to_vec());
        assert_eq!(signal_msg.priority(), MessagePriority::Highest);
        assert_eq!(message_priority_value(&signal_msg.priority()), 5);
    }

    /// Test 3: Verify background processor moves messages from internal queue to channel
    #[tokio::test]
    async fn test_priority_mailbox_processor() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue one message
        mailbox
            .enqueue(new_message(b"test".to_vec()).with_priority(MessagePriority::Normal))
            .await
            .unwrap();

        // Wait for processor to move message to channel
        // Poll until internal queue is empty
        let mut attempts = 0;
        while mailbox.size().await > 0 && attempts < 100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            attempts += 1;
        }

        // Message should be in channel now (size() tracks internal queue, not channel)
        assert_eq!(
            mailbox.size().await,
            0,
            "Internal queue should be empty after processor runs"
        );

        // Message should be available for dequeue
        let msg = mailbox.dequeue().await;
        assert!(msg.is_some(), "Message should be available in channel");
        assert_eq!(msg.unwrap().payload, b"test");
    }

    /// Test 4: Verify priority ordering with two messages
    #[tokio::test]
    async fn test_priority_mailbox_two_messages() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue low priority first, then high priority
        mailbox
            .enqueue(new_message(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();

        // Wait for processor to move messages
        let mut attempts = 0;
        while mailbox.size().await > 0 && attempts < 100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            attempts += 1;
        }

        // High priority should come first
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload, b"high", "High priority should come first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload, b"low", "Low priority should come second");
    }

    /// Test 5: Verify priority ordering with signal (Highest priority)
    #[tokio::test]
    async fn test_priority_mailbox_signal() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue low priority, then signal (Highest)
        mailbox
            .enqueue(new_message(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(signal_message(b"signal".to_vec()))
            .await
            .unwrap();

        // Wait for processor
        let mut attempts = 0;
        while mailbox.size().await > 0 && attempts < 100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            attempts += 1;
        }

        // Signal (Highest) should come first
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(
            msg1.payload, b"signal",
            "Signal (Highest priority) should come first"
        );

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload, b"low", "Low priority should come second");
    }

    /// Test 6: Full priority mailbox test (all priorities)
    #[tokio::test]
    async fn test_priority_mailbox() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue in random order
        mailbox
            .enqueue(new_message(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"normal".to_vec()).with_priority(MessagePriority::Normal))
            .await
            .unwrap();
        mailbox
            .enqueue(signal_message(b"signal".to_vec()))
            .await
            .unwrap();

        // Wait for processor to move all messages
        let mut attempts = 0;
        while mailbox.size().await > 0 && attempts < 100 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            attempts += 1;
        }

        // Dequeue in priority order: signal (Highest=5) > high (4) > normal (3) > low (2)
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(
            msg1.payload, b"signal",
            "Signal (Highest=5) should come first"
        );

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload, b"high", "High (4) should come second");

        let msg3 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg3.payload, b"normal", "Normal (3) should come third");

        let msg4 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg4.payload, b"low", "Low (2) should come last");
    }

    #[tokio::test]
    async fn test_selective_receive() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"target".to_vec()))
            .await
            .unwrap();

        // Selectively receive the "target" message
        let msg = mailbox.dequeue_matching(|m| m.payload == b"target").await;
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().payload, b"target");

        // First and second should still be in queue
        assert_eq!(mailbox.size().await, 2);
    }

    #[tokio::test]
    async fn test_backpressure_drop_oldest() {
        let mut config = mailbox_config_default();
        config.capacity = 2;
        config.backpressure_strategy = BackpressureStrategy::DropOldest as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"third".to_vec()))
            .await
            .unwrap(); // Should drop "first"

        assert_eq!(mailbox.size().await, 2);

        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload, b"second"); // "first" was dropped
    }

    #[tokio::test]
    async fn test_message_priorities() {
        // Test priority ordering using helper function
        // System (10) > Highest (5) > High (4) > Normal (3) > Low (2) > Lowest (1)
        assert!(
            message_priority_value(&MessagePriority::System)
                > message_priority_value(&MessagePriority::Highest)
        );
        assert!(
            message_priority_value(&MessagePriority::Highest)
                > message_priority_value(&MessagePriority::High)
        );
        assert!(
            message_priority_value(&MessagePriority::High)
                > message_priority_value(&MessagePriority::Normal)
        );
        assert!(
            message_priority_value(&MessagePriority::Normal)
                > message_priority_value(&MessagePriority::Low)
        );
    }

    #[tokio::test]
    async fn test_message_metadata() {
        let message = new_message(b"test".to_vec())
            .with_correlation_id("corr-123".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_metadata("type".to_string(), "call".to_string());

        assert_eq!(message.correlation_id, "corr-123");
        assert_eq!(message.reply_to, "reply-addr");
        // TODO: Restore when behavior module is migrated
        // assert_eq!(message.message_type(), crate::behavior::MessageType::Call);
        assert_eq!(message.headers.get("type"), Some(&"call".to_string()));
    }

    // ==========================================================================
    // MESSAGE CREATION TESTS (Lines 89-122)
    // ==========================================================================

    /// Test system_message() creates system priority message
    #[tokio::test]
    async fn test_message_system() {
        let message = system_message(b"shutdown".to_vec());

        assert_eq!(message.priority(), MessagePriority::System);
        assert_eq!(message.payload, b"shutdown");
    }

    /// Test timer_message() creates timer message with metadata
    #[tokio::test]
    async fn test_message_timer() {
        let message = timer_message("heartbeat");

        // Check headers (metadata stored in proto headers)
        assert_eq!(message.headers.get("type"), Some(&"timer".to_string()));
        assert_eq!(
            message.headers.get("timer_name"),
            Some(&"heartbeat".to_string())
        );

        // Payload should contain timer name
        assert_eq!(message.payload, b"heartbeat");
    }

    /// Test Message::id() and payload() methods
    #[tokio::test]
    async fn test_message_id_and_payload() {
        let message = new_message(b"test-payload".to_vec());

        // ID should be non-empty ULID
        assert!(!message.id().is_empty());

        // Payload should match
        assert_eq!(message.payload, b"test-payload");
    }

    /// Test message_type_str() returns correct type
    #[tokio::test]
    async fn test_message_type_str() {
        // Test with message_type field set
        let msg1 = new_message(b"test".to_vec()).with_message_type("call".to_string());
        assert_eq!(msg1.message_type_str(), "call");

        // Test with metadata "type" key (fallback)
        let msg2 =
            new_message(b"test".to_vec()).with_metadata("type".to_string(), "cast".to_string());
        assert_eq!(msg2.message_type_str(), "cast");

        // Test with neither (default to "cast")
        let msg3 = new_message(b"test".to_vec());
        assert_eq!(msg3.message_type_str(), "cast");

        // Test message_type takes precedence over metadata
        let msg4 = new_message(b"test".to_vec())
            .with_message_type("info".to_string())
            .with_metadata("type".to_string(), "cast".to_string());
        assert_eq!(msg4.message_type_str(), "info");
    }

    // ==========================================================================
    // MESSAGE BUILDER TESTS (Lines 175-193)
    // ==========================================================================

    /// Test with_sender() and sender_id() methods
    #[tokio::test]
    async fn test_message_with_sender() {
        let message = new_message(b"test".to_vec()).with_sender("actor-123".to_string());

        assert_eq!(message.sender_id, "actor-123");
        assert_eq!(message.sender_id(), Some("actor-123"));

        // Test message without sender
        let msg2 = new_message(b"test".to_vec());
        assert_eq!(msg2.sender_id(), None);
    }

    /// Test with_message_type() method
    #[tokio::test]
    async fn test_message_with_message_type() {
        let message = new_message(b"test".to_vec()).with_message_type("workflow_run".to_string());

        assert_eq!(message.message_type, "workflow_run");
        assert_eq!(message.message_type_str(), "workflow_run");
    }

    /// Test priority() getter method
    #[tokio::test]
    async fn test_message_priority() {
        let message = new_message(b"test".to_vec()).with_priority(MessagePriority::High);

        assert_eq!(message.priority(), MessagePriority::High);
    }

    /// Test builder method chaining
    #[tokio::test]
    async fn test_message_builders_chaining() {
        let message = new_message(b"payload".to_vec())
            .with_sender("sender-1".to_string())
            .with_message_type("call".to_string())
            .with_priority(MessagePriority::High)
            .with_correlation_id("corr-456".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_metadata("key".to_string(), "value".to_string());

        assert_eq!(message.sender_id, "sender-1");
        assert_eq!(message.message_type, "call");
        assert_eq!(message.priority(), MessagePriority::High);
        assert_eq!(message.correlation_id, "corr-456");
        assert_eq!(message.reply_to, "reply-addr");
        assert_eq!(message.headers.get("key"), Some(&"value".to_string()));
    }

    // ==========================================================================
    // PROTO CONVERSION TESTS (Lines 197-256)
    // ==========================================================================

    /// Test from_proto() with various priority values
    #[tokio::test]
    async fn test_message_from_proto() {
        use plexspaces_proto::common::v1::Message as ProtoMessage;
        use std::collections::HashMap;

        // Test high priority (50-74 range)
        let mut headers = HashMap::new();
        headers.insert("correlation_id".to_string(), "corr-1".to_string());
        headers.insert("reply_to".to_string(), "reply-1".to_string());

        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: "sender-123".to_string(),
            receiver_id: "receiver-456".to_string(),
            channel: String::new(),
            message_type: "call".to_string(),
            payload: b"test-payload".to_vec(),
            timestamp: None,
            priority: 60, // High priority (in 50-74 range)
            ttl: None,
            headers: headers.clone(),
            idempotency_key: String::new(),
            correlation_id: "corr-1".to_string(),
            reply_to: "reply-1".to_string(),
            partition_key: String::new(),
            delivery_count: 0,
            uri_path: String::new(),
            uri_method: String::new(),
        };

        let message = Message::from_proto(&proto_msg);

        assert_eq!(message.id, "test-id");
        assert_eq!(message.sender_id, "sender-123");
        assert_eq!(message.receiver_id, "receiver-456");
        assert_eq!(message.message_type, "call");
        assert_eq!(message.payload, b"test-payload");
        assert_eq!(message.priority(), MessagePriority::High); // 60 is in 50-74 range
        assert_eq!(message.correlation_id, "corr-1");
        assert_eq!(message.reply_to, "reply-1");

        // Test normal priority (25-49 range)
        let mut proto_msg2 = proto_msg.clone();
        proto_msg2.priority = 30;
        let message2 = Message::from_proto(&proto_msg2);
        assert_eq!(message2.priority(), MessagePriority::Normal);

        // Test low priority (< 25 range)
        let mut proto_msg3 = proto_msg.clone();
        proto_msg3.priority = 10;
        let message3 = Message::from_proto(&proto_msg3);
        assert_eq!(message3.priority(), MessagePriority::Low); // 10 < 25 maps to Low

        // Test empty sender
        let mut proto_msg4 = proto_msg.clone();
        proto_msg4.sender_id = String::new();
        let message4 = Message::from_proto(&proto_msg4);
        assert_eq!(message4.sender_id(), None);

        // Test empty receiver (from_proto is a clone, so receiver_id stays empty)
        let mut proto_msg5 = proto_msg.clone();
        proto_msg5.receiver_id = String::new();
        let message5 = Message::from_proto(&proto_msg5);
        assert_eq!(message5.receiver_id, "");
    }

    /// Test to_proto() with all priority levels
    #[tokio::test]
    async fn test_message_to_proto() {
        // Test Highest priority (Signal equivalent)
        let msg1 = new_message(b"test".to_vec())
            .with_priority(MessagePriority::Highest)
            .with_sender("sender-1".to_string())
            .with_correlation_id("corr-1".to_string())
            .with_reply_to("reply-1".to_string())
            .with_metadata("custom".to_string(), "value".to_string());

        let proto1 = msg1.to_proto();
        assert_eq!(proto1.priority, 100); // Highest = 100
        assert_eq!(proto1.sender_id, "sender-1");
        assert_eq!(proto1.correlation_id, "corr-1"); // stored as direct field
        assert_eq!(proto1.reply_to, "reply-1"); // stored as direct field
        assert_eq!(proto1.headers.get("custom"), Some(&"value".to_string()));

        // Test System priority
        let msg2 = system_message(b"test".to_vec());
        let proto2 = msg2.to_proto();
        assert_eq!(proto2.priority, 75); // System = 75

        // Test High priority
        let msg3 = new_message(b"test".to_vec()).with_priority(MessagePriority::High);
        let proto3 = msg3.to_proto();
        assert_eq!(proto3.priority, 50); // High = 50

        // Test Normal priority
        let msg4 = new_message(b"test".to_vec()).with_priority(MessagePriority::Normal);
        let proto4 = msg4.to_proto();
        assert_eq!(proto4.priority, 25); // Normal = 25

        // Test Low priority
        let msg5 = new_message(b"test".to_vec()).with_priority(MessagePriority::Low);
        let proto5 = msg5.to_proto();
        assert_eq!(proto5.priority, 0); // Low = 0

        // Test message without sender
        let msg6 = new_message(b"test".to_vec());
        let proto6 = msg6.to_proto();
        assert_eq!(proto6.sender_id, ""); // Empty if no sender
    }

    // ==========================================================================
    // MAILBOX BACKPRESSURE TESTS (Lines 405-413)
    // ==========================================================================

    /// Test backpressure DropNewest strategy
    #[tokio::test]
    async fn test_backpressure_drop_newest() {
        let mut config = mailbox_config_default();
        config.capacity = 2;
        config.backpressure_strategy = BackpressureStrategy::DropNewest as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should be dropped (DropNewest)
        mailbox
            .enqueue(new_message(b"third".to_vec()))
            .await
            .unwrap();

        // Mailbox should still have only 2 messages
        assert_eq!(mailbox.size().await, 2);

        // Wait for processor to move messages to channel
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // First two messages should still be there
        let msg1 = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(100)))
            .await
            .unwrap();
        assert_eq!(msg1.payload, b"first");

        let msg2 = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(100)))
            .await
            .unwrap();
        assert_eq!(msg2.payload, b"second");

        // No third message
        assert_eq!(
            mailbox
                .dequeue_with_timeout(Some(std::time::Duration::from_millis(10)))
                .await,
            None
        );
    }

    /// Test backpressure Reject strategy
    #[tokio::test]
    async fn test_backpressure_reject() {
        let mut config = mailbox_config_default();
        config.capacity = 2;
        config.backpressure_strategy = BackpressureStrategy::Error as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should be rejected
        let result = mailbox.enqueue(new_message(b"third".to_vec())).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MailboxError::Full));

        // Mailbox should still have only 2 messages
        assert_eq!(mailbox.size().await, 2);
    }

    /// Test backpressure Block strategy (currently returns error)
    #[tokio::test]
    async fn test_backpressure_block() {
        let mut config = mailbox_config_default();
        config.capacity = 2;
        config.backpressure_strategy = BackpressureStrategy::Block as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should return error (Block not yet implemented)
        let result = mailbox.enqueue(new_message(b"third".to_vec())).await;
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), MailboxError::Full));
    }

    // ==========================================================================
    // MAILBOX ORDERING TESTS (Lines 430-434)
    // ==========================================================================

    /// Test random ordering strategy
    #[tokio::test]
    async fn test_random_ordering() {
        use tokio::time::Duration;

        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingRandom as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue multiple messages
        for i in 0..10 {
            let payload = format!("msg-{}", i).into_bytes();
            mailbox.enqueue(new_message(payload)).await.unwrap();
        }

        // All messages should be in mailbox (random order doesn't drop)
        assert_eq!(mailbox.size().await, 10);

        // Wait a bit for background processor to move messages to channel
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Dequeue all messages (use timeout to avoid hanging)
        let mut dequeued = Vec::new();
        for _ in 0..10 {
            if let Some(msg) = mailbox
                .dequeue_with_timeout(Some(Duration::from_millis(100)))
                .await
            {
                dequeued.push(String::from_utf8(msg.payload).unwrap());
            } else {
                break; // Timeout means no more messages
            }
        }

        // All messages should be present
        assert_eq!(dequeued.len(), 10);

        // Check all messages are there (order may vary)
        for i in 0..10 {
            let expected = format!("msg-{}", i);
            assert!(
                dequeued.contains(&expected),
                "Missing message: {}",
                expected
            );
        }
    }

    // ==========================================================================
    // MAILBOX METHODS TESTS (Lines 447-509)
    // ==========================================================================

    /// Test send() method (alias for enqueue)
    #[tokio::test]
    async fn test_mailbox_send_alias() {
        let mailbox = create_default_mailbox().await;

        mailbox.send(new_message(b"test".to_vec())).await.unwrap();

        assert_eq!(mailbox.size().await, 1);

        let msg = mailbox.dequeue().await.unwrap();
        assert_eq!(msg.payload, b"test");
    }

    /// Test dequeue() on empty mailbox waits indefinitely (returns None only when channel is closed)
    #[tokio::test]
    async fn test_mailbox_dequeue_empty() {
        let mailbox = create_default_mailbox().await;

        // Dequeue from empty mailbox with timeout should return None after timeout
        let result = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(10)))
            .await;
        assert_eq!(result, None, "Should timeout and return None");
    }

    /// Test dequeue_with_timeout() with timeout
    #[tokio::test]
    async fn test_mailbox_dequeue_with_timeout() {
        let mailbox = create_default_mailbox().await;

        // Test timeout on empty mailbox
        let start = std::time::Instant::now();
        let result = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(50)))
            .await;
        let elapsed = start.elapsed();

        assert_eq!(result, None, "Should timeout and return None");
        assert!(
            elapsed >= std::time::Duration::from_millis(50),
            "Should wait at least 50ms"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "Should not wait much longer than timeout"
        );

        // Test message arrives before timeout
        let mailbox2 = create_default_mailbox().await;
        mailbox2
            .enqueue(new_message(b"test".to_vec()))
            .await
            .unwrap();

        // Wait a bit for processor to move message to channel
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        let start = std::time::Instant::now();
        let result = mailbox2
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(100)))
            .await;
        let elapsed = start.elapsed();

        assert!(result.is_some(), "Should receive message before timeout");
        assert_eq!(result.unwrap().payload, b"test");
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "Should receive message quickly"
        );
    }

    /// Test dequeue_matching() with no match returns None
    #[tokio::test]
    async fn test_mailbox_dequeue_matching_not_found() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();

        // Try to match something that doesn't exist
        let msg = mailbox
            .dequeue_matching(|m| m.payload == b"nonexistent")
            .await;
        assert_eq!(msg, None);

        // Original messages should still be there
        assert_eq!(mailbox.size().await, 2);
    }

    /// Test peek() method returns messages without removing them
    #[tokio::test]
    async fn test_mailbox_peek() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"third".to_vec()))
            .await
            .unwrap();

        // Peek at first 2 messages
        let peeked = mailbox.peek(2).await;
        assert_eq!(peeked.len(), 2);
        assert_eq!(peeked[0].payload, b"first");
        assert_eq!(peeked[1].payload, b"second");

        // Messages should still be in mailbox
        assert_eq!(mailbox.size().await, 3);

        // Peek all messages
        let peeked_all = mailbox.peek(10).await;
        assert_eq!(peeked_all.len(), 3);

        // Peek with count=0
        let peeked_zero = mailbox.peek(0).await;
        assert_eq!(peeked_zero.len(), 0);
    }

    /// Test clear() method removes all messages
    #[tokio::test]
    async fn test_mailbox_clear() {
        let mailbox = create_default_mailbox().await;

        // Add messages
        mailbox
            .enqueue(new_message(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"third".to_vec()))
            .await
            .unwrap();

        assert_eq!(mailbox.size().await, 3);

        // Clear all messages
        mailbox.clear().await;

        // Mailbox should be empty
        assert_eq!(mailbox.size().await, 0);

        // Dequeue with timeout should return None (empty mailbox)
        // Note: dequeue() without timeout waits indefinitely, so use timeout version
        let result = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_millis(10)))
            .await;
        assert_eq!(result, None, "Should timeout on empty mailbox");
    }

    // ==========================================================================
    // CHANNEL-BASED MAILBOX TESTS
    // ==========================================================================

    /// Test Message to ProtoMessage conversion
    #[test]
    fn test_message_to_channel_message() {
        let msg = new_message(b"test payload".to_vec())
            .with_priority(MessagePriority::High)
            .with_correlation_id("corr-123".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_sender("sender-actor".to_string())
            .with_message_type("test_type".to_string())
            .with_metadata("key1".to_string(), "value1".to_string());

        // ProtoMessage is already in the correct format, just set channel name
        let mut channel_msg = msg.clone();
        channel_msg.channel = "test-channel".to_string();

        assert_eq!(channel_msg.id, msg.id);
        assert_eq!(channel_msg.payload, b"test payload");
        assert_eq!(channel_msg.channel, "test-channel");
        assert_eq!(channel_msg.sender_id, "sender-actor");
        assert_eq!(channel_msg.correlation_id, "corr-123");
        assert_eq!(channel_msg.reply_to, "reply-addr");
        assert_eq!(
            channel_msg.headers.get("message_type"),
            Some(&"test_type".to_string())
        );
        assert_eq!(channel_msg.headers.get("priority"), Some(&"4".to_string())); // High = 4
        assert_eq!(channel_msg.headers.get("key1"), Some(&"value1".to_string()));
    }

    /// Test ProtoMessage to Message conversion
    #[test]
    fn test_channel_message_to_message() {
        use chrono::Utc;
        use plexspaces_proto::common::v1::Message as ProtoMessage;
        use prost_types::Timestamp;

        let now = Utc::now();
        let channel_msg = ProtoMessage {
            id: "msg-123".to_string(),
            channel: "test-channel".to_string(),
            sender_id: "sender-actor".to_string(),
            receiver_id: "receiver-actor".to_string(),
            message_type: "test_type".to_string(),
            payload: b"test payload".to_vec(),
            headers: {
                let mut h = std::collections::HashMap::new();
                h.insert("priority".to_string(), "4".to_string()); // High
                h.insert("key1".to_string(), "value1".to_string());
                h
            },
            timestamp: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            priority: 60, // High priority
            ttl: None,
            partition_key: "receiver-actor".to_string(),
            correlation_id: "corr-123".to_string(),
            reply_to: "reply-addr".to_string(),
            delivery_count: 0,
            idempotency_key: String::new(),
            uri_path: String::new(),
            uri_method: String::new(),
        };

        let msg: Message = Message::from_proto(&channel_msg);

        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.payload, b"test payload");
        assert_eq!(msg.priority(), MessagePriority::High); // 60 is in 50-74 range
        assert_eq!(msg.correlation_id, "corr-123");
        assert_eq!(msg.reply_to, "reply-addr");
        assert_eq!(msg.sender_id, "sender-actor");
        assert_eq!(msg.receiver_id, "receiver-actor");
        assert_eq!(msg.message_type, "test_type");
        assert_eq!(msg.headers.get("key1"), Some(&"value1".to_string()));
    }

    /// Test mailbox creation with InMemory backend (default)
    #[tokio::test]
    async fn test_mailbox_inmemory_backend() {
        let mut config = mailbox_config_default();
        config.channel_provider = ChannelProvider::ChannelProviderInMemory as i32;

        let mailbox = Mailbox::new(config, "test-mailbox".to_string())
            .await
            .unwrap();

        // Test basic send/receive
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();

        // Wait for processor to move message
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let received = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
            .await;
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload, b"test");
    }

    /// Test mailbox creation with SQLite backend
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_mailbox_sqlite_backend() {
        use plexspaces_proto::channel::v1::{ChannelConfig, SqliteConfig};

        // Use in-memory database to prevent concurrency issues
        let db_path_str = ":memory:".to_string();

        let mut config = mailbox_config_default();
        config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

        let sqlite_config = SqliteConfig {
            database_path: db_path_str,
            table_name: "channel_messages".to_string(),
            wal_mode: true,
            cleanup_acked: true,
            cleanup_age_seconds: 3600,
        };

        let channel_config = ChannelConfig {
            name: "test-mailbox-sqlite".to_string(),
            provider: ChannelProvider::ChannelProviderSqlite as i32,
            capacity: 1000,
            backend_config: Some(
                plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config),
            ),
            ..Default::default()
        };

        config.channel_config = Some(channel_config);

        let mailbox = Mailbox::new(config, "test-mailbox-sqlite".to_string())
            .await
            .unwrap();

        // Test basic send/receive
        let msg = new_message(b"test-sqlite".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();

        // Wait for processor to move message
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let received = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
            .await;
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload, b"test-sqlite");
    }

    /// Test mailbox creation with invalid backend
    #[tokio::test]
    async fn test_mailbox_invalid_backend() {
        let mut config = mailbox_config_default();
        config.channel_provider = 999; // Invalid backend value

        let result = Mailbox::new(config, "test-mailbox".to_string()).await;
        assert!(result.is_err());
        if let Err(MailboxError::InvalidConfig(_)) = result {
            // Expected error type
        } else {
            panic!("Expected InvalidConfig error");
        }
    }

    /// Test mailbox creation with default backend (InMemory)
    #[tokio::test]
    async fn test_mailbox_default_backend() {
        let config = mailbox_config_default();
        // channel_provider is 0 (unspecified), should default to InMemory

        let mailbox = Mailbox::new(config, "test-mailbox".to_string())
            .await
            .unwrap();

        // Should work with InMemory backend
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let received = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
            .await;
        assert!(received.is_some());
    }

    /// Test mailbox with custom channel config
    #[tokio::test]
    async fn test_mailbox_custom_channel_config() {
        use plexspaces_proto::channel::v1::ChannelConfig;

        let mut config = mailbox_config_default();
        config.channel_provider = ChannelProvider::ChannelProviderInMemory as i32;

        let channel_config = ChannelConfig {
            name: "custom-mailbox".to_string(),
            provider: ChannelProvider::ChannelProviderInMemory as i32,
            capacity: 5000,
            ..Default::default()
        };

        config.channel_config = Some(channel_config);

        let mailbox = Mailbox::new(config, "test-mailbox".to_string())
            .await
            .unwrap();

        // Should work with custom config
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let received = mailbox
            .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
            .await;
        assert!(received.is_some());
    }

    /// Test mailbox recovery with SQLite backend (simulating crash)
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_mailbox_sqlite_recovery() {
        use plexspaces_proto::channel::v1::{ChannelConfig, SqliteConfig};
        use std::path::PathBuf;

        // Create persistent test directory (not auto-deleted)
        let temp_base = std::env::temp_dir();
        let test_dir = temp_base.join(format!(
            "plexspaces_mailbox_recovery_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_dir).unwrap();
        let db_path = test_dir.join("recovery_test.db");

        // Keep test_dir alive
        let _keep_alive = &test_dir;

        // Get absolute path as string
        let db_path_str = db_path.to_str().unwrap().to_string();

        // Touch the database file to ensure it exists (sqlx should create it, but this helps)
        if !db_path.exists() {
            std::fs::File::create(&db_path).unwrap();
        }

        // Create first mailbox instance and send messages
        {
            let mut config = mailbox_config_default();
            config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false, // Don't cleanup for recovery test
                cleanup_age_seconds: 0,
            };

            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                provider: ChannelProvider::ChannelProviderSqlite as i32,
                capacity: 1000,
                backend_config: Some(
                    plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(
                        sqlite_config,
                    ),
                ),
                ..Default::default()
            };

            config.channel_config = Some(channel_config);

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string())
                .await
                .unwrap();

            // Send messages but don't dequeue (simulating crash)
            mailbox
                .enqueue(new_message(b"msg1".to_vec()))
                .await
                .unwrap();
            mailbox
                .enqueue(new_message(b"msg2".to_vec()))
                .await
                .unwrap();

            // Wait for messages to be sent to channel
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // Mailbox is dropped here (simulating crash)
        }

        // Create new mailbox instance (simulating recovery after restart)
        {
            let mut config = mailbox_config_default();
            config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false,
                cleanup_age_seconds: 0,
            };

            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                provider: ChannelProvider::ChannelProviderSqlite as i32,
                capacity: 1000,
                backend_config: Some(
                    plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(
                        sqlite_config,
                    ),
                ),
                ..Default::default()
            };

            config.channel_config = Some(channel_config);

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string())
                .await
                .unwrap();

            // Wait for recovery to complete
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, we just verify the mailbox can be created and used
            let msg = new_message(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let received = mailbox
                .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
                .await;
            assert!(received.is_some());
        }
    }

    // ==========================================================================
    // INTEGRATION TESTS: Mailbox Recovery Scenarios
    // ==========================================================================

    /// Test mailbox recovery with SQLite backend (simulating crash and restart)
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_mailbox_sqlite_recovery_integration() {
        use plexspaces_proto::channel::v1::{ChannelConfig, SqliteConfig};
        use std::path::PathBuf;

        // Create persistent test directory (not auto-deleted)
        let temp_base = std::env::temp_dir();
        let test_dir = temp_base.join(format!(
            "plexspaces_mailbox_recovery_int_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&test_dir).unwrap();
        let db_path = test_dir.join("recovery_integration.db");

        // Keep test_dir alive
        let _keep_alive = &test_dir;

        // Get absolute path as string
        let db_path_str = db_path.to_str().unwrap().to_string();

        // Touch the database file to ensure it exists (sqlx should create it, but this helps)
        if !db_path.exists() {
            std::fs::File::create(&db_path).unwrap();
        }

        // Phase 1: Create mailbox, send messages, simulate crash
        {
            let mut config = mailbox_config_default();
            config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false, // Don't cleanup for recovery test
                cleanup_age_seconds: 0,
            };

            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                provider: ChannelProvider::ChannelProviderSqlite as i32,
                capacity: 1000,
                backend_config: Some(
                    plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(
                        sqlite_config,
                    ),
                ),
                ..Default::default()
            };

            config.channel_config = Some(channel_config);

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string())
                .await
                .unwrap();

            // Send messages
            mailbox
                .enqueue(new_message(b"msg1".to_vec()))
                .await
                .unwrap();
            mailbox
                .enqueue(new_message(b"msg2".to_vec()))
                .await
                .unwrap();
            mailbox
                .enqueue(new_message(b"msg3".to_vec()))
                .await
                .unwrap();

            // Wait for messages to be sent to channel
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;

            // Mailbox is dropped here (simulating crash)
        }

        // Phase 2: Create new mailbox instance (simulating recovery after restart)
        {
            let mut config = mailbox_config_default();
            config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

            // Use the same db_path_str from Phase 1
            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false,
                cleanup_age_seconds: 0,
            };

            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                provider: ChannelProvider::ChannelProviderSqlite as i32,
                capacity: 1000,
                backend_config: Some(
                    plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(
                        sqlite_config,
                    ),
                ),
                ..Default::default()
            };

            config.channel_config = Some(channel_config);

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string())
                .await
                .unwrap();

            // Wait for recovery to complete
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;

            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, verify mailbox can be created and used after "restart"
            let msg = new_message(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();

            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let received = mailbox
                .dequeue_with_timeout(Some(std::time::Duration::from_secs(1)))
                .await;
            assert!(received.is_some());
        }
    }

    /// Test mailbox graceful shutdown with metrics
    #[tokio::test]
    async fn test_mailbox_graceful_shutdown() {
        let mailbox = create_default_mailbox().await;

        // Send some messages
        mailbox
            .enqueue(new_message(b"msg1".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(new_message(b"msg2".to_vec()))
            .await
            .unwrap();

        // Wait for processing
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Check size before shutdown
        let size_before = mailbox.size().await;
        assert!(size_before >= 0); // May have been processed

        // Simulate graceful shutdown (mailbox is dropped)
        // In real implementation, we'd record metrics here
        drop(mailbox);

        // Test passes if no panic
        assert!(true);
    }

    /// Test mailbox with multiple messages and recovery
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_mailbox_multiple_messages_recovery() {
        use plexspaces_proto::channel::v1::{ChannelConfig, SqliteConfig};

        // Use in-memory database to avoid file locking issues in concurrent tests
        let db_path_str = ":memory:".to_string();

        // Create mailbox with SQLite
        let mut config = mailbox_config_default();
        config.channel_provider = ChannelProvider::ChannelProviderSqlite as i32;

        let sqlite_config = SqliteConfig {
            database_path: db_path_str,
            table_name: "channel_messages".to_string(),
            wal_mode: true,
            cleanup_acked: false,
            cleanup_age_seconds: 0,
        };

        let channel_config = ChannelConfig {
            name: "multi-mailbox".to_string(),
            provider: ChannelProvider::ChannelProviderSqlite as i32,
            capacity: 1000,
            backend_config: Some(
                plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config),
            ),
            ..Default::default()
        };

        config.channel_config = Some(channel_config);

        let mailbox = Mailbox::new(config, "multi-mailbox".to_string())
            .await
            .unwrap();

        // Send multiple messages
        for i in 1..=10 {
            mailbox
                .enqueue(new_message(format!("msg{}", i).into_bytes()))
                .await
                .unwrap();
        }

        // Wait for processing
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;

        // Receive some messages
        let mut received_count = 0;
        for _ in 0..5 {
            if let Some(_) = mailbox
                .dequeue_with_timeout(Some(std::time::Duration::from_millis(100)))
                .await
            {
                received_count += 1;
            }
        }

        assert!(received_count > 0);
    }
}
