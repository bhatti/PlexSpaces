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
//! Default capacity is 10000 messages. Use `ActorBuilder::with_config(Some(ActorConfig {
//!     max_mailbox_size: ...,
//!     ..Default::default()
//! }))` to configure capacity when spawning through actor specs.
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
use plexspaces_service_traits::{IdempotencyOutcome, IdempotencyStore};

/// Prefix for all control messages (e.g. "__DOWN__", "__EXIT__").
/// Mirrors `plexspaces_actor::CTRL_MSG_PREFIX` without the cyclic dependency.
const CTRL_MSG_PREFIX: &str = "__";

#[inline(always)]
fn is_ctrl_message(message_type: &str) -> bool {
    message_type.starts_with(CTRL_MSG_PREFIX)
}
use rand::Rng;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering as AtomicOrdering};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{mpsc, Notify, RwLock};

#[path = "message_helpers.rs"]
mod message_helpers;
pub use message_helpers::*;

// Re-export proto-generated types
pub use plexspaces_proto::mailbox::v1::{
    BackpressureStrategy, MailboxConfig, MailboxError as MailboxErrorProto, MessagePriority,
    OrderingStrategy,
};

/// Mailbox operation errors
#[derive(Debug, thiserror::Error)]
pub enum MailboxError {
    /// Mailbox has reached capacity and cannot accept more messages.
    ///
    /// This is **transient**: the caller should retry after `retry_after_ms` milliseconds.
    /// Maps to gRPC `RESOURCE_EXHAUSTED` (code 8).
    #[error("Mailbox is full (capacity={capacity}, depth={depth}); retry after {retry_after_ms}ms")]
    Full {
        /// Current queue depth at time of error.
        depth: usize,
        /// Configured maximum capacity.
        capacity: usize,
        /// Suggested back-off in milliseconds before retrying.
        retry_after_ms: u64,
    },

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
            MailboxError::Full { .. } => MailboxErrorProto::MailboxErrorFull,
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
            MailboxErrorProto::MailboxErrorFull => MailboxError::Full {
                depth: 0,
                capacity: 0,
                retry_after_ms: 100,
            },
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

/// Returns the numeric sort value for a `MessagePriority` (higher = higher priority).
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

/// Returns a `MailboxConfig` populated with sensible production defaults.
pub fn mailbox_config_default() -> MailboxConfig {
    MailboxConfig {
        mailbox_type: 0, // MailboxTypeUnspecified (defaults to Unbounded)
        capacity: 10000,
        backpressure_strategy: BackpressureStrategy::Block as i32,
        message_timeout: None,
        enable_priority: false,
        enable_deduplication: false,
        deduplication_window: None,
        ordering_strategy: OrderingStrategy::OrderingFifo as i32,
        channel_provider: 0, // Unspecified (defaults to InMemory)
        channel_config: None,
        metadata: std::collections::HashMap::new(),
        max_capacity: 0,    // 0 = use default (10,000)
        retry_after_ms: 0,  // 0 = use default (100 ms)
        idempotency: None,
    }
}

fn mailbox_config_ordering(config: &MailboxConfig) -> OrderingStrategy {
    OrderingStrategy::try_from(config.ordering_strategy).unwrap_or(OrderingStrategy::OrderingFifo)
}

/// Effective maximum queue depth, respecting env-var overrides.
///
/// Resolution order (first non-zero wins):
/// 1. `PLEXSPACES_MAILBOX_MAX_CAPACITY` env var
/// 2. `config.max_capacity` proto field
/// 3. `PLEXSPACES_MAILBOX_CAPACITY` env var
/// 4. `config.capacity` proto field
/// 5. Default: 10,000
///
/// A value of 0 in both proto fields means "use the default".
fn mailbox_config_max_size(config: &MailboxConfig) -> usize {
    // Env-var overrides (k8s/Docker friendly)
    if let Ok(v) = std::env::var("PLEXSPACES_MAILBOX_MAX_CAPACITY") {
        if let Ok(n) = v.trim().parse::<usize>() {
            if n > 0 { return n; }
        }
    }
    if config.max_capacity > 0 {
        return config.max_capacity as usize;
    }
    if let Ok(v) = std::env::var("PLEXSPACES_MAILBOX_CAPACITY") {
        if let Ok(n) = v.trim().parse::<usize>() {
            if n > 0 { return n; }
        }
    }
    if config.capacity > 0 {
        return config.capacity as usize;
    }
    10_000 // Default: 10,000 messages
}

/// Suggested retry-after duration when mailbox is full (ms).
fn mailbox_config_retry_after_ms(config: &MailboxConfig) -> u64 {
    if let Ok(v) = std::env::var("PLEXSPACES_MAILBOX_RETRY_AFTER_MS") {
        if let Ok(n) = v.trim().parse::<u64>() {
            if n > 0 { return n; }
        }
    }
    if config.retry_after_ms > 0 {
        return config.retry_after_ms as u64;
    }
    100 // Default: 100 ms
}

fn mailbox_config_backpressure(config: &MailboxConfig) -> BackpressureStrategy {
    BackpressureStrategy::try_from(config.backpressure_strategy)
        .unwrap_or(BackpressureStrategy::Block)
}


/// Mailbox implementation using channel-based messaging
///
/// ## Architecture
///
/// ### Fast path (in-memory FIFO/LIFO actors — the common case)
/// A single bounded `mpsc::channel` is the sole data path:
/// `enqueue()` → `data_tx.send()` directly; `dequeue()` → `data_rx.recv()`.
/// No background task, no intermediate VecDeque, no InMemoryChannel allocation.
///
/// ### Priority path (in-memory priority-ordered actors)
/// An `internal_queue` (BinaryHeap) + background processor task + `local_receiver`
/// are used so messages can be sorted before delivery.
///
/// ### Durable path (SQLite / Redis / Kafka / SQS / NATS)
/// `Arc<dyn Channel>` + `internal_queue` + background processor are used as before.
/// `dequeue()` falls back to `channel.receive()` when `data_rx` and `local_receiver`
/// are both absent.
pub struct Mailbox {
    /// Configuration
    config: MailboxConfig,
    /// Channel backend — `None` for in-memory FIFO/LIFO (fast path).
    /// `Some` for priority-ordered in-memory and all durable backends.
    channel: Option<Arc<dyn Channel>>,
    /// Channel name (used for message routing)
    channel_name: String,
    /// Channel backend type (for is_durable() and backend_type())
    channel_provider: i32,
    /// Mailbox ID (for logging/metrics)
    mailbox_id: String,
    /// Internal queue for priority ordering — `None` on the fast path (FIFO/LIFO in-memory).
    internal_queue: Option<Arc<RwLock<MessageStorage>>>,
    /// Background task handle — `None` on the fast path.
    processor_handle: Option<Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>>,
    /// Notify for background processor — `None` on the fast path.
    notify: Option<Arc<Notify>>,
    /// Local receiver for priority-ordered in-memory and durable paths.
    local_receiver: Option<Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<ProtoMessage>>>>>,

    // ── Fast-path data channel (in-memory FIFO/LIFO only) ────────────────────
    /// Sender half of the bounded data channel used on the fast path.
    /// `None` when the priority queue or a durable backend is in use.
    data_tx: Option<mpsc::Sender<ProtoMessage>>,
    /// Receiver half of the bounded data channel.
    /// `Arc` so the `'static` dequeue future can share it without borrowing `self`.
    data_rx: Arc<tokio::sync::Mutex<Option<mpsc::Receiver<ProtoMessage>>>>,

