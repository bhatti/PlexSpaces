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

//! DynamoDB-based scheduling state store implementation.
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for scheduling request storage
//! with proper tenant isolation, efficient status queries, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Key**: `{tenant_id}#{namespace}#{request_id}` for tenant isolation
//! - **GSI for status queries**: `status_created_index` for efficient PENDING queries
//! - **Auto-table creation**: Creates table with proper schema on initialization
//! - **Extensible schema**: Uses JSON for metadata, schema_version for future compatibility
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Table Schema
//! ```text
//! Partition Key (pk): "{tenant_id}#{namespace}#{request_id}"
//! Sort Key (sk): "REQUEST" (for future extensibility)
//! Attributes:
//!   - request_id: String
//!   - status: String (PENDING, SCHEDULED, FAILED)
//!   - requirements_json: String (base64-encoded proto)
//!   - namespace: String
//!   - tenant_id: String
//!   - selected_node_id: String
//!   - actor_id: String
//!   - error_message: String
//!   - created_at: Number (UNIX timestamp in seconds)
//!   - scheduled_at: Number (UNIX timestamp in seconds)
//!   - completed_at: Number (UNIX timestamp in seconds)
//!   - updated_at: Number (UNIX timestamp in seconds)
//!   - schema_version: Number (for extensibility, default: 1)
//! ```
//!
//! ## GSI (Global Secondary Index)
//! - **GSI-1**: `status_created_index`
//!   - Partition Key (status): `status`
//!   - Sort Key (created_at): `created_at`
//!   - Purpose: Efficient queries by status (especially PENDING for recovery)
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

use crate::state_store::SchedulingStateStore;
use async_trait::async_trait;
use plexspaces_core::RequestContext;
use aws_sdk_dynamodb::{
    types::{
        AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement,
        KeyType, Projection, ProjectionType, ScalarAttributeType,
    },
    Client as DynamoDbClient,
};
use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use plexspaces_proto::scheduling::v1::{SchedulingRequest, SchedulingStatus};
use prost::Message;
use std::collections::HashMap;
use std::error::Error;
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

/// DynamoDB scheduling state store with production-grade observability.
///
/// ## Example
/// ```rust,no_run
/// use plexspaces_scheduler::state_store::ddb::DynamoDBSchedulingStateStore;
/// use plexspaces_proto::scheduling::v1::SchedulingRequest;
/// use plexspaces_core::RequestContext;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let store = DynamoDBSchedulingStateStore::new(
///     "us-east-1".to_string(),
///     "plexspaces-scheduler".to_string(),
///     Some("http://localhost:8000".to_string()), // For local testing
/// ).await?;
///
/// let ctx = RequestContext::new_without_auth("tenant1".to_string(), "default".to_string());
/// let request = SchedulingRequest {
///     request_id: "req-1".to_string(),
///     namespace: "default".to_string(),
///     tenant_id: "tenant1".to_string(),
///     status: 0,
///     selected_node_id: String::new(),
///     actor_id: String::new(),
///     error_message: String::new(),
///     requirements: None,
///     created_at: None,
///     scheduled_at: None,
///     completed_at: None,
/// };
/// use plexspaces_scheduler::state_store::SchedulingStateStore;
/// SchedulingStateStore::store_request(&store, &ctx, request).await?;
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct DynamoDBSchedulingStateStore {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name
    table_name: String,
    /// Schema version (for extensibility)
    schema_version: u32,
}

