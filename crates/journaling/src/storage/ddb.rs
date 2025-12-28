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

//! DynamoDB-based JournalStorage implementation.
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for journal storage
//! with proper tenant isolation, efficient replay, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Key**: `{actor_id}` for actor isolation
//! - **Sort Key**: `sequence` (Number) for ordered replay
//! - **Auto-table creation**: Creates tables with proper schema on initialization
//! - **GSI for queries**: For efficient queries by timestamp, event type, etc.
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Table Schema
//!
//! ### journal_entries
//! ```
//! Partition Key: pk = "{actor_id}"
//! Sort Key: sk = "ENTRY#{sequence}"
//! Attributes:
//!   - actor_id: String
//!   - sequence: Number
//!   - entry_type: String (MESSAGE_RECEIVED, MESSAGE_PROCESSED, etc.)
//!   - entry_data: Binary (serialized JournalEntry)
//!   - timestamp: Number (UNIX timestamp in seconds)
//!   - created_at: Number (UNIX timestamp)
//! ```
//!
//! ### checkpoints
//! ```
//! Partition Key: pk = "{actor_id}"
//! Sort Key: sk = "CHECKPOINT"
//! Attributes:
//!   - actor_id: String
//!   - sequence: Number
//!   - state_data: Binary (compressed state)
//!   - timestamp: Number (UNIX timestamp)
//!   - created_at: Number (UNIX timestamp)
//! ```
//!
//! ### events
//! ```
//! Partition Key: pk = "{actor_id}"
//! Sort Key: sk = "EVENT#{sequence}"
//! Attributes:
//!   - actor_id: String
//!   - sequence: Number
//!   - event_type: String
//!   - payload: Binary
//!   - timestamp: Number (UNIX timestamp)
//! ```
//!
//! ### reminders
//! ```
//! Partition Key: pk = "{actor_id}"
//! Sort Key: sk = "REMINDER#{reminder_name}"
//! Attributes:
//!   - actor_id: String
//!   - reminder_name: String
//!   - reminder_data: Binary (serialized ReminderState)
//!   - next_fire_time: Number (UNIX timestamp, for GSI)
//!   - is_active: Number (0 or 1)
//! ```
//!
//! ### GSI: next_fire_time_index (for reminders)
//! - Partition Key: `is_active` = "1"
//! - Sort Key: `next_fire_time` (Number)
//! - Purpose: Efficient query for due reminders

use crate::{Checkpoint, JournalEntry, JournalResult, JournalStats, ActorEvent, ActorHistory, JournalStorage};
use async_trait::async_trait;
use aws_sdk_dynamodb::{
    error::ProvideErrorMetadata,
    types::{
        AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement,
        KeyType, Projection, ProjectionType, ScalarAttributeType,
    },
    Client as DynamoDbClient,
};
use chrono::Utc;
use plexspaces_proto::common::v1::{PageRequest, PageResponse};
use plexspaces_proto::timer::v1::ReminderState;
use prost::Message;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tracing::{debug, error, instrument, warn};

/// DynamoDB JournalStorage implementation.
#[derive(Clone)]
pub struct DynamoDBJournalStorage {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name prefix
    table_prefix: String,
    /// Schema version
    schema_version: u32,
}

