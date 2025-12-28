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

//! DynamoDB-based lock manager implementation.
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for distributed lock/lease coordination
//! with proper tenant isolation, version-based optimistic locking, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Key**: `{tenant_id}#{namespace}#{lock_key}` for tenant isolation
//! - **Version-based optimistic locking**: Prevents lost updates
//! - **TTL support**: Uses DynamoDB TTL for automatic expiration cleanup
//! - **Auto-table creation**: Creates table with proper schema on initialization
//! - **Extensible schema**: Uses JSON for metadata, schema_version for future compatibility
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Table Schema
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{lock_key}"
//! Sort Key: sk = "LOCK" (for future extensibility)
//! Attributes:
//!   - holder_id: String (current lock holder)
//!   - version: String (ULID for optimistic locking)
//!   - expires_at: Number (UNIX timestamp in seconds)
//!   - lease_duration_secs: Number
//!   - last_heartbeat: Number (UNIX timestamp)
//!   - locked: Number (1 = locked, 0 = unlocked)
//!   - metadata: String (JSON-encoded)
//!   - schema_version: Number (for extensibility, default: 1)
//!   - ttl: Number (DynamoDB TTL attribute, expires_at + buffer)
//! ```
//!
//! ## GSI (Global Secondary Index)
//! - **GSI-1**: `tenant_namespace_index`
//!   - Partition Key: `tenant_id`
//!   - Sort Key: `namespace#expires_at`
//!   - Purpose: Efficient queries by tenant/namespace, expired lock cleanup
//!
//! ## Performance Optimizations
//! - Batch operations where possible
//! - Conditional writes for optimistic locking
//! - Connection pooling via AWS SDK
//! - Efficient key design for hot partition avoidance
//!
//! ## Observability
//! - Metrics: Operation latency, error rates, throughput
//! - Tracing: Distributed tracing with tenant context
//! - Logging: Structured logging with request context

use crate::{AcquireLockOptions, Lock, LockError, LockManager, LockResult, ReleaseLockOptions, RenewLockOptions};
use async_trait::async_trait;
use aws_sdk_dynamodb::{
    error::ProvideErrorMetadata,
    types::{AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement, KeyType, Projection, ProjectionType, ScalarAttributeType, TimeToLiveSpecification},
    Client as DynamoDbClient,
};
use chrono::{DateTime, Utc};
use plexspaces_common::RequestContext;
use plexspaces_proto::prost_types::Timestamp;
use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use tracing::{debug, error, instrument, warn};
use ulid::Ulid;

/// DynamoDB lock manager with production-grade observability.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_locks::{LockManager, ddb::DynamoDBLockManager};
/// use plexspaces_common::RequestContext;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let manager = DynamoDBLockManager::new(
///     "us-east-1".to_string(),
///     "plexspaces-locks".to_string(),
///     Some("http://localhost:8000".to_string()), // For local testing
/// ).await?;
///
/// let ctx = RequestContext::new_without_auth("tenant-1".to_string(), "namespace-1".to_string());
/// let lock = manager.acquire_lock(&ctx, options).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct DynamoDBLockManager {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name
    table_name: String,
    /// Schema version (for extensibility)
    schema_version: u32,
}

