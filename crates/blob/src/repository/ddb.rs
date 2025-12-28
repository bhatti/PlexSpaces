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

//! DynamoDB-based BlobRepository implementation.
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for blob metadata storage
//! with proper tenant isolation, efficient queries, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Key**: `{tenant_id}#{namespace}#{blob_id}` for tenant isolation
//! - **Auto-table creation**: Creates tables with proper schema on initialization
//! - **GSI for queries**: Multiple GSIs for efficient querying by SHA256, name prefix, etc.
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Table Schema
//!
//! ### blob_metadata
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{blob_id}"
//! Sort Key: sk = "METADATA"
//! Attributes:
//!   - blob_id: String
//!   - tenant_id: String
//!   - namespace: String
//!   - name: String
//!   - sha256: String
//!   - content_type: String
//!   - content_length: Number
//!   - etag: String
//!   - blob_group: String (optional)
//!   - kind: String (optional)
//!   - metadata_json: String (JSON object)
//!   - tags_json: String (JSON object)
//!   - expires_at: Number (UNIX timestamp, optional)
//!   - created_at: Number (UNIX timestamp)
//!   - updated_at: Number (UNIX timestamp)
//! ```
//!
//! ### GSI: sha256_index
//! - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//! - Sort Key: `sha256` = "{sha256}"
//! - Purpose: Query by SHA256 hash
//!
//! ### GSI: name_index
//! - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//! - Sort Key: `name` = "{name}"
//! - Purpose: Query by name prefix
//!
//! ### GSI: expires_at_index
//! - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//! - Sort Key: `expires_at` = "{expires_at}"
//! - Purpose: Query expired blobs

use super::{BlobRepository, ListFilters};
use crate::{BlobError, BlobResult};
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
use plexspaces_core::RequestContext;
use plexspaces_proto::storage::v1::BlobMetadata;
use prost_types::Timestamp;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

/// DynamoDB BlobRepository implementation.
#[derive(Clone)]
pub struct DynamoDBBlobRepository {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name
    table_name: String,
    /// Schema version
    schema_version: u32,
}

