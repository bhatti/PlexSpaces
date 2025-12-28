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

//! DynamoDB-based KeyValue store for facet.
//!
//! ## Purpose
//! Provides a DynamoDB backend for the facet's KeyValueStore trait.
//! Uses the same table schema as the main keyvalue DDB implementation
//! but with a simpler interface matching the facet's needs.

use super::KeyValueStore;
use async_trait::async_trait;
use aws_sdk_dynamodb::{
    error::ProvideErrorMetadata,
    types::{
        AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement,
        KeyType, Projection, ProjectionType, ScalarAttributeType, TimeToLiveSpecification,
    },
    Client as DynamoDbClient,
};
use chrono::Utc;
use plexspaces_common::RequestContext;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

/// DynamoDB KeyValue store for facet.
///
/// Uses the same table schema as the main keyvalue implementation
/// but with a simpler interface.
#[derive(Clone)]
pub struct DynamoDBStore {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name
    table_name: String,
    /// Schema version
    schema_version: u32,
}

impl DynamoDBStore {
    /// Create a new DynamoDB KeyValue store.
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
    ) -> Result<Self, String> {
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

        // Enable TTL for automatic expiration cleanup
        Self::enable_ttl(&client, &table_name).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_facet_keyvalue_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        debug!(
            table_name = %table_name,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB KeyValue store initialized"
        );

        Ok(Self {
            client,
            table_name,
            schema_version: 1,
        })
    }

    /// Ensure table exists, create if it doesn't.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_table_exists(client: &DynamoDbClient, table_name: &str) -> Result<(), String> {
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
                let error_code = e.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                let error_message = e.message().map(|m| m.to_string()).unwrap_or_else(|| error_msg.clone());

                if !error_msg.contains("ResourceNotFoundException") && error_code != "ResourceNotFoundException" {
                    error!(
                        error = %e,
                        error_code = %error_code,
                        error_message = %error_message,
                        "DynamoDB describe_table failed"
                    );
                    return Err(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    ));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB table");

        // Create table (same schema as main keyvalue DDB implementation)
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

        let tenant_namespace_attr = AttributeDefinition::builder()
            .attribute_name("tenant_namespace")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let prefix_key_attr = AttributeDefinition::builder()
            .attribute_name("prefix_key")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| format!("Failed to build attribute definition: {}", e))?;

        let gsi_pk_schema = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| format!("Failed to build GSI key schema: {}", e))?;

        let gsi_sk_schema = KeySchemaElement::builder()
            .attribute_name("prefix_key")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| format!("Failed to build GSI key schema: {}", e))?;

        let gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let gsi = GlobalSecondaryIndex::builder()
            .index_name("prefix_index")
            .key_schema(gsi_pk_schema)
            .key_schema(gsi_sk_schema)
            .projection(gsi_projection)
            .build()
            .map_err(|e| format!("Failed to build GSI: {}", e))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest)
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(tenant_namespace_attr)
            .attribute_definitions(prefix_key_attr)
            .global_secondary_indexes(gsi)
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
                    Err(format!("Failed to create DynamoDB table: {}", e))
                }
            }
        }
    }

    /// Wait for table to become active.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn wait_for_table_active(client: &DynamoDbClient, table_name: &str) -> Result<(), String> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30;

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
                            ));
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(format!("Table in unexpected status: {:?}", status));
                    }
                }
            } else {
                return Err("Table status not available".to_string());
            }
        }
    }

    /// Enable TTL on the table.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn enable_ttl(client: &DynamoDbClient, table_name: &str) -> Result<(), String> {
        let ttl_spec = TimeToLiveSpecification::builder()
            .enabled(true)
            .attribute_name("ttl")
            .build()
            .map_err(|e| format!("Failed to build TTL spec: {}", e))?;

        match client
            .update_time_to_live()
            .table_name(table_name)
            .time_to_live_specification(ttl_spec)
            .send()
            .await
        {
            Ok(_) => {
                debug!(table_name = %table_name, "TTL enabled");
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("TimeToLiveAlreadyEnabled") {
                    debug!(table_name = %table_name, "TTL already enabled");
                    Ok(())
                } else {
                    warn!(error = %e, table_name = %table_name, "Failed to enable TTL (non-critical)");
                    Ok(())
                }
            }
        }
    }

    /// Create composite partition key.
    fn composite_key(ctx: &RequestContext, key: &str) -> String {
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
        format!("{}#{}#{}", tenant_id, namespace, key)
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

    /// Extract prefix from key.
    fn extract_prefix(key: &str) -> String {
        if let Some(pos) = key.rfind(':') {
            format!("{}:", &key[..=pos])
        } else {
            String::new()
        }
    }

    /// Create prefix_key for GSI.
    fn prefix_key(prefix: &str, key: &str) -> String {
        if prefix.is_empty() {
            key.to_string()
        } else {
            format!("{}#{}", prefix, key)
        }
    }

    /// Convert key-value to DynamoDB item.
    fn kv_to_item(
        &self,
        ctx: &RequestContext,
        key: &str,
        value: &[u8],
        expires_at_secs: Option<i64>,
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

        let pk = Self::composite_key(ctx, key);
        let tenant_namespace = Self::tenant_namespace_key(ctx);
        let prefix = Self::extract_prefix(key);
        let prefix_key = Self::prefix_key(&prefix, key);

        let now_secs = Utc::now().timestamp();
        let ttl_secs = expires_at_secs.map(|exp| exp + 86400);

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("KV".to_string()));
        item.insert("key".to_string(), AttributeValue::S(key.to_string()));
        item.insert("value".to_string(), AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(value)));
        item.insert("tenant_namespace".to_string(), AttributeValue::S(tenant_namespace));
        item.insert("prefix_key".to_string(), AttributeValue::S(prefix_key));
        item.insert("prefix".to_string(), AttributeValue::S(prefix));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert(
            "schema_version".to_string(),
            AttributeValue::N(self.schema_version.to_string()),
        );

        if let Some(expires_at) = expires_at_secs {
            item.insert("expires_at".to_string(), AttributeValue::N(expires_at.to_string()));
        }

        if let Some(ttl) = ttl_secs {
            item.insert("ttl".to_string(), AttributeValue::N(ttl.to_string()));
        }

        item
    }
}