impl DynamoDBLockManager {
    /// Create a new DynamoDB lock manager.
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_name` - DynamoDB table name
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local testing)
    ///
    /// ## Behavior
    /// - Creates table if it doesn't exist (idempotent)
    /// - Sets up TTL for automatic expiration cleanup
    /// - Creates GSI for efficient queries
    ///
    /// ## Returns
    /// DynamoDBLockManager or error if table creation fails
    #[instrument(skip(region, table_name, endpoint_url), fields(region = %region, table_name = %table_name))]
    pub async fn new(
        region: String,
        table_name: String,
        endpoint_url: Option<String>,
    ) -> LockResult<Self> {
        let start_time = std::time::Instant::now();

        // Build AWS config
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.clone()));

        // Set endpoint URL if provided (for local testing)
        // The AWS SDK will automatically use AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY from env
        if let Some(endpoint) = endpoint_url {
            config_builder = config_builder.endpoint_url(endpoint);
        }

        let config = config_builder.load().await;
        let client = DynamoDbClient::new(&config);

        // Create table if it doesn't exist (idempotent)
        Self::ensure_table_exists(&client, &table_name).await?;

        // Enable TTL for automatic expiration cleanup
        Self::enable_ttl(&client, &table_name).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_locks_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        ).record(duration.as_secs_f64());

        debug!(
            table_name = %table_name,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB lock manager initialized"
        );

        Ok(Self {
            client,
            table_name,
            schema_version: 1,
        })
    }

    /// Ensure table exists, create if it doesn't.
    ///
    /// ## Schema
    /// - Partition Key: `pk` (String) - Composite: `{tenant_id}#{namespace}#{lock_key}`
    /// - Sort Key: `sk` (String) - Always "LOCK" for now (extensibility)
    /// - GSI: `tenant_namespace_index` for efficient queries
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_table_exists(client: &DynamoDbClient, table_name: &str) -> LockResult<()> {
        // Check if table exists
        match client
            .describe_table()
            .table_name(table_name)
            .send()
            .await
        {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB table already exists");
                return Ok(());
            }
            Err(e) => {
                // Table doesn't exist, create it
                let error_msg = format!("{}", e);
                let error_code = e.code().unwrap_or("unknown");
                let error_message = e.message().unwrap_or(&error_msg);
                
                debug!(
                    table_name = %table_name,
                    error_code = %error_code,
                    error_message = %error_message,
                    "DynamoDB describe_table result"
                );
                
                if !error_msg.contains("ResourceNotFoundException") && error_code != "ResourceNotFoundException" {
                    error!(
                        table_name = %table_name,
                        error_code = %error_code,
                        error_message = %error_message,
                        error = %e,
                        "DynamoDB describe_table failed with unexpected error"
                    );
                    return Err(LockError::BackendError(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB table");

        // Create table with composite key for tenant isolation
        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build attribute definition: {}", e)))?;

        let tenant_id_attr = AttributeDefinition::builder()
            .attribute_name("tenant_id")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build attribute definition: {}", e)))?;

        let namespace_expires_attr = AttributeDefinition::builder()
            .attribute_name("namespace_expires")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build attribute definition: {}", e)))?;

        let gsi_pk_schema = KeySchemaElement::builder()
            .attribute_name("tenant_id")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build GSI key schema: {}", e)))?;

        let gsi_sk_schema = KeySchemaElement::builder()
            .attribute_name("namespace_expires")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build GSI key schema: {}", e)))?;

        let gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let gsi = GlobalSecondaryIndex::builder()
            .index_name("tenant_namespace_index")
            .key_schema(gsi_pk_schema)
            .key_schema(gsi_sk_schema)
            .projection(gsi_projection)
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build GSI: {}", e)))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest) // On-demand for scalability
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(tenant_id_attr)
            .attribute_definitions(namespace_expires_attr)
            .global_secondary_indexes(gsi)
            .send()
            .await;

        match create_table_result {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB table created successfully");
                // Wait for table to be active
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                // Check if table was created concurrently
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(LockError::BackendError(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Wait for table to become active.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn wait_for_table_active(client: &DynamoDbClient, table_name: &str) -> LockResult<()> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30; // 30 seconds max wait

        loop {
            let describe_result = client
                .describe_table()
                .table_name(table_name)
                .send()
                .await
                .map_err(|e| {
                    LockError::BackendError(format!("Failed to describe table: {}", e))
                })?;

            if let Some(status) = describe_result.table().and_then(|t| t.table_status()) {
                match status {
                    TableStatus::Active => {
                        debug!(table_name = %table_name, "Table is now active");
                        return Ok(());
                    }
                    TableStatus::Creating => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(LockError::BackendError(format!(
                                "Table creation timeout after {} attempts",
                                max_attempts
                            )));
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(LockError::BackendError(format!(
                            "Table in unexpected status: {:?}",
                            status
                        )));
                    }
                }
            } else {
                return Err(LockError::BackendError(
                    "Table status not available".to_string(),
                ));
            }
        }
    }

    /// Enable TTL on the table for automatic expiration cleanup.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn enable_ttl(client: &DynamoDbClient, table_name: &str) -> LockResult<()> {
        let ttl_spec = TimeToLiveSpecification::builder()
            .enabled(true)
            .attribute_name("ttl")
            .build()
            .map_err(|e| LockError::BackendError(format!("Failed to build TTL spec: {}", e)))?;

        match client
            .update_time_to_live()
            .table_name(table_name)
            .time_to_live_specification(ttl_spec)
            .send()
            .await
        {
            Ok(_) => {
                debug!(table_name = %table_name, "TTL enabled for automatic expiration cleanup");
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                // TTL might already be enabled, ignore that error
                if error_str.contains("TimeToLiveAlreadyEnabled") {
                    debug!(table_name = %table_name, "TTL already enabled");
                    Ok(())
                } else {
                    warn!(
                        error = %e,
                        table_name = %table_name,
                        "Failed to enable TTL (non-critical, continuing)"
                    );
                    // Non-critical, continue anyway
                    Ok(())
                }
            }
        }
    }

    /// Create composite partition key from tenant_id, namespace, and lock_key.
    /// Format: "{tenant_id}#{namespace}#{lock_key}"
    fn composite_key(ctx: &RequestContext, lock_key: &str) -> String {
        let tenant_id = if ctx.tenant_id().is_empty() {
            "default"
        } else {
            ctx.tenant_id()
        };
        let namespace = if ctx.namespace().is_empty() {
            "default"
        } else {
            ctx.namespace()
        };
        format!("{}#{}#{}", tenant_id, namespace, lock_key)
    }

    /// Create GSI sort key for namespace and expiration queries.
    /// Format: "{namespace}#{expires_at}"
    fn namespace_expires_key(namespace: &str, expires_at: i64) -> String {
        format!("{}#{}", namespace, expires_at)
    }

    /// Convert DynamoDB item to Lock.
    fn item_to_lock(item: &HashMap<String, AttributeValue>) -> LockResult<Lock> {
        let pk = item
            .get("pk")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| LockError::BackendError("Missing pk attribute".to_string()))?;

        // Extract lock_key from composite key: "{tenant_id}#{namespace}#{lock_key}"
        let lock_key = pk
            .split('#')
            .nth(2)
            .ok_or_else(|| LockError::BackendError("Invalid pk format".to_string()))?
            .to_string();

        let holder_id = item
            .get("holder_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| LockError::BackendError("Missing holder_id".to_string()))?
            .to_string();

        let version = item
            .get("version")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| LockError::BackendError("Missing version".to_string()))?
            .to_string();

        let expires_at_secs = item
            .get("expires_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| LockError::BackendError("Missing or invalid expires_at".to_string()))?;

        let lease_duration_secs = item
            .get("lease_duration_secs")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<u32>().ok())
            .ok_or_else(|| LockError::BackendError("Missing or invalid lease_duration_secs".to_string()))?;

        let last_heartbeat_secs = item
            .get("last_heartbeat")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .ok_or_else(|| LockError::BackendError("Missing or invalid last_heartbeat".to_string()))?;

        let locked = item
            .get("locked")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .map(|n| n != 0)
            .unwrap_or(false);

        let metadata: HashMap<String, String> = item
            .get("metadata")
            .and_then(|v| v.as_s().ok())
            .and_then(|json| serde_json::from_str(json).ok())
            .unwrap_or_default();

        // Convert UNIX timestamps to Timestamp
        let expires_at = SystemTime::UNIX_EPOCH + Duration::from_secs(expires_at_secs as u64);
        let last_heartbeat = SystemTime::UNIX_EPOCH + Duration::from_secs(last_heartbeat_secs as u64);

        Ok(Lock {
            lock_key,
            holder_id,
            version,
            expires_at: Some(Timestamp::from(expires_at)),
            lease_duration_secs,
            last_heartbeat: Some(Timestamp::from(last_heartbeat)),
            metadata,
            locked,
        })
    }

    /// Convert Lock to DynamoDB item.
    fn lock_to_item(
        &self,
        ctx: &RequestContext,
        lock: &Lock,
        expires_at_secs: i64,
        last_heartbeat_secs: i64,
    ) -> HashMap<String, AttributeValue> {
        let tenant_id = if ctx.tenant_id().is_empty() {
            "default"
        } else {
            ctx.tenant_id()
        };
        let namespace = if ctx.namespace().is_empty() {
            "default"
        } else {
            ctx.namespace()
        };

        let pk = Self::composite_key(ctx, &lock.lock_key);
        let namespace_expires = Self::namespace_expires_key(namespace, expires_at_secs);

        // TTL: expires_at + 1 day buffer for DynamoDB cleanup
        let ttl_secs = expires_at_secs + 86400;

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("LOCK".to_string()));
        item.insert("tenant_id".to_string(), AttributeValue::S(tenant_id.to_string()));
        item.insert("namespace_expires".to_string(), AttributeValue::S(namespace_expires));
        item.insert("holder_id".to_string(), AttributeValue::S(lock.holder_id.clone()));
        item.insert("version".to_string(), AttributeValue::S(lock.version.clone()));
        item.insert(
            "expires_at".to_string(),
            AttributeValue::N(expires_at_secs.to_string()),
        );
        item.insert(
            "lease_duration_secs".to_string(),
            AttributeValue::N(lock.lease_duration_secs.to_string()),
        );
        item.insert(
            "last_heartbeat".to_string(),
            AttributeValue::N(last_heartbeat_secs.to_string()),
        );
        item.insert(
            "locked".to_string(),
            AttributeValue::N(if lock.locked { "1" } else { "0" }.to_string()),
        );
        item.insert(
            "schema_version".to_string(),
            AttributeValue::N(self.schema_version.to_string()),
        );
        item.insert("ttl".to_string(), AttributeValue::N(ttl_secs.to_string()));

        // Metadata as JSON string
        if !lock.metadata.is_empty() {
            if let Ok(json) = serde_json::to_string(&lock.metadata) {
                item.insert("metadata".to_string(), AttributeValue::S(json));
            }
        }

        item
    }
}

#[async_trait]
impl LockManager for DynamoDBLockManager {
    #[instrument(
        skip(self, ctx, options),
        fields(
            lock_key = %options.lock_key,
            holder_id = %options.holder_id,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace(),
            lease_duration_secs = options.lease_duration_secs
        )
    )]
    async fn acquire_lock(&self, ctx: &RequestContext, options: AcquireLockOptions) -> LockResult<Lock> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, &options.lock_key);
        let now = Utc::now();
        let now_secs = now.timestamp();
        let expires_at_secs = now_secs + options.lease_duration_secs as i64;