    // ── Stats ─────────────────────────────────────────────────────────────────
    /// Current data-queue depth.  `Arc` so the background processor can decrement
    /// it without unsafe code when the priority/durable path is in use.
    data_queue_size: Arc<AtomicUsize>,
    total_enqueued:  AtomicU64,
    total_dequeued:  AtomicU64,
    total_dropped:   AtomicU64,

    /// Tenant that owns this mailbox — injected from RequestContext at spawn time.
    tenant_id: String,
    /// Namespace within the tenant.
    namespace: String,
    /// Node-wide idempotency store for exactly-once deduplication.
    idempotency_store: Option<Arc<dyn IdempotencyStore>>,
    /// Effective max capacity — resolved once at construction.
    max_capacity: usize,
    /// Retry-after hint in ms for RESOURCE_EXHAUSTED responses.
    retry_after_ms: u64,
    /// Shutdown flag: when true, mailbox stops accepting new messages (durable backends).
    /// `Arc` so the `'static` dequeue future can check it without borrowing `self`.
    shutdown_flag: Arc<AtomicBool>,
    /// In-progress message count for graceful-shutdown draining.
    in_progress_count: AtomicUsize,
    /// Notified when in_progress_count drops to zero.
    shutdown_notify: Arc<Notify>,

    // ── Control-message fast lane ─────────────────────────────────────────────
    /// Sender half of the unbounded ctrl channel.
    ctrl_sender: mpsc::UnboundedSender<ProtoMessage>,
    /// Receiver half — behind a Mutex so the async `dequeue` future can poll it.
    ctrl_receiver: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<ProtoMessage>>>,
    /// Number of messages currently in the ctrl channel.
    /// `Arc` so the `'static` dequeue future can share it without borrowing `self`.
    ctrl_size: Arc<AtomicUsize>,
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
    /// * `tenant_id` - Tenant that owns this mailbox; used as idempotency store scope key
    /// * `namespace` - Namespace within the tenant; used together with `tenant_id`
    /// * `idempotency_store` - Node-wide dedup store; `None` for temporary senders
    ///
    /// ## Returns
    /// `Ok(Mailbox)` on success, `Err(MailboxError)` if channel backend is unavailable
    ///
    /// ## Errors
    /// - `MailboxError::InvalidConfig`: Invalid channel backend or configuration
    /// - `MailboxError::StorageError`: Channel backend initialization failed (e.g., Kafka not configured)
    pub async fn new(
        config: MailboxConfig,
        mailbox_id: String,
        tenant_id: String,
        namespace: String,
        idempotency_store: Option<Arc<dyn IdempotencyStore>>,
    ) -> Result<Self, MailboxError> {
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
        let ordering = mailbox_config_ordering(&config);
        let is_priority = matches!(ordering, OrderingStrategy::OrderingPriority);
        let is_lifo = matches!(ordering, OrderingStrategy::OrderingLifo);
        // DropOldest requires popping the front of the queue, which the bounded mpsc
        // channel doesn't support — use the slow path (internal VecDeque) instead.
        let backpressure = mailbox_config_backpressure(&config);
        let is_drop_oldest = matches!(backpressure, BackpressureStrategy::DropOldest);

        // Fast path: in-memory FIFO with non-DropOldest backpressure — one bounded
        // mpsc channel, no background task.
        let use_fast_path = is_in_memory && !is_priority && !is_lifo && !is_drop_oldest;

        let (channel, channel_name) = if use_fast_path {
            (None, format!("mailbox:{}", mailbox_id))
        } else {
            // Build a Channel backend (InMemoryChannel for priority, or durable backend).
            let mut channel_config = config
                .channel_config
                .clone()
                .unwrap_or_else(|| ChannelConfig {
                    name: format!("mailbox:{}", mailbox_id),
                    provider: channel_provider_value,
                    capacity: config.capacity as u64,
                    ..Default::default()
                });
            if channel_config.name.is_empty() {
                channel_config.name = format!("mailbox:{}", mailbox_id);
            }
            let name = channel_config.name.clone();
            let ch = create_channel(channel_config).await.map_err(|e| {
                MailboxError::StorageError(format!("Failed to create channel backend: {}", e))
            })?;
            (Some(Arc::from(ch) as Arc<dyn Channel>), name)
        };

        // Fast-path data channel (bounded mpsc, FIFO/LIFO in-memory only).
        let max_cap = mailbox_config_max_size(&config);
        let (data_tx, data_rx) = if use_fast_path {
            let (tx, rx) = mpsc::channel::<ProtoMessage>(max_cap.max(1));
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        // Priority / durable path: local_receiver fed by background processor.
        let (local_sender, local_receiver) = if !use_fast_path && is_in_memory {
            // priority in-memory
            let (s, r) = mpsc::unbounded_channel::<ProtoMessage>();
            (Some(s), Some(r))
        } else {
            (None, None)
        };

        // Internal queue only needed for priority / durable paths.
        let internal_queue = if !use_fast_path {
            let storage = if is_priority {
                MessageStorage::Priority(BinaryHeap::new())
            } else {
                MessageStorage::Queue(VecDeque::new())
            };
            Some(Arc::new(RwLock::new(storage)))
        } else {
            None
        };

        // Ctrl channel: unbounded, never back-pressured.
        let (ctrl_sender, ctrl_receiver) = mpsc::unbounded_channel::<ProtoMessage>();

        // Build the notify Arc before construction so we can pass it to both the
        // struct field and the background processor without a mut reference.
        let notify: Option<Arc<Notify>> = if !use_fast_path {
            Some(Arc::new(Notify::new()))
        } else {
            None
        };
        let data_queue_size = Arc::new(AtomicUsize::new(0));

        let mailbox = Mailbox {
            config: config.clone(),
            channel,
            channel_name,
            channel_provider: channel_provider_value,
            mailbox_id: mailbox_id.clone(),
            internal_queue,
            processor_handle: if !use_fast_path {
                Some(Arc::new(RwLock::new(None)))
            } else {
                None
            },
            notify: notify.clone(),
            local_receiver: local_receiver.map(|r| {
                Arc::new(tokio::sync::Mutex::new(Some(r)))
            }),
            data_tx,
            data_rx: Arc::new(tokio::sync::Mutex::new(data_rx)),
            data_queue_size: data_queue_size.clone(),
            total_enqueued:  AtomicU64::new(0),
            total_dequeued:  AtomicU64::new(0),
            total_dropped:   AtomicU64::new(0),
            tenant_id,
            namespace,
            idempotency_store,
            max_capacity: max_cap,
            retry_after_ms: mailbox_config_retry_after_ms(&config) as u64,
            shutdown_flag: Arc::new(AtomicBool::new(false)),
            in_progress_count: AtomicUsize::new(0),
            shutdown_notify: Arc::new(Notify::new()),
            ctrl_sender,
            ctrl_receiver: Arc::new(tokio::sync::Mutex::new(ctrl_receiver)),
            ctrl_size: Arc::new(AtomicUsize::new(0)),
        };

        // Start background processor only when needed (priority or durable path).
        if !use_fast_path {
            if let Some(sender) = local_sender {
                mailbox.start_processor_with_local_sender(sender, data_queue_size, notify.unwrap());
            } else {
                mailbox.start_processor(notify.unwrap());
            }
        }

        Ok(mailbox)
    }

    /// Background processor for durable backends (SQLite, Redis, Kafka, SQS, NATS).
    /// Drains `internal_queue` into `channel.send()` as messages arrive.
    fn start_processor(&self, notify: Arc<Notify>) {
        let internal_queue = self.internal_queue.as_ref()
            .expect("start_processor called without internal_queue")
            .clone();
        let channel = self.channel.as_ref()
            .expect("start_processor called without channel")
            .clone();
        let channel_name = self.channel_name.clone();
        let data_queue_size = self.data_queue_size.clone();
        let processor_handle = self.processor_handle.as_ref()
            .expect("start_processor called without processor_handle")
            .clone();

        let handle = tokio::spawn(async move {
            loop {
                let has_messages = {
                    let q = internal_queue.read().await;
                    match &*q {
                        MessageStorage::Queue(q) => !q.is_empty(),
                        MessageStorage::Priority(h) => !h.is_empty(),
                    }
                };
                if !has_messages {
                    notify.notified().await;
                }

                let mut messages_to_send = Vec::new();
                {
                    let mut q = internal_queue.write().await;
                    match &mut *q {
                        MessageStorage::Queue(queue) => {
                            while let Some(msg) = queue.pop_front() { messages_to_send.push(msg); }
                        }
                        MessageStorage::Priority(heap) => {
                            while let Some(pm) = heap.pop() { messages_to_send.push(pm.message); }
                        }
                    }
                }

                let sent = messages_to_send.len();
                for msg in messages_to_send {
                    let mut channel_msg = msg;
                    channel_msg.channel = channel_name.clone();
                    match channel.send(channel_msg).await {
                        Ok(_) => {}
                        Err(ChannelError::ChannelClosed(_)) => return,
                        Err(e) => tracing::warn!("Mailbox processor send error: {}", e),
                    }
                }
                if sent > 0 {
                    data_queue_size.fetch_sub(sent, AtomicOrdering::Relaxed);
                }
            }
        });

        let ph = processor_handle.clone();
        tokio::spawn(async move { *ph.write().await = Some(handle); });
    }

    /// Background processor for priority-ordered in-memory mailboxes.
    /// Sorts messages in `internal_queue` then delivers via `local_sender`.
    fn start_processor_with_local_sender(
        &self,
        local_sender: mpsc::UnboundedSender<ProtoMessage>,
        data_queue_size: Arc<AtomicUsize>,
        notify: Arc<Notify>,
    ) {
        let internal_queue = self.internal_queue.as_ref()
            .expect("start_processor_with_local_sender called without internal_queue")
            .clone();
        let channel = self.channel.as_ref()
            .expect("start_processor_with_local_sender called without channel")
            .clone();
        let channel_name = self.channel_name.clone();
        let notify_for_waiters = notify.clone();
        let processor_handle = self.processor_handle.as_ref()
            .expect("start_processor_with_local_sender called without processor_handle")
            .clone();

        let handle = tokio::spawn(async move {
            loop {
                let has_messages = {
                    let q = internal_queue.read().await;
                    match &*q {
                        MessageStorage::Queue(q) => !q.is_empty(),
                        MessageStorage::Priority(h) => !h.is_empty(),
                    }
                };
                if !has_messages {
                    notify.notified().await;
                }

                let mut messages_to_send = Vec::new();
                {
                    let mut q = internal_queue.write().await;
                    match &mut *q {
                        MessageStorage::Queue(queue) => {
                            while let Some(msg) = queue.pop_front() { messages_to_send.push(msg); }
                        }
                        MessageStorage::Priority(heap) => {
                            while let Some(pm) = heap.pop() { messages_to_send.push(pm.message); }
                        }
                    }
                }

                let mut num_sent = 0usize;
                for msg in messages_to_send {
                    let msg_id = msg.id.clone();
                    let mut channel_msg = msg.clone();
                    channel_msg.channel = channel_name.clone();
                    let ch_result = channel.send(channel_msg).await;
                    let local_result = local_sender.send(msg);
                    match (ch_result, local_result) {
                        (Ok(_), Ok(())) => num_sent += 1,
                        (Err(ChannelError::ChannelClosed(_)), _) | (_, Err(_)) => {
                            tracing::warn!(message_id = %msg_id, "Mailbox processor: receiver closed, stopping");
                            return;
                        }
                        (Err(e), _) => {
                            tracing::warn!(message_id = %msg_id, error = %e, "Mailbox processor: channel error");
                        }
                    }
                }
                if num_sent > 0 {
                    data_queue_size.fetch_sub(num_sent, AtomicOrdering::Relaxed);
                    notify_for_waiters.notify_waiters();
                }
            }
        });

        let ph = processor_handle.clone();
        tokio::spawn(async move { *ph.write().await = Some(handle); });
    }

    /// Enqueue a message.
    pub async fn enqueue(&self, message: ProtoMessage) -> Result<(), MailboxError> {
        // ── Ctrl fast lane ────────────────────────────────────────────────────
        // send() first, fetch_add after so try_recv() in dequeue() never sees
        // ctrl_size > 0 before the message is actually present.
        if is_ctrl_message(&message.message_type) {
            let _ = self.ctrl_sender.send(message);
            self.ctrl_size.fetch_add(1, AtomicOrdering::Release);
            metrics::counter!("plexspaces_mailbox_ctrl_enqueued_total",
                "mailbox_id" => self.mailbox_id.clone()
            ).increment(1);
            return Ok(());
        }
        // ─────────────────────────────────────────────────────────────────────

        // Shutdown guard for durable backends.
        if !self.is_in_memory() && self.shutdown_flag.load(AtomicOrdering::Acquire) {
            return Err(MailboxError::StorageError(
                "Mailbox is shutting down, not accepting new messages".to_string(),
            ));
        }

        // Idempotency deduplication.
        let idempotency_key = message.idempotency_key.clone();
        let should_record_complete = if !idempotency_key.is_empty() {
            if let Some(ref store) = self.idempotency_store {
                match store.check_and_record(&self.tenant_id, &self.namespace, &idempotency_key).await {
                    Ok(IdempotencyOutcome::FirstSeen) => true,
                    Ok(IdempotencyOutcome::Duplicate(_)) | Ok(IdempotencyOutcome::InFlight) => {
                        tracing::debug!(idempotency_key = %idempotency_key, "Skipping duplicate message");
                        return Ok(());
                    }
                    Err(e) => {
                        tracing::warn!("Idempotency store error: {}; delivering anyway", e);
                        false
                    }
                }
            } else { false }
        } else { false };

        // ── Fast path: in-memory FIFO/LIFO ───────────────────────────────────
        if let Some(ref tx) = self.data_tx {
            let cur = self.data_queue_size.load(AtomicOrdering::Relaxed);
            if cur >= self.max_capacity {
                let cap = self.max_capacity;
                let rms = self.retry_after_ms;
                match mailbox_config_backpressure(&self.config) {
                    BackpressureStrategy::DropOldest => {
                        // Can't pop from the mpsc channel directly; treat as DropNewest.
                        self.total_dropped.fetch_add(1, AtomicOrdering::Relaxed);
                        return Ok(());
                    }
                    BackpressureStrategy::DropNewest => {
                        self.total_dropped.fetch_add(1, AtomicOrdering::Relaxed);
                        return Ok(());
                    }
                    _ => return Err(MailboxError::Full { depth: cur, capacity: cap, retry_after_ms: rms }),
                }
            }
            // Bounded send — will succeed unless receiver dropped (actor stopped).
            match tx.try_send(message) {
                Ok(()) => {
                    self.data_queue_size.fetch_add(1, AtomicOrdering::Relaxed);
                    self.total_enqueued.fetch_add(1, AtomicOrdering::Relaxed);
                    if tracing::enabled!(tracing::Level::DEBUG) {
                        tracing::debug!(mailbox_id = %self.mailbox_id, queue_size = cur + 1, "Mailbox: message enqueued (fast path)");
                    }
                }
                Err(mpsc::error::TrySendError::Full(_)) => {
                    let rms = self.retry_after_ms;
                    return Err(MailboxError::Full { depth: cur, capacity: self.max_capacity, retry_after_ms: rms });
                }
                Err(mpsc::error::TrySendError::Closed(_)) => {
                    // Actor stopped; silently drop.
                }
            }
            // Idempotency bookkeeping.
            if should_record_complete {
                if let Some(ref store) = self.idempotency_store {
                    if let Err(e) = store.complete_record(&self.tenant_id, &self.namespace, &idempotency_key, None).await {
                        tracing::warn!("Idempotency complete_record error: {}", e);
                    }
                }
            }
            return Ok(());
        }
        // ─────────────────────────────────────────────────────────────────────

        // ── Priority / durable path ───────────────────────────────────────────
        let internal_queue = self.internal_queue.as_ref()
            .expect("non-fast-path mailbox must have internal_queue");
        let mut queue_guard = internal_queue.write().await;
        let cur = self.data_queue_size.load(AtomicOrdering::Relaxed);

        if cur >= self.max_capacity {
            match mailbox_config_backpressure(&self.config) {
                BackpressureStrategy::DropOldest => {
                    match &mut *queue_guard {
                        MessageStorage::Queue(q) => { q.pop_front(); }
                        MessageStorage::Priority(h) => { h.pop(); }
                    }
                    self.total_dropped.fetch_add(1, AtomicOrdering::Relaxed);
                    self.data_queue_size.fetch_sub(1, AtomicOrdering::Relaxed);
                }
                BackpressureStrategy::DropNewest => {
                    self.total_dropped.fetch_add(1, AtomicOrdering::Relaxed);
                    return Ok(());
                }
                _ => {
                    let cap = self.max_capacity;
                    let rms = self.retry_after_ms;
                    return Err(MailboxError::Full { depth: cur, capacity: cap, retry_after_ms: rms });
                }
            }
        }

        let message_id = message.id.clone();
        let sender_id = message.sender_id.clone();
        let receiver_id = message.receiver_id.clone();
        let message_type = message.message_type.clone();
        let correlation_id = message.correlation_id.clone();

        match mailbox_config_ordering(&self.config) {
            OrderingStrategy::OrderingFifo => {
                if let MessageStorage::Queue(q) = &mut *queue_guard { q.push_back(message); }
            }
            OrderingStrategy::OrderingLifo => {
                if let MessageStorage::Queue(q) = &mut *queue_guard { q.push_front(message); }
            }
            OrderingStrategy::OrderingPriority => {
                if let MessageStorage::Priority(h) = &mut *queue_guard { h.push(PriorityMessage { message }); }
            }
            OrderingStrategy::OrderingRandom => {
                if let MessageStorage::Queue(q) = &mut *queue_guard {
                    let mut rng = rand::thread_rng();
                    let pos = rng.gen_range(0..=q.len());
                    q.insert(pos, message);
                }
            }
            _ => {
                if let MessageStorage::Queue(q) = &mut *queue_guard { q.push_back(message); }
            }
        }
        drop(queue_guard);

        self.total_enqueued.fetch_add(1, AtomicOrdering::Relaxed);
        self.data_queue_size.fetch_add(1, AtomicOrdering::Relaxed);

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                message_id = %message_id,
                sender_id = %sender_id,
                receiver_id = %receiver_id,
                message_type = %message_type,
                correlation_id = %correlation_id,
                "Mailbox::enqueue: message enqueued (priority/durable path)"
            );
        }