impl DynamoDBJournalStorage {
    /// Create a new DynamoDB JournalStorage.
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_prefix` - Table name prefix
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local testing)
    #[instrument(skip(region, table_prefix, endpoint_url), fields(region = %region, table_prefix = %table_prefix))]
    pub async fn new(
        region: String,
        table_prefix: String,
        endpoint_url: Option<String>,
    ) -> JournalResult<Self> {
        let start_time = std::time::Instant::now();

        // Build AWS config
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.clone()));

        if let Some(endpoint) = endpoint_url {
            config_builder = config_builder.endpoint_url(endpoint);
        }

        let config = config_builder.load().await;
        let client = DynamoDbClient::new(&config);

        // Create tables if they don't exist
        Self::ensure_entries_table_exists(&client, &format!("{}-entries", table_prefix)).await?;
        Self::ensure_checkpoints_table_exists(&client, &format!("{}-checkpoints", table_prefix)).await?;
        Self::ensure_events_table_exists(&client, &format!("{}-events", table_prefix)).await?;
        Self::ensure_reminders_table_exists(&client, &format!("{}-reminders", table_prefix)).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        debug!(
            table_prefix = %table_prefix,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB JournalStorage initialized"
        );

        Ok(Self {
            client,
            table_prefix,
            schema_version: 1,
        })
    }

    /// Ensure journal_entries table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_entries_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> JournalResult<()> {
        // Check if table exists
        match client.describe_table().table_name(table_name).send().await {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB table already exists");
                return Ok(());
            }
            Err(e) => {
                // Table doesn't exist, create it
                let error_msg = format!("{}", e);
                let error_code = e.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                let error_message = e.message().map(|m| m.to_string()).unwrap_or_else(|| error_msg.clone());

                if !error_msg.contains("ResourceNotFoundException") && error_code != "ResourceNotFoundException" {
                    error!(
                        error = %e,
                        error_code = %error_code,
                        error_message = %error_message,
                        "DynamoDB describe_table failed"
                    );
                    return Err(crate::JournalError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB journal_entries table");

        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest)
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .send()
            .await;

        match create_table_result {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB table created successfully");
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(crate::JournalError::Storage(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Ensure checkpoints table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_checkpoints_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> JournalResult<()> {
        // Similar to ensure_entries_table_exists
        Self::ensure_entries_table_exists(client, table_name).await
    }

    /// Ensure events table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_events_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> JournalResult<()> {
        // Similar to ensure_entries_table_exists
        Self::ensure_entries_table_exists(client, table_name).await
    }

    /// Ensure reminders table exists with GSI.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_reminders_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> JournalResult<()> {
        // Check if table exists
        match client.describe_table().table_name(table_name).send().await {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB table already exists");
                return Ok(());
            }
            Err(e) => {
                // Table doesn't exist, create it
                let error_msg = format!("{}", e);
                let error_code = e.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                let error_message = e.message().map(|m| m.to_string()).unwrap_or_else(|| error_msg.clone());

                if !error_msg.contains("ResourceNotFoundException") && error_code != "ResourceNotFoundException" {
                    error!(
                        error = %e,
                        error_code = %error_code,
                        error_message = %error_message,
                        "DynamoDB describe_table failed"
                    );
                    return Err(crate::JournalError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB reminders table with GSI");

        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        // GSI for querying due reminders
        let is_active_attr = AttributeDefinition::builder()
            .attribute_name("is_active")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let next_fire_time_attr = AttributeDefinition::builder()
            .attribute_name("next_fire_time")
            .attribute_type(ScalarAttributeType::N)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let gsi_pk = KeySchemaElement::builder()
            .attribute_name("is_active")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let gsi_sk = KeySchemaElement::builder()
            .attribute_name("next_fire_time")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let gsi = GlobalSecondaryIndex::builder()
            .index_name("next_fire_time_index")
            .key_schema(gsi_pk)
            .key_schema(gsi_sk)
            .projection(gsi_projection)
            .build()
            .map_err(|e| crate::JournalError::Storage(format!("Failed to build GSI: {}", e)))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest)
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(is_active_attr)
            .attribute_definitions(next_fire_time_attr)
            .global_secondary_indexes(gsi)
            .send()
            .await;

        match create_table_result {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB reminders table created successfully");
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(crate::JournalError::Storage(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Wait for table to become active.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn wait_for_table_active(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> JournalResult<()> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30;

        loop {
            let describe_result = client
                .describe_table()
                .table_name(table_name)
                .send()
                .await
                .map_err(|e| crate::JournalError::Storage(format!("Failed to describe table: {}", e)))?;

            if let Some(status) = describe_result.table().and_then(|t| t.table_status()) {
                match status {
                    TableStatus::Active => {
                        debug!(table_name = %table_name, "Table is now active");
                        return Ok(());
                    }
                    TableStatus::Creating => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(crate::JournalError::Storage(format!(
                                "Table creation timeout after {} attempts",
                                max_attempts
                            )));
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(crate::JournalError::Storage(format!(
                            "Table in unexpected status: {:?}",
                            status
                        )));
                    }
                }
            } else {
                return Err(crate::JournalError::Storage("Table status not available".to_string()));
            }
        }
    }
}

#[async_trait]
impl JournalStorage for DynamoDBJournalStorage {
    async fn append_entry(&self, entry: &JournalEntry) -> JournalResult<u64> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-entries", self.table_prefix);
        let pk = entry.actor_id.clone();
        let sk = format!("ENTRY#{}", entry.sequence);

        // Serialize entry to binary
        let mut entry_data = Vec::new();
        entry.encode(&mut entry_data)
            .map_err(|e| crate::JournalError::Storage(format!("Failed to encode entry: {}", e)))?;

        let timestamp_secs = entry.timestamp.as_ref().map(|ts| ts.seconds).unwrap_or(Utc::now().timestamp());
        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S(sk));
        item.insert("actor_id".to_string(), AttributeValue::S(entry.actor_id.clone()));
        item.insert("sequence".to_string(), AttributeValue::N(entry.sequence.to_string()));
        item.insert("entry_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(entry_data)));
        item.insert("timestamp".to_string(), AttributeValue::N(timestamp_secs.to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

        // Extract entry type for filtering
        let entry_type = if let Some(entry_variant) = &entry.entry {
            match entry_variant {
                plexspaces_proto::journaling::v1::journal_entry::Entry::MessageReceived(_) => "MESSAGE_RECEIVED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::MessageProcessed(_) => "MESSAGE_PROCESSED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::StateChanged(_) => "STATE_CHANGED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::SideEffectExecuted(_) => "SIDE_EFFECT_EXECUTED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::TimerScheduled(_) => "TIMER_SCHEDULED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::TimerFired(_) => "TIMER_FIRED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::PromiseCreated(_) => "PROMISE_CREATED",
                plexspaces_proto::journaling::v1::journal_entry::Entry::PromiseResolved(_) => "PROMISE_RESOLVED",
            }
        } else {
            "UNKNOWN"
        };
        item.insert("entry_type".to_string(), AttributeValue::S(entry_type.to_string()));

        match self
            .client
            .put_item()
            .table_name(&table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_journaling_ddb_append_entry_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_journaling_ddb_append_entry_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(entry.sequence)
            }
            Err(e) => {
                error!(error = %e, actor_id = %entry.actor_id, sequence = entry.sequence, "Failed to append journal entry");
                metrics::counter!(
                    "plexspaces_journaling_ddb_append_entry_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(crate::JournalError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn append_batch(&self, entries: &[JournalEntry]) -> JournalResult<(u64, u64, usize)> {
        if entries.is_empty() {
            return Ok((0, 0, 0));
        }

        let start_time = std::time::Instant::now();
        let table_name = format!("{}-entries", self.table_prefix);

        // DynamoDB batch_write_item supports up to 25 items
        const BATCH_SIZE: usize = 25;
        let mut first_sequence = entries[0].sequence;
        let mut last_sequence = entries[0].sequence;

        for chunk in entries.chunks(BATCH_SIZE) {
            let mut write_requests = Vec::new();

            for entry in chunk {
                let pk = entry.actor_id.clone();
                let sk = format!("ENTRY#{}", entry.sequence);

                let mut entry_data = Vec::new();
                entry.encode(&mut entry_data)
                    .map_err(|e| crate::JournalError::Storage(format!("Failed to encode entry: {}", e)))?;

                let timestamp_secs = entry.timestamp.as_ref().map(|ts| ts.seconds).unwrap_or(Utc::now().timestamp());
                let now_secs = Utc::now().timestamp();

                let mut item = HashMap::new();
                item.insert("pk".to_string(), AttributeValue::S(pk));
                item.insert("sk".to_string(), AttributeValue::S(sk));
                item.insert("actor_id".to_string(), AttributeValue::S(entry.actor_id.clone()));
                item.insert("sequence".to_string(), AttributeValue::N(entry.sequence.to_string()));
                item.insert("entry_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(entry_data)));
                item.insert("timestamp".to_string(), AttributeValue::N(timestamp_secs.to_string()));
                item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

                let entry_type = if let Some(entry_variant) = &entry.entry {
                    match entry_variant {
                        plexspaces_proto::journaling::v1::journal_entry::Entry::MessageReceived(_) => "MESSAGE_RECEIVED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::MessageProcessed(_) => "MESSAGE_PROCESSED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::StateChanged(_) => "STATE_CHANGED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::SideEffectExecuted(_) => "SIDE_EFFECT_EXECUTED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::TimerScheduled(_) => "TIMER_SCHEDULED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::TimerFired(_) => "TIMER_FIRED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::PromiseCreated(_) => "PROMISE_CREATED",
                        plexspaces_proto::journaling::v1::journal_entry::Entry::PromiseResolved(_) => "PROMISE_RESOLVED",
                    }
                } else {
                    "UNKNOWN"
                };
                item.insert("entry_type".to_string(), AttributeValue::S(entry_type.to_string()));

                let put_request = aws_sdk_dynamodb::types::PutRequest::builder()
                    .set_item(Some(item))
                    .build()
                    .map_err(|e| crate::JournalError::Storage(format!("Failed to build put request: {}", e)))?;
                
                let write_request = aws_sdk_dynamodb::types::WriteRequest::builder()
                    .put_request(put_request)
                    .build();
                
                write_requests.push(write_request);

                if entry.sequence < first_sequence {
                    first_sequence = entry.sequence;
                }
                if entry.sequence > last_sequence {
                    last_sequence = entry.sequence;
                }
            }

            match self
                .client
                .batch_write_item()
                .request_items(&table_name, write_requests)
                .send()
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    return Err(crate::JournalError::Storage(format!(
                        "DynamoDB batch_write_item failed: {}",
                        e
                    )));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_append_batch_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_append_batch_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => entries.len().to_string()
        )
        .increment(1);

        Ok((first_sequence, last_sequence, entries.len()))
    }

    async fn replay_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<JournalEntry>> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-entries", self.table_prefix);
        let pk = actor_id.to_string();

        let mut entries = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("ENTRY#".to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        // Parse sequence from sk
                        if let Some(sk_attr) = item.get("sk") {
                            if let Ok(sk) = sk_attr.as_s() {
                                if let Some(seq_str) = sk.strip_prefix("ENTRY#") {
                                    if let Ok(seq) = seq_str.parse::<u64>() {
                                        if seq >= from_sequence {
                                            // Deserialize entry
                                            if let Some(entry_data_attr) = item.get("entry_data") {
                                                if let Ok(blob) = entry_data_attr.as_b() {
                                                    match JournalEntry::decode(blob.as_ref()) {
                                                        Ok(entry) => entries.push(entry),
                                                        Err(e) => {
                                                            warn!(error = %e, "Failed to decode journal entry, skipping");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, actor_id = %actor_id, "Failed to replay journal entries");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        // Sort by sequence
        entries.sort_by(|a, b| a.sequence.cmp(&b.sequence));

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_replay_from_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_replay_from_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => entries.len().to_string()
        )
        .increment(1);

        Ok(entries)
    }

    async fn get_latest_checkpoint(&self, actor_id: &str) -> JournalResult<Checkpoint> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-checkpoints", self.table_prefix);
        let pk = actor_id.to_string();

        match self
            .client
            .get_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("CHECKPOINT".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                if let Some(item) = result.item().cloned() {
                    if let Some(checkpoint_data_attr) = item.get("checkpoint_data") {
                        if let Ok(blob) = checkpoint_data_attr.as_b() {
                            match Checkpoint::decode(blob.as_ref()) {
                                Ok(checkpoint) => {
                                    let duration = start_time.elapsed();
                                    metrics::histogram!(
                                        "plexspaces_journaling_ddb_get_latest_checkpoint_duration_seconds",
                                        "backend" => "dynamodb"
                                    )
                                    .record(duration.as_secs_f64());
                                    metrics::counter!(
                                        "plexspaces_journaling_ddb_get_latest_checkpoint_total",
                                        "backend" => "dynamodb",
                                        "result" => "found"
                                    )
                                    .increment(1);
                                    return Ok(checkpoint);
                                }
                                Err(e) => {
                                    return Err(crate::JournalError::Storage(format!(
                                        "Failed to decode checkpoint: {}",
                                        e
                                    )));
                                }
                            }
                        }
                    }
                }

                metrics::counter!(
                    "plexspaces_journaling_ddb_get_latest_checkpoint_total",
                    "backend" => "dynamodb",
                    "result" => "not_found"
                )
                .increment(1);
                Err(crate::JournalError::CheckpointNotFound(actor_id.to_string()))
            }
            Err(e) => {
                error!(error = %e, actor_id = %actor_id, "Failed to get checkpoint");
                Err(crate::JournalError::Storage(format!("DynamoDB get_item failed: {}", e)))
            }
        }
    }

    async fn save_checkpoint(&self, checkpoint: &Checkpoint) -> JournalResult<()> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-checkpoints", self.table_prefix);
        let pk = checkpoint.actor_id.clone();

        // Serialize checkpoint to binary
        let mut checkpoint_data = Vec::new();
        checkpoint.encode(&mut checkpoint_data)
            .map_err(|e| crate::JournalError::Storage(format!("Failed to encode checkpoint: {}", e)))?;

        let timestamp_secs = checkpoint.timestamp.as_ref().map(|ts| ts.seconds).unwrap_or(Utc::now().timestamp());
        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("CHECKPOINT".to_string()));
        item.insert("actor_id".to_string(), AttributeValue::S(checkpoint.actor_id.clone()));
        item.insert("sequence".to_string(), AttributeValue::N(checkpoint.sequence.to_string()));
        item.insert("checkpoint_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(checkpoint_data)));
        item.insert("timestamp".to_string(), AttributeValue::N(timestamp_secs.to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

        match self
            .client
            .put_item()
            .table_name(&table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_journaling_ddb_save_checkpoint_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_journaling_ddb_save_checkpoint_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, actor_id = %checkpoint.actor_id, "Failed to save checkpoint");
                metrics::counter!(
                    "plexspaces_journaling_ddb_save_checkpoint_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(crate::JournalError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn truncate_to(&self, actor_id: &str, sequence: u64) -> JournalResult<u64> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-entries", self.table_prefix);
        let pk = actor_id.to_string();

        // Query all entries up to sequence
        let mut entries_to_delete = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("ENTRY#".to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        if let Some(sk_attr) = item.get("sk") {
                            if let Ok(sk) = sk_attr.as_s() {
                                if let Some(seq_str) = sk.strip_prefix("ENTRY#") {
                                    if let Ok(seq) = seq_str.parse::<u64>() {
                                        if seq <= sequence {
                                            entries_to_delete.push((pk.clone(), sk.clone()));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, actor_id = %actor_id, "Failed to query entries for truncation");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        // Delete entries in batches
        const BATCH_SIZE: usize = 25;
        let mut deleted_count = 0;

        for chunk in entries_to_delete.chunks(BATCH_SIZE) {
            let mut write_requests = Vec::new();

            for (pk_val, sk_val) in chunk {
                let delete_request = aws_sdk_dynamodb::types::DeleteRequest::builder()
                    .key("pk", AttributeValue::S(pk_val.clone()))
                    .key("sk", AttributeValue::S(sk_val.clone()))
                    .build()
                    .map_err(|e| crate::JournalError::Storage(format!("Failed to build delete request: {}", e)))?;
                
                let write_request = aws_sdk_dynamodb::types::WriteRequest::builder()
                    .delete_request(delete_request)
                    .build();
                
                write_requests.push(write_request);
            }

            match self
                .client
                .batch_write_item()
                .request_items(&table_name, write_requests)
                .send()
                .await
            {
                Ok(_) => {
                    deleted_count += chunk.len();
                }
                Err(e) => {
                    error!(error = %e, "Failed to delete entries in batch");
                    // Continue with next batch
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_truncate_to_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_truncate_to_total",
            "backend" => "dynamodb",
            "result" => "success",
            "deleted" => deleted_count.to_string()
        )
        .increment(1);

        Ok(deleted_count as u64)
    }

    async fn get_stats(&self, actor_id: Option<&str>) -> JournalResult<JournalStats> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-entries", self.table_prefix);
        let checkpoints_table_name = format!("{}-checkpoints", self.table_prefix);

        let mut stats = JournalStats {
            total_entries: 0,
            total_checkpoints: 0,
            storage_bytes: 0,
            entries_by_actor: std::collections::HashMap::new(),
            oldest_entry: None,
            newest_entry: None,
        };

        if let Some(aid) = actor_id {
            // Stats for specific actor
            let pk = aid.to_string();
            let mut last_evaluated_key = None;
            let mut entry_count = 0u64;
            let mut oldest_ts: Option<i64> = None;
            let mut newest_ts: Option<i64> = None;

            // Count entries for this actor
            loop {
                let mut query = self
                    .client
                    .query()
                    .table_name(&table_name)
                    .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                    .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                    .expression_attribute_values(":sk_prefix", AttributeValue::S("ENTRY#".to_string()))
                    .select(aws_sdk_dynamodb::types::Select::AllAttributes);

                if let Some(lek) = last_evaluated_key {
                    query = query.set_exclusive_start_key(Some(lek));
                }

                match query.send().await {
                    Ok(result) => {
                        entry_count += result.count() as u64;
                        for item in result.items() {
                            if let Some(ts_attr) = item.get("timestamp") {
                                if let Ok(ts_str) = ts_attr.as_n() {
                                    if let Ok(ts) = ts_str.parse::<i64>() {
                                        oldest_ts = oldest_ts.map(|o| o.min(ts)).or(Some(ts));
                                        newest_ts = newest_ts.map(|n| n.max(ts)).or(Some(ts));
                                    }
                                }
                            }
                        }
                        last_evaluated_key = result.last_evaluated_key().cloned();
                        if last_evaluated_key.is_none() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, actor_id = %aid, "Failed to query entries for stats");
                        break;
                    }
                }
            }

            stats.total_entries = entry_count;
            stats.entries_by_actor.insert(aid.to_string(), entry_count);
            if let Some(ts) = oldest_ts {
                stats.oldest_entry = chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| prost_types::Timestamp {
                        seconds: dt.timestamp(),
                        nanos: dt.timestamp_subsec_nanos() as i32,
                    });
            }
            if let Some(ts) = newest_ts {
                stats.newest_entry = chrono::DateTime::from_timestamp(ts, 0)
                    .map(|dt| prost_types::Timestamp {
                        seconds: dt.timestamp(),
                        nanos: dt.timestamp_subsec_nanos() as i32,
                    });
            }

            // Count checkpoints for this actor
            let mut checkpoint_count = 0u64;
            let mut last_evaluated_key = None;
            loop {
                let mut query = self
                    .client
                    .query()
                    .table_name(&checkpoints_table_name)
                    .key_condition_expression("pk = :pk AND sk = :sk")
                    .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                    .expression_attribute_values(":sk", AttributeValue::S("CHECKPOINT".to_string()))
                    .select(aws_sdk_dynamodb::types::Select::Count);

                if let Some(lek) = last_evaluated_key {
                    query = query.set_exclusive_start_key(Some(lek));
                }

                match query.send().await {
                    Ok(result) => {
                        checkpoint_count += result.count() as u64;
                        last_evaluated_key = result.last_evaluated_key().cloned();
                        if last_evaluated_key.is_none() {
                            break;
                        }
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
            stats.total_checkpoints = checkpoint_count;
        } else {
            // Global stats - would require scanning, which is expensive
            // For now, return empty stats (can be enhanced with DynamoDB Streams + Lambda)
            warn!("Global stats not implemented for DynamoDB (would require expensive scan)");
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_get_stats_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        Ok(stats)
    }

    async fn flush(&self) -> JournalResult<()> {
        // DynamoDB writes are immediately durable (no buffering)
        // This is a no-op but we record metrics
        metrics::counter!(
            "plexspaces_journaling_ddb_flush_total",
            "backend" => "dynamodb"
        )
        .increment(1);
        Ok(())
    }

    // ==================== Event Sourcing Methods ====================

    async fn append_event(&self, event: &ActorEvent) -> JournalResult<u64> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-events", self.table_prefix);
        let pk = event.actor_id.clone();
        let sk = format!("EVENT#{}", event.sequence);

        // Serialize event to binary
        let mut event_data = Vec::new();
        event.encode(&mut event_data)
            .map_err(|e| crate::JournalError::Storage(format!("Failed to encode event: {}", e)))?;

        let timestamp_secs = event.timestamp.as_ref().map(|ts| ts.seconds).unwrap_or(Utc::now().timestamp());
        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S(sk));
        item.insert("actor_id".to_string(), AttributeValue::S(event.actor_id.clone()));
        item.insert("sequence".to_string(), AttributeValue::N(event.sequence.to_string()));
        item.insert("event_type".to_string(), AttributeValue::S(event.event_type.clone()));
        item.insert("event_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(event_data)));
        // event_data already inserted above
        item.insert("timestamp".to_string(), AttributeValue::N(timestamp_secs.to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

        match self
            .client
            .put_item()
            .table_name(&table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_journaling_ddb_append_event_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_journaling_ddb_append_event_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(event.sequence)
            }
            Err(e) => {
                error!(error = %e, actor_id = %event.actor_id, sequence = event.sequence, "Failed to append event");
                metrics::counter!(
                    "plexspaces_journaling_ddb_append_event_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(crate::JournalError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn append_events_batch(&self, events: &[ActorEvent]) -> JournalResult<(u64, u64, usize)> {
        if events.is_empty() {
            return Ok((0, 0, 0));
        }

        let start_time = std::time::Instant::now();
        let table_name = format!("{}-events", self.table_prefix);

        const BATCH_SIZE: usize = 25;
        let mut first_sequence = events[0].sequence;
        let mut last_sequence = events[0].sequence;

        for chunk in events.chunks(BATCH_SIZE) {
            let mut write_requests = Vec::new();

            for event in chunk {
                let pk = event.actor_id.clone();
                let sk = format!("EVENT#{}", event.sequence);

                let mut event_data = Vec::new();
                event.encode(&mut event_data)
                    .map_err(|e| crate::JournalError::Storage(format!("Failed to encode event: {}", e)))?;

                let timestamp_secs = event.timestamp.as_ref().map(|ts| ts.seconds).unwrap_or(Utc::now().timestamp());
                let now_secs = Utc::now().timestamp();

                let mut item = HashMap::new();
                item.insert("pk".to_string(), AttributeValue::S(pk));
                item.insert("sk".to_string(), AttributeValue::S(sk));
                item.insert("actor_id".to_string(), AttributeValue::S(event.actor_id.clone()));
                item.insert("sequence".to_string(), AttributeValue::N(event.sequence.to_string()));
                item.insert("event_type".to_string(), AttributeValue::S(event.event_type.clone()));
                item.insert("event_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(event_data)));
                // event_data already inserted above
                item.insert("timestamp".to_string(), AttributeValue::N(timestamp_secs.to_string()));
                item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

                let put_request = aws_sdk_dynamodb::types::PutRequest::builder()
                    .set_item(Some(item))
                    .build()
                    .map_err(|e| crate::JournalError::Storage(format!("Failed to build put request: {}", e)))?;
                
                let write_request = aws_sdk_dynamodb::types::WriteRequest::builder()
                    .put_request(put_request)
                    .build();
                
                write_requests.push(write_request);

                if event.sequence < first_sequence {
                    first_sequence = event.sequence;
                }
                if event.sequence > last_sequence {
                    last_sequence = event.sequence;
                }
            }

            match self
                .client
                .batch_write_item()
                .request_items(&table_name, write_requests)
                .send()
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    return Err(crate::JournalError::Storage(format!(
                        "DynamoDB batch_write_item failed: {}",
                        e
                    )));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_append_events_batch_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_append_events_batch_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => events.len().to_string()
        )
        .increment(1);

        Ok((first_sequence, last_sequence, events.len()))
    }

    async fn replay_events_from(
        &self,
        actor_id: &str,
        from_sequence: u64,
    ) -> JournalResult<Vec<ActorEvent>> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-events", self.table_prefix);
        let pk = actor_id.to_string();

        let mut events = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("EVENT#".to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        if let Some(sk_attr) = item.get("sk") {
                            if let Ok(sk) = sk_attr.as_s() {
                                if let Some(seq_str) = sk.strip_prefix("EVENT#") {
                                    if let Ok(seq) = seq_str.parse::<u64>() {
                                        if seq >= from_sequence {
                                            // Deserialize event
                                            if let Some(event_data_attr) = item.get("event_data") {
                                                if let Ok(blob) = event_data_attr.as_b() {
                                                    match ActorEvent::decode(blob.as_ref()) {
                                                        Ok(event) => events.push(event),
                                                        Err(e) => {
                                                            warn!(error = %e, "Failed to decode event, skipping");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, actor_id = %actor_id, "Failed to replay events");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        // Sort by sequence
        events.sort_by(|a, b| a.sequence.cmp(&b.sequence));

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_replay_events_from_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_replay_events_from_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => events.len().to_string()
        )
        .increment(1);

        Ok(events)
    }

    async fn replay_events_from_paginated(
        &self,
        actor_id: &str,
        from_sequence: u64,
        page_request: &PageRequest,
    ) -> JournalResult<(Vec<ActorEvent>, PageResponse)> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-events", self.table_prefix);
        let pk = actor_id.to_string();

        let page_size = page_request.limit.max(1).min(1000) as usize;
        let start_seq = if page_request.offset > 0 {
            from_sequence.max(page_request.offset as u64)
        } else {
            from_sequence
        };

        let mut events = Vec::new();
        let mut last_evaluated_key = None;
        let mut found_start = start_seq == from_sequence;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("EVENT#".to_string()))
                .limit(page_size as i32);

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        if let Some(sk_attr) = item.get("sk") {
                            if let Ok(sk) = sk_attr.as_s() {
                                if let Some(seq_str) = sk.strip_prefix("EVENT#") {
                                    if let Ok(seq) = seq_str.parse::<u64>() {
                                        if !found_start && seq < start_seq {
                                            continue; // Skip until we reach start_seq
                                        }
                                        found_start = true;

                                        if seq >= from_sequence && events.len() < page_size {
                                            if let Some(event_data_attr) = item.get("event_data") {
                                                if let Ok(blob) = event_data_attr.as_b() {
                                                    match ActorEvent::decode(blob.as_ref()) {
                                                        Ok(event) => events.push(event),
                                                        Err(e) => {
                                                            warn!(error = %e, "Failed to decode event, skipping");
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        if events.len() >= page_size {
                            break;
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() || events.len() >= page_size {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, actor_id = %actor_id, "Failed to replay events paginated");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        // Sort by sequence
        events.sort_by(|a, b| a.sequence.cmp(&b.sequence));

        // Build page response
        let page_response = PageResponse {
            total_size: 0, // Total size not available without full scan
            offset: page_request.offset,
            limit: page_request.limit,
            has_next: last_evaluated_key.is_some(),
        };

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_replay_events_from_paginated_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_replay_events_from_paginated_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => events.len().to_string()
        )
        .increment(1);

        Ok((events, page_response))
    }

    async fn get_actor_history(&self, actor_id: &str) -> JournalResult<ActorHistory> {
        // Get all events for the actor
        let events = self.replay_events_from(actor_id, 0).await?;

        let latest_sequence = events.last().map(|e| e.sequence).unwrap_or(0);
        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at: None,
            updated_at: None,
            metadata: std::collections::HashMap::new(),
            page_response: None,
        })
    }

    async fn get_actor_history_paginated(
        &self,
        actor_id: &str,
        page_request: &PageRequest,
    ) -> JournalResult<ActorHistory> {
        let from_sequence = if page_request.offset > 0 {
            page_request.offset as u64
        } else {
            0
        };

        let (events, page_response) = self
            .replay_events_from_paginated(actor_id, from_sequence, page_request)
            .await?;

        let latest_sequence = events.last().map(|e| e.sequence).unwrap_or(0);
        Ok(ActorHistory {
            actor_id: actor_id.to_string(),
            events,
            latest_sequence,
            created_at: None,
            updated_at: None,
            metadata: std::collections::HashMap::new(),
            page_response: Some(page_response),
        })
    }

    // ==================== Reminder Methods ====================

    async fn register_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-reminders", self.table_prefix);
        let registration = reminder_state.registration.as_ref()
            .ok_or_else(|| crate::JournalError::Storage("ReminderState missing registration".to_string()))?;
        let pk = registration.actor_id.clone();
        let sk = format!("REMINDER#{}", registration.reminder_name);

        // Serialize reminder state to binary
        let mut reminder_data = Vec::new();
        reminder_state.encode(&mut reminder_data)
            .map_err(|e| crate::JournalError::Storage(format!("Failed to encode reminder: {}", e)))?;

        let next_fire_time_secs = reminder_state
            .next_fire_time
            .as_ref()
            .map(|ts| ts.seconds)
            .unwrap_or(0);
        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S(sk));
        item.insert("actor_id".to_string(), AttributeValue::S(registration.actor_id.clone()));
        item.insert("reminder_name".to_string(), AttributeValue::S(registration.reminder_name.clone()));
        item.insert("reminder_data".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(reminder_data)));
        item.insert("next_fire_time".to_string(), AttributeValue::N(next_fire_time_secs.to_string()));
        item.insert("is_active".to_string(), AttributeValue::S(if reminder_state.is_active { "1" } else { "0" }.to_string()));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));

        match self
            .client
            .put_item()
            .table_name(&table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_journaling_ddb_register_reminder_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_journaling_ddb_register_reminder_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, actor_id = %registration.actor_id, reminder_name = %registration.reminder_name, "Failed to register reminder");
                metrics::counter!(
                    "plexspaces_journaling_ddb_register_reminder_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(crate::JournalError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn unregister_reminder(&self, actor_id: &str, reminder_name: &str) -> JournalResult<()> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-reminders", self.table_prefix);
        let pk = actor_id.to_string();
        let sk = format!("REMINDER#{}", reminder_name);

        match self
            .client
            .delete_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(sk))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_journaling_ddb_unregister_reminder_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_journaling_ddb_unregister_reminder_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, actor_id = %actor_id, reminder_name = %reminder_name, "Failed to unregister reminder");
                metrics::counter!(
                    "plexspaces_journaling_ddb_unregister_reminder_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(crate::JournalError::Storage(format!("DynamoDB delete_item failed: {}", e)))
            }
        }
    }

    async fn load_reminders(&self, actor_id: &str) -> JournalResult<Vec<ReminderState>> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-reminders", self.table_prefix);
        let pk = actor_id.to_string();

        let mut reminders = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("REMINDER#".to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        // Only load active reminders
                        if let Some(is_active_attr) = item.get("is_active") {
                            if let Ok(is_active_str) = is_active_attr.as_s() {
                                if is_active_str != "1" {
                                    continue; // Skip inactive reminders
                                }
                            }
                        }

                        // Deserialize reminder
                        if let Some(reminder_data_attr) = item.get("reminder_data") {
                            if let Ok(blob) = reminder_data_attr.as_b() {
                                match ReminderState::decode(blob.as_ref()) {
                                    Ok(reminder) => reminders.push(reminder),
                                    Err(e) => {
                                        warn!(error = %e, "Failed to decode reminder, skipping");
                                    }
                                }
                            }
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, actor_id = %actor_id, "Failed to load reminders");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_load_reminders_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_load_reminders_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => reminders.len().to_string()
        )
        .increment(1);

        Ok(reminders)
    }

    async fn update_reminder(&self, reminder_state: &ReminderState) -> JournalResult<()> {
        // Update is same as register (put_item overwrites)
        self.register_reminder(reminder_state).await
    }

    async fn query_due_reminders(&self, before_time: SystemTime) -> JournalResult<Vec<ReminderState>> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-reminders", self.table_prefix);
        let before_time_secs = before_time.duration_since(std::time::UNIX_EPOCH)
            .unwrap_or(Duration::from_secs(0))
            .as_secs() as i64;

        let mut reminders = Vec::new();
        let mut last_evaluated_key = None;

        // Query using GSI: next_fire_time_index
        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .index_name("next_fire_time_index")
                .key_condition_expression("is_active = :active AND next_fire_time <= :before_time")
                .expression_attribute_values(":active", AttributeValue::S("1".to_string()))
                .expression_attribute_values(":before_time", AttributeValue::N(before_time_secs.to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        // Deserialize reminder
                        if let Some(reminder_data_attr) = item.get("reminder_data") {
                            if let Ok(blob) = reminder_data_attr.as_b() {
                                match ReminderState::decode(blob.as_ref()) {
                                    Ok(reminder) => {
                                        // Double-check is_active (defense in depth)
                                        if reminder.is_active {
                                            reminders.push(reminder);
                                        }
                                    }
                                    Err(e) => {
                                        warn!(error = %e, "Failed to decode reminder, skipping");
                                    }
                                }
                            }
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to query due reminders");
                    return Err(crate::JournalError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_journaling_ddb_query_due_reminders_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_journaling_ddb_query_due_reminders_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => reminders.len().to_string()
        )
        .increment(1);

        Ok(reminders)
    }
}