        // Try to get existing lock
        let get_result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S("LOCK".to_string()))
            .send()
            .await;

        let existing_item = match get_result {
            Ok(result) => result.item().cloned(),
            Err(e) => {
                error!(
                    error = %e,
                    lock_key = %options.lock_key,
                    "Failed to get existing lock from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_locks_ddb_acquire_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "get_item_failed"
                ).increment(1);
                return Err(LockError::BackendError(format!("DynamoDB get_item failed: {}", e)));
            }
        };

        if let Some(item) = existing_item {
            // Lock exists, check if expired or held by same holder
            let existing_lock = Self::item_to_lock(&item)?;
            let expires_dt = DateTime::<Utc>::from_timestamp(
                existing_lock.expires_at.as_ref().map(|t| t.seconds).unwrap_or(0),
                existing_lock.expires_at.as_ref().map(|t| t.nanos as u32).unwrap_or(0),
            )
            .ok_or_else(|| LockError::BackendError("Invalid expiration timestamp".to_string()))?;

            let expired = expires_dt < now || !existing_lock.locked;

            let existing_version = existing_lock.version.clone();
            let existing_holder_id = existing_lock.holder_id.clone();

            if !expired && existing_holder_id != options.holder_id {
                // Lock is held by someone else
                metrics::counter!(
                    "plexspaces_locks_ddb_acquire_already_held_total",
                    "backend" => "dynamodb"
                ).increment(1);
                return Err(LockError::LockAlreadyHeld(existing_holder_id));
            }

            // If same holder and not expired, return existing lock
            if !expired && existing_holder_id == options.holder_id {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_locks_ddb_acquire_duration_seconds",
                    "backend" => "dynamodb"
                ).record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_locks_ddb_acquire_total",
                    "backend" => "dynamodb",
                    "result" => "existing"
                ).increment(1);
                return Ok(existing_lock);
            }

            // Either expired or held by same holder - update with new version
            let new_version = Ulid::new().to_string();
            let new_lock = Lock {
                lock_key: options.lock_key.clone(),
                holder_id: options.holder_id.clone(),
                version: new_version.clone(),
                expires_at: Some(Timestamp {
                    seconds: expires_at_secs,
                    nanos: 0,
                }),
                lease_duration_secs: options.lease_duration_secs,
                last_heartbeat: Some(Timestamp {
                    seconds: now_secs,
                    nanos: 0,
                }),
                metadata: if options.metadata.is_empty() {
                    existing_lock.metadata
                } else {
                    options.metadata
                },
                locked: true,
            };

            let item = self.lock_to_item(ctx, &new_lock, expires_at_secs, now_secs);

            // Conditional update: only if expired or version matches
            let mut update_request = self
                .client
                .put_item()
                .table_name(&self.table_name)
                .set_item(Some(item));

            if expired {
                // If expired, we can acquire it
                update_request = update_request
                    .condition_expression("expires_at < :now OR locked = :zero")
                    .expression_attribute_values(":now", AttributeValue::N(now_secs.to_string()))
                    .expression_attribute_values(":zero", AttributeValue::N("0".to_string()));
            } else {
                // Should not reach here (already handled same holder case above)
                // But if we do, use version check
                update_request = update_request
                    .condition_expression("version = :old_version")
                    .expression_attribute_values(":old_version", AttributeValue::S(existing_version));
            }

            match update_request.send().await {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!(
                        "plexspaces_locks_ddb_acquire_duration_seconds",
                        "backend" => "dynamodb"
                    ).record(duration.as_secs_f64());
                    metrics::counter!(
                        "plexspaces_locks_ddb_acquire_total",
                        "backend" => "dynamodb",
                        "result" => "success"
                    ).increment(1);

                    debug!(
                        lock_key = %options.lock_key,
                        holder_id = %options.holder_id,
                        version = %new_version,
                        duration_ms = duration.as_millis(),
                        "Lock acquired successfully"
                    );

                    Ok(new_lock)
                }
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("ConditionalCheckFailedException") {
                        // Version mismatch or lock acquired by another process
                        metrics::counter!(
                            "plexspaces_locks_ddb_acquire_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "conditional_check_failed"
                        ).increment(1);
                        Err(LockError::LockAlreadyHeld("Lock acquired by another process".to_string()))
                    } else {
                        error!(
                            error = %e,
                            lock_key = %options.lock_key,
                            "Failed to update lock in DynamoDB"
                        );
                        metrics::counter!(
                            "plexspaces_locks_ddb_acquire_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "put_item_failed"
                        ).increment(1);
                        Err(LockError::BackendError(format!("DynamoDB put_item failed: {}", e)))
                    }
                }
            }
        } else {
            // No existing lock - create new
            let version = Ulid::new().to_string();
            let new_lock = Lock {
                lock_key: options.lock_key.clone(),
                holder_id: options.holder_id.clone(),
                version: version.clone(),
                expires_at: Some(Timestamp {
                    seconds: expires_at_secs,
                    nanos: 0,
                }),
                lease_duration_secs: options.lease_duration_secs,
                last_heartbeat: Some(Timestamp {
                    seconds: now_secs,
                    nanos: 0,
                }),
                metadata: options.metadata,
                locked: true,
            };

            let item = self.lock_to_item(ctx, &new_lock, expires_at_secs, now_secs);

            // Conditional put: only if lock doesn't exist
            match self
                .client
                .put_item()
                .table_name(&self.table_name)
                .set_item(Some(item))
                .condition_expression("attribute_not_exists(pk)")
                .send()
                .await
            {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!(
                        "plexspaces_locks_ddb_acquire_duration_seconds",
                        "backend" => "dynamodb"
                    ).record(duration.as_secs_f64());
                    metrics::counter!(
                        "plexspaces_locks_ddb_acquire_total",
                        "backend" => "dynamodb",
                        "result" => "success"
                    ).increment(1);

                    debug!(
                        lock_key = %options.lock_key,
                        holder_id = %options.holder_id,
                        version = %version,
                        duration_ms = duration.as_millis(),
                        "Lock created successfully"
                    );

                    Ok(new_lock)
                }
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("ConditionalCheckFailedException") {
                        // Lock was created concurrently
                        metrics::counter!(
                            "plexspaces_locks_ddb_acquire_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "concurrent_creation"
                        ).increment(1);
                        Err(LockError::LockAlreadyHeld("Lock created concurrently".to_string()))
                    } else {
                        error!(
                            error = %e,
                            lock_key = %options.lock_key,
                            "Failed to create lock in DynamoDB"
                        );
                        metrics::counter!(
                            "plexspaces_locks_ddb_acquire_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "put_item_failed"
                        ).increment(1);
                        Err(LockError::BackendError(format!("DynamoDB put_item failed: {}", e)))
                    }
                }
            }
        }
    }

    #[instrument(
        skip(self, ctx, options),
        fields(
            lock_key = %options.lock_key,
            holder_id = %options.holder_id,
            version = %options.version,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn renew_lock(&self, ctx: &RequestContext, options: RenewLockOptions) -> LockResult<Lock> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, &options.lock_key);
        let now = Utc::now();
        let now_secs = now.timestamp();
        let new_expires_at_secs = now_secs + options.lease_duration_secs as i64;

        // Get existing lock
        let get_result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S("LOCK".to_string()))
            .send()
            .await
            .map_err(|e| {
                error!(
                    error = %e,
                    lock_key = %options.lock_key,
                    "Failed to get lock from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_locks_ddb_renew_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "get_item_failed"
                ).increment(1);
                LockError::BackendError(format!("DynamoDB get_item failed: {}", e))
            })?;

        let item = get_result
            .item()
            .cloned()
            .ok_or_else(|| {
                metrics::counter!(
                    "plexspaces_locks_ddb_renew_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "not_found"
                ).increment(1);
                LockError::LockNotFound(options.lock_key.clone())
            })?;

        let existing_lock = Self::item_to_lock(&item)?;

        let existing_version = existing_lock.version.clone();
        let existing_holder_id = existing_lock.holder_id.clone();
        let request_version = options.version.clone();

        // Validate version (optimistic locking)
        if existing_version != request_version {
            metrics::counter!(
                "plexspaces_locks_ddb_renew_errors_total",
                "backend" => "dynamodb",
                "error_type" => "version_mismatch"
            ).increment(1);
            return Err(LockError::VersionMismatch {
                expected: existing_version,
                actual: request_version,
            });
        }

        // Validate holder
        if existing_holder_id != options.holder_id {
            metrics::counter!(
                "plexspaces_locks_ddb_renew_errors_total",
                "backend" => "dynamodb",
                "error_type" => "wrong_holder"
            ).increment(1);
            return Err(LockError::LockAlreadyHeld(existing_holder_id));
        }

        // Check if expired
        if let Some(expires) = &existing_lock.expires_at {
            let expires_dt = DateTime::<Utc>::from_timestamp(expires.seconds, expires.nanos as u32)
                .ok_or_else(|| LockError::BackendError("Invalid expiration timestamp".to_string()))?;
            if expires_dt < now {
                metrics::counter!(
                    "plexspaces_locks_ddb_renew_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "expired"
                ).increment(1);
                return Err(LockError::LockExpired(options.lock_key.clone()));
            }
        }

        // Renew lock with new version
        let new_version = Ulid::new().to_string();
        let renewed_lock = Lock {
            lock_key: options.lock_key.clone(),
            holder_id: options.holder_id.clone(),
            version: new_version.clone(),
            expires_at: Some(Timestamp {
                seconds: new_expires_at_secs,
                nanos: 0,
            }),
            lease_duration_secs: options.lease_duration_secs,
            last_heartbeat: Some(Timestamp {
                seconds: now_secs,
                nanos: 0,
            }),
            metadata: if options.metadata.is_empty() {
                existing_lock.metadata
            } else {
                options.metadata
            },
            locked: true,
        };

        let item = self.lock_to_item(ctx, &renewed_lock, new_expires_at_secs, now_secs);

        // Conditional update: only if version matches
        match self
            .client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .condition_expression("version = :old_version")
            .expression_attribute_values(":old_version", AttributeValue::S(request_version.clone()))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_locks_ddb_renew_duration_seconds",
                    "backend" => "dynamodb"
                ).record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_locks_ddb_renew_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                ).increment(1);

                debug!(
                    lock_key = %options.lock_key,
                    holder_id = %options.holder_id,
                    new_version = %new_version,
                    duration_ms = duration.as_millis(),
                    "Lock renewed successfully"
                );

                Ok(renewed_lock)
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("ConditionalCheckFailedException") {
                    // Version mismatch
                    metrics::counter!(
                        "plexspaces_locks_ddb_renew_errors_total",
                        "backend" => "dynamodb",
                        "error_type" => "version_mismatch"
                    ).increment(1);
                    Err(LockError::VersionMismatch {
                        expected: existing_version,
                        actual: request_version,
                    })
                } else {
                    error!(
                        error = %e,
                        lock_key = %options.lock_key,
                        "Failed to renew lock in DynamoDB"
                    );
                    metrics::counter!(
                        "plexspaces_locks_ddb_renew_errors_total",
                        "backend" => "dynamodb",
                        "error_type" => "put_item_failed"
                    ).increment(1);
                    Err(LockError::BackendError(format!("DynamoDB put_item failed: {}", e)))
                }
            }
        }
    }

    #[instrument(
        skip(self, ctx, options),
        fields(
            lock_key = %options.lock_key,
            holder_id = %options.holder_id,
            version = %options.version,
            delete_lock = options.delete_lock,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn release_lock(&self, ctx: &RequestContext, options: ReleaseLockOptions) -> LockResult<()> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, &options.lock_key);

        // Get existing lock
        let get_result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk.clone()))
            .key("sk", AttributeValue::S("LOCK".to_string()))
            .send()
            .await
            .map_err(|e| {
                error!(
                    error = %e,
                    lock_key = %options.lock_key,
                    "Failed to get lock from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_locks_ddb_release_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "get_item_failed"
                ).increment(1);
                LockError::BackendError(format!("DynamoDB get_item failed: {}", e))
            })?;

        let item = get_result
            .item()
            .cloned()
            .ok_or_else(|| {
                metrics::counter!(
                    "plexspaces_locks_ddb_release_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "not_found"
                ).increment(1);
                LockError::LockNotFound(options.lock_key.clone())
            })?;

        let existing_lock = Self::item_to_lock(&item)?;

        let existing_version = existing_lock.version.clone();
        let existing_holder_id = existing_lock.holder_id.clone();
        let request_version = options.version.clone();

        // Validate version
        if existing_version != request_version {
            metrics::counter!(
                "plexspaces_locks_ddb_release_errors_total",
                "backend" => "dynamodb",
                "error_type" => "version_mismatch"
            ).increment(1);
            return Err(LockError::VersionMismatch {
                expected: existing_version,
                actual: request_version.clone(),
            });
        }

        // Validate holder
        if existing_holder_id != options.holder_id {
            metrics::counter!(
                "plexspaces_locks_ddb_release_errors_total",
                "backend" => "dynamodb",
                "error_type" => "wrong_holder"
            ).increment(1);
            return Err(LockError::LockAlreadyHeld(existing_holder_id));
        }

        if options.delete_lock {
            // Delete lock
            match self
                .client
                .delete_item()
                .table_name(&self.table_name)
                .key("pk", AttributeValue::S(pk))
                .key("sk", AttributeValue::S("LOCK".to_string()))
                .condition_expression("version = :version")
                .expression_attribute_values(":version", AttributeValue::S(request_version.clone()))
                .send()
                .await
            {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!(
                        "plexspaces_locks_ddb_release_duration_seconds",
                        "backend" => "dynamodb"
                    ).record(duration.as_secs_f64());
                    metrics::counter!(
                        "plexspaces_locks_ddb_release_total",
                        "backend" => "dynamodb",
                        "result" => "deleted"
                    ).increment(1);

                    debug!(
                        lock_key = %options.lock_key,
                        duration_ms = duration.as_millis(),
                        "Lock deleted successfully"
                    );
                    Ok(())
                }
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("ConditionalCheckFailedException") {
                        metrics::counter!(
                            "plexspaces_locks_ddb_release_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "version_mismatch"
                        ).increment(1);
                        Err(LockError::VersionMismatch {
                            expected: existing_version,
                            actual: request_version.clone(),
                        })
                    } else {
                        error!(
                            error = %e,
                            lock_key = %options.lock_key,
                            "Failed to delete lock from DynamoDB"
                        );
                        metrics::counter!(
                            "plexspaces_locks_ddb_release_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "delete_item_failed"
                        ).increment(1);
                        Err(LockError::BackendError(format!("DynamoDB delete_item failed: {}", e)))
                    }
                }
            }
        } else {
            // Update lock to set locked = false
            let mut updated_lock = existing_lock;
            updated_lock.locked = false;

            let now_secs = Utc::now().timestamp();
            let item = self.lock_to_item(ctx, &updated_lock, updated_lock.expires_at.as_ref().map(|t| t.seconds).unwrap_or(now_secs), now_secs);

            match self
                .client
                .put_item()
                .table_name(&self.table_name)
                .set_item(Some(item))
                .condition_expression("version = :old_version")
                .expression_attribute_values(":old_version", AttributeValue::S(request_version.clone()))
                .send()
                .await
            {
                Ok(_) => {
                    let duration = start_time.elapsed();
                    metrics::histogram!(
                        "plexspaces_locks_ddb_release_duration_seconds",
                        "backend" => "dynamodb"
                    ).record(duration.as_secs_f64());
                    metrics::counter!(
                        "plexspaces_locks_ddb_release_total",
                        "backend" => "dynamodb",
                        "result" => "unlocked"
                    ).increment(1);

                    debug!(
                        lock_key = %options.lock_key,
                        duration_ms = duration.as_millis(),
                        "Lock released (unlocked) successfully"
                    );
                    Ok(())
                }
                Err(e) => {
                    let error_str = e.to_string();
                    if error_str.contains("ConditionalCheckFailedException") {
                        metrics::counter!(
                            "plexspaces_locks_ddb_release_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "version_mismatch"
                        ).increment(1);
                        Err(LockError::VersionMismatch {
                            expected: existing_version,
                            actual: request_version.clone(),
                        })
                    } else {
                        error!(
                            error = %e,
                            lock_key = %options.lock_key,
                            "Failed to update lock in DynamoDB"
                        );
                        metrics::counter!(
                            "plexspaces_locks_ddb_release_errors_total",
                            "backend" => "dynamodb",
                            "error_type" => "put_item_failed"
                        ).increment(1);
                        Err(LockError::BackendError(format!("DynamoDB put_item failed: {}", e)))
                    }
                }
            }
        }
    }

    #[instrument(
        skip(self, ctx),
        fields(
            lock_key = %lock_key,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> LockResult<Option<Lock>> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, lock_key);

        match self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("LOCK".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_locks_ddb_get_duration_seconds",
                    "backend" => "dynamodb"
                ).record(duration.as_secs_f64());

                if let Some(item) = result.item().cloned() {
                    let lock = Self::item_to_lock(&item)?;
                    metrics::counter!(
                        "plexspaces_locks_ddb_get_total",
                        "backend" => "dynamodb",
                        "result" => "found"
                    ).increment(1);
                    Ok(Some(lock))
                } else {
                    metrics::counter!(
                        "plexspaces_locks_ddb_get_total",
                        "backend" => "dynamodb",
                        "result" => "not_found"
                    ).increment(1);
                    Ok(None)
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    lock_key = %lock_key,
                    "Failed to get lock from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_locks_ddb_get_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "get_item_failed"
                ).increment(1);
                Err(LockError::BackendError(format!("DynamoDB get_item failed: {}", e)))
            }
        }
    }
}

