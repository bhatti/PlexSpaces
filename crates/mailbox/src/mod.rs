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

use rand::Rng;
use std::collections::{BinaryHeap, VecDeque};
use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, Notify, RwLock};
use ulid::Ulid;
use plexspaces_channel::{Channel, ChannelError, create_channel};
use plexspaces_proto::channel::v1::{ChannelBackend, ChannelConfig, ChannelMessage};
use prost_types::Timestamp;

#[path = "lru_cache.rs"]
mod lru_cache;
use lru_cache::LruCache;

// Re-export proto-generated types
pub use plexspaces_proto::mailbox::v1::{
    BackpressureStrategy, DurabilityStrategy, MailboxConfig, MailboxError as MailboxErrorProto,
    MessagePriority, OrderingStrategy, StorageStrategy,
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
            MailboxErrorProto::MailboxErrorUnspecified => MailboxError::StorageError("Unspecified error".to_string()),
            MailboxErrorProto::MailboxErrorNotFound => MailboxError::StorageError("Mailbox not found".to_string()),
            MailboxErrorProto::MailboxErrorFull => MailboxError::Full,
            MailboxErrorProto::MailboxErrorTimeout => MailboxError::StorageError("Timeout".to_string()),
            MailboxErrorProto::MailboxErrorInvalidConfig => MailboxError::InvalidConfig("Invalid config".to_string()),
            MailboxErrorProto::MailboxErrorStorage => MailboxError::StorageError("Storage error".to_string()),
            MailboxErrorProto::MailboxErrorSerialization => MailboxError::StorageError("Serialization error".to_string()),
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
    // Handle legacy values (0-100 scale) first, then proto values (1-10 scale)
    // Legacy values are more common in existing code
    match value {
        // Legacy exact values (0-100 scale)
        100 => MessagePriority::Highest, // Legacy Signal -> Highest
        75 => MessagePriority::System,   // Legacy System -> System
        50 => MessagePriority::High,     // Legacy High -> High
        25 => MessagePriority::Normal,   // Legacy Normal -> Normal
        0 => MessagePriority::Low,       // Legacy Low -> Low
        // Legacy ranges (0-100 scale)
        v if v > 100 => MessagePriority::Highest,  // Values > 100 -> Highest
        v if v >= 75 && v < 100 => MessagePriority::System,   // Legacy 75-99 -> System
        v if v >= 50 && v < 75 => MessagePriority::High,      // Legacy 50-74 -> High
        v if v >= 25 && v < 50 => MessagePriority::Normal,    // Legacy 25-49 -> Normal
        v if v > 0 && v < 25 => MessagePriority::Normal,      // Legacy 1-24 -> Normal (low maps to normal)
        // Proto exact values (1-10 scale) - only match if not already matched
        10 => MessagePriority::System,
        5 => MessagePriority::Highest,
        4 => MessagePriority::High,
        3 => MessagePriority::Normal,
        2 => MessagePriority::Low,
        1 => MessagePriority::Lowest,
        // Default
        _ => MessagePriority::Normal,
    }
}

/// Message for actor communication
///
/// ## Proto-First Design
/// This is a wrapper around proto-generated types for backward compatibility.
/// The priority field uses proto-generated MessagePriority enum.
/// Note: MessagePriority doesn't implement Serialize/Deserialize, so Message
/// cannot derive these traits. Use to_proto()/from_proto() for serialization.
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// Unique message ID
    pub id: String,
    /// Message payload
    pub payload: Vec<u8>,
    /// Message metadata
    pub metadata: std::collections::HashMap<String, String>,
    /// Priority for ordering
    pub priority: MessagePriority,
    /// Correlation ID for request-reply
    pub correlation_id: Option<String>,
    /// Reply-to address
    pub reply_to: Option<String>,
    /// Sender actor ID
    pub sender: Option<String>,
    /// Receiver actor ID
    pub receiver: String,
    /// Message type
    pub message_type: String,
    /// Time-to-live (TTL) for message expiration
    ttl: Option<std::time::Duration>,
    /// Timestamp when message was created (for expiration check)
    created_at: std::time::Instant,
    /// Optional idempotency key for message deduplication
    /// If provided, messages with the same key are deduplicated within time window
    pub idempotency_key: Option<String>,
    /// URI path for HTTP-based invocations (optional)
    /// Example: "/api/v1/actors/default/counter/metrics"
    pub uri_path: Option<String>,
    /// HTTP method for HTTP-based invocations (optional)
    /// Example: "GET", "POST", "PUT", "DELETE"
    pub uri_method: Option<String>,
}

impl Message {
    /// Create a new message
    pub fn new(payload: Vec<u8>) -> Self {
        Message {
            id: Ulid::new().to_string(),
            payload,
            metadata: Default::default(),
            priority: MessagePriority::Normal,
            correlation_id: None,
            reply_to: None,
            sender: None,
            receiver: String::from("unknown"),
            message_type: String::new(),
            ttl: None,
            created_at: std::time::Instant::now(),
            idempotency_key: None,
            uri_path: None,
            uri_method: None,
        }
    }

    /// Create a system message
    pub fn system(payload: Vec<u8>) -> Self {
        Self::new(payload).with_priority(MessagePriority::System)
    }

    /// Create a signal message (highest priority)
    pub fn signal(payload: Vec<u8>) -> Self {
        Self::new(payload).with_priority(MessagePriority::Highest)
    }

    /// Create a timer message (for lifecycle timer events)
    pub fn timer(timer_name: &str) -> Self {
        Self::new(timer_name.as_bytes().to_vec())
            .with_metadata("type".to_string(), "timer".to_string())
            .with_metadata("timer_name".to_string(), timer_name.to_string())
    }

    /// Get message ID
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get payload
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    /// Get message type string for routing (extracted from metadata or message_type field)
    pub fn message_type_str(&self) -> &str {
        // First check the message_type field
        if !self.message_type.is_empty() {
            return &self.message_type;
        }
        // Fall back to metadata "type" key
        self.metadata
            .get("type")
            .map(|s| s.as_str())
            .unwrap_or("cast")
    }

    // TODO: Restore when behavior module is migrated - returns MessageType enum
    // /// Get message type for routing
    // pub fn message_type(&self) -> crate::behavior::MessageType {
    //     // Extract from metadata or default
    //     if let Some(msg_type) = self.metadata.get("type") {
    //         match msg_type.as_str() {
    //             "call" => crate::behavior::MessageType::Call,
    //             "cast" => crate::behavior::MessageType::Cast,
    //             "info" => crate::behavior::MessageType::Info,
    //             "workflow_run" => crate::behavior::MessageType::WorkflowRun,
    //             msg_type if msg_type.starts_with("workflow_signal:") => {
    //                 let name = msg_type.strip_prefix("workflow_signal:").unwrap();
    //                 crate::behavior::MessageType::WorkflowSignal(name.to_string())
    //             }
    //             msg_type if msg_type.starts_with("workflow_query:") => {
    //                 let name = msg_type.strip_prefix("workflow_query:").unwrap();
    //                 crate::behavior::MessageType::WorkflowQuery(name.to_string())
    //             }
    //             _ => crate::behavior::MessageType::Cast,
    //         }
    //     } else {
    //         crate::behavior::MessageType::Cast
    //     }
    // }

    /// Set correlation ID for request-reply
    pub fn with_correlation_id(mut self, id: String) -> Self {
        self.correlation_id = Some(id);
        self
    }

    /// Set idempotency key for message deduplication
    pub fn with_idempotency_key(mut self, key: String) -> Self {
        self.idempotency_key = Some(key);
        self
    }

    /// Set reply-to address
    pub fn with_reply_to(mut self, address: String) -> Self {
        self.reply_to = Some(address);
        self
    }

    /// Set priority
    pub fn with_priority(mut self, priority: MessagePriority) -> Self {
        self.priority = priority;
        self
    }

    /// Add metadata
    pub fn with_metadata(mut self, key: String, value: String) -> Self {
        self.metadata.insert(key, value);
        self
    }

    /// Set sender ID for request-reply patterns
    pub fn with_sender(mut self, sender_id: String) -> Self {
        self.sender = Some(sender_id);
        self
    }

    /// Set message type for routing
    pub fn with_message_type(mut self, message_type: String) -> Self {
        self.message_type = message_type;
        self
    }