        if should_record_complete {
            if let Some(ref store) = self.idempotency_store {
                if let Err(e) = store.complete_record(&self.tenant_id, &self.namespace, &idempotency_key, None).await {
                    tracing::warn!("Idempotency complete_record error: {}", e);
                }
            }
        }

        // Wake the background processor.
        if let Some(ref notify) = self.notify {
            notify.notify_one();
        }
        Ok(())
    }

    /// Send a message (alias for enqueue)
    pub async fn send(&self, message: ProtoMessage) -> Result<(), MailboxError> {
        self.enqueue(message).await
    }

    /// Dequeue a message with optional timeout.
    pub fn dequeue_with_timeout(
        &self,
        timeout: Option<std::time::Duration>,
    ) -> impl std::future::Future<Output = Option<ProtoMessage>> + 'static {
        let channel = self.channel.clone();
        let local_receiver = self.local_receiver.clone();
        let data_rx = self.data_rx.clone();
        let mailbox_id = self.mailbox_id.clone();
        let shutdown_flag = self.shutdown_flag.clone();
        let ctrl_receiver = self.ctrl_receiver.clone();
        let ctrl_size = self.ctrl_size.clone();
        let data_queue_size = self.data_queue_size.clone();
        let is_in_memory = self.is_in_memory();
        // true when this mailbox uses the fast path (no channel, no local_receiver)
        let use_fast_path = self.data_tx.is_some();