#[async_trait]
impl KeyValueStore for DynamoDBStore {
    async fn get(&self, ctx: &RequestContext, key: &str) -> Result<Option<Vec<u8>>, String> {
        let pk = Self::composite_key(ctx, key);

        match self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("KV".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                if let Some(item) = result.item().cloned() {
                    // Check if expired
                    if let Some(expires_at_attr) = item.get("expires_at") {
                        if let Some(expires_at_secs) = expires_at_attr.as_n().ok().and_then(|s| s.parse::<i64>().ok()) {
                            let now_secs = Utc::now().timestamp();
                            if expires_at_secs <= now_secs {
                                return Ok(None);
                            }
                        }
                    }

                    if let Some(value_attr) = item.get("value") {
                        if let Ok(bytes) = value_attr.as_b() {
                            return Ok(Some(bytes.as_ref().to_vec()));
                        }
                    }
                }
                Ok(None)
            }
            Err(e) => Err(format!("DynamoDB get_item failed: {}", e)),
        }
    }

    async fn set(
        &self,
        ctx: &RequestContext,
        key: &str,
        value: Vec<u8>,
        ttl: Option<u64>,
    ) -> Result<(), String> {
        let expires_at_secs = ttl.map(|t| Utc::now().timestamp() + t as i64);
        let item = self.kv_to_item(ctx, key, &value, expires_at_secs);

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| format!("DynamoDB put_item failed: {}", e))?;

        Ok(())
    }

    async fn delete(&self, ctx: &RequestContext, key: &str) -> Result<bool, String> {
        let pk = Self::composite_key(ctx, key);

        // Check if key exists first
        let exists = self.get(ctx, key).await?.is_some();

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("KV".to_string()))
            .send()
            .await
            .map_err(|e| format!("DynamoDB delete_item failed: {}", e))?;

        Ok(exists)
    }

    async fn exists(&self, ctx: &RequestContext, key: &str) -> Result<bool, String> {
        match self.get(ctx, key).await {
            Ok(Some(_)) => Ok(true),
            Ok(None) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn list_keys(&self, ctx: &RequestContext, prefix: &str) -> Result<Vec<String>, String> {
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        let mut keys = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&self.table_name)
                .index_name("prefix_index")
                .key_condition_expression("tenant_namespace = :tn")
                .expression_attribute_values(":tn", AttributeValue::S(tenant_namespace.clone()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        if let Some(key_attr) = item.get("key") {
                            if let Ok(key_str) = key_attr.as_s() {
                                // Filter by key prefix in application code (more reliable)
                                if prefix.is_empty() || key_str.starts_with(prefix) {
                                    keys.push(key_str.clone());
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
                    error!(
                        error = %e,
                        prefix = %prefix,
                        "Failed to query keys from DynamoDB"
                    );
                    return Err(format!("DynamoDB query failed: {}", e));
                }
            }
        }

        Ok(keys)
    }
}

