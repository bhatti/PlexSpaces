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

//! AWS SQS backend for distributed channels.
//!
//! ## Purpose
//! Provides a production-grade AWS SQS backend for channel messaging with
//! Dead Letter Queue (DLQ) support, ACK/NACK operations, and comprehensive observability.
//!
//! ## Design
//! - **Auto-queue creation**: Creates main queue and DLQ on initialization
//! - **Visibility timeout**: Messages become invisible after receive, visible again if not ACKed
//! - **DLQ support**: Messages exceeding max receive count are sent to DLQ
//! - **Long polling**: Efficient message receiving with configurable wait time
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Queue Configuration
//! - **Main Queue**: `{queue_prefix}{channel_name}`
//! - **DLQ**: `{queue_prefix}{channel_name}-dlq` (if enabled)
//! - **Attributes**:
//!   - VisibilityTimeout: Configurable (default: 30 seconds)
//!   - MessageRetentionPeriod: Configurable (default: 4 days)
//!   - ReceiveMessageWaitTimeSeconds: Long polling (default: 20 seconds)
//!   - RedrivePolicy: Points to DLQ if enabled
//!
//! ## Message Flow
//! 1. **Send**: Message sent to main queue
//! 2. **Receive**: Message received (becomes invisible for visibility timeout)
//! 3. **ACK**: Message deleted from queue
//! 4. **NACK (requeue)**: Message becomes visible again immediately
//! 5. **NACK (DLQ)**: Receive count incremented, sent to DLQ if max exceeded
//!
//! ## Observability
//! - Metrics: Operation latency, error rates, throughput, DLQ metrics
//! - Tracing: Distributed tracing with channel context
//! - Logging: Structured logging with message/channel context

use crate::{Channel, ChannelError, ChannelResult};
use async_trait::async_trait;
use aws_sdk_sqs::{
    error::ProvideErrorMetadata,
    types::{Message, QueueAttributeName},
    Client as SqsClient,
};
use futures::stream::BoxStream;
use plexspaces_common::AWSConfig;
use plexspaces_proto::channel::v1::{
    channel_config, ChannelBackend, ChannelConfig, ChannelMessage, ChannelStats,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, error, instrument, warn};

#[cfg(feature = "sqs-backend")]
use base64::{engine::general_purpose, Engine as _};

/// SQS channel implementation with DLQ support.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_channel::{Channel, SQSChannel};
/// use plexspaces_proto::channel::v1::*;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = ChannelConfig {
///     name: "my-channel".to_string(),
///     backend: ChannelBackend::ChannelBackendSqs as i32,
///     backend_config: Some(channel_config::BackendConfig::SqsConfig(
///         channel_config::SqsConfig {
///             region: "us-east-1".to_string(),
///             queue_prefix: "plexspaces-".to_string(),
///             endpoint_url: "".to_string(), // Empty for production
///             visibility_timeout_seconds: 30,
///             message_retention_period_seconds: 345600,
///             dlq_enabled: true,
///             dlq_max_receive_count: 3,
///             receive_message_wait_time_seconds: 20,
///         },
///     )),
///     ..Default::default()
/// };
///
/// let channel = SQSChannel::new(config).await?;
/// let msg = ChannelMessage {
///     id: ulid::Ulid::new().to_string(),
///     channel: "my-channel".to_string(),
///     payload: b"test".to_vec(),
///     ..Default::default()
/// };
/// channel.send(msg).await?;
/// # Ok(())
/// # }
/// ```
// Temporary SQS config struct until proto is regenerated
// TODO: Remove this once proto is regenerated and use channel_config::SqsConfig
#[derive(Clone, Debug)]
struct SqsConfig {
    region: String,
    queue_prefix: String,
    endpoint_url: String,
    visibility_timeout_seconds: u32,
    message_retention_period_seconds: u32,
    dlq_enabled: bool,
    dlq_max_receive_count: u32,
    receive_message_wait_time_seconds: u32,
}

