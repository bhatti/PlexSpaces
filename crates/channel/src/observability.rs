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

//! Channel observability helpers
//!
//! ## Purpose
//! Provides reusable observability helpers for channel operations (ACK, NACK, DLQ)
//! that can be used both within channel implementations and by external consumers.
//!
//! ## Usage
//! ```rust
//! use plexspaces_channel::observability::*;
//!
//! // In channel ack implementation
//! record_channel_ack(&channel_name, &message_id, &backend);
//!
//! // In channel nack implementation
//! record_channel_nack(&channel_name, &message_id, requeue, delivery_count, &backend);
//!
//! // In channel DLQ implementation
//! record_channel_dlq(&channel_name, &message_id, delivery_count, reason, &backend);
//! ```

use std::time::Instant;
use tracing::{debug, trace, warn};

/// Record a successful ACK operation
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `message_id` - ID of the message being acknowledged
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at trace level for high-volume operations
/// - Logs at debug level with structured fields
/// - Can be extended with metrics counters/histograms
pub fn record_channel_ack(channel_name: &str, message_id: &str, backend: &str) {
    trace!(
        channel = %channel_name,
        message_id = %message_id,
        backend = %backend,
        "Channel message acknowledged"
    );

    debug!(
        channel = %channel_name,
        message_id = %message_id,
        backend = %backend,
        operation = "ack",
        "✅ Channel ACK: Message acknowledged successfully"
    );
}

/// Record a NACK operation (failed processing, may requeue or DLQ)
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `message_id` - ID of the message being nacked
/// * `requeue` - Whether message will be requeued
/// * `delivery_count` - Current delivery attempt count
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at trace level for high-volume operations
/// - Logs at debug level with structured fields
/// - Can be extended with metrics counters/histograms
pub fn record_channel_nack(
    channel_name: &str,
    message_id: &str,
    requeue: bool,
    delivery_count: u32,
    backend: &str,
) {
    trace!(
        channel = %channel_name,
        message_id = %message_id,
        requeue = requeue,
        delivery_count = delivery_count,
        backend = %backend,
        "Channel message nacked"
    );

    debug!(
        channel = %channel_name,
        message_id = %message_id,
        requeue = requeue,
        delivery_count = delivery_count,
        backend = %backend,
        operation = "nack",
        "⚠️ Channel NACK: Message nacked (requeue={}, delivery_count={})",
        requeue,
        delivery_count
    );
}

/// Record a message sent to Dead Letter Queue (DLQ)
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `message_id` - ID of the message sent to DLQ
/// * `delivery_count` - Number of delivery attempts before DLQ
/// * `reason` - Reason for DLQ (e.g., "max_retries_exceeded", "poisonous_message")
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at warn level (DLQ is a significant event)
/// - Includes structured fields for filtering/alerting
/// - Can be extended with metrics counters
pub fn record_channel_dlq(
    channel_name: &str,
    message_id: &str,
    delivery_count: u32,
    reason: &str,
    backend: &str,
) {
    warn!(
        channel = %channel_name,
        message_id = %message_id,
        delivery_count = delivery_count,
        reason = %reason,
        backend = %backend,
        operation = "dlq",
        "🚨 Channel DLQ: Message sent to Dead Letter Queue (delivery_count={}, reason={})",
        delivery_count,
        reason
    );
}

/// Record a channel operation error
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `operation` - Operation that failed (e.g., "ack", "nack", "send", "receive")
/// * `error` - Error message
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at error level
/// - Includes structured fields for error tracking
/// - Can be extended with error counters
pub fn record_channel_error(
    channel_name: &str,
    operation: &str,
    error: &str,
    backend: &str,
) {
    tracing::error!(
        channel = %channel_name,
        operation = %operation,
        error = %error,
        backend = %backend,
        "❌ Channel Error: Operation '{}' failed: {}",
        operation,
        error
    );
}

/// Record channel operation latency
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `operation` - Operation name (e.g., "ack", "nack", "send", "receive")
/// * `duration` - Duration of the operation
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at trace level (high volume)
/// - Can be extended with histogram metrics
pub fn record_channel_latency(
    channel_name: &str,
    operation: &str,
    duration: std::time::Duration,
    backend: &str,
) {
    let latency_us = duration.as_micros() as u64;
    
    trace!(
        channel = %channel_name,
        operation = %operation,
        latency_us = latency_us,
        backend = %backend,
        "Channel operation latency"
    );
}

/// Helper to measure and record operation latency
///
/// ## Usage
/// ```rust
/// let start = Instant::now();
/// // ... perform operation ...
/// record_channel_latency_from_start(&channel_name, "ack", start, &backend);
/// ```
pub fn record_channel_latency_from_start(
    channel_name: &str,
    operation: &str,
    start: Instant,
    backend: &str,
) {
    let duration = start.elapsed();
    record_channel_latency(channel_name, operation, duration, backend);
}

/// Record channel statistics snapshot
///
/// ## Arguments
/// * `channel_name` - Name of the channel
/// * `messages_sent` - Total messages sent
/// * `messages_received` - Total messages received
/// * `messages_pending` - Current pending messages
/// * `messages_acked` - Total messages acked
/// * `messages_failed` - Total messages failed
/// * `backend` - Backend type (for metrics tagging)
///
/// ## Observability
/// - Logs at debug level
/// - Can be extended with gauge metrics
pub fn record_channel_stats(
    channel_name: &str,
    messages_sent: u64,
    messages_received: u64,
    messages_pending: u64,
    messages_acked: u64,
    messages_failed: u64,
    backend: &str,
) {
    debug!(
        channel = %channel_name,
        messages_sent = messages_sent,
        messages_received = messages_received,
        messages_pending = messages_pending,
        messages_acked = messages_acked,
        messages_failed = messages_failed,
        backend = %backend,
        "Channel statistics snapshot"
    );
}

/// Get backend name as string for observability
///
/// ## Arguments
/// * `backend` - ChannelBackend enum value
///
/// ## Returns
/// String representation of backend name
pub fn backend_name(backend: i32) -> &'static str {
    use plexspaces_proto::channel::v1::ChannelBackend;
    match ChannelBackend::try_from(backend).ok() {
        Some(ChannelBackend::ChannelBackendInMemory) => "in_memory",
        Some(ChannelBackend::ChannelBackendRedis) => "redis",
        Some(ChannelBackend::ChannelBackendKafka) => "kafka",
        Some(ChannelBackend::ChannelBackendNats) => "nats",
        Some(ChannelBackend::ChannelBackendSqlite) => "sqlite",
        Some(ChannelBackend::ChannelBackendCustom) => "custom",
        _ => "unknown",
    }
}