impl DynamoDBSchedulingStateStore {
    /// Create a new DynamoDB scheduling state store.
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_name` - DynamoDB table name
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local testing)
    ///
    /// ## Behavior
    /// - Creates table if it doesn't exist (idempotent)
    /// - Creates GSI for efficient status queries
    ///
    /// ## Returns
    /// DynamoDBSchedulingStateStore or error if table creation fails
    #[instrument(skip(region, table_name, endpoint_url), fields(region = %region, table_name = %table_name))]
    pub async fn new(
        region: String,
        table_name: String,
        endpoint_url: Option<String>,
    ) -> Result<Self, Box<dyn Error + Send + Sync>> {
        let start_time = std::time::Instant::now();

        // Build AWS config
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.clone()));

        // Set endpoint URL if provided (for local testing)
        if let Some(endpoint) = endpoint_url {
            config_builder = config_builder.endpoint_url(endpoint);
        }

        let config = config_builder.load().await;
        let client = DynamoDbClient::new(&config);

        // Create table if it doesn't exist (idempotent)
        Self::ensure_table_exists(&client, &table_name).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_scheduler_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        debug!(
            table_name = %table_name,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB scheduling state store initialized"
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
    /// - Partition Key: `pk` (String) - Composite: `{tenant_id}#{namespace}#{request_id}`
    /// - Sort Key: `sk` (String) - Always "REQUEST" for now (extensibility)
    /// - GSI: `status_created_index` for efficient status queries
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_table_exists(client: &DynamoDbClient, table_name: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
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
                if !e.to_string().contains("ResourceNotFoundException") {
                    return Err(format!("Failed to check table existence: {}", e).into());
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB table");

        // Create table with composite key for tenant isolation
        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| format!("Failed to build key schema: {}", e))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| format!("Failed to build key schema: {}", e))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let status_attr = AttributeDefinition::builder()
            .attribute_name("status")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let created_at_attr = AttributeDefinition::builder()
            .attribute_name("created_at")
            .attribute_type(ScalarAttributeType::N)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let gsi_pk_schema = KeySchemaElement::builder()
            .attribute_name("status")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| format!("Failed to build GSI key schema: {}", e))?;

        let gsi_sk_schema = KeySchemaElement::builder()
            .attribute_name("created_at")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| format!("Failed to build GSI key schema: {}", e))?;

        let gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let gsi = GlobalSecondaryIndex::builder()
            .index_name("status_created_index")
            .key_schema(gsi_pk_schema)
            .key_schema(gsi_sk_schema)
            .projection(gsi_projection)
            .build()
            .map_err(|e| format!("Failed to build GSI: {}", e))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest) // On-demand for scalability
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(status_attr)
            .attribute_definitions(created_at_attr)
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
                    Err(format!("Failed to create DynamoDB table: {}", e).into())
                }
            }
        }
    }

    /// Wait for table to become active.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn wait_for_table_active(client: &DynamoDbClient, table_name: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30; // 30 seconds max wait

        loop {
            let describe_result = client
                .describe_table()
                .table_name(table_name)
                .send()
                .await
                .map_err(|e| format!("Failed to describe table: {}", e))?;

            if let Some(status) = describe_result.table().and_then(|t| t.table_status()) {
                match status {
                    TableStatus::Active => {
                        debug!(table_name = %table_name, "Table is now active");
                        return Ok(());
                    }
                    TableStatus::Creating => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(format!(
                                "Table creation timeout after {} attempts",
                                max_attempts
                            )
                            .into());
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(format!("Table in unexpected status: {:?}", status).into());
                    }
                }
            } else {
                return Err("Table status not available".into());
            }
        }
    }

    /// Create composite partition key from tenant_id, namespace, and request_id.
    /// Format: "{tenant_id}#{namespace}#{request_id}"
    fn composite_key(tenant_id: &str, namespace: &str, request_id: &str) -> String {
        let tenant_id = if tenant_id.is_empty() {
            "default"
        } else {
            tenant_id
        };
        let namespace = if namespace.is_empty() {
            "default"
        } else {
            namespace
        };
        format!("{}#{}#{}", tenant_id, namespace, request_id)
    }

    /// Convert DynamoDB item to SchedulingRequest.
    fn item_to_request(item: &HashMap<String, AttributeValue>) -> Result<SchedulingRequest, Box<dyn Error + Send + Sync>> {
        let request_id = item
            .get("request_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| "Missing request_id attribute".to_string())?
            .to_string();

        let status_str = item
            .get("status")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| "Missing status attribute".to_string())?
            .to_string();

        let status = match status_str.as_str() {
            "PENDING" => SchedulingStatus::SchedulingStatusPending as i32,
            "SCHEDULED" => SchedulingStatus::SchedulingStatusScheduled as i32,
            "FAILED" => SchedulingStatus::SchedulingStatusFailed as i32,
            _ => SchedulingStatus::SchedulingStatusUnspecified as i32,
        };

        let requirements_json = item
            .get("requirements_json")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| "Missing requirements_json attribute".to_string())?
            .to_string();

        // Decode requirements from base64
        let requirements_bytes = general_purpose::STANDARD.decode(&requirements_json)?;
        let requirements = plexspaces_proto::v1::actor::ActorResourceRequirements::decode(
            requirements_bytes.as_slice(),
        )?;

        let namespace = item
            .get("namespace")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| "Missing namespace attribute".to_string())?
            .to_string();

        let tenant_id = item
            .get("tenant_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| "Missing tenant_id attribute".to_string())?
            .to_string();

        let selected_node_id = item
            .get("selected_node_id")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let actor_id = item
            .get("actor_id")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let error_message = item
            .get("error_message")
            .and_then(|v| v.as_s().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();

        let created_at_secs = item
            .get("created_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or_else(|| Utc::now().timestamp());

        let scheduled_at_secs = item
            .get("scheduled_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok());

        let completed_at_secs = item
            .get("completed_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok());

        Ok(SchedulingRequest {
            request_id,
            requirements: Some(requirements),
            namespace,
            tenant_id,
            status,
            selected_node_id,
            actor_id,
            error_message,
            created_at: Some(plexspaces_proto::prost_types::Timestamp {
                seconds: created_at_secs,
                nanos: 0,
            }),
            scheduled_at: scheduled_at_secs.map(|secs| plexspaces_proto::prost_types::Timestamp {
                seconds: secs,
                nanos: 0,
            }),
            completed_at: completed_at_secs.map(|secs| plexspaces_proto::prost_types::Timestamp {
                seconds: secs,
                nanos: 0,
            }),
        })
    }

    /// Convert SchedulingRequest to DynamoDB item.
    fn request_to_item(
        &self,
        request: &SchedulingRequest,
    ) -> Result<HashMap<String, AttributeValue>, Box<dyn Error + Send + Sync>> {
        let tenant_id = if request.tenant_id.is_empty() {
            "default"
        } else {
            &request.tenant_id
        };
        let namespace = if request.namespace.is_empty() {
            "default"
        } else {
            &request.namespace
        };

        let pk = Self::composite_key(tenant_id, namespace, &request.request_id);

        // Encode requirements to base64
        let requirements_json = if let Some(ref req) = request.requirements {
            let mut buf = Vec::new();
            req.encode(&mut buf)?;
            general_purpose::STANDARD.encode(&buf)
        } else {
            return Err("requirements field is required".into());
        };

        let status_str = match SchedulingStatus::try_from(request.status) {
            Ok(SchedulingStatus::SchedulingStatusPending) => "PENDING",
            Ok(SchedulingStatus::SchedulingStatusScheduled) => "SCHEDULED",
            Ok(SchedulingStatus::SchedulingStatusFailed) => "FAILED",
            _ => "UNSPECIFIED",
        };

        let created_at_secs = request
            .created_at
            .as_ref()
            .map(|t| t.seconds)
            .unwrap_or_else(|| Utc::now().timestamp());

        let scheduled_at_secs = request.scheduled_at.as_ref().map(|t| t.seconds);
        let completed_at_secs = request.completed_at.as_ref().map(|t| t.seconds);
        let updated_at_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("REQUEST".to_string()));
        item.insert("request_id".to_string(), AttributeValue::S(request.request_id.clone()));
        item.insert("status".to_string(), AttributeValue::S(status_str.to_string()));
        item.insert("requirements_json".to_string(), AttributeValue::S(requirements_json));
        item.insert("namespace".to_string(), AttributeValue::S(namespace.to_string()));
        item.insert("tenant_id".to_string(), AttributeValue::S(tenant_id.to_string()));
        item.insert(
            "selected_node_id".to_string(),
            AttributeValue::S(request.selected_node_id.clone()),
        );
        item.insert("actor_id".to_string(), AttributeValue::S(request.actor_id.clone()));
        item.insert(
            "error_message".to_string(),
            AttributeValue::S(request.error_message.clone()),
        );
        item.insert(
            "created_at".to_string(),
            AttributeValue::N(created_at_secs.to_string()),
        );
        if let Some(secs) = scheduled_at_secs {
            item.insert("scheduled_at".to_string(), AttributeValue::N(secs.to_string()));
        }
        if let Some(secs) = completed_at_secs {
            item.insert("completed_at".to_string(), AttributeValue::N(secs.to_string()));
        }
        item.insert("updated_at".to_string(), AttributeValue::N(updated_at_secs.to_string()));
        item.insert(
            "schema_version".to_string(),
            AttributeValue::N(self.schema_version.to_string()),
        );

        Ok(item)
    }
}

#[async_trait]
impl SchedulingStateStore for DynamoDBSchedulingStateStore {
    #[instrument(
        skip(self, ctx, request),
        fields(
            request_id = %request.request_id,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn store_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Validate tenant/namespace match
        if request.tenant_id != ctx.tenant_id() || request.namespace != ctx.namespace() {
            return Err(format!(
                "Request tenant_id/namespace ({}/{}) does not match context ({}/{})",
                request.tenant_id,
                request.namespace,
                ctx.tenant_id(),
                ctx.namespace()
            )
            .into());
        }
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(&request.tenant_id, &request.namespace, &request.request_id);

        let item = self.request_to_item(&request)?;

        // Conditional put: only if request doesn't exist
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
                    "plexspaces_scheduler_ddb_store_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_scheduler_ddb_store_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);

                debug!(
                    request_id = %request.request_id,
                    duration_ms = duration.as_millis(),
                    "Scheduling request stored successfully"
                );
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("ConditionalCheckFailedException") {
                    // Request was created concurrently
                    metrics::counter!(
                        "plexspaces_scheduler_ddb_store_errors_total",
                        "backend" => "dynamodb",
                        "error_type" => "concurrent_creation"
                    )
                    .increment(1);
                    Err(format!("Request {} already exists", request.request_id).into())
                } else {
                    error!(
                        error = %e,
                        request_id = %request.request_id,
                        "Failed to store scheduling request in DynamoDB"
                    );
                    metrics::counter!(
                        "plexspaces_scheduler_ddb_store_errors_total",
                        "backend" => "dynamodb",
                        "error_type" => "put_item_failed"
                    )
                    .increment(1);
                    Err(format!("DynamoDB put_item failed: {}", e).into())
                }
            }
        }
    }

    #[instrument(
        skip(self, ctx),
        fields(
            request_id = %request_id,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn get_request(
        &self,
        ctx: &RequestContext,
        request_id: &str,
    ) -> Result<Option<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx.tenant_id(), ctx.namespace(), request_id);

        // Use composite key for direct lookup (secure and efficient)
        match self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REQUEST".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_scheduler_ddb_get_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());

                if let Some(item) = result.item().cloned() {
                    let request = Self::item_to_request(&item)?;
                    // Double-check tenant/namespace match (defense in depth)
                    if request.tenant_id == ctx.tenant_id() && request.namespace == ctx.namespace() {
                        metrics::counter!(
                            "plexspaces_scheduler_ddb_get_total",
                            "backend" => "dynamodb",
                            "result" => "found"
                        )
                        .increment(1);
                        Ok(Some(request))
                    } else {
                        // This should never happen if composite key is correct, but log for security
                        warn!(
                            request_id = %request_id,
                            "Request tenant/namespace mismatch - possible security issue"
                        );
                        metrics::counter!(
                            "plexspaces_scheduler_ddb_get_total",
                            "backend" => "dynamodb",
                            "result" => "not_found"
                        )
                        .increment(1);
                        Ok(None)
                    }
                } else {
                    metrics::counter!(
                        "plexspaces_scheduler_ddb_get_total",
                        "backend" => "dynamodb",
                        "result" => "not_found"
                    )
                    .increment(1);
                    Ok(None)
                }
            }
            Err(e) => {
                error!(
                    error = %e,
                    request_id = %request_id,
                    "Failed to get scheduling request from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_scheduler_ddb_get_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "get_item_failed"
                )
                .increment(1);
                Err(format!("DynamoDB get_item failed: {}", e).into())
            }
        }
    }

    #[instrument(
        skip(self, ctx, request),
        fields(
            request_id = %request.request_id,
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn update_request(
        &self,
        ctx: &RequestContext,
        request: SchedulingRequest,
    ) -> Result<(), Box<dyn Error + Send + Sync>> {
        // Validate tenant/namespace match
        if request.tenant_id != ctx.tenant_id() || request.namespace != ctx.namespace() {
            return Err(format!(
                "Request tenant_id/namespace ({}/{}) does not match context ({}/{})",
                request.tenant_id,
                request.namespace,
                ctx.tenant_id(),
                ctx.namespace()
            )
            .into());
        }
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(&request.tenant_id, &request.namespace, &request.request_id);

        let item = self.request_to_item(&request)?;

        // Update item (will create if doesn't exist)
        match self
            .client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_scheduler_ddb_update_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_scheduler_ddb_update_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);

                debug!(
                    request_id = %request.request_id,
                    duration_ms = duration.as_millis(),
                    "Scheduling request updated successfully"
                );
                Ok(())
            }
            Err(e) => {
                error!(
                    error = %e,
                    request_id = %request.request_id,
                    "Failed to update scheduling request in DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_scheduler_ddb_update_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "put_item_failed"
                )
                .increment(1);
                Err(format!("DynamoDB put_item failed: {}", e).into())
            }
        }
    }

    #[instrument(
        skip(self, ctx),
        fields(
            tenant_id = %ctx.tenant_id(),
            namespace = %ctx.namespace()
        )
    )]
    async fn query_pending_requests(
        &self,
        ctx: &RequestContext,
    ) -> Result<Vec<SchedulingRequest>, Box<dyn Error + Send + Sync>> {
        let start_time = std::time::Instant::now();

        // Query GSI for PENDING status, filtered by tenant/namespace
        // Note: GSI has status as PK and created_at as SK, so we need to filter by tenant/namespace
        // We'll use a filter expression for tenant/namespace
        match self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("status_created_index")
            .key_condition_expression("status = :status")
            .filter_expression("tenant_id = :tenant_id AND namespace = :namespace")
            .expression_attribute_values(":status", AttributeValue::S("PENDING".to_string()))
            .expression_attribute_values(":tenant_id", AttributeValue::S(ctx.tenant_id().to_string()))
            .expression_attribute_values(":namespace", AttributeValue::S(ctx.namespace().to_string()))
            .scan_index_forward(true) // Sort by created_at ascending
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_scheduler_ddb_query_pending_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());

                let mut requests = Vec::new();
                for item in result.items() {
                    match Self::item_to_request(&item) {
                        Ok(request) => {
                            // Double-check tenant/namespace match (defense in depth)
                            if request.tenant_id == ctx.tenant_id()
                                && request.namespace == ctx.namespace()
                            {
                                requests.push(request);
                            } else {
                                warn!(
                                    "Request tenant/namespace mismatch - skipping (security check)"
                                );
                            }
                        }
                        Err(e) => {
                            warn!(
                                error = %e,
                                "Failed to parse scheduling request from DynamoDB item, skipping"
                            );
                        }
                    }
                }

                metrics::counter!(
                    "plexspaces_scheduler_ddb_query_pending_total",
                    "backend" => "dynamodb",
                    "result" => "success",
                    "count" => requests.len().to_string()
                )
                .increment(1);

                debug!(
                    count = requests.len(),
                    duration_ms = duration.as_millis(),
                    "Queried pending scheduling requests"
                );

                Ok(requests)
            }
            Err(e) => {
                error!(
                    error = %e,
                    "Failed to query pending scheduling requests from DynamoDB"
                );
                metrics::counter!(
                    "plexspaces_scheduler_ddb_query_pending_errors_total",
                    "backend" => "dynamodb",
                    "error_type" => "query_failed"
                )
                .increment(1);
                Err(format!("DynamoDB query failed: {}", e).into())
            }
        }
    }
}