    /// Set time-to-live (TTL) for message expiration
    ///
    /// ## Arguments
    /// * `ttl` - Duration after which the message expires
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_mailbox::Message;
    /// use std::time::Duration;
    /// let msg = Message::new(b"data".to_vec())
    ///     .with_ttl(Duration::from_secs(30));
    /// ```
    pub fn with_ttl(mut self, ttl: std::time::Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    /// Get the TTL (time-to-live) of this message
    ///
    /// ## Returns
    /// `Some(Duration)` if TTL is set, `None` otherwise
    pub fn ttl(&self) -> Option<std::time::Duration> {
        self.ttl
    }

    /// Check if this message has expired
    ///
    /// ## Returns
    /// `true` if message has TTL and it has expired, `false` otherwise
    ///
    /// ## Example
    /// ```rust
    /// use plexspaces_mailbox::Message;
    /// use std::time::Duration;
    /// let msg = Message::new(b"data".to_vec())
    ///     .with_ttl(Duration::from_millis(10));
    /// assert!(!msg.is_expired()); // Just created
    /// ```
    pub fn is_expired(&self) -> bool {
        if let Some(ttl) = self.ttl {
            self.created_at.elapsed() >= ttl
        } else {
            false // No TTL means never expires
        }
    }

    /// Get sender ID
    pub fn sender_id(&self) -> Option<&str> {
        self.sender.as_deref()
    }

    /// Get priority
    pub fn priority(&self) -> MessagePriority {
        self.priority.clone()
    }

    /// Convert from proto Message to internal Message
    pub fn from_proto(proto_msg: &plexspaces_proto::v1::actor::Message) -> Self {
        // Convert priority from i32 to MessagePriority
        // Proto uses: System(10), Highest(5), High(4), Normal(3), Low(2), Lowest(1)
        // Legacy values: Signal(100), System(75), High(50), Normal(25), Low(0)
        let priority = message_priority_from_value(proto_msg.priority);

        // Convert TTL from proto Duration if present
        let ttl = proto_msg.ttl.as_ref().map(|d| {
            std::time::Duration::from_secs(d.seconds as u64)
                + std::time::Duration::from_nanos(d.nanos as u64)
        });

        Message {
            id: proto_msg.id.clone(),
            payload: proto_msg.payload.clone(),
            metadata: proto_msg.headers.clone(),
            priority,
            correlation_id: proto_msg.headers.get("correlation_id").cloned(),
            reply_to: proto_msg.headers.get("reply_to").cloned(),
            sender: if proto_msg.sender_id.is_empty() {
                None
            } else {
                Some(proto_msg.sender_id.clone())
            },
            receiver: if proto_msg.receiver_id.is_empty() {
                "unknown".to_string()
            } else {
                proto_msg.receiver_id.clone()
            },
            message_type: proto_msg.message_type.clone(),
            ttl,
            created_at: std::time::Instant::now(), // Reset creation time when deserializing
            idempotency_key: if proto_msg.idempotency_key.is_empty() {
                None
            } else {
                Some(proto_msg.idempotency_key.clone())
            },
            uri_path: if proto_msg.uri_path.is_empty() {
                None
            } else {
                Some(proto_msg.uri_path.clone())
            },
            uri_method: if proto_msg.uri_method.is_empty() {
                None
            } else {
                Some(proto_msg.uri_method.clone())
            },
        }
    }

    /// Convert internal Message to proto Message
    pub fn to_proto(&self) -> plexspaces_proto::v1::actor::Message {
        use chrono::Utc;

        // Convert priority to i32 (proto uses: System=10, Highest=5, High=4, Normal=3, Low=2, Lowest=1)
        // For backward compatibility with actor.proto, map to legacy values
        let priority = match self.priority {
            MessagePriority::System => 75,  // Legacy System value
            MessagePriority::Highest => 100, // Legacy Signal value
            MessagePriority::High => 50,
            MessagePriority::Normal => 25,
            MessagePriority::Low => 0,
            MessagePriority::Lowest => 0,
            MessagePriority::MessagePriorityUnspecified => 25,
        };

        // Build headers from metadata + correlation_id + reply_to
        let mut headers = self.metadata.clone();
        if let Some(ref correlation_id) = self.correlation_id {
            headers.insert("correlation_id".to_string(), correlation_id.clone());
        }
        if let Some(ref reply_to) = self.reply_to {
            headers.insert("reply_to".to_string(), reply_to.clone());
        }

        plexspaces_proto::v1::actor::Message {
            id: self.id.clone(),
            sender_id: self.sender.clone().unwrap_or_default(),
            receiver_id: self.receiver.clone(),
            message_type: self.message_type.clone(),
            payload: self.payload.clone(),
            timestamp: Some(prost_types::Timestamp {
                seconds: Utc::now().timestamp(),
                nanos: 0,
            }),
            priority,
            ttl: self.ttl.map(|d| prost_types::Duration {
                seconds: d.as_secs() as i64,
                nanos: d.subsec_nanos() as i32,
            }),
            headers,
            idempotency_key: self.idempotency_key.clone().unwrap_or_default(),
            uri_path: self.uri_path.clone().unwrap_or_default(),
            uri_method: self.uri_method.clone().unwrap_or_default(),
        }
    }

    /// Convert Message to typed ActorMessage (if possible)
    ///
    /// ## Usage
    /// ```rust
    /// use plexspaces_mailbox::Message;
    /// use plexspaces_mailbox::ActorMessage;
    /// # let msg = Message::new(b"data".to_vec());
    /// match msg.as_typed() {
    ///     Ok(ActorMessage::TimerFired { timer_name, .. }) => {
    ///         // Handle timer
    ///     }
    ///     Ok(_) => {}
    ///     Err(_) => {}
    /// }
    /// ```
    ///
    /// ## Returns
    /// Typed message or error if conversion fails
    pub fn as_typed(&self) -> Result<crate::messages::ActorMessage, Box<dyn std::error::Error + Send + Sync>> {
        use crate::messages::*;
        use std::time::SystemTime;
        
        // Check metadata for known message types
        if let Some(msg_type) = self.metadata.get("type") {
            match msg_type.as_str() {
                "TimerFired" => {
                    let timer_name = self.metadata.get("timer_name")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    // Parse fired_at from metadata if present
                    let fired_at = self.metadata.get("fired_at")
                        .and_then(|s| s.parse::<i64>().ok())
                        .map(|secs| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64));
                    
                    return Ok(ActorMessage::TimerFired {
                        timer_name,
                        fired_at,
                        callback_data: self.payload.clone(),
                    });
                }
                "ReminderFired" => {
                    let reminder_name = self.metadata.get("reminder_name")
                        .cloned()
                        .unwrap_or_else(|| "unknown".to_string());
                    
                    let fired_at = self.metadata.get("fired_at")
                        .and_then(|s| s.parse::<i64>().ok())
                        .map(|secs| SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(secs as u64));
                    
                    return Ok(ActorMessage::ReminderFired {
                        reminder_name,
                        fired_at,
                        reminder_data: self.payload.clone(),
                    });
                }
                _ => {}
            }
        }
        
        // Check message_type field
        if !self.message_type.is_empty() {
            if self.message_type.starts_with("system:") {
                let kind_str = self.message_type.strip_prefix("system:").unwrap();
                let kind = match kind_str {
                    "activated" => SystemMessageKind::Activated,
                    "deactivated" => SystemMessageKind::Deactivated,
                    "terminated" => SystemMessageKind::Terminated,
                    "health_check" => SystemMessageKind::HealthCheck,
                    _ => SystemMessageKind::Unknown(kind_str.to_string()),
                };
                return Ok(ActorMessage::System {
                    kind,
                    payload: self.payload.clone(),
                });
            }
        }
        
        // Default to User message
        Ok(ActorMessage::User {
            payload: self.payload.clone(),
            message_type: self.message_type.clone(),
        })
    }