impl DynamoDBBlobRepository {
    /// Create a new DynamoDB BlobRepository.
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_name` - DynamoDB table name
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local testing)
    #[instrument(skip(region, table_name, endpoint_url), fields(region = %region, table_name = %table_name))]
    pub async fn new(
        region: String,
        table_name: String,
        endpoint_url: Option<String>,
    ) -> BlobResult<Self> {
        let start_time = std::time::Instant::now();

        // Build AWS config
        let mut config_builder = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .region(aws_config::Region::new(region.clone()));

        if let Some(endpoint) = endpoint_url {
            config_builder = config_builder.endpoint_url(endpoint);
        }

        let config = config_builder.load().await;
        let client = DynamoDbClient::new(&config);

        // Create table if it doesn't exist
        Self::ensure_table_exists(&client, &table_name).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_blob_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        debug!(
            table_name = %table_name,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB BlobRepository initialized"
        );

        Ok(Self {
            client,
            table_name,
            schema_version: 1,
        })
    }

    /// Ensure table exists with GSIs.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), BlobError> {
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
                    return Err(BlobError::StorageError(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB blob_metadata table with GSIs");

        // Create table with GSIs
        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        let tenant_namespace_attr = AttributeDefinition::builder()
            .attribute_name("tenant_namespace")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        let sha256_attr = AttributeDefinition::builder()
            .attribute_name("sha256")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        let name_attr = AttributeDefinition::builder()
            .attribute_name("name")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        let expires_at_attr = AttributeDefinition::builder()
            .attribute_name("expires_at")
            .attribute_type(ScalarAttributeType::N)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build attribute definition: {}", e)))?;

        // GSI: sha256_index
        let sha256_gsi_pk = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let sha256_gsi_sk = KeySchemaElement::builder()
            .attribute_name("sha256")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let sha256_gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let sha256_gsi = GlobalSecondaryIndex::builder()
            .index_name("sha256_index")
            .key_schema(sha256_gsi_pk)
            .key_schema(sha256_gsi_sk)
            .projection(sha256_gsi_projection)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI: {}", e)))?;

        // GSI: name_index
        let name_gsi_pk = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let name_gsi_sk = KeySchemaElement::builder()
            .attribute_name("name")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let name_gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let name_gsi = GlobalSecondaryIndex::builder()
            .index_name("name_index")
            .key_schema(name_gsi_pk)
            .key_schema(name_gsi_sk)
            .projection(name_gsi_projection)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI: {}", e)))?;

        // GSI: expires_at_index
        let expires_gsi_pk = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let expires_gsi_sk = KeySchemaElement::builder()
            .attribute_name("expires_at")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI key schema: {}", e)))?;

        let expires_gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let expires_gsi = GlobalSecondaryIndex::builder()
            .index_name("expires_at_index")
            .key_schema(expires_gsi_pk)
            .key_schema(expires_gsi_sk)
            .projection(expires_gsi_projection)
            .build()
            .map_err(|e| BlobError::StorageError(format!("Failed to build GSI: {}", e)))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest)
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(tenant_namespace_attr)
            .attribute_definitions(sha256_attr)
            .attribute_definitions(name_attr)
            .attribute_definitions(expires_at_attr)
            .global_secondary_indexes(sha256_gsi)
            .global_secondary_indexes(name_gsi)
            .global_secondary_indexes(expires_gsi)
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
                    Err(BlobError::StorageError(format!(
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
    ) -> Result<(), BlobError> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30;

        loop {
            let describe_result = client
                .describe_table()
                .table_name(table_name)
                .send()
                .await
                .map_err(|e| BlobError::StorageError(format!("Failed to describe table: {}", e)))?;

            if let Some(status) = describe_result.table().and_then(|t| t.table_status()) {
                match status {
                    TableStatus::Active => {
                        debug!(table_name = %table_name, "Table is now active");
                        return Ok(());
                    }
                    TableStatus::Creating => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(BlobError::StorageError(format!(
                                "Table creation timeout after {} attempts",
                                max_attempts
                            )));
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(BlobError::StorageError(format!(
                            "Table in unexpected status: {:?}",
                            status
                        )));
                    }
                }
            } else {
                return Err(BlobError::StorageError("Table status not available".to_string()));
            }
        }
    }

    /// Create composite partition key for tenant isolation.
    fn composite_key(ctx: &RequestContext, blob_id: &str) -> String {
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
        format!("{}#{}#{}", tenant_id, namespace, blob_id)
    }

    /// Create tenant_namespace key for GSI.
    fn tenant_namespace_key(ctx: &RequestContext) -> String {
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
        format!("{}#{}", tenant_id, namespace)
    }

    /// Convert DDB item to BlobMetadata.
    fn item_to_metadata(
        item: &HashMap<String, AttributeValue>,
    ) -> Result<BlobMetadata, BlobError> {
        let blob_id = item
            .get("blob_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| BlobError::StorageError("Missing blob_id".to_string()))?
            .clone();

        let tenant_id = item
            .get("tenant_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| BlobError::StorageError("Missing tenant_id".to_string()))?
            .clone();

        let namespace = item
            .get("namespace")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| BlobError::StorageError("Missing namespace".to_string()))?
            .clone();

        let name = item
            .get("name")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| BlobError::StorageError("Missing name".to_string()))?
            .clone();

        let sha256 = item
            .get("sha256")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| BlobError::StorageError("Missing sha256".to_string()))?
            .clone();

        let content_type = item
            .get("content_type")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_default();

        let content_length = item
            .get("content_length")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(0);

        let etag = item
            .get("etag")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_default();

        let blob_group = item
            .get("blob_group")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_default();

        let kind = item
            .get("kind")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_default();

        let metadata_json = item
            .get("metadata_json")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_else(|| "{}".to_string());
        let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
            .unwrap_or_default();

        let tags_json = item
            .get("tags_json")
            .and_then(|v| v.as_s().ok())
            .cloned()
            .unwrap_or_else(|| "{}".to_string());
        let tags: HashMap<String, String> = serde_json::from_str(&tags_json).unwrap_or_default();

        let expires_at_secs = item
            .get("expires_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok());
        let expires_at = expires_at_secs.map(|secs| Timestamp {
            seconds: secs,
            nanos: 0,
        });

        let created_at_secs = item
            .get("created_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(Utc::now().timestamp());
        let created_at = Timestamp {
            seconds: created_at_secs,
            nanos: 0,
        };

        let updated_at_secs = item
            .get("updated_at")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok())
            .unwrap_or(Utc::now().timestamp());
        let updated_at = Timestamp {
            seconds: updated_at_secs,
            nanos: 0,
        };

        Ok(BlobMetadata {
            blob_id,
            tenant_id,
            namespace,
            name,
            sha256,
            content_type,
            content_length,
            etag,
            blob_group,
            kind,
            metadata,
            tags,
            expires_at,
            created_at: Some(created_at),
            updated_at: Some(updated_at),
        })
    }

    /// Convert BlobMetadata to DDB item.
    fn metadata_to_item(
        &self,
        ctx: &RequestContext,
        metadata: &BlobMetadata,
        now_secs: i64,
    ) -> HashMap<String, AttributeValue> {
        let pk = Self::composite_key(ctx, &metadata.blob_id);
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        let metadata_json = serde_json::to_string(&metadata.metadata).unwrap_or_else(|_| "{}".to_string());
        let tags_json = serde_json::to_string(&metadata.tags).unwrap_or_else(|_| "{}".to_string());

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("METADATA".to_string()));
        item.insert("blob_id".to_string(), AttributeValue::S(metadata.blob_id.clone()));
        item.insert("tenant_id".to_string(), AttributeValue::S(metadata.tenant_id.clone()));
        item.insert("namespace".to_string(), AttributeValue::S(metadata.namespace.clone()));
        item.insert("name".to_string(), AttributeValue::S(metadata.name.clone()));
        item.insert("sha256".to_string(), AttributeValue::S(metadata.sha256.clone()));
        item.insert("content_type".to_string(), AttributeValue::S(metadata.content_type.clone()));
        item.insert("content_length".to_string(), AttributeValue::N(metadata.content_length.to_string()));
        item.insert("etag".to_string(), AttributeValue::S(metadata.etag.clone()));
        item.insert("metadata_json".to_string(), AttributeValue::S(metadata_json));
        item.insert("tags_json".to_string(), AttributeValue::S(tags_json));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert("tenant_namespace".to_string(), AttributeValue::S(tenant_namespace));

        if !metadata.blob_group.is_empty() {
            item.insert("blob_group".to_string(), AttributeValue::S(metadata.blob_group.clone()));
        }

        if !metadata.kind.is_empty() {
            item.insert("kind".to_string(), AttributeValue::S(metadata.kind.clone()));
        }

        if let Some(expires_at) = &metadata.expires_at {
            item.insert("expires_at".to_string(), AttributeValue::N(expires_at.seconds.to_string()));
        }

        item
    }
}

#[async_trait]
impl BlobRepository for DynamoDBBlobRepository {
    async fn get(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> BlobResult<Option<BlobMetadata>> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, blob_id);

        match self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("METADATA".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_blob_ddb_get_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());

                if let Some(item) = result.item().cloned() {
                    match Self::item_to_metadata(&item) {
                        Ok(metadata) => {
                            metrics::counter!(
                                "plexspaces_blob_ddb_get_total",
                                "backend" => "dynamodb",
                                "result" => "found"
                            )
                            .increment(1);
                            Ok(Some(metadata))
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse metadata");
                            Err(e)
                        }
                    }
                } else {
                    metrics::counter!(
                        "plexspaces_blob_ddb_get_total",
                        "backend" => "dynamodb",
                        "result" => "not_found"
                    )
                    .increment(1);
                    Ok(None)
                }
            }
            Err(e) => {
                error!(error = %e, blob_id = %blob_id, "Failed to get blob metadata");
                metrics::counter!(
                    "plexspaces_blob_ddb_get_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(BlobError::StorageError(format!("DynamoDB get_item failed: {}", e)))
            }
        }
    }

    async fn get_by_sha256(
        &self,
        ctx: &RequestContext,
        sha256: &str,
    ) -> BlobResult<Option<BlobMetadata>> {
        let start_time = std::time::Instant::now();
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        // Query using sha256_index GSI
        match self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("sha256_index")
            .key_condition_expression("tenant_namespace = :tn AND sha256 = :sha256")
            .expression_attribute_values(":tn", AttributeValue::S(tenant_namespace))
            .expression_attribute_values(":sha256", AttributeValue::S(sha256.to_string()))
            .limit(1)
            .scan_index_forward(false) // Descending (most recent first)
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_blob_ddb_get_by_sha256_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());

                if let Some(item) = result.items().first() {
                    match Self::item_to_metadata(item) {
                        Ok(metadata) => {
                            metrics::counter!(
                                "plexspaces_blob_ddb_get_by_sha256_total",
                                "backend" => "dynamodb",
                                "result" => "found"
                            )
                            .increment(1);
                            return Ok(Some(metadata));
                        }
                        Err(e) => {
                            error!(error = %e, "Failed to parse metadata");
                            return Err(e);
                        }
                    }
                }

                metrics::counter!(
                    "plexspaces_blob_ddb_get_by_sha256_total",
                    "backend" => "dynamodb",
                    "result" => "not_found"
                )
                .increment(1);
                Ok(None)
            }
            Err(e) => {
                error!(error = %e, sha256 = %sha256, "Failed to get blob by SHA256");
                metrics::counter!(
                    "plexspaces_blob_ddb_get_by_sha256_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(BlobError::StorageError(format!("DynamoDB query failed: {}", e)))
            }
        }
    }

    async fn save(
        &self,
        ctx: &RequestContext,
        metadata: &BlobMetadata,
    ) -> BlobResult<()> {
        // Validate tenant_id and namespace match context
        if metadata.tenant_id != ctx.tenant_id() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata tenant_id '{}' does not match context tenant_id '{}'",
                metadata.tenant_id,
                ctx.tenant_id()
            )));
        }

        if metadata.namespace != ctx.namespace() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata namespace '{}' does not match context namespace '{}'",
                metadata.namespace,
                ctx.namespace()
            )));
        }

        let start_time = std::time::Instant::now();
        let now_secs = Utc::now().timestamp();
        let item = self.metadata_to_item(ctx, metadata, now_secs);

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
                    "plexspaces_blob_ddb_save_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_blob_ddb_save_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, blob_id = %metadata.blob_id, "Failed to save blob metadata");
                metrics::counter!(
                    "plexspaces_blob_ddb_save_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(BlobError::StorageError(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn update(
        &self,
        ctx: &RequestContext,
        metadata: &BlobMetadata,
    ) -> BlobResult<()> {
        // Validate tenant_id and namespace match context
        if metadata.tenant_id != ctx.tenant_id() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata tenant_id '{}' does not match context tenant_id '{}'",
                metadata.tenant_id,
                ctx.tenant_id()
            )));
        }

        if metadata.namespace != ctx.namespace() {
            return Err(BlobError::InvalidInput(format!(
                "Metadata namespace '{}' does not match context namespace '{}'",
                metadata.namespace,
                ctx.namespace()
            )));
        }

        // Update is same as save (put_item overwrites)
        self.save(ctx, metadata).await
    }

    async fn delete(
        &self,
        ctx: &RequestContext,
        blob_id: &str,
    ) -> BlobResult<()> {
        let start_time = std::time::Instant::now();
        let pk = Self::composite_key(ctx, blob_id);

        match self
            .client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("METADATA".to_string()))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_blob_ddb_delete_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_blob_ddb_delete_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, blob_id = %blob_id, "Failed to delete blob metadata");
                metrics::counter!(
                    "plexspaces_blob_ddb_delete_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(BlobError::StorageError(format!("DynamoDB delete_item failed: {}", e)))
            }
        }
    }

    async fn list(
        &self,
        ctx: &RequestContext,
        filters: &ListFilters,
        page_size: i64,
        offset: i64,
    ) -> BlobResult<(Vec<BlobMetadata>, i64)> {
        let start_time = std::time::Instant::now();
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        let mut results = Vec::new();
        let mut last_evaluated_key = None;
        let mut scanned_count = 0;

        // Always use name_index GSI to query by tenant_namespace
        // This is efficient since tenant_namespace is the partition key of the GSI
        loop {
            let mut query = self.client
                .query()
                .table_name(&self.table_name)
                .index_name("name_index")
                .key_condition_expression("tenant_namespace = :tn");

            // If name_prefix filter is provided, add begins_with condition
            if let Some(name_prefix) = &filters.name_prefix {
                query = query
                    .key_condition_expression("tenant_namespace = :tn AND begins_with(#name, :name_prefix)")
                    .expression_attribute_names("#name", "name")
                    .expression_attribute_values(":name_prefix", AttributeValue::S(name_prefix.clone()));
            }

            query = query.expression_attribute_values(":tn", AttributeValue::S(tenant_namespace.clone()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    scanned_count += result.count() as i64;

                    for item in result.items() {
                        // Apply filters
                        if let Some(blob_group) = &filters.blob_group {
                            if let Some(item_group) = item.get("blob_group").and_then(|v| v.as_s().ok()) {
                                if item_group != blob_group {
                                    continue;
                                }
                            } else {
                                continue; // Item has no blob_group but filter requires it
                            }
                        }

                        if let Some(kind) = &filters.kind {
                            if let Some(item_kind) = item.get("kind").and_then(|v| v.as_s().ok()) {
                                if item_kind != kind {
                                    continue;
                                }
                            } else {
                                continue; // Item has no kind but filter requires it
                            }
                        }

                        if let Some(sha256) = &filters.sha256 {
                            if let Some(item_sha256) = item.get("sha256").and_then(|v| v.as_s().ok()) {
                                if item_sha256 != sha256 {
                                    continue;
                                }
                            } else {
                                continue;
                            }
                        }

                        match Self::item_to_metadata(&item) {
                            Ok(metadata) => results.push(metadata),
                            Err(e) => {
                                warn!(error = %e, "Failed to parse metadata, skipping");
                            }
                        }

                        // Stop if we have enough results (accounting for offset)
                        if results.len() >= (page_size + offset) as usize {
                            break;
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() || results.len() >= (page_size + offset) as usize {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to list blob metadata");
                    return Err(BlobError::StorageError(format!("DynamoDB query/scan failed: {}", e)));
                }
            }
        }

        // Apply pagination
        let total_count = scanned_count; // Approximate total (DynamoDB doesn't provide exact count efficiently)
        let start_idx = offset as usize;
        let end_idx = (offset + page_size) as usize;
        let paginated_results = if start_idx < results.len() {
            results[start_idx..end_idx.min(results.len())].to_vec()
        } else {
            Vec::new()
        };

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_blob_ddb_list_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_blob_ddb_list_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => paginated_results.len().to_string()
        )
        .increment(1);

        Ok((paginated_results, total_count))
    }

    async fn find_expired(
        &self,
        ctx: &RequestContext,
        limit: i64,
    ) -> BlobResult<Vec<BlobMetadata>> {
        let start_time = std::time::Instant::now();
        let tenant_namespace = Self::tenant_namespace_key(ctx);
        let now_secs = Utc::now().timestamp();

        let mut expired = Vec::new();
        let mut last_evaluated_key = None;

        // Query using expires_at_index GSI
        loop {
            let mut query = self
                .client
                .query()
                .table_name(&self.table_name)
                .index_name("expires_at_index")
                .key_condition_expression("tenant_namespace = :tn AND expires_at <= :now")
                .expression_attribute_values(":tn", AttributeValue::S(tenant_namespace.clone()))
                .expression_attribute_values(":now", AttributeValue::N(now_secs.to_string()))
                .limit(limit as i32);

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        match Self::item_to_metadata(&item) {
                            Ok(metadata) => expired.push(metadata),
                            Err(e) => {
                                warn!(error = %e, "Failed to parse metadata, skipping");
                            }
                        }

                        if expired.len() >= limit as usize {
                            break;
                        }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() || expired.len() >= limit as usize {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to find expired blobs");
                    return Err(BlobError::StorageError(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_blob_ddb_find_expired_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_blob_ddb_find_expired_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => expired.len().to_string()
        )
        .increment(1);

        Ok(expired)
    }
}