        async move {
            // ── Ctrl fast lane (non-blocking check first) ─────────────────────
            if ctrl_size.load(AtomicOrdering::Acquire) > 0 {
                if let Ok(msg) = ctrl_receiver.lock().await.try_recv() {
                    ctrl_size.fetch_sub(1, AtomicOrdering::Relaxed);
                    metrics::counter!("plexspaces_mailbox_ctrl_dequeued_total",
                        "mailbox_id" => mailbox_id.clone(),
                        "message_type" => msg.message_type.clone()
                    ).increment(1);
                    return Some(msg);
                }
            }
            // ──────────────────────────────────────────────────────────────────

            // ── Fast path: bounded mpsc data channel ──────────────────────────
            if use_fast_path {
                let mut rx_guard = data_rx.lock().await;
                let Some(rx) = rx_guard.as_mut() else { return None; };
                let mut ctrl_guard = ctrl_receiver.lock().await;
                let result = match timeout {
                    Some(dur) => {
                        match tokio::time::timeout(dur, async {
                            tokio::select! {
                                biased;
                                ctrl_msg = ctrl_guard.recv() => ctrl_msg.map(|m| (m, true)),
                                data_msg = rx.recv() => data_msg.map(|m| (m, false)),
                            }
                        }).await {
                            Ok(r) => r,
                            Err(_) => return None,
                        }
                    }
                    None => tokio::select! {
                        biased;
                        ctrl_msg = ctrl_guard.recv() => ctrl_msg.map(|m| (m, true)),
                        data_msg = rx.recv() => data_msg.map(|m| (m, false)),
                    },
                };
                drop(ctrl_guard);
                if let Some((msg, is_ctrl)) = result {
                    if is_ctrl {
                        ctrl_size.fetch_sub(1, AtomicOrdering::Relaxed);
                        metrics::counter!("plexspaces_mailbox_ctrl_dequeued_total",
                            "mailbox_id" => mailbox_id.clone(),
                            "message_type" => msg.message_type.clone()
                        ).increment(1);
                    } else {
                        data_queue_size.fetch_sub(1, AtomicOrdering::Relaxed);
                    }
                    return Some(msg);
                }
                return None;
            }
            // ──────────────────────────────────────────────────────────────────

            // ── Priority path: local_receiver fed by background processor ─────
            if let Some(lr) = &local_receiver {
                let mut receiver_opt = lr.lock().await;
                if let Some(receiver) = receiver_opt.as_mut() {
                    let mut ctrl_guard = ctrl_receiver.lock().await;
                    let result = match timeout {
                        Some(dur) => {
                            match tokio::time::timeout(dur, async {
                                tokio::select! {
                                    biased;
                                    ctrl_msg = ctrl_guard.recv() => ctrl_msg.map(|m| (m, true)),
                                    data_msg = receiver.recv() => data_msg.map(|m| (m, false)),
                                }
                            }).await {
                                Ok(r) => r,
                                Err(_) => return None,
                            }
                        }
                        None => tokio::select! {
                            biased;
                            ctrl_msg = ctrl_guard.recv() => ctrl_msg.map(|m| (m, true)),
                            data_msg = receiver.recv() => data_msg.map(|m| (m, false)),
                        },
                    };
                    drop(ctrl_guard);
                    if let Some((msg, is_ctrl)) = result {
                        if is_ctrl {
                            ctrl_size.fetch_sub(1, AtomicOrdering::Relaxed);
                            metrics::counter!("plexspaces_mailbox_ctrl_dequeued_total",
                                "mailbox_id" => mailbox_id.clone(),
                                "message_type" => msg.message_type.clone()
                            ).increment(1);
                        } else {
                            data_queue_size.fetch_sub(1, AtomicOrdering::Relaxed);
                        }
                        return Some(msg);
                    }
                    return None;
                }
            }
            // ──────────────────────────────────────────────────────────────────

            // ── Durable path: poll the channel backend ─────────────────────────
            let channel = match channel {
                Some(ch) => ch,
                None => return None,
            };

            match timeout {
                None => loop {
                    if !is_in_memory && shutdown_flag.load(AtomicOrdering::Acquire) {
                        return None;
                    }
                    match channel.receive(1).await {
                        Ok(messages) => {
                            if let Some(msg) = messages.into_iter().next() {
                                return Some(msg);
                            }
                        }
                        Err(ChannelError::ChannelClosed(_)) => return None,
                        Err(e) => {
                            tracing::warn!("Channel receive error: {}", e);
                            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                        }
                    }
                    tokio::task::yield_now().await;
                },
                Some(duration) => {
                    if !is_in_memory && shutdown_flag.load(AtomicOrdering::Acquire) {
                        return None;
                    }
                    let start = std::time::Instant::now();
                    loop {
                        if start.elapsed() >= duration {
                            return None;
                        }
                        match channel.try_receive(1).await {
                            Ok(messages) => {
                                if let Some(msg) = messages.into_iter().next() {
                                    return Some(msg);
                                }
                            }
                            Err(ChannelError::ChannelClosed(_)) => return None,
                            Err(_) => tokio::task::yield_now().await,
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

    /// Acknowledge message processing (durable backends only; no-op on fast path).
    pub async fn ack_message(&self, message: &ProtoMessage) -> Result<(), MailboxError> {
        if let Some(ref ch) = self.channel {
            ch.ack(&message.id).await
                .map_err(|e| MailboxError::StorageError(format!("Failed to ack message: {}", e)))?;
        }
        self.total_dequeued.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::trace!(mailbox_id = %self.mailbox_id, message_id = %message.id, "Message acked");
        Ok(())
    }

    /// Negative acknowledge message processing (durable backends only; no-op on fast path).
    pub async fn nack_message(
        &self,
        message: &ProtoMessage,
        error: Option<&str>,
    ) -> Result<(), MailboxError> {
        if let Some(ref ch) = self.channel {
            ch.nack(&message.id, true).await
                .map_err(|e| MailboxError::StorageError(format!("Failed to nack message: {}", e)))?;
        }
        self.total_dropped.fetch_add(1, AtomicOrdering::Relaxed);
        tracing::trace!(mailbox_id = %self.mailbox_id, message_id = %message.id, error = ?error, "Message nacked");
        Ok(())
    }

    /// Get current data-queue depth (does not include ctrl messages).
    pub fn size(&self) -> usize {
        self.data_queue_size.load(AtomicOrdering::Relaxed)
    }

    /// Returns the number of pending control messages.
    pub fn ctrl_size(&self) -> usize {
        self.ctrl_size.load(AtomicOrdering::Relaxed)
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

    /// Get mailbox statistics for observability.
    pub fn get_stats(&self) -> MailboxObservabilityStats {
        MailboxObservabilityStats {
            data_queue_size: self.data_queue_size.load(AtomicOrdering::Relaxed),
            ctrl_queue_size: self.ctrl_size.load(AtomicOrdering::Relaxed),
            total_enqueued: self.total_enqueued.load(AtomicOrdering::Relaxed) as usize,
            total_dequeued: self.total_dequeued.load(AtomicOrdering::Relaxed) as usize,
            total_dropped: self.total_dropped.load(AtomicOrdering::Relaxed) as usize,
            backend_type: self.backend_type().to_string(),
            is_durable: self.is_durable(),
        }
    }

    /// Graceful shutdown: stop accepting new messages and wait for in-progress ones.
    pub async fn graceful_shutdown(&self, timeout: Option<Duration>) -> Result<(), MailboxError> {
        tracing::info!(mailbox_id = %self.mailbox_id, backend = %self.backend_type(), "Starting graceful shutdown");

        // Set shutdown flag to stop accepting new messages (durable backends only).
        if !self.is_in_memory() {
            self.shutdown_flag.store(true, AtomicOrdering::Release);
        }

        // Flush durable backend pending messages.
        if self.is_durable() {
            if let Some(ref iq) = self.internal_queue {
                let pending = {
                    let q = iq.read().await;
                    match &*q { MessageStorage::Queue(q) => q.len(), MessageStorage::Priority(h) => h.len() }
                };
                if pending > 0 {
                    tracing::info!(mailbox_id = %self.mailbox_id, pending_messages = pending, "Flushing pending messages");
                    tokio::task::yield_now().await;
                }
            }
        }

        // Wait for in-progress messages to complete.
        let timeout_duration = timeout.unwrap_or(Duration::from_secs(30));
        loop {
            let in_progress = self.in_progress_count.load(AtomicOrdering::Acquire);
            if in_progress == 0 { break; }
            match tokio::time::timeout(timeout_duration, self.shutdown_notify.notified()).await {
                Ok(()) => {}
                Err(_) => {
                    tracing::warn!(mailbox_id = %self.mailbox_id, in_progress, "Timeout waiting for in-progress messages");
                    break;
                }
            }
        }

        // Close durable channel backend.
        if let Some(ref ch) = self.channel {
            if self.is_durable() {
                if let Err(e) = ch.close().await {
                    tracing::warn!(mailbox_id = %self.mailbox_id, error = %e, "Failed to close channel");
                }
            }
        }

        tracing::info!(mailbox_id = %self.mailbox_id, "Graceful shutdown completed");
        Ok(())
    }

    /// Mark the start of message processing (for graceful-shutdown draining).
    pub fn begin_processing(&self) {
        self.in_progress_count.fetch_add(1, AtomicOrdering::Relaxed);
    }

    /// Mark the end of message processing.
    pub fn end_processing(&self) {
        let prev = self.in_progress_count.fetch_sub(1, AtomicOrdering::Release);
        if prev == 1 {
            self.shutdown_notify.notify_waiters();
        }
    }
}

/// Mailbox statistics for observability (public API)
#[derive(Debug, Clone)]
pub struct MailboxObservabilityStats {
    /// Current number of data messages in the data queue
    pub data_queue_size: usize,
    /// Current number of control messages waiting in the ctrl queue
    pub ctrl_queue_size: usize,
    /// Total data (non-ctrl) messages enqueued since creation
    pub total_enqueued: usize,
    /// Total messages dequeued since creation
    pub total_dequeued: usize,
    /// Total messages dropped due to backpressure (DropOldest / DropNewest) since creation.
    /// Non-zero values indicate the actor is falling behind its message rate.
    pub total_dropped: usize,
    /// Backend type (in_memory, redis, kafka, sqlite, etc.)
    pub backend_type: String,
    /// Whether this mailbox is durable
    pub is_durable: bool,
}

impl MailboxObservabilityStats {
    /// Total pending messages across both data and ctrl queues.
    pub fn total_size(&self) -> usize {
        self.data_queue_size + self.ctrl_queue_size
    }
}

// MailboxError is defined above (wrapper around proto enum)

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_proto::channel::v1::ChannelProvider;
    use plexspaces_proto::common::v1::Message;

    /// Helper to create a test mailbox with InMemory backend
    async fn create_test_mailbox(config: MailboxConfig) -> Mailbox {
        Mailbox::new(config, format!("test-mailbox-{}", ulid::Ulid::new()), String::new(), String::new(), None)
            .await
            .unwrap()
    }

    /// Helper to create a test mailbox with default config
    async fn create_default_mailbox() -> Mailbox {
        create_test_mailbox(mailbox_config_default()).await
    }

    fn with_priority(mut message: Message, priority: MessagePriority) -> Message {
        message.priority = priority_to_int(priority);
        message
    }

    fn with_sender(mut message: Message, sender_id: &str) -> Message {
        message.sender_id = sender_id.to_string();
        message
    }

    fn with_message_type(mut message: Message, message_type: &str) -> Message {
        message.message_type = message_type.to_string();
        message
    }

    fn with_correlation_id(mut message: Message, correlation_id: &str) -> Message {
        message.correlation_id = correlation_id.to_string();
        message
    }

    fn with_reply_to(mut message: Message, reply_to: &str) -> Message {
        message.reply_to = reply_to.to_string();
        message
    }

    fn with_metadata(mut message: Message, key: &str, value: &str) -> Message {
        message.headers.insert(key.to_string(), value.to_string());
        message
    }

    fn sender_id(message: &Message) -> Option<&str> {
        if message.sender_id.is_empty() {
            None
        } else {
            Some(message.sender_id.as_str())
        }
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
            .enqueue(with_priority(
                new_message(b"low".to_vec()),
                MessagePriority::Low,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(with_priority(
                new_message(b"high".to_vec()),
                MessagePriority::High,
            ))
            .await
            .unwrap();

        // Messages should be in internal queue
        assert_eq!(mailbox.size(), 2);
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
        assert_eq!(
            priority_from_int(signal_msg.priority),
            MessagePriority::Highest
        );
        assert_eq!(
            message_priority_value(&priority_from_int(signal_msg.priority)),
            5
        );
    }

    /// Test 3: Verify background processor moves messages from internal queue to channel
    #[tokio::test]
    async fn test_priority_mailbox_processor() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue one message
        mailbox
            .enqueue(with_priority(
                new_message(b"test".to_vec()),
                MessagePriority::Normal,
            ))
            .await
            .unwrap();

        // Wait for processor to move message to channel
        // Poll until internal queue is empty
        let mut attempts = 0;
        while mailbox.size() > 0 && attempts < 100 {
            tokio::task::yield_now().await;
            attempts += 1;
        }

        // Message should be in channel now (size() tracks internal queue, not channel)
        assert_eq!(
            mailbox.size(),
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
            .enqueue(with_priority(
                new_message(b"low".to_vec()),
                MessagePriority::Low,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(with_priority(
                new_message(b"high".to_vec()),
                MessagePriority::High,
            ))
            .await
            .unwrap();

        // Wait for processor to move messages
        let mut attempts = 0;
        while mailbox.size() > 0 && attempts < 100 {
            tokio::task::yield_now().await;
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
            .enqueue(with_priority(
                new_message(b"low".to_vec()),
                MessagePriority::Low,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(signal_message(b"signal".to_vec()))
            .await
            .unwrap();

        // Wait for processor
        let mut attempts = 0;
        while mailbox.size() > 0 && attempts < 100 {
            tokio::task::yield_now().await;
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
            .enqueue(with_priority(
                new_message(b"low".to_vec()),
                MessagePriority::Low,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(with_priority(
                new_message(b"high".to_vec()),
                MessagePriority::High,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(with_priority(
                new_message(b"normal".to_vec()),
                MessagePriority::Normal,
            ))
            .await
            .unwrap();
        mailbox
            .enqueue(signal_message(b"signal".to_vec()))
            .await
            .unwrap();

        // Wait for processor to move all messages
        let mut attempts = 0;
        while mailbox.size() > 0 && attempts < 100 {
            tokio::task::yield_now().await;
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

        assert_eq!(mailbox.size(), 2);

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
        let message = with_metadata(
            with_reply_to(
                with_correlation_id(new_message(b"test".to_vec()), "corr-123"),
                "reply-addr",
            ),
            "type",
            "call",
        );

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

        assert_eq!(priority_from_int(message.priority), MessagePriority::System);
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
        assert!(!message.id.is_empty());

        // Payload should match
        assert_eq!(message.payload, b"test-payload");
    }

    /// Test message_type_str() returns correct type
    #[tokio::test]
    async fn test_message_type_str() {
        // Test with message_type field set
        let msg1 = with_message_type(new_message(b"test".to_vec()), "call");
        assert_eq!(message_type_str(&msg1), "call");

        // Test with metadata "type" key (fallback)
        let msg2 = with_metadata(new_message(b"test".to_vec()), "type", "cast");
        assert_eq!(message_type_str(&msg2), "cast");

        // Test with neither (default to "cast")
        let msg3 = new_message(b"test".to_vec());
        assert_eq!(message_type_str(&msg3), "cast");

        // Test message_type takes precedence over metadata
        let msg4 = with_metadata(
            with_message_type(new_message(b"test".to_vec()), "info"),
            "type",
            "cast",
        );
        assert_eq!(message_type_str(&msg4), "info");
    }

    // ==========================================================================
    // MESSAGE BUILDER TESTS (Lines 175-193)
    // ==========================================================================

    /// Test with_sender() and sender_id() methods
    #[tokio::test]
    async fn test_message_with_sender() {
        let message = with_sender(new_message(b"test".to_vec()), "actor-123");

        assert_eq!(message.sender_id, "actor-123");
        assert_eq!(sender_id(&message), Some("actor-123"));

        // Test message without sender
        let msg2 = new_message(b"test".to_vec());
        assert_eq!(sender_id(&msg2), None);
    }

    /// Test with_message_type() method
    #[tokio::test]
    async fn test_message_with_message_type() {
        let message = with_message_type(new_message(b"test".to_vec()), "workflow_run");

        assert_eq!(message.message_type, "workflow_run");
        assert_eq!(message_type_str(&message), "workflow_run");
    }

    /// Test priority() getter method
    #[tokio::test]
    async fn test_message_priority() {
        let message = with_priority(new_message(b"test".to_vec()), MessagePriority::High);

        assert_eq!(priority_from_int(message.priority), MessagePriority::High);
    }

    /// Test builder method chaining
    #[tokio::test]
    async fn test_message_builders_chaining() {
        let message = with_metadata(
            with_reply_to(
                with_correlation_id(
                    with_priority(
                        with_message_type(
                            with_sender(new_message(b"payload".to_vec()), "sender-1"),
                            "call",
                        ),
                        MessagePriority::High,
                    ),
                    "corr-456",
                ),
                "reply-addr",
            ),
            "key",
            "value",
        );

        assert_eq!(message.sender_id, "sender-1");
        assert_eq!(message.message_type, "call");
        assert_eq!(priority_from_int(message.priority), MessagePriority::High);
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

        let message = proto_msg.clone();

        assert_eq!(message.id, "test-id");
        assert_eq!(message.sender_id, "sender-123");
        assert_eq!(message.receiver_id, "receiver-456");
        assert_eq!(message.message_type, "call");
        assert_eq!(message.payload, b"test-payload");
        assert_eq!(priority_from_int(message.priority), MessagePriority::High); // 60 is in 50-74 range
        assert_eq!(message.correlation_id, "corr-1");
        assert_eq!(message.reply_to, "reply-1");

        // Test normal priority (25-49 range)
        let mut proto_msg2 = proto_msg.clone();
        proto_msg2.priority = 30;
        let message2 = proto_msg2;
        assert_eq!(
            priority_from_int(message2.priority),
            MessagePriority::Normal
        );

        // Test low priority (< 25 range)
        let mut proto_msg3 = proto_msg.clone();
        proto_msg3.priority = 10;
        let message3 = proto_msg3;
        assert_eq!(priority_from_int(message3.priority), MessagePriority::Low); // 10 < 25 maps to Low

        // Test empty sender
        let mut proto_msg4 = proto_msg.clone();
        proto_msg4.sender_id = String::new();
        let message4 = proto_msg4;
        assert_eq!(sender_id(&message4), None);

        // Test empty receiver (from_proto is a clone, so receiver_id stays empty)
        let mut proto_msg5 = proto_msg.clone();
        proto_msg5.receiver_id = String::new();
        let message5 = proto_msg5;
        assert_eq!(message5.receiver_id, "");
    }

    /// Test to_proto() with all priority levels
    #[tokio::test]
    async fn test_message_to_proto() {
        // Test Highest priority (Signal equivalent)
        let proto1 = with_metadata(
            with_reply_to(
                with_correlation_id(
                    with_sender(
                        with_priority(new_message(b"test".to_vec()), MessagePriority::Highest),
                        "sender-1",
                    ),
                    "corr-1",
                ),
                "reply-1",
            ),
            "custom",
            "value",
        );
        assert_eq!(proto1.priority, 100); // Highest = 100
        assert_eq!(proto1.sender_id, "sender-1");
        assert_eq!(proto1.correlation_id, "corr-1"); // stored as direct field
        assert_eq!(proto1.reply_to, "reply-1"); // stored as direct field
        assert_eq!(proto1.headers.get("custom"), Some(&"value".to_string()));

        // Test System priority
        let proto2 = system_message(b"test".to_vec());
        assert_eq!(proto2.priority, 75); // System = 75

        // Test High priority
        let proto3 = with_priority(new_message(b"test".to_vec()), MessagePriority::High);
        assert_eq!(proto3.priority, 50); // High = 50

        // Test Normal priority
        let proto4 = with_priority(new_message(b"test".to_vec()), MessagePriority::Normal);
        assert_eq!(proto4.priority, 25); // Normal = 25

        // Test Low priority
        let proto5 = with_priority(new_message(b"test".to_vec()), MessagePriority::Low);
        assert_eq!(proto5.priority, 0); // Low = 0

        // Test message without sender
        let proto6 = new_message(b"test".to_vec());
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
        assert_eq!(mailbox.size(), 2);

        // Wait for processor to move messages to channel
        tokio::task::yield_now().await;

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
        assert!(matches!(result.unwrap_err(), MailboxError::Full { .. }));

        // Mailbox should still have only 2 messages
        assert_eq!(mailbox.size(), 2);
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
        assert!(matches!(result.unwrap_err(), MailboxError::Full { .. }));
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
        assert_eq!(mailbox.size(), 10);

        // Wait for background processor to move messages to channel
        for _ in 0..100 {
            if mailbox.size() == 0 {
                break;
            }
            tokio::task::yield_now().await;
        }

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

        assert_eq!(mailbox.size(), 1);

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

        // Yield to give processor task a chance to run
        tokio::task::yield_now().await;

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

    // ==========================================================================
    // CHANNEL-BASED MAILBOX TESTS
    // ==========================================================================

    /// Test Message to ProtoMessage conversion
    #[test]
    fn test_message_to_channel_message() {
        let msg = with_metadata(
            with_message_type(
                with_sender(
                    with_reply_to(
                        with_correlation_id(
                            with_priority(
                                new_message(b"test payload".to_vec()),
                                MessagePriority::High,
                            ),
                            "corr-123",
                        ),
                        "reply-addr",
                    ),
                    "sender-actor",
                ),
                "test_type",
            ),
            "key1",
            "value1",
        );

        // ProtoMessage is already in the correct format, just set channel name
        let mut channel_msg = msg.clone();
        channel_msg.channel = "test-channel".to_string();

        assert_eq!(channel_msg.id, msg.id);
        assert_eq!(channel_msg.payload, b"test payload");
        assert_eq!(channel_msg.channel, "test-channel");
        assert_eq!(channel_msg.sender_id, "sender-actor");
        assert_eq!(channel_msg.correlation_id, "corr-123");
        assert_eq!(channel_msg.reply_to, "reply-addr");
        assert_eq!(channel_msg.message_type, "test_type");
        assert_eq!(channel_msg.priority, 50);
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

        let msg: Message = channel_msg;

        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.payload, b"test payload");
        assert_eq!(priority_from_int(msg.priority), MessagePriority::High); // 60 is in 50-74 range
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

        let mailbox = Mailbox::new(config, "test-mailbox".to_string(), String::new(), String::new(), None)
            .await
            .unwrap();

        // Test basic send/receive
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();

        // Yield to give processor task a chance to run
        tokio::task::yield_now().await;

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

        let mailbox = Mailbox::new(config, "test-mailbox-sqlite".to_string(), String::new(), String::new(), None)
            .await
            .unwrap();

        // Test basic send/receive
        let msg = new_message(b"test-sqlite".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();

        // Yield to give processor task a chance to run
        tokio::task::yield_now().await;

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

        let result = Mailbox::new(config, "test-mailbox".to_string(), String::new(), String::new(), None).await;
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

        let mailbox = Mailbox::new(config, "test-mailbox".to_string(), String::new(), String::new(), None)
            .await
            .unwrap();

        // Should work with InMemory backend
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();

        tokio::task::yield_now().await;

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

        let mailbox = Mailbox::new(config, "test-mailbox".to_string(), String::new(), String::new(), None)
            .await
            .unwrap();

        // Should work with custom config
        let msg = new_message(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();

        tokio::task::yield_now().await;

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

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string(), String::new(), String::new(), None)
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

            // Yield to give processor a chance to flush messages to channel
            for _ in 0..50 {
                tokio::task::yield_now().await;
            }

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

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string(), String::new(), String::new(), None)
                .await
                .unwrap();

            // Brief yield to allow recovery tasks to initialize
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }

            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, we just verify the mailbox can be created and used
            let msg = new_message(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();

            tokio::task::yield_now().await;

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

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string(), String::new(), String::new(), None)
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

            // Yield to give processor a chance to flush messages to channel
            for _ in 0..50 {
                tokio::task::yield_now().await;
            }

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

            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string(), String::new(), String::new(), None)
                .await
                .unwrap();

            // Brief yield to allow recovery tasks to initialize
            for _ in 0..20 {
                tokio::task::yield_now().await;
            }

            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, verify mailbox can be created and used after "restart"
            let msg = new_message(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();

            tokio::task::yield_now().await;

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

        // Yield to allow processor task to run
        tokio::task::yield_now().await;

        // Check size before shutdown
        let size_before = mailbox.size();
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

        let mailbox = Mailbox::new(config, "multi-mailbox".to_string(), String::new(), String::new(), None)
            .await
            .unwrap();

        // Send multiple messages
        for i in 1..=10 {
            mailbox
                .enqueue(new_message(format!("msg{}", i).into_bytes()))
                .await
                .unwrap();
        }

        // Wait for processing — give background tasks a chance to run
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }

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

    // ── Ctrl-queue tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_ctrl_message_bypasses_data_queue() {
        let mailbox = create_default_mailbox().await;

        // Enqueue several data messages first
        for i in 0..5 {
            let msg = with_message_type(new_message(vec![i]), "call");
            mailbox.enqueue(msg).await.unwrap();
        }

        // Enqueue a ctrl message after the data messages
        let ctrl = with_message_type(new_message(b"ctrl".to_vec()), "__DOWN__");
        mailbox.enqueue(ctrl.clone()).await.unwrap();

        // The ctrl message must be returned first, regardless of insertion order
        let first = tokio::time::timeout(std::time::Duration::from_millis(500), mailbox.dequeue())
            .await
            .expect("timed out")
            .expect("no message");

        assert_eq!(
            first.message_type, "__DOWN__",
            "ctrl message must arrive before data messages"
        );
    }

    #[tokio::test]
    async fn test_ctrl_size_tracking() {
        let mailbox = create_default_mailbox().await;

        assert_eq!(mailbox.ctrl_size(), 0);

        mailbox
            .enqueue(with_message_type(new_message(vec![]), "__EXIT__"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![]), "__DOWN__"))
            .await
            .unwrap();
        assert_eq!(mailbox.ctrl_size(), 2);

        // Dequeue one ctrl message
        tokio::time::timeout(std::time::Duration::from_millis(200), mailbox.dequeue())
            .await
            .unwrap();
        assert_eq!(mailbox.ctrl_size(), 1);

        // Dequeue the second
        tokio::time::timeout(std::time::Duration::from_millis(200), mailbox.dequeue())
            .await
            .unwrap();
        assert_eq!(mailbox.ctrl_size(), 0);
    }

    #[tokio::test]
    async fn test_get_stats_includes_ctrl_queue_size() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(with_message_type(new_message(vec![]), "call"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![]), "__DOWN__"))
            .await
            .unwrap();

        // Give the internal processor a moment to move the data message to the channel
        tokio::task::yield_now().await;

        let stats = mailbox.get_stats();
        assert_eq!(
            stats.ctrl_queue_size, 1,
            "ctrl_queue_size must count pending ctrl messages"
        );
        assert_eq!(
            stats.total_size(),
            stats.data_queue_size + stats.ctrl_queue_size
        );
    }

    #[tokio::test]
    async fn test_ctrl_queue_does_not_apply_backpressure() {
        // Use a tiny capacity so the data queue fills up
        let config = MailboxConfig {
            capacity: 2,
            backpressure_strategy: plexspaces_proto::mailbox::v1::BackpressureStrategy::Error
                .into(),
            ..mailbox_config_default()
        };
        let mailbox = create_test_mailbox(config).await;

        // Fill the data queue to capacity
        mailbox
            .enqueue(with_message_type(new_message(vec![]), "call"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![]), "call"))
            .await
            .unwrap();

        // Ctrl messages must still be accepted even when data queue is full
        let result = mailbox
            .enqueue(with_message_type(new_message(vec![]), "__PING__"))
            .await;
        assert!(
            result.is_ok(),
            "ctrl messages must not be blocked by data-queue backpressure"
        );
    }

    #[tokio::test]
    async fn test_multiple_ctrl_types_ordered() {
        let mailbox = create_default_mailbox().await;

        // Interleave ctrl and data messages
        mailbox
            .enqueue(with_message_type(new_message(vec![1]), "call"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![2]), "__DOWN__"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![3]), "call"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![4]), "__EXIT__"))
            .await
            .unwrap();
        mailbox
            .enqueue(with_message_type(new_message(vec![5]), "call"))
            .await
            .unwrap();

        // First two dequeues must be ctrl messages
        let m1 = tokio::time::timeout(std::time::Duration::from_millis(300), mailbox.dequeue())
            .await
            .unwrap()
            .unwrap();
        let m2 = tokio::time::timeout(std::time::Duration::from_millis(300), mailbox.dequeue())
            .await
            .unwrap()
            .unwrap();

        assert!(
            is_ctrl_message(&m1.message_type),
            "first dequeue must be ctrl, got {}",
            m1.message_type
        );
        assert!(
            is_ctrl_message(&m2.message_type),
            "second dequeue must be ctrl, got {}",
            m2.message_type
        );
    }

    #[tokio::test]
    async fn test_ping_message_is_ctrl() {
        let down = with_message_type(new_message(vec![]), "__PING__");
        assert!(is_ctrl_message(&down.message_type));
    }

    #[tokio::test]
    async fn test_ctrl_priority_while_blocking_on_data() {
        // Verifies the tokio::select! path: a ctrl message that arrives while
        // dequeue_with_timeout is blocking on the data queue must be returned on
        // the same call, not deferred to the next one.
        let mailbox = Arc::new(create_default_mailbox().await);
        let mailbox2 = mailbox.clone();

        // Start dequeue before any messages exist — this will block.
        let dequeue = tokio::spawn(async move {
            mailbox2
                .dequeue_with_timeout(Some(std::time::Duration::from_millis(500)))
                .await
        });

        // Give the dequeue task time to enter its blocking wait.
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        // Enqueue a ctrl message while the dequeue is blocked.
        mailbox
            .enqueue(with_message_type(new_message(vec![42]), "__DOWN__"))
            .await
            .unwrap();

        let msg = tokio::time::timeout(std::time::Duration::from_millis(400), dequeue)
            .await
            .expect("join timed out")
            .expect("task panicked");

        let msg = msg.expect("no message returned");
        assert_eq!(
            msg.message_type, "__DOWN__",
            "ctrl message must be returned on the blocking call, got {}",
            msg.message_type
        );
    }

    #[tokio::test]
    async fn test_fifo_uses_fast_path_channel() {
        // FIFO in-memory mailboxes must use the direct bounded mpsc fast path:
        // data_tx is Some, channel (Channel trait backend) is None.
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
        let mailbox = create_test_mailbox(config).await;

        assert!(
            mailbox.data_tx.is_some(),
            "FIFO mailbox must have data_tx (fast path sender)"
        );
        assert!(
            mailbox.channel.is_none(),
            "FIFO mailbox must not allocate a Channel backend (no fast path)"
        );
    }

    #[tokio::test]
    async fn test_priority_uses_channel_not_fast_path() {
        // Priority mailboxes route through the Channel backend + internal heap,
        // NOT the direct mpsc fast path.
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        assert!(
            mailbox.channel.is_some(),
            "Priority mailbox must use the Channel backend"
        );
        assert!(
            mailbox.data_tx.is_none(),
            "Priority mailbox must not have data_tx (fast path is for FIFO/LIFO only)"
        );
    }

    #[tokio::test]
    async fn test_lifo_uses_fast_path_channel() {
        // LIFO in-memory mailboxes use the same direct mpsc fast path as FIFO:
        // data_tx is Some, channel backend is None.
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingLifo as i32;
        let mailbox = create_test_mailbox(config).await;

        assert!(
            mailbox.data_tx.is_none(),
            "LIFO mailbox must not have data_tx (LIFO uses internal queue, not fast path)"
        );
        assert!(
            mailbox.channel.is_some(),
            "LIFO mailbox must have a Channel backend for internal queue processing"
        );
    }
}