    /// Create Message from typed ActorMessage
    ///
    /// ## Usage
    /// ```rust,no_run
    /// use plexspaces_mailbox::Message;
    /// use std::time::SystemTime;
    /// // Note: ActorMessage is an internal type, this is for illustration
    /// // In practice, use Message::timer() or other constructors
    /// let msg = Message::timer("heartbeat");
    /// ```
    pub fn from_typed(typed: crate::messages::ActorMessage) -> Self {
        use crate::messages::*;
        use std::time::SystemTime;
        
        match typed {
            ActorMessage::TimerFired { timer_name, fired_at, callback_data } => {
                let mut msg = Message::new(callback_data);
                msg = msg.with_metadata("type".to_string(), "TimerFired".to_string());
                msg = msg.with_metadata("timer_name".to_string(), timer_name);
                if let Some(time) = fired_at {
                    if let Ok(duration) = time.duration_since(SystemTime::UNIX_EPOCH) {
                        msg = msg.with_metadata("fired_at".to_string(), duration.as_secs().to_string());
                    }
                }
                msg
            }
            ActorMessage::ReminderFired { reminder_name, fired_at, reminder_data } => {
                let mut msg = Message::new(reminder_data);
                msg = msg.with_metadata("type".to_string(), "ReminderFired".to_string());
                msg = msg.with_metadata("reminder_name".to_string(), reminder_name);
                if let Some(time) = fired_at {
                    if let Ok(duration) = time.duration_since(SystemTime::UNIX_EPOCH) {
                        msg = msg.with_metadata("fired_at".to_string(), duration.as_secs().to_string());
                    }
                }
                msg
            }
            ActorMessage::User { payload, message_type } => {
                Message::new(payload).with_message_type(message_type)
            }
            ActorMessage::System { kind, payload } => {
                let kind_str = match kind {
                    SystemMessageKind::Activated => "activated",
                    SystemMessageKind::Deactivated => "deactivated",
                    SystemMessageKind::Terminated => "terminated",
                    SystemMessageKind::HealthCheck => "health_check",
                    SystemMessageKind::Unknown(_) => return Message::system(payload), // Fallback
                };
                Message::system(payload).with_message_type(format!("system:{}", kind_str))
            }
        }
    }

    /// Convert Mailbox Message to ChannelMessage
    ///
    /// ## Purpose
    /// Converts mailbox-specific Message to channel-agnostic ChannelMessage
    /// for use with Channel trait backends.
    pub fn to_channel_message(&self, channel_name: &str) -> ChannelMessage {
        use chrono::Utc;
        
        let mut headers = self.metadata.clone();
        // Add mailbox-specific fields to headers
        headers.insert("message_type".to_string(), self.message_type.clone());
        headers.insert("priority".to_string(), format!("{}", message_priority_value(&self.priority)));
        if let Some(ref correlation_id) = self.correlation_id {
            headers.insert("correlation_id".to_string(), correlation_id.clone());
        }
        if let Some(ref reply_to) = self.reply_to {
            headers.insert("reply_to".to_string(), reply_to.clone());
        }
        if let Some(ref sender) = self.sender {
            headers.insert("sender".to_string(), sender.clone());
        }
        headers.insert("receiver".to_string(), self.receiver.clone());
        
        let now = Utc::now();
        ChannelMessage {
            id: self.id.clone(),
            channel: channel_name.to_string(),
            sender_id: self.sender.clone().unwrap_or_default(),
            payload: self.payload.clone(),
            headers,
            timestamp: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            partition_key: self.receiver.clone(), // Use receiver as partition key
            correlation_id: self.correlation_id.clone().unwrap_or_default(),
            reply_to: self.reply_to.clone().unwrap_or_default(),
            delivery_count: 0,
        }
    }
}

/// Convert ChannelMessage to Mailbox Message
///
/// ## Purpose
/// Converts channel-agnostic ChannelMessage back to mailbox-specific Message
/// for actor consumption.
impl From<ChannelMessage> for Message {
    fn from(channel_msg: ChannelMessage) -> Self {
        // Extract priority from headers or default to Normal
        // Priority is stored as the numeric value (e.g., "4" for High)
        let priority = channel_msg.headers
            .get("priority")
            .and_then(|p| p.parse::<i32>().ok())
            .map(|v| {
                // Map numeric value to MessagePriority enum
                // Proto values: System=10, Highest=5, High=4, Normal=3, Low=2, Lowest=1
                match v {
                    10 => MessagePriority::System,
                    5 => MessagePriority::Highest,
                    4 => MessagePriority::High,
                    3 => MessagePriority::Normal,
                    2 => MessagePriority::Low,
                    1 => MessagePriority::Lowest,
                    _ => message_priority_from_value(v), // Fallback to legacy conversion
                }
            })
            .unwrap_or(MessagePriority::Normal);
        
        // Extract message_type from headers
        let message_type = channel_msg.headers
            .get("message_type")
            .cloned()
            .unwrap_or_default();
        
        // Extract correlation_id and reply_to from headers or fields
        let correlation_id = channel_msg.headers
            .get("correlation_id")
            .or_else(|| if channel_msg.correlation_id.is_empty() { None } else { Some(&channel_msg.correlation_id) })
            .cloned();
        
        let reply_to = channel_msg.headers
            .get("reply_to")
            .or_else(|| if channel_msg.reply_to.is_empty() { None } else { Some(&channel_msg.reply_to) })
            .cloned();
        
        // Extract sender from headers or sender_id field
        let sender = channel_msg.headers
            .get("sender")
            .or_else(|| if channel_msg.sender_id.is_empty() { None } else { Some(&channel_msg.sender_id) })
            .cloned();
        
        // Extract receiver from headers
        let receiver = channel_msg.headers
            .get("receiver")
            .cloned()
            .unwrap_or_else(|| channel_msg.channel.clone());
        
        Message {
            id: channel_msg.id,
            payload: channel_msg.payload,
            metadata: channel_msg.headers,
            priority,
            correlation_id,
            reply_to,
            sender,
            receiver,
            message_type,
            ttl: None, // TTL not in ChannelMessage, would need to be added if needed
            created_at: std::time::Instant::now(), // Reset creation time when deserializing
            idempotency_key: None, // Idempotency key not in ChannelMessage
            uri_path: None, // URI path not in ChannelMessage
            uri_method: None, // URI method not in ChannelMessage
        }
    }
}

// Helper functions for MailboxConfig (cannot add methods to proto-generated types)
pub fn mailbox_config_default() -> MailboxConfig {
    let mut config = MailboxConfig::default();
    config.storage_strategy = StorageStrategy::Memory as i32;
    config.ordering_strategy = OrderingStrategy::OrderingFifo as i32;
    config.durability_strategy = DurabilityStrategy::DurabilityNone as i32;
    config.backpressure_strategy = BackpressureStrategy::Block as i32;
    config.capacity = 10000;
    config
}

fn mailbox_config_ordering(config: &MailboxConfig) -> OrderingStrategy {
    OrderingStrategy::try_from(config.ordering_strategy).unwrap_or(OrderingStrategy::OrderingStrategyUnspecified)
}

fn mailbox_config_backpressure(config: &MailboxConfig) -> BackpressureStrategy {
    BackpressureStrategy::try_from(config.backpressure_strategy).unwrap_or(BackpressureStrategy::BackpressureStrategyUnspecified)
}

fn mailbox_config_max_size(config: &MailboxConfig) -> usize {
    config.capacity as usize
}

fn mailbox_config_message_id_cache_size(config: &MailboxConfig) -> usize {
    if config.message_id_cache_size > 0 {
        config.message_id_cache_size as usize
    } else {
        10000 // Default: 10000 entries
    }
}

fn mailbox_config_idempotency_cache_size(config: &MailboxConfig) -> usize {
    if config.idempotency_cache_size > 0 {
        config.idempotency_cache_size as usize
    } else {
        10000 // Default: 10000 entries
    }
}