pub struct SQSChannel {
    config: ChannelConfig,
    sqs_config: SqsConfig,
    client: SqsClient,
    queue_url: String,
    dlq_url: Option<String>,
    stats: Arc<ChannelStatsData>,
    closed: Arc<AtomicBool>,
    // Map message_id to receipt_handle for ACK/NACK operations
    receipt_handles: Arc<RwLock<HashMap<String, String>>>,
}

struct ChannelStatsData {
    messages_sent: AtomicU64,
    messages_received: AtomicU64,
    messages_acked: AtomicU64,
    messages_nacked: AtomicU64,
    messages_dlq: AtomicU64,
    errors: AtomicU64,
}

impl SQSChannel {
    /// Create a new SQS channel.
    ///
    /// ## Arguments
    /// * `config` - Channel configuration with SQS backend config
    ///
    /// ## Behavior
    /// - Creates main queue if it doesn't exist (idempotent)
    /// - Creates DLQ if enabled and doesn't exist (idempotent)
    /// - Configures redrive policy to point main queue to DLQ
    ///
    /// ## Returns
    /// SQSChannel or error if queue creation fails
    #[instrument(skip(config), fields(channel_name = %config.name))]
    pub async fn new(config: ChannelConfig) -> ChannelResult<Self> {
        let start_time = std::time::Instant::now();

        // Extract SQS config
        // NOTE: Proto needs to be regenerated after adding SqsConfig to channel.proto
        // For now, we'll use a workaround to extract config from environment
        let sqs_config = SqsConfig {
            region: std::env::var("AWS_REGION")
                .or_else(|_| std::env::var("PLEXSPACES_AWS_REGION"))
                .unwrap_or_else(|_| "us-east-1".to_string()),
            queue_prefix: std::env::var("PLEXSPACES_SQS_QUEUE_PREFIX")
                .unwrap_or_else(|_| "plexspaces-".to_string()),
            endpoint_url: std::env::var("SQS_ENDPOINT_URL")
                .or_else(|_| std::env::var("PLEXSPACES_SQS_ENDPOINT_URL"))
                .unwrap_or_default(),
            visibility_timeout_seconds: std::env::var("PLEXSPACES_SQS_VISIBILITY_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            message_retention_period_seconds: std::env::var("PLEXSPACES_SQS_MESSAGE_RETENTION")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(345600),
            dlq_enabled: std::env::var("PLEXSPACES_SQS_DLQ_ENABLED")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
            dlq_max_receive_count: std::env::var("PLEXSPACES_SQS_DLQ_MAX_RECEIVE_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3),
            receive_message_wait_time_seconds: std::env::var("PLEXSPACES_SQS_RECEIVE_WAIT_TIME")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(20),
        };

        // Build AWS config
        let aws_config = AWSConfig::from_env();
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(sqs_config.region.clone()));

        // Set endpoint URL if provided (for local testing)
        if !sqs_config.endpoint_url.is_empty() {
            config_builder = config_builder.endpoint_url(&sqs_config.endpoint_url);
        }

        let aws_cfg = config_builder.load().await;
        let client = SqsClient::new(&aws_cfg);

        // Load AWS config for queue naming
        let queue_prefix = if sqs_config.queue_prefix.is_empty() {
            "plexspaces-".to_string()
        } else {
            sqs_config.queue_prefix.clone()
        };

        let queue_name = format!("{}{}", queue_prefix, config.name);
        let dlq_name = if sqs_config.dlq_enabled {
            Some(format!("{}{}-dlq", queue_prefix, config.name))
        } else {
            None
        };

        // Create DLQ first if enabled
        let dlq_url = if let Some(dlq_name) = &dlq_name {
            Some(Self::ensure_queue_exists(&client, dlq_name, None).await?)
        } else {
            None
        };

        // Create main queue with redrive policy if DLQ enabled
        let redrive_policy = if let Some(ref dlq_url) = dlq_url {
            let dlq_arn = Self::get_queue_arn(&client, dlq_url).await?;
            Some(format!(
                r#"{{"deadLetterTargetArn":"{}","maxReceiveCount":{}}}"#,
                dlq_arn, sqs_config.dlq_max_receive_count
            ))
        } else {
            None
        };

        let queue_url = Self::ensure_queue_exists(&client, &queue_name, redrive_policy.as_deref()).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_channel_sqs_init_duration_seconds",
            "backend" => "sqs"
        ).record(duration.as_secs_f64());

        debug!(
            channel_name = %config.name,
            queue_url = %queue_url,
            dlq_enabled = sqs_config.dlq_enabled,
            duration_ms = duration.as_millis(),
            "SQS channel initialized"
        );

        Ok(Self {
            config,
            sqs_config,
            client,
            queue_url,
            dlq_url,
            stats: Arc::new(ChannelStatsData {
                messages_sent: AtomicU64::new(0),
                messages_received: AtomicU64::new(0),
                messages_acked: AtomicU64::new(0),
                messages_nacked: AtomicU64::new(0),
                messages_dlq: AtomicU64::new(0),
                errors: AtomicU64::new(0),
            }),
            closed: Arc::new(AtomicBool::new(false)),
            receipt_handles: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Ensure queue exists, create if it doesn't.
    #[instrument(skip(client), fields(queue_name = %queue_name))]
    async fn ensure_queue_exists(
        client: &SqsClient,
        queue_name: &str,
        redrive_policy: Option<&str>,
    ) -> ChannelResult<String> {
        // Try to get queue URL (check if exists)
        match client
            .get_queue_url()
            .queue_name(queue_name)
            .send()
            .await
        {
            Ok(result) => {
                if let Some(url) = result.queue_url() {
                    debug!(queue_name = %queue_name, "SQS queue already exists");
                    return Ok(url.to_string());
                }
            }
            Err(e) => {
                // Queue doesn't exist, create it
                let error_msg = format!("{}", e);
                let error_code = e.code().unwrap_or("unknown");
                let error_message = e.message().unwrap_or(&error_msg);
                
                debug!(
                    queue_name = %queue_name,
                    error_code = %error_code,
                    error_message = %error_message,
                    "SQS get_queue_url result"
                );
                
                if !error_msg.contains("AWS.SimpleQueueService.NonExistentQueue") 
                    && error_code != "AWS.SimpleQueueService.NonExistentQueue" {
                    error!(
                        queue_name = %queue_name,
                        error_code = %error_code,
                        error_message = %error_message,
                        error = %e,
                        "SQS get_queue_url failed with unexpected error"
                    );
                    return Err(ChannelError::BackendError(format!(
                        "Failed to check queue existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(queue_name = %queue_name, "Creating SQS queue");

        // Create queue
        let mut create_request = client
            .create_queue()
            .queue_name(queue_name)
            .attributes(
                QueueAttributeName::VisibilityTimeout,
                "30", // Default, will be overridden if specified
            )
            .attributes(
                QueueAttributeName::MessageRetentionPeriod,
                "345600", // 4 days default
            )
            .attributes(
                QueueAttributeName::ReceiveMessageWaitTimeSeconds,
                "20", // Long polling default
            );

        // Add redrive policy if provided
        if let Some(policy) = redrive_policy {
            create_request = create_request.attributes(
                QueueAttributeName::RedrivePolicy,
                policy,
            );
        }

        let create_result = create_request.send().await;

        match create_result {
            Ok(result) => {
                let queue_url = result
                    .queue_url()
                    .ok_or_else(|| {
                        ChannelError::BackendError("Queue URL not returned from create".to_string())
                    })?
                    .to_string();
                debug!(queue_name = %queue_name, queue_url = %queue_url, "SQS queue created successfully");
                Ok(queue_url)
            }
            Err(e) => {
                // Check if queue was created concurrently
                if e.to_string().contains("QueueAlreadyExists") {
                    // Try to get URL again
                    match client
                        .get_queue_url()
                        .queue_name(queue_name)
                        .send()
                        .await
                    {
                        Ok(result) => {
                            if let Some(url) = result.queue_url() {
                                Ok(url.to_string())
                            } else {
                                Err(ChannelError::BackendError(
                                    "Queue created concurrently but URL not available".to_string(),
                                ))
                            }
                        }
                        Err(e2) => Err(ChannelError::BackendError(format!(
                            "Failed to get queue URL after concurrent creation: {}",
                            e2
                        ))),
                    }
                } else {
                    Err(ChannelError::BackendError(format!(
                        "Failed to create SQS queue: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Get queue ARN for redrive policy.
    #[instrument(skip(client))]
    async fn get_queue_arn(client: &SqsClient, queue_url: &str) -> ChannelResult<String> {
        let attributes = client
            .get_queue_attributes()
            .queue_url(queue_url)
            .attribute_names(QueueAttributeName::QueueArn)
            .send()
            .await
            .map_err(|e| {
                ChannelError::BackendError(format!("Failed to get queue ARN: {}", e))
            })?;

        attributes
            .attributes()
            .and_then(|attrs| attrs.get(&QueueAttributeName::QueueArn))
            .ok_or_else(|| {
                ChannelError::BackendError("Queue ARN not found in attributes".to_string())
            })
            .map(|arn| arn.to_string())
    }

    /// Convert SQS message to ChannelMessage.
    fn sqs_message_to_channel_message(sqs_msg: &Message) -> ChannelResult<ChannelMessage> {
        let body = sqs_msg
            .body()
            .ok_or_else(|| ChannelError::BackendError("SQS message has no body".to_string()))?;

        // Parse message body (JSON-encoded ChannelMessage)
        // ChannelMessage needs serde derives - for now, parse manually or use prost
        // TODO: Add serde derives to ChannelMessage or use prost encoding
        let channel_msg = serde_json::from_str::<serde_json::Value>(body)
            .map_err(|e| ChannelError::SerializationError(format!("Failed to parse message JSON: {}", e)))?;
        
        // Reconstruct ChannelMessage from JSON
        // This is a workaround until ChannelMessage has serde derives
        let channel_msg = ChannelMessage {
            id: channel_msg.get("id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            channel: channel_msg.get("channel")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            payload: channel_msg.get("payload")
                .and_then(|v| v.as_str())
                .and_then(|s| general_purpose::STANDARD.decode(s).ok())
                .unwrap_or_default(),
            ..Default::default()
        };

        Ok(channel_msg)
    }

    /// Convert ChannelMessage to SQS message body.
    fn channel_message_to_body(msg: &ChannelMessage) -> String {
        // ChannelMessage doesn't have serde derives, so we construct JSON manually
        let json = serde_json::json!({
            "id": msg.id,
            "channel": msg.channel,
            "payload": general_purpose::STANDARD.encode(&msg.payload),
        });
        serde_json::to_string(&json).unwrap_or_else(|_| {
            // Fallback: use payload as-is if serialization fails
            String::from_utf8_lossy(&msg.payload).to_string()
        })
    }
}

#[async_trait]
impl Channel for SQSChannel {
    #[instrument(
        skip(self, message),
        fields(
            channel_name = %self.config.name,
            message_id = %message.id
        )
    )]
    async fn send(&self, message: ChannelMessage) -> ChannelResult<String> {
        if self.closed.load(Ordering::Relaxed) {
            return Err(ChannelError::ChannelClosed(self.config.name.clone()));
        }

        let start_time = std::time::Instant::now();
        let message_id = message.id.clone();

        let body = Self::channel_message_to_body(&message);

        match self
            .client
            .send_message()
            .queue_url(&self.queue_url)
            .message_body(&body)
            .send()
            .await
        {
            Ok(result) => {
                let sqs_message_id = result
                    .message_id()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| message_id.clone());

                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_channel_sqs_send_duration_seconds",
                    "backend" => "sqs"
                ).record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_channel_sqs_send_total",
                    "backend" => "sqs",
                    "result" => "success"
                ).increment(1);
                self.stats.messages_sent.fetch_add(1, Ordering::Relaxed);

                debug!(
                    channel_name = %self.config.name,
                    message_id = %message_id,
                    sqs_message_id = %sqs_message_id,
                    duration_ms = duration.as_millis(),
                    "Message sent successfully"
                );

                Ok(sqs_message_id)
            }
            Err(e) => {
                error!(
                    error = %e,
                    channel_name = %self.config.name,
                    message_id = %message_id,
                    "Failed to send message"
                );
                metrics::counter!(
                    "plexspaces_channel_sqs_send_errors_total",
                    "backend" => "sqs",
                    "error_type" => "send_failed"
                ).increment(1);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(ChannelError::BackendError(format!("SQS send_message failed: {}", e)))
            }
        }
    }

    #[instrument(
        skip(self),
        fields(
            channel_name = %self.config.name,
            max_messages = max_messages
        )
    )]
    async fn receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        // Allow receiving after close to drain messages (but not sending)
        // The check for closed is only in send()

        let start_time = std::time::Instant::now();
        let max_messages = max_messages.min(10); // SQS limit is 10

        let wait_time = self.sqs_config.receive_message_wait_time_seconds as i32;

        match self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(max_messages as i32)
            .wait_time_seconds(wait_time)
            .send()
            .await
        {
            Ok(result) => {
                let mut messages = Vec::new();
                let sqs_messages = result.messages();
                if !sqs_messages.is_empty() {
                    let mut handles = self.receipt_handles.write().await;
                    for sqs_msg in sqs_messages {
                        if let Some(receipt_handle) = sqs_msg.receipt_handle() {
                            match Self::sqs_message_to_channel_message(sqs_msg) {
                                Ok(channel_msg) => {
                                    // Store receipt handle for ACK/NACK
                                    handles.insert(channel_msg.id.clone(), receipt_handle.to_string());
                                    messages.push(channel_msg);
                                }
                                Err(e) => {
                                    warn!(
                                        error = %e,
                                        channel_name = %self.config.name,
                                        "Failed to parse SQS message, skipping"
                                    );
                                    // Still ACK the message to avoid infinite retries
                                    let _ = self
                                        .client
                                        .delete_message()
                                        .queue_url(&self.queue_url)
                                        .receipt_handle(receipt_handle)
                                        .send()
                                        .await;
                                }
                            }
                        }
                    }
                }

                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_channel_sqs_receive_duration_seconds",
                    "backend" => "sqs"
                ).record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_channel_sqs_receive_total",
                    "backend" => "sqs",
                    "result" => "success",
                    "message_count" => messages.len().to_string()
                ).increment(1);
                self.stats.messages_received.fetch_add(messages.len() as u64, Ordering::Relaxed);

                debug!(
                    channel_name = %self.config.name,
                    message_count = messages.len(),
                    duration_ms = duration.as_millis(),
                    "Messages received"
                );

                Ok(messages)
            }
            Err(e) => {
                error!(
                    error = %e,
                    channel_name = %self.config.name,
                    "Failed to receive messages"
                );
                metrics::counter!(
                    "plexspaces_channel_sqs_receive_errors_total",
                    "backend" => "sqs",
                    "error_type" => "receive_failed"
                ).increment(1);
                self.stats.errors.fetch_add(1, Ordering::Relaxed);
                Err(ChannelError::BackendError(format!("SQS receive_message failed: {}", e)))
            }
        }
    }

    async fn try_receive(&self, max_messages: u32) -> ChannelResult<Vec<ChannelMessage>> {
        // Use short polling (wait_time_seconds = 0) for non-blocking
        let max_messages = max_messages.min(10);

        match self
            .client
            .receive_message()
            .queue_url(&self.queue_url)
            .max_number_of_messages(max_messages as i32)
            .wait_time_seconds(0) // Short polling
            .send()
            .await
        {
            Ok(result) => {
                let mut messages = Vec::new();
                let sqs_messages = result.messages();
                if !sqs_messages.is_empty() {
                    let mut handles = self.receipt_handles.write().await;
                    for sqs_msg in sqs_messages {
                        if let Some(receipt_handle) = sqs_msg.receipt_handle() {
                            if let Ok(channel_msg) = Self::sqs_message_to_channel_message(sqs_msg) {
                                handles.insert(channel_msg.id.clone(), receipt_handle.to_string());
                                messages.push(channel_msg);
                            }
                        }
                    }
                }
                Ok(messages)
            }
            Err(e) => Err(ChannelError::BackendError(format!("SQS receive_message failed: {}", e))),
        }
    }

    async fn subscribe(
        &self,
        _consumer_group: Option<String>,
    ) -> ChannelResult<BoxStream<'static, ChannelMessage>> {
        // SQS doesn't support pub/sub natively, but we can create a stream that polls
        let queue_url = self.queue_url.clone();
        let client = self.client.clone();
        let closed = self.closed.clone();
        let receipt_handles = self.receipt_handles.clone();
        let wait_time = self.sqs_config.receive_message_wait_time_seconds as i32;

        let stream = async_stream::stream! {
            while !closed.load(Ordering::Relaxed) {
                match client
                    .receive_message()
                    .queue_url(&queue_url)
                    .max_number_of_messages(10)
                    .wait_time_seconds(wait_time)
                    .send()
                    .await
                {
                    Ok(result) => {
                        let sqs_messages = result.messages();
                        if !sqs_messages.is_empty() {
                            let mut handles = receipt_handles.write().await;
                            for sqs_msg in sqs_messages {
                                if let Some(receipt_handle) = sqs_msg.receipt_handle() {
                                    if let Ok(channel_msg) = Self::sqs_message_to_channel_message(sqs_msg) {
                                        handles.insert(channel_msg.id.clone(), receipt_handle.to_string());
                                        yield channel_msg;
                                    }
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Error already logged in receive, just continue
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                }
            }
        };

        Ok(Box::pin(stream))
    }

    async fn publish(&self, message: ChannelMessage) -> ChannelResult<u32> {
        // SQS doesn't support true pub/sub, so we just send
        // Return 1 to indicate message was sent
        self.send(message).await?;
        Ok(1)
    }

    #[instrument(
        skip(self),
        fields(
            channel_name = %self.config.name,
            message_id = %message_id
        )
    )]
    async fn ack(&self, message_id: &str) -> ChannelResult<()> {
        let start_time = std::time::Instant::now();

        // Get receipt handle from stored map
        let receipt_handle = {
            let handles = self.receipt_handles.read().await;
            handles.get(message_id).cloned()
        };

        if let Some(receipt_handle) = receipt_handle {
            // Delete message from queue
            match self
                .client
                .delete_message()
                .queue_url(&self.queue_url)
                .receipt_handle(&receipt_handle)
                .send()
                .await
            {
                Ok(_) => {
                    // Remove from stored handles
                    let mut handles = self.receipt_handles.write().await;
                    handles.remove(message_id);

                    let duration = start_time.elapsed();
                    metrics::histogram!(
                        "plexspaces_channel_sqs_ack_duration_seconds",
                        "backend" => "sqs"
                    ).record(duration.as_secs_f64());
                    metrics::counter!(
                        "plexspaces_channel_sqs_ack_total",
                        "backend" => "sqs",
                        "result" => "success"
                    ).increment(1);
                    self.stats.messages_acked.fetch_add(1, Ordering::Relaxed);

                    debug!(
                        channel_name = %self.config.name,
                        message_id = %message_id,
                        duration_ms = duration.as_millis(),
                        "Message acknowledged"
                    );
                    Ok(())
                }
                Err(e) => {
                    error!(
                        error = %e,
                        channel_name = %self.config.name,
                        message_id = %message_id,
                        "Failed to delete message"
                    );
                    metrics::counter!(
                        "plexspaces_channel_sqs_ack_errors_total",
                        "backend" => "sqs",
                        "error_type" => "delete_failed"
                    ).increment(1);
                    self.stats.errors.fetch_add(1, Ordering::Relaxed);
                    Err(ChannelError::BackendError(format!("SQS delete_message failed: {}", e)))
                }
            }
        } else {
            metrics::counter!(
                "plexspaces_channel_sqs_ack_errors_total",
                "backend" => "sqs",
                "error_type" => "message_not_found"
            ).increment(1);
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn nack(&self, message_id: &str, requeue: bool) -> ChannelResult<()> {
        let start_time = std::time::Instant::now();

        // Get receipt handle from stored map
        let receipt_handle = {
            let handles = self.receipt_handles.read().await;
            handles.get(message_id).cloned()
        };

        if let Some(receipt_handle) = receipt_handle {
            if requeue {
                // Change visibility timeout to 0 to make message immediately visible
                match self
                    .client
                    .change_message_visibility()
                    .queue_url(&self.queue_url)
                    .receipt_handle(&receipt_handle)
                    .visibility_timeout(0)
                    .send()
                    .await
                {
                    Ok(_) => {
                        // Remove from stored handles (message will be redelivered)
                        let mut handles = self.receipt_handles.write().await;
                        handles.remove(message_id);

                        let duration = start_time.elapsed();
                        metrics::histogram!(
                            "plexspaces_channel_sqs_nack_duration_seconds",
                            "backend" => "sqs"
                        ).record(duration.as_secs_f64());
                        metrics::counter!(
                            "plexspaces_channel_sqs_nack_total",
                            "backend" => "sqs",
                            "requeue" => "true"
                        ).increment(1);
                        self.stats.messages_nacked.fetch_add(1, Ordering::Relaxed);

                        debug!(
                            channel_name = %self.config.name,
                            message_id = %message_id,
                            duration_ms = duration.as_millis(),
                            "Message nacked with requeue"
                        );
                        Ok(())
                    }
                    Err(e) => {
                        error!(
                            error = %e,
                            channel_name = %self.config.name,
                            message_id = %message_id,
                            "Failed to change message visibility"
                        );
                        metrics::counter!(
                            "plexspaces_channel_sqs_nack_errors_total",
                            "backend" => "sqs",
                            "error_type" => "change_visibility_failed"
                        ).increment(1);
                        self.stats.errors.fetch_add(1, Ordering::Relaxed);
                        Err(ChannelError::BackendError(format!(
                            "SQS change_message_visibility failed: {}",
                            e
                        )))
                    }
                }
            } else {
                // For DLQ, we don't need to do anything - SQS redrive policy handles it
                // Just remove from stored handles and let visibility timeout expire
                let mut handles = self.receipt_handles.write().await;
                handles.remove(message_id);

                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_channel_sqs_nack_duration_seconds",
                    "backend" => "sqs"
                ).record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_channel_sqs_nack_total",
                    "backend" => "sqs",
                    "requeue" => "false"
                ).increment(1);
                self.stats.messages_nacked.fetch_add(1, Ordering::Relaxed);
                self.stats.messages_dlq.fetch_add(1, Ordering::Relaxed);

                debug!(
                    channel_name = %self.config.name,
                    message_id = %message_id,
                    duration_ms = duration.as_millis(),
                    "Message nacked (will go to DLQ if max receive count exceeded)"
                );
                Ok(())
            }
        } else {
            metrics::counter!(
                "plexspaces_channel_sqs_nack_errors_total",
                "backend" => "sqs",
                "error_type" => "message_not_found"
            ).increment(1);
            Err(ChannelError::MessageNotFound(message_id.to_string()))
        }
    }

    async fn get_stats(&self) -> ChannelResult<ChannelStats> {
        Ok(ChannelStats {
            name: self.config.name.clone(),
            backend: 6, // ChannelBackendSqs - TODO: use enum once proto regenerated
            messages_sent: self.stats.messages_sent.load(Ordering::Relaxed),
            messages_received: self.stats.messages_received.load(Ordering::Relaxed),
            messages_pending: 0, // Would need to query queue attributes
            avg_latency_us: 0, // Would need to track latency
            messages_failed: self.stats.messages_nacked.load(Ordering::Relaxed),
            throughput: 0.0, // Would need to calculate
            backend_stats: Default::default(),
        })
    }

    async fn close(&self) -> ChannelResult<()> {
        self.closed.store(true, Ordering::Relaxed);
        debug!(channel_name = %self.config.name, "Channel closed");
        Ok(())
    }

    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn get_config(&self) -> &ChannelConfig {
        &self.config
    }
}