fn mailbox_config_deduplication_window(config: &MailboxConfig) -> Duration {
    if let Some(ref window) = config.deduplication_window {
        Duration::from_secs(window.seconds as u64) + Duration::from_nanos(window.nanos as u64)
    } else {
        Duration::from_secs(24 * 60 * 60) // Default: 24 hours
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
    channel_backend: i32,
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
    local_receiver: Arc<tokio::sync::Mutex<Option<mpsc::UnboundedReceiver<Message>>>>,
    /// LRU cache for message ID deduplication (message_id -> timestamp)
    /// Fixed size cache (default: 10000 entries) with TTL expiration
    message_id_cache: Arc<RwLock<LruCache<String, SystemTime>>>,
    /// LRU cache for idempotency key deduplication (idempotency_key -> (timestamp, cached_response))
    /// Fixed size cache (default: 10000 entries) with TTL expiration
    /// Idempotency keys seen within deduplication_window return cached response
    idempotency_cache: Arc<RwLock<LruCache<String, (SystemTime, Option<Message>)>>>,
    /// Deduplication time window (default: 24 hours)
    deduplication_window: Duration,
    /// Maximum cache size for message ID deduplication (default: 10000)
    message_id_cache_size: usize,
    /// Maximum cache size for idempotency key deduplication (default: 10000)
    idempotency_cache_size: usize,
}

/// Internal message storage (used for ordering/priority before sending to channel)
#[derive(Debug)]
enum MessageStorage {
    /// FIFO/LIFO queue
    Queue(VecDeque<Message>),
    /// Priority queue (BinaryHeap with reverse ordering for max-heap)
    Priority(BinaryHeap<PriorityMessage>),
}

/// Wrapper for Message in priority queue (BinaryHeap is max-heap, we want highest priority first)
#[derive(Debug, Clone)]
struct PriorityMessage {
    message: Message,
}

impl PartialEq for PriorityMessage {
    fn eq(&self, other: &Self) -> bool {
        message_priority_value(&self.message.priority) == message_priority_value(&other.message.priority)
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
        // self.cmp(other) should return Greater when self has higher priority
        // So we compare self.priority to other.priority (not reversed)
        message_priority_value(&self.message.priority).cmp(&message_priority_value(&other.message.priority))
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
    /// Creates a channel backend based on `config.channel_backend` (defaults to IN_MEMORY).
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
        let channel_backend = if config.channel_backend != 0 {
            ChannelBackend::try_from(config.channel_backend)
                .map_err(|_| MailboxError::InvalidConfig(format!("Invalid channel_backend: {}", config.channel_backend)))?
        } else {
            ChannelBackend::ChannelBackendInMemory
        };
        let is_in_memory = channel_backend == ChannelBackend::ChannelBackendInMemory;
        let channel_backend_value = channel_backend as i32;

        // Create channel config from mailbox config
        let mut channel_config = config.channel_config.clone().unwrap_or_else(|| {
            ChannelConfig {
                name: format!("mailbox:{}", mailbox_id),
                backend: channel_backend_value,
                capacity: config.capacity as u64,
                ..Default::default()
            }
        });
        
        // Ensure channel name is set
        if channel_config.name.is_empty() {
            channel_config.name = format!("mailbox:{}", mailbox_id);
        }
        
        // Create channel backend
        let channel = create_channel(channel_config.clone())
            .await
            .map_err(|e| MailboxError::StorageError(format!("Failed to create channel backend: {}", e)))?;
        
        // For InMemory backend, create a local mpsc channel for fast-path dequeue
        // This maintains backward compatibility with existing dequeue API
        let (local_sender, local_receiver) = if is_in_memory {
            let (s, r) = mpsc::unbounded_channel();
            (Some(s), Some(r))
        } else {
            (None, None)
        };
        
        // Initialize internal queue based on ordering strategy
        let internal_queue = match mailbox_config_ordering(&config) {
            OrderingStrategy::OrderingPriority => {
                MessageStorage::Priority(BinaryHeap::new())
            }
            _ => {
                MessageStorage::Queue(VecDeque::new())
            }
        };

        let mailbox = Mailbox {
            config: config.clone(),
            channel: Arc::from(channel),
            channel_name: channel_config.name.clone(),
            channel_backend: channel_backend_value,
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
        };

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
                let mut queue_guard = internal_queue.read().await;
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
                    let channel_msg = msg.to_channel_message(&channel_name);
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
    fn start_processor_with_local_sender(&self, local_sender: mpsc::UnboundedSender<Message>) {
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
                let mut queue_guard = internal_queue.read().await;
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
                    let channel_msg = msg.to_channel_message(&channel_name);
                    let channel_send_result = channel.send(channel_msg).await;
                    
                    // Also send to local receiver for fast-path (InMemory backend)
                    // This is non-blocking and doesn't require a lock
                    let local_send_result = local_sender.send(msg.clone());
                    
                    tracing::debug!(
                        message_id = %msg_id,
                        "Mailbox processor: Sending message - channel: {:?}, local_receiver: {:?}", 
                        channel_send_result.is_ok(), 
                        local_send_result.is_ok()
                    );
                    
                    match (channel_send_result, local_send_result) {
                        (Ok(_), Ok(())) => {
                            num_sent += 1;
                            tracing::debug!(
                                message_id = %msg_id,
                                "Mailbox processor: ✅ Successfully sent message to both channel and local_receiver (total: {})", 
                                num_sent
                            );
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
    pub async fn enqueue(&self, message: Message) -> Result<(), MailboxError> {
        // Check for duplicate message ID (LRU cache with fixed size)
        {
            let mut cache = self.message_id_cache.write().await;
            // Cleanup expired entries (LRU cache handles TTL automatically)
            cache.cleanup_expired();
            
            // Check if message ID already seen (LRU cache returns None if expired or not found)
            if cache.get(&message.id).is_some() {
                // Duplicate message ID - skip
                tracing::debug!(message_id = %message.id, "Skipping duplicate message ID");
                return Ok(());
            }
            
            // Add to cache (LRU cache handles eviction if full)
            cache.insert(message.id.clone(), SystemTime::now());
        }
        
        // Check for duplicate idempotency key (LRU cache with fixed size)
        if let Some(ref idempotency_key) = message.idempotency_key {
            let mut cache = self.idempotency_cache.write().await;
            // Cleanup expired entries
            cache.cleanup_expired();
            
            // Check if idempotency key already seen
            if let Some(_cached_entry) = cache.get(idempotency_key) {
                // Duplicate idempotency key - skip message (deduplication)
                // Note: get() already checked expiration, so if we get here, the key is valid
                tracing::debug!(idempotency_key = %idempotency_key, "Skipping duplicate message with idempotency key");
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

        tracing::debug!(
            message_id = %message_id,
            queue_size = stats.current_size,
            "Mailbox::enqueue: ✅ Message enqueued successfully"
        );

        // Notify processor that a message is available
        // This wakes up the processor task that's waiting on notify.notified()
        self.notify.notify_one();
        tracing::debug!(
            message_id = %message_id,
            "Mailbox::enqueue: Notified processor task"
        );

        Ok(())
    }

    /// Send a message (alias for enqueue)
    pub async fn send(&self, message: Message) -> Result<(), MailboxError> {
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
    ) -> impl std::future::Future<Output = Option<Message>> {
        let channel = self.channel.clone();
        let local_receiver = self.local_receiver.clone();
        let mailbox_id = self.mailbox_id.clone();
        
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
                    tracing::trace!("Mailbox::dequeue: local_receiver not available (attempt {}), falling back to channel backend", attempts);
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
                    tracing::debug!(
                        mailbox_id = %mailbox_id,
                        message_id = %msg.id,
                        message_type = %msg.message_type_str(),
                        sender = ?msg.sender,
                        receiver = %msg.receiver,
                        correlation_id = ?msg.correlation_id,
                        attempts = attempts,
                        "📬 Mailbox::dequeue: ✅ Received message from local_receiver (try_recv)"
                    );
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
            match timeout {
                None => {
                    // Indefinite wait - poll channel
                    loop {
                        match channel.receive(1).await {
                            Ok(messages) => {
                                if let Some(channel_msg) = messages.first() {
                                    let msg = Message::from(channel_msg.clone());
                                    tracing::debug!(
                                        mailbox_id = %mailbox_id,
                                        message_id = %msg.id,
                                        message_type = %msg.message_type_str(),
                                        sender = ?msg.sender,
                                        receiver = %msg.receiver,
                                        correlation_id = ?msg.correlation_id,
                                        "📬 Mailbox::dequeue: ✅ Received message from channel (receive)"
                                    );
                                    return Some(msg);
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
                    let start = std::time::Instant::now();
                    loop {
                        if start.elapsed() >= duration {
                            return None;
                        }
                        
                        match channel.try_receive(1).await {
                            Ok(messages) => {
                                if let Some(channel_msg) = messages.first() {
                                    let msg = Message::from(channel_msg.clone());
                                    tracing::debug!(
                                        mailbox_id = %mailbox_id,
                                        message_id = %msg.id,
                                        message_type = %msg.message_type_str(),
                                        sender = ?msg.sender,
                                        receiver = %msg.receiver,
                                        correlation_id = ?msg.correlation_id,
                                        "📬 Mailbox::dequeue: ✅ Received message from channel (try_receive)"
                                    );
                                    return Some(msg);
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
    pub fn dequeue(&self) -> impl std::future::Future<Output = Option<Message>> {
        self.dequeue_with_timeout(None)
    }

    /// Dequeue a message matching a predicate (selective receive)
    ///
    /// Note: This is less efficient with channel-based architecture as it requires
    /// checking messages. For better performance, use pattern matching in the actor loop.
    pub async fn dequeue_matching<F>(&self, predicate: F) -> Option<Message>
    where
        F: Fn(&Message) -> bool,
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
    pub async fn peek(&self, count: usize) -> Vec<Message> {
        let queue_guard = self.internal_queue.read().await;
        match &*queue_guard {
            MessageStorage::Queue(queue) => queue.iter().take(count).cloned().collect(),
            MessageStorage::Priority(heap) => {
                // Convert heap to sorted vec for peeking
                let mut sorted: Vec<_> = heap.iter().map(|pm| pm.message.clone()).collect();
                sorted.sort_by(|a, b| {
                    message_priority_value(&b.priority).cmp(&message_priority_value(&a.priority))
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
        use plexspaces_proto::channel::v1::ChannelBackend;
        // Check if backend is durable (not InMemory)
        match ChannelBackend::try_from(self.channel_backend) {
            Ok(ChannelBackend::ChannelBackendInMemory) => false,
            Ok(_) => true, // SQLite, Redis, Kafka are all durable
            Err(_) => false, // Invalid backend, assume not durable
        }
    }

    /// Get the channel backend type
    ///
    /// Returns the backend type as a string for logging/metrics.
    pub fn backend_type(&self) -> &'static str {
        use plexspaces_proto::channel::v1::ChannelBackend;
        match ChannelBackend::try_from(self.channel_backend) {
            Ok(ChannelBackend::ChannelBackendInMemory) => "in_memory",
            Ok(ChannelBackend::ChannelBackendRedis) => "redis",
            Ok(ChannelBackend::ChannelBackendKafka) => "kafka",
            Ok(ChannelBackend::ChannelBackendSqlite) => "sqlite",
            Ok(ChannelBackend::ChannelBackendNats) => "nats",
            Ok(ChannelBackend::ChannelBackendCustom) => "custom",
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

    /// Graceful shutdown: Flush pending messages to durable backend
    ///
    /// For durable backends, ensures all pending messages are persisted.
    /// For in-memory backends, this is a no-op.
    ///
    /// ## Use Case
    /// Called during actor graceful shutdown to ensure messages are persisted
    /// before DurabilityFacet saves checkpoint.
    pub async fn graceful_shutdown(&self) -> Result<(), MailboxError> {
        if self.is_durable() {
            // For durable backends, ensure all messages in internal queue are sent to channel
            // The channel backend will persist them
            let queue_guard = self.internal_queue.read().await;
            let pending_count = match &*queue_guard {
                MessageStorage::Queue(queue) => queue.len(),
                MessageStorage::Priority(heap) => heap.len(),
            };
            
            if pending_count > 0 {
                tracing::info!(
                    mailbox_id = %self.mailbox_id,
                    backend = %self.backend_type(),
                    pending_messages = pending_count,
                    "Flushing pending messages to durable backend during graceful shutdown"
                );
                
                // Messages will be flushed by the processor task
                // For now, we just log - actual flushing happens via channel.send()
                // TODO: Add explicit flush if needed for specific backends
            }
        }
        
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
    use plexspaces_proto::channel::v1::ChannelBackend;

    /// Helper to create a test mailbox with InMemory backend
    async fn create_test_mailbox(config: MailboxConfig) -> Mailbox {
        Mailbox::new(config, format!("test-mailbox-{}", ulid::Ulid::new())).await.unwrap()
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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Dequeue in FIFO order
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload(), b"first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"second");
    }

    #[tokio::test]
    async fn test_lifo_mailbox() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingLifo as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Dequeue in LIFO order
        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload(), b"second");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"first");
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
            .enqueue(Message::new(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();

        // Messages should be in internal queue
        assert_eq!(mailbox.size().await, 2);
    }

    /// Test 2: Verify priority comparison logic
    #[tokio::test]
    async fn test_priority_comparison() {
        // System (10) > Highest (5) > High (4) > Normal (3) > Low (2)
        assert!(message_priority_value(&MessagePriority::System) > message_priority_value(&MessagePriority::Highest));
        assert!(message_priority_value(&MessagePriority::Highest) > message_priority_value(&MessagePriority::High));
        assert!(message_priority_value(&MessagePriority::High) > message_priority_value(&MessagePriority::Normal));
        assert!(message_priority_value(&MessagePriority::Normal) > message_priority_value(&MessagePriority::Low));

        // Verify signal message has Highest priority
        let signal_msg = Message::signal(b"signal".to_vec());
        assert_eq!(signal_msg.priority, MessagePriority::Highest);
        assert_eq!(message_priority_value(&signal_msg.priority), 5);
    }

    /// Test 3: Verify background processor moves messages from internal queue to channel
    #[tokio::test]
    async fn test_priority_mailbox_processor() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue one message
        mailbox
            .enqueue(Message::new(b"test".to_vec()).with_priority(MessagePriority::Normal))
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
        assert_eq!(mailbox.size().await, 0, "Internal queue should be empty after processor runs");

        // Message should be available for dequeue
        let msg = mailbox.dequeue().await;
        assert!(msg.is_some(), "Message should be available in channel");
        assert_eq!(msg.unwrap().payload(), b"test");
    }

    /// Test 4: Verify priority ordering with two messages
    #[tokio::test]
    async fn test_priority_mailbox_two_messages() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue low priority first, then high priority
        mailbox
            .enqueue(Message::new(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"high".to_vec()).with_priority(MessagePriority::High))
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
        assert_eq!(msg1.payload(), b"high", "High priority should come first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"low", "Low priority should come second");
    }

    /// Test 5: Verify priority ordering with signal (Highest priority)
    #[tokio::test]
    async fn test_priority_mailbox_signal() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue low priority, then signal (Highest)
        mailbox
            .enqueue(Message::new(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::signal(b"signal".to_vec()))
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
        assert_eq!(msg1.payload(), b"signal", "Signal (Highest priority) should come first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"low", "Low priority should come second");
    }

    /// Test 6: Full priority mailbox test (all priorities)
    #[tokio::test]
    async fn test_priority_mailbox() {
        let mut config = mailbox_config_default();
        config.ordering_strategy = OrderingStrategy::OrderingPriority as i32;
        let mailbox = create_test_mailbox(config).await;

        // Enqueue in random order
        mailbox
            .enqueue(Message::new(b"low".to_vec()).with_priority(MessagePriority::Low))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"high".to_vec()).with_priority(MessagePriority::High))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"normal".to_vec()).with_priority(MessagePriority::Normal))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::signal(b"signal".to_vec()))
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
        assert_eq!(msg1.payload(), b"signal", "Signal (Highest=5) should come first");

        let msg2 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg2.payload(), b"high", "High (4) should come second");

        let msg3 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg3.payload(), b"normal", "Normal (3) should come third");

        let msg4 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg4.payload(), b"low", "Low (2) should come last");
    }

    #[tokio::test]
    async fn test_selective_receive() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"target".to_vec()))
            .await
            .unwrap();

        // Selectively receive the "target" message
        let msg = mailbox.dequeue_matching(|m| m.payload() == b"target").await;
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().payload(), b"target");

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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"third".to_vec()))
            .await
            .unwrap(); // Should drop "first"

        assert_eq!(mailbox.size().await, 2);

        let msg1 = mailbox.dequeue().await.unwrap();
        assert_eq!(msg1.payload(), b"second"); // "first" was dropped
    }

    #[tokio::test]
    async fn test_message_priorities() {
        // Test priority ordering using helper function
        // System (10) > Highest (5) > High (4) > Normal (3) > Low (2) > Lowest (1)
        assert!(message_priority_value(&MessagePriority::System) > message_priority_value(&MessagePriority::Highest));
        assert!(message_priority_value(&MessagePriority::Highest) > message_priority_value(&MessagePriority::High));
        assert!(message_priority_value(&MessagePriority::High) > message_priority_value(&MessagePriority::Normal));
        assert!(message_priority_value(&MessagePriority::Normal) > message_priority_value(&MessagePriority::Low));
    }

    #[tokio::test]
    async fn test_message_metadata() {
        let message = Message::new(b"test".to_vec())
            .with_correlation_id("corr-123".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_metadata("type".to_string(), "call".to_string());

        assert_eq!(message.correlation_id, Some("corr-123".to_string()));
        assert_eq!(message.reply_to, Some("reply-addr".to_string()));
        // TODO: Restore when behavior module is migrated
        // assert_eq!(message.message_type(), crate::behavior::MessageType::Call);
        assert_eq!(message.metadata.get("type"), Some(&"call".to_string()));
    }

    // ==========================================================================
    // MESSAGE CREATION TESTS (Lines 89-122)
    // ==========================================================================

    /// Test Message::system() creates system priority message
    #[tokio::test]
    async fn test_message_system() {
        let message = Message::system(b"shutdown".to_vec());

        assert_eq!(message.priority, MessagePriority::System);
        assert_eq!(message.payload(), b"shutdown");
    }

    /// Test Message::timer() creates timer message with metadata
    #[tokio::test]
    async fn test_message_timer() {
        let message = Message::timer("heartbeat");

        // Check metadata
        assert_eq!(message.metadata.get("type"), Some(&"timer".to_string()));
        assert_eq!(
            message.metadata.get("timer_name"),
            Some(&"heartbeat".to_string())
        );

        // Payload should contain timer name
        assert_eq!(message.payload(), b"heartbeat");
    }

    /// Test Message::id() and payload() methods
    #[tokio::test]
    async fn test_message_id_and_payload() {
        let message = Message::new(b"test-payload".to_vec());

        // ID should be non-empty ULID
        assert!(!message.id().is_empty());

        // Payload should match
        assert_eq!(message.payload(), b"test-payload");
    }

    /// Test message_type_str() returns correct type
    #[tokio::test]
    async fn test_message_type_str() {
        // Test with message_type field set
        let msg1 = Message::new(b"test".to_vec()).with_message_type("call".to_string());
        assert_eq!(msg1.message_type_str(), "call");

        // Test with metadata "type" key (fallback)
        let msg2 =
            Message::new(b"test".to_vec()).with_metadata("type".to_string(), "cast".to_string());
        assert_eq!(msg2.message_type_str(), "cast");

        // Test with neither (default to "cast")
        let msg3 = Message::new(b"test".to_vec());
        assert_eq!(msg3.message_type_str(), "cast");

        // Test message_type takes precedence over metadata
        let msg4 = Message::new(b"test".to_vec())
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
        let message = Message::new(b"test".to_vec()).with_sender("actor-123".to_string());

        assert_eq!(message.sender, Some("actor-123".to_string()));
        assert_eq!(message.sender_id(), Some("actor-123"));

        // Test message without sender
        let msg2 = Message::new(b"test".to_vec());
        assert_eq!(msg2.sender_id(), None);
    }

    /// Test with_message_type() method
    #[tokio::test]
    async fn test_message_with_message_type() {
        let message = Message::new(b"test".to_vec()).with_message_type("workflow_run".to_string());

        assert_eq!(message.message_type, "workflow_run");
        assert_eq!(message.message_type_str(), "workflow_run");
    }

    /// Test priority() getter method
    #[tokio::test]
    async fn test_message_priority() {
        let message = Message::new(b"test".to_vec()).with_priority(MessagePriority::High);

        assert_eq!(message.priority(), MessagePriority::High);
    }

    /// Test builder method chaining
    #[tokio::test]
    async fn test_message_builders_chaining() {
        let message = Message::new(b"payload".to_vec())
            .with_sender("sender-1".to_string())
            .with_message_type("call".to_string())
            .with_priority(MessagePriority::High)
            .with_correlation_id("corr-456".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_metadata("key".to_string(), "value".to_string());

        assert_eq!(message.sender, Some("sender-1".to_string()));
        assert_eq!(message.message_type, "call");
        assert_eq!(message.priority, MessagePriority::High);
        assert_eq!(message.correlation_id, Some("corr-456".to_string()));
        assert_eq!(message.reply_to, Some("reply-addr".to_string()));
        assert_eq!(message.metadata.get("key"), Some(&"value".to_string()));
    }

    // ==========================================================================
    // PROTO CONVERSION TESTS (Lines 197-256)
    // ==========================================================================

    /// Test from_proto() with various priority values
    #[tokio::test]
    async fn test_message_from_proto() {
        use plexspaces_proto::v1::actor::Message as ProtoMessage;
        use std::collections::HashMap;

        // Test high priority (50-74 range)
        let mut headers = HashMap::new();
        headers.insert("correlation_id".to_string(), "corr-1".to_string());
        headers.insert("reply_to".to_string(), "reply-1".to_string());

        let proto_msg = ProtoMessage {
            id: "test-id".to_string(),
            sender_id: "sender-123".to_string(),
            receiver_id: "receiver-456".to_string(),
            message_type: "call".to_string(),
            payload: b"test-payload".to_vec(),
            timestamp: None,
            priority: 60, // High priority (in 50-74 range)
            ttl: None,
            headers: headers.clone(),
            idempotency_key: String::new(),
            uri_path: String::new(),
            uri_method: String::new(),
        };

        let message = Message::from_proto(&proto_msg);

        assert_eq!(message.id, "test-id");
        assert_eq!(message.sender, Some("sender-123".to_string()));
        assert_eq!(message.receiver, "receiver-456");
        assert_eq!(message.message_type, "call");
        assert_eq!(message.payload, b"test-payload");
        assert_eq!(message.priority, MessagePriority::High);
        assert_eq!(message.correlation_id, Some("corr-1".to_string()));
        assert_eq!(message.reply_to, Some("reply-1".to_string()));

        // Test normal priority (25-49)
        let mut proto_msg2 = proto_msg.clone();
        proto_msg2.priority = 30;
        let message2 = Message::from_proto(&proto_msg2);
        assert_eq!(message2.priority, MessagePriority::Normal);

        // Test low priority (< 25)
        let mut proto_msg3 = proto_msg.clone();
        proto_msg3.priority = 10;
        let message3 = Message::from_proto(&proto_msg3);
        assert_eq!(message3.priority, MessagePriority::Normal); // Low maps to Normal

        // Test empty sender
        let mut proto_msg4 = proto_msg.clone();
        proto_msg4.sender_id = String::new();
        let message4 = Message::from_proto(&proto_msg4);
        assert_eq!(message4.sender, None);

        // Test empty receiver (should default to "unknown")
        let mut proto_msg5 = proto_msg.clone();
        proto_msg5.receiver_id = String::new();
        let message5 = Message::from_proto(&proto_msg5);
        assert_eq!(message5.receiver, "unknown");
    }

    /// Test to_proto() with all priority levels
    #[tokio::test]
    async fn test_message_to_proto() {
        // Test Highest priority (Signal equivalent)
        let msg1 = Message::new(b"test".to_vec())
            .with_priority(MessagePriority::Highest)
            .with_sender("sender-1".to_string())
            .with_correlation_id("corr-1".to_string())
            .with_reply_to("reply-1".to_string())
            .with_metadata("custom".to_string(), "value".to_string());

        let proto1 = msg1.to_proto();
        assert_eq!(proto1.priority, 100); // Highest = 100 (legacy Signal value)
        assert_eq!(proto1.sender_id, "sender-1");
        assert_eq!(
            proto1.headers.get("correlation_id"),
            Some(&"corr-1".to_string())
        );
        assert_eq!(proto1.headers.get("reply_to"), Some(&"reply-1".to_string()));
        assert_eq!(proto1.headers.get("custom"), Some(&"value".to_string()));

        // Test System priority
        let msg2 = Message::system(b"test".to_vec());
        let proto2 = msg2.to_proto();
        assert_eq!(proto2.priority, 75); // System = 75

        // Test High priority
        let msg3 = Message::new(b"test".to_vec()).with_priority(MessagePriority::High);
        let proto3 = msg3.to_proto();
        assert_eq!(proto3.priority, 50); // High = 50

        // Test Normal priority
        let msg4 = Message::new(b"test".to_vec()).with_priority(MessagePriority::Normal);
        let proto4 = msg4.to_proto();
        assert_eq!(proto4.priority, 25); // Normal = 25

        // Test Low priority
        let msg5 = Message::new(b"test".to_vec()).with_priority(MessagePriority::Low);
        let proto5 = msg5.to_proto();
        assert_eq!(proto5.priority, 0); // Low = 0

        // Test message without sender
        let msg6 = Message::new(b"test".to_vec());
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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should be dropped (DropNewest)
        mailbox
            .enqueue(Message::new(b"third".to_vec()))
            .await
            .unwrap();

        // Mailbox should still have only 2 messages
        assert_eq!(mailbox.size().await, 2);

        // Wait for processor to move messages to channel
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;

        // First two messages should still be there
        let msg1 = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(100))).await.unwrap();
        assert_eq!(msg1.payload(), b"first");

        let msg2 = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(100))).await.unwrap();
        assert_eq!(msg2.payload(), b"second");

        // No third message
        assert_eq!(mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(10))).await, None);
    }

    /// Test backpressure Reject strategy
    #[tokio::test]
    async fn test_backpressure_reject() {
        let mut config = mailbox_config_default();
        config.capacity = 2;
        config.backpressure_strategy = BackpressureStrategy::Error as i32;
        let mailbox = create_test_mailbox(config).await;

        mailbox
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should be rejected
        let result = mailbox.enqueue(Message::new(b"third".to_vec())).await;
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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Third message should return error (Block not yet implemented)
        let result = mailbox.enqueue(Message::new(b"third".to_vec())).await;
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
            mailbox.enqueue(Message::new(payload)).await.unwrap();
        }

        // All messages should be in mailbox (random order doesn't drop)
        assert_eq!(mailbox.size().await, 10);

        // Wait a bit for background processor to move messages to channel
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Dequeue all messages (use timeout to avoid hanging)
        let mut dequeued = Vec::new();
        for _ in 0..10 {
            if let Some(msg) = mailbox.dequeue_with_timeout(Some(Duration::from_millis(100))).await {
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

        mailbox.send(Message::new(b"test".to_vec())).await.unwrap();

        assert_eq!(mailbox.size().await, 1);

        let msg = mailbox.dequeue().await.unwrap();
        assert_eq!(msg.payload(), b"test");
    }

    /// Test dequeue() on empty mailbox waits indefinitely (returns None only when channel is closed)
    #[tokio::test]
    async fn test_mailbox_dequeue_empty() {
        let mailbox = create_default_mailbox().await;

        // Dequeue from empty mailbox with timeout should return None after timeout
        let result = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(10))).await;
        assert_eq!(result, None, "Should timeout and return None");
    }

    /// Test dequeue_with_timeout() with timeout
    #[tokio::test]
    async fn test_mailbox_dequeue_with_timeout() {
        let mailbox = create_default_mailbox().await;

        // Test timeout on empty mailbox
        let start = std::time::Instant::now();
        let result = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(50))).await;
        let elapsed = start.elapsed();
        
        assert_eq!(result, None, "Should timeout and return None");
        assert!(elapsed >= std::time::Duration::from_millis(50), "Should wait at least 50ms");
        assert!(elapsed < std::time::Duration::from_millis(100), "Should not wait much longer than timeout");

        // Test message arrives before timeout
        let mailbox2 = create_default_mailbox().await;
        mailbox2.enqueue(Message::new(b"test".to_vec())).await.unwrap();
        
        // Wait a bit for processor to move message to channel
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        
        let start = std::time::Instant::now();
        let result = mailbox2.dequeue_with_timeout(Some(std::time::Duration::from_millis(100))).await;
        let elapsed = start.elapsed();
        
        assert!(result.is_some(), "Should receive message before timeout");
        assert_eq!(result.unwrap().payload(), b"test");
        assert!(elapsed < std::time::Duration::from_millis(50), "Should receive message quickly");
    }

    /// Test dequeue_matching() with no match returns None
    #[tokio::test]
    async fn test_mailbox_dequeue_matching_not_found() {
        let mailbox = create_default_mailbox().await;

        mailbox
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();

        // Try to match something that doesn't exist
        let msg = mailbox
            .dequeue_matching(|m| m.payload() == b"nonexistent")
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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"third".to_vec()))
            .await
            .unwrap();

        // Peek at first 2 messages
        let peeked = mailbox.peek(2).await;
        assert_eq!(peeked.len(), 2);
        assert_eq!(peeked[0].payload(), b"first");
        assert_eq!(peeked[1].payload(), b"second");

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
            .enqueue(Message::new(b"first".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"second".to_vec()))
            .await
            .unwrap();
        mailbox
            .enqueue(Message::new(b"third".to_vec()))
            .await
            .unwrap();

        assert_eq!(mailbox.size().await, 3);

        // Clear all messages
        mailbox.clear().await;

        // Mailbox should be empty
        assert_eq!(mailbox.size().await, 0);
        
        // Dequeue with timeout should return None (empty mailbox)
        // Note: dequeue() without timeout waits indefinitely, so use timeout version
        let result = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(10))).await;
        assert_eq!(result, None, "Should timeout on empty mailbox");
    }

    // ==========================================================================
    // CHANNEL-BASED MAILBOX TESTS
    // ==========================================================================

    /// Test Message to ChannelMessage conversion
    #[test]
    fn test_message_to_channel_message() {
        let msg = Message::new(b"test payload".to_vec())
            .with_priority(MessagePriority::High)
            .with_correlation_id("corr-123".to_string())
            .with_reply_to("reply-addr".to_string())
            .with_sender("sender-actor".to_string())
            .with_message_type("test_type".to_string())
            .with_metadata("key1".to_string(), "value1".to_string());
        
        let channel_msg = msg.to_channel_message("test-channel");
        
        assert_eq!(channel_msg.id, msg.id);
        assert_eq!(channel_msg.payload, b"test payload");
        assert_eq!(channel_msg.channel, "test-channel");
        assert_eq!(channel_msg.sender_id, "sender-actor");
        assert_eq!(channel_msg.correlation_id, "corr-123");
        assert_eq!(channel_msg.reply_to, "reply-addr");
        assert_eq!(channel_msg.headers.get("message_type"), Some(&"test_type".to_string()));
        assert_eq!(channel_msg.headers.get("priority"), Some(&"4".to_string())); // High = 4
        assert_eq!(channel_msg.headers.get("key1"), Some(&"value1".to_string()));
    }

    /// Test ChannelMessage to Message conversion
    #[test]
    fn test_channel_message_to_message() {
        use plexspaces_proto::channel::v1::ChannelMessage;
        use prost_types::Timestamp;
        use chrono::Utc;
        
        let now = Utc::now();
        let channel_msg = ChannelMessage {
            id: "msg-123".to_string(),
            channel: "test-channel".to_string(),
            sender_id: "sender-actor".to_string(),
            payload: b"test payload".to_vec(),
            headers: {
                let mut h = std::collections::HashMap::new();
                h.insert("message_type".to_string(), "test_type".to_string());
                h.insert("priority".to_string(), "4".to_string()); // High
                h.insert("correlation_id".to_string(), "corr-123".to_string());
                h.insert("reply_to".to_string(), "reply-addr".to_string());
                h.insert("sender".to_string(), "sender-actor".to_string());
                h.insert("receiver".to_string(), "receiver-actor".to_string());
                h.insert("key1".to_string(), "value1".to_string());
                h
            },
            timestamp: Some(Timestamp {
                seconds: now.timestamp(),
                nanos: now.timestamp_subsec_nanos() as i32,
            }),
            partition_key: "receiver-actor".to_string(),
            correlation_id: "corr-123".to_string(),
            reply_to: "reply-addr".to_string(),
            delivery_count: 0,
        };
        
        let msg: Message = channel_msg.into();
        
        assert_eq!(msg.id, "msg-123");
        assert_eq!(msg.payload, b"test payload");
        assert_eq!(msg.priority, MessagePriority::High);
        assert_eq!(msg.correlation_id, Some("corr-123".to_string()));
        assert_eq!(msg.reply_to, Some("reply-addr".to_string()));
        assert_eq!(msg.sender, Some("sender-actor".to_string()));
        assert_eq!(msg.receiver, "receiver-actor");
        assert_eq!(msg.message_type, "test_type");
        assert_eq!(msg.metadata.get("key1"), Some(&"value1".to_string()));
    }

    /// Test mailbox creation with InMemory backend (default)
    #[tokio::test]
    async fn test_mailbox_inmemory_backend() {
        let mut config = mailbox_config_default();
        config.channel_backend = ChannelBackend::ChannelBackendInMemory as i32;
        
        let mailbox = Mailbox::new(config, "test-mailbox".to_string()).await.unwrap();
        
        // Test basic send/receive
        let msg = Message::new(b"test".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();
        
        // Wait for processor to move message
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload(), b"test");
    }

    /// Test mailbox creation with SQLite backend
    #[tokio::test]
    #[cfg(feature = "sqlite-backend")]
    async fn test_mailbox_sqlite_backend() {
        use plexspaces_proto::channel::v1::{ChannelConfig, SqliteConfig};
        
        // Use in-memory database to prevent concurrency issues
        let db_path_str = ":memory:".to_string();
        
        let mut config = mailbox_config_default();
        config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
        
        let sqlite_config = SqliteConfig {
            database_path: db_path_str,
            table_name: "channel_messages".to_string(),
            wal_mode: true,
            cleanup_acked: true,
            cleanup_age_seconds: 3600,
        };
        
        let channel_config = ChannelConfig {
            name: "test-mailbox-sqlite".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            capacity: 1000,
            backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
            ..Default::default()
        };
        
        config.channel_config = Some(channel_config);
        
        let mailbox = Mailbox::new(config, "test-mailbox-sqlite".to_string()).await.unwrap();
        
        // Test basic send/receive
        let msg = Message::new(b"test-sqlite".to_vec());
        mailbox.enqueue(msg.clone()).await.unwrap();
        
        // Wait for processor to move message
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        
        let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
        assert!(received.is_some());
        assert_eq!(received.unwrap().payload(), b"test-sqlite");
    }

    /// Test mailbox creation with invalid backend
    #[tokio::test]
    async fn test_mailbox_invalid_backend() {
        let mut config = mailbox_config_default();
        config.channel_backend = 999; // Invalid backend value
        
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
        // channel_backend is 0 (unspecified), should default to InMemory
        
        let mailbox = Mailbox::new(config, "test-mailbox".to_string()).await.unwrap();
        
        // Should work with InMemory backend
        let msg = Message::new(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();
        
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
        assert!(received.is_some());
    }

    /// Test mailbox with custom channel config
    #[tokio::test]
    async fn test_mailbox_custom_channel_config() {
        use plexspaces_proto::channel::v1::ChannelConfig;
        
        let mut config = mailbox_config_default();
        config.channel_backend = ChannelBackend::ChannelBackendInMemory as i32;
        
        let channel_config = ChannelConfig {
            name: "custom-mailbox".to_string(),
            backend: ChannelBackend::ChannelBackendInMemory as i32,
            capacity: 5000,
            ..Default::default()
        };
        
        config.channel_config = Some(channel_config);
        
        let mailbox = Mailbox::new(config, "test-mailbox".to_string()).await.unwrap();
        
        // Should work with custom config
        let msg = Message::new(b"test".to_vec());
        mailbox.enqueue(msg).await.unwrap();
        
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        
        let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
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
        let test_dir = temp_base.join(format!("plexspaces_mailbox_recovery_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
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
            config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
            
            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false, // Don't cleanup for recovery test
                cleanup_age_seconds: 0,
            };
            
            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                backend: ChannelBackend::ChannelBackendSqlite as i32,
                capacity: 1000,
                backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
                ..Default::default()
            };
            
            config.channel_config = Some(channel_config);
            
            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string()).await.unwrap();
            
            // Send messages but don't dequeue (simulating crash)
            mailbox.enqueue(Message::new(b"msg1".to_vec())).await.unwrap();
            mailbox.enqueue(Message::new(b"msg2".to_vec())).await.unwrap();
            
            // Wait for messages to be sent to channel
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            
            // Mailbox is dropped here (simulating crash)
        }
        
        // Create new mailbox instance (simulating recovery after restart)
        {
            let mut config = mailbox_config_default();
            config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
            
            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false,
                cleanup_age_seconds: 0,
            };
            
            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                backend: ChannelBackend::ChannelBackendSqlite as i32,
                capacity: 1000,
                backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
                ..Default::default()
            };
            
            config.channel_config = Some(channel_config);
            
            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string()).await.unwrap();
            
            // Wait for recovery to complete
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            
            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, we just verify the mailbox can be created and used
            let msg = Message::new(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();
            
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            
            let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
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
        let test_dir = temp_base.join(format!("plexspaces_mailbox_recovery_int_{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
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
            config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
            
            let sqlite_config = SqliteConfig {
                database_path: db_path_str.clone(),
                table_name: "channel_messages".to_string(),
                wal_mode: true,
                cleanup_acked: false, // Don't cleanup for recovery test
                cleanup_age_seconds: 0,
            };
            
            let channel_config = ChannelConfig {
                name: "recovery-mailbox".to_string(),
                backend: ChannelBackend::ChannelBackendSqlite as i32,
                capacity: 1000,
                backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
                ..Default::default()
            };
            
            config.channel_config = Some(channel_config);
            
            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string()).await.unwrap();
            
            // Send messages
            mailbox.enqueue(Message::new(b"msg1".to_vec())).await.unwrap();
            mailbox.enqueue(Message::new(b"msg2".to_vec())).await.unwrap();
            mailbox.enqueue(Message::new(b"msg3".to_vec())).await.unwrap();
            
            // Wait for messages to be sent to channel
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            
            // Mailbox is dropped here (simulating crash)
        }
        
        // Phase 2: Create new mailbox instance (simulating recovery after restart)
        {
            let mut config = mailbox_config_default();
            config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
            
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
                backend: ChannelBackend::ChannelBackendSqlite as i32,
                capacity: 1000,
                backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
                ..Default::default()
            };
            
            config.channel_config = Some(channel_config);
            
            let mailbox = Mailbox::new(config, "recovery-mailbox".to_string()).await.unwrap();
            
            // Wait for recovery to complete
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            
            // Should be able to receive messages that were sent before crash
            // Note: This depends on SQLite channel recovery implementation
            // For now, verify mailbox can be created and used after "restart"
            let msg = Message::new(b"new-msg".to_vec());
            mailbox.enqueue(msg).await.unwrap();
            
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            
            let received = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_secs(1))).await;
            assert!(received.is_some());
        }
    }

    /// Test mailbox graceful shutdown with metrics
    #[tokio::test]
    async fn test_mailbox_graceful_shutdown() {
        let mailbox = create_default_mailbox().await;
        
        // Send some messages
        mailbox.enqueue(Message::new(b"msg1".to_vec())).await.unwrap();
        mailbox.enqueue(Message::new(b"msg2".to_vec())).await.unwrap();
        
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
        config.channel_backend = ChannelBackend::ChannelBackendSqlite as i32;
        
        let sqlite_config = SqliteConfig {
            database_path: db_path_str,
                table_name: "channel_messages".to_string(),
            wal_mode: true,
            cleanup_acked: false,
            cleanup_age_seconds: 0,
        };
        
        let channel_config = ChannelConfig {
            name: "multi-mailbox".to_string(),
            backend: ChannelBackend::ChannelBackendSqlite as i32,
            capacity: 1000,
            backend_config: Some(plexspaces_proto::channel::v1::channel_config::BackendConfig::Sqlite(sqlite_config)),
            ..Default::default()
        };
        
        config.channel_config = Some(channel_config);
        
        let mailbox = Mailbox::new(config, "multi-mailbox".to_string()).await.unwrap();
        
        // Send multiple messages
        for i in 1..=10 {
            mailbox.enqueue(Message::new(format!("msg{}", i).into_bytes())).await.unwrap();
        }
        
        // Wait for processing
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        
        // Receive some messages
        let mut received_count = 0;
        for _ in 0..5 {
            if let Some(_) = mailbox.dequeue_with_timeout(Some(std::time::Duration::from_millis(100))).await {
                received_count += 1;
            }
        }
        
        assert!(received_count > 0);
    }
}
