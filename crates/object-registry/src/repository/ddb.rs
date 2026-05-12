// SPDX-License-Identifier: AGPL-3.0-or-later
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

//! DynamoDB-based Object Registry Repository implementation
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for object registrations
//! with proper tenant isolation, GSI support for fast queries, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Key**: `{tenant_id}#{namespace}#{object_id}` for tenant isolation
//! - **GSI for type queries**: `type_index` for efficient discover by object_type
//! - **GSI for heartbeat queries**: `heartbeat_index` for stale detection
//! - **Auto-table creation**: Creates table with proper schema on initialization
//!
//! ## Table Schema
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{object_id}"
//! Sort Key: sk = "REG" (for future extensibility)
//! Attributes:
//!   - object_type: Number (enum value)
//!   - object_name: String
//!   - node_id: String
//!   - grpc_address: String
//!   - object_category: String
//!   - health_status: Number
//!   - last_heartbeat: Number (Unix timestamp)
//!   - created_at: Number (Unix timestamp)
//!   - updated_at: Number (Unix timestamp)
//!   - registration_blob: Binary (full protobuf)
//! ```
//!
//! ## GSI (Global Secondary Indexes)
//! - **GSI-1**: `type_index`
//!   - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//!   - Sort Key: `object_type_id` = "{object_type}#{object_id}"
//! - **GSI-2**: `heartbeat_index`
//!   - Partition Key: `tenant_namespace`
//!   - Sort Key: `last_heartbeat`

use super::{DiscoverFilter, ObjectRegistryRepository, RepositoryError, RepositoryResult};
use async_trait::async_trait;
use aws_sdk_dynamodb::{
    error::ProvideErrorMetadata,
    types::{
        AttributeDefinition, AttributeValue, BillingMode, GlobalSecondaryIndex, KeySchemaElement,
        KeyType, Projection, ProjectionType, ScalarAttributeType,
    },
    Client as DynamoDbClient,
};
use plexspaces_common::{RequestContext, RequestContextExt};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost::Message;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{debug, error, instrument, warn};

/// DynamoDB Object Registry Repository
///
/// ## Purpose
/// Provides production-grade DynamoDB storage with GSI support for efficient queries.
#[derive(Debug, Clone)]
pub struct DynamoDBObjectRegistryRepository {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name
    table_name: String,
}

impl DynamoDBObjectRegistryRepository {
    /// Create a new DynamoDB repository
    ///
    /// ## Arguments
    /// * `region` - AWS region
    /// * `table_name` - DynamoDB table name
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local)
    pub async fn new(
        region: String,
        table_name: String,
        endpoint_url: Option<String>,
    ) -> RepositoryResult<Self> {
        let config = if let Some(ref endpoint) = endpoint_url {
            aws_config::from_env()
                .region(aws_config::Region::new(region))
                .endpoint_url(endpoint)
                .load()
                .await
        } else {
            aws_config::from_env()
                .region(aws_config::Region::new(region))
                .load()
                .await
        };

        let client = DynamoDbClient::new(&config);

        let repo = Self { client, table_name };
        repo.ensure_table_exists().await?;

        Ok(repo)
    }

    /// Ensure table exists with proper schema
    async fn ensure_table_exists(&self) -> RepositoryResult<()> {
        // Check if table exists
        match self
            .client
            .describe_table()
            .table_name(&self.table_name)
            .send()
            .await
        {
            Ok(_) => {
                debug!(table_name = %self.table_name, "Table already exists");
                return Ok(());
            }
            Err(e) => {
                let error_code = e.code().unwrap_or("Unknown");
                if error_code != "ResourceNotFoundException" {
                    return Err(RepositoryError::Storage(format!(
                        "Failed to describe table: {}",
                        e
                    )));
                }
            }
        }

        // Create table with GSIs
        debug!(table_name = %self.table_name, "Creating table with GSIs");

        self.client
            .create_table()
            .table_name(&self.table_name)
            .billing_mode(BillingMode::PayPerRequest)
            // Key schema
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("pk")
                    .key_type(KeyType::Hash)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .key_schema(
                KeySchemaElement::builder()
                    .attribute_name("sk")
                    .key_type(KeyType::Range)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            // Attribute definitions
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("pk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("sk")
                    .attribute_type(ScalarAttributeType::S)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("tenant_namespace")
                    .attribute_type(ScalarAttributeType::S)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("object_type_id")
                    .attribute_type(ScalarAttributeType::S)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .attribute_definitions(
                AttributeDefinition::builder()
                    .attribute_name("last_heartbeat")
                    .attribute_type(ScalarAttributeType::N)
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            // GSI for type queries
            .global_secondary_indexes(
                GlobalSecondaryIndex::builder()
                    .index_name("type_index")
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("tenant_namespace")
                            .key_type(KeyType::Hash)
                            .build()
                            .map_err(|e| RepositoryError::Storage(e.to_string()))?,
                    )
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("object_type_id")
                            .key_type(KeyType::Range)
                            .build()
                            .map_err(|e| RepositoryError::Storage(e.to_string()))?,
                    )
                    .projection(
                        Projection::builder()
                            .projection_type(ProjectionType::All)
                            .build(),
                    )
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            // GSI for heartbeat queries
            .global_secondary_indexes(
                GlobalSecondaryIndex::builder()
                    .index_name("heartbeat_index")
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("tenant_namespace")
                            .key_type(KeyType::Hash)
                            .build()
                            .map_err(|e| RepositoryError::Storage(e.to_string()))?,
                    )
                    .key_schema(
                        KeySchemaElement::builder()
                            .attribute_name("last_heartbeat")
                            .key_type(KeyType::Range)
                            .build()
                            .map_err(|e| RepositoryError::Storage(e.to_string()))?,
                    )
                    .projection(
                        Projection::builder()
                            .projection_type(ProjectionType::All)
                            .build(),
                    )
                    .build()
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?,
            )
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(format!("Failed to create table: {}", e)))?;

        // Wait for table to be active
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let resp = self
                .client
                .describe_table()
                .table_name(&self.table_name)
                .send()
                .await
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;

            if let Some(table) = resp.table() {
                if let Some(status) = table.table_status() {
                    if status == &aws_sdk_dynamodb::types::TableStatus::Active {
                        break;
                    }
                }
            }
        }

        debug!(table_name = %self.table_name, "Table created and active");
        Ok(())
    }

    /// Create primary key
    fn make_pk(tenant_id: &str, namespace: &str, object_id: &str) -> String {
        format!("{}#{}#{}", tenant_id, namespace, object_id)
    }

    /// Create tenant_namespace for GSI
    fn make_tenant_namespace(tenant_id: &str, namespace: &str) -> String {
        format!("{}#{}", tenant_id, namespace)
    }

    /// Create object_type_id for GSI
    fn make_type_id(object_type: i32, object_id: &str) -> String {
        format!("{}#{}", object_type, object_id)
    }

    /// Get current Unix timestamp
    fn now_unix() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    /// Parse ObjectRegistration from DynamoDB item
    fn parse_registration(
        item: &HashMap<String, AttributeValue>,
    ) -> RepositoryResult<ObjectRegistration> {
        let blob = item
            .get("registration_blob")
            .and_then(|v| v.as_b().ok())
            .ok_or_else(|| RepositoryError::Storage("Missing registration_blob".to_string()))?;

        ObjectRegistration::decode(blob.as_ref())
            .map_err(|e| RepositoryError::Serialization(e.to_string()))
    }
}

#[async_trait]
impl ObjectRegistryRepository for DynamoDBObjectRegistryRepository {
    #[instrument(skip(self, ctx, registration), fields(tenant_id = %ctx.tenant_id(), object_id = %registration.object_id))]
    async fn put(
        &self,
        ctx: &RequestContext,
        registration: &ObjectRegistration,
    ) -> RepositoryResult<()> {
        let now = Self::now_unix();
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), &registration.object_id);
        let tenant_namespace = Self::make_tenant_namespace(ctx.tenant_id(), ctx.namespace());
        let type_id = Self::make_type_id(registration.object_type, &registration.object_id);
        let blob = registration.encode_to_vec();
        let last_heartbeat = registration
            .last_heartbeat
            .as_ref()
            .map(|t| t.seconds)
            .unwrap_or(now);

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("REG".to_string()));
        item.insert(
            "tenant_namespace".to_string(),
            AttributeValue::S(tenant_namespace),
        );
        item.insert("object_type_id".to_string(), AttributeValue::S(type_id));
        item.insert(
            "object_id".to_string(),
            AttributeValue::S(registration.object_id.clone()),
        );
        item.insert(
            "object_type".to_string(),
            AttributeValue::N(registration.object_type.to_string()),
        );
        item.insert(
            "grpc_address".to_string(),
            AttributeValue::S(registration.grpc_address.clone()),
        );
        item.insert(
            "health_status".to_string(),
            AttributeValue::N(registration.health_status.to_string()),
        );
        item.insert(
            "last_heartbeat".to_string(),
            AttributeValue::N(last_heartbeat.to_string()),
        );
        item.insert("created_at".to_string(), AttributeValue::N(now.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(now.to_string()));
        item.insert(
            "registration_blob".to_string(),
            AttributeValue::B(aws_sdk_dynamodb::primitives::Blob::new(blob)),
        );

        // Optional fields
        if !registration.object_name.is_empty() {
            item.insert(
                "object_name".to_string(),
                AttributeValue::S(registration.object_name.clone()),
            );
        }
        if !registration.node_id.is_empty() {
            item.insert(
                "node_id".to_string(),
                AttributeValue::S(registration.node_id.clone()),
            );
        }
        if !registration.object_category.is_empty() {
            item.insert(
                "object_category".to_string(),
                AttributeValue::S(registration.object_category.clone()),
            );
        }
        if !registration.version.is_empty() {
            item.insert(
                "version".to_string(),
                AttributeValue::S(registration.version.clone()),
            );
        }

        self.client
            .put_item()
            .table_name(&self.table_name)
            .set_item(Some(item))
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn get(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);

        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        match result.item() {
            Some(item) => {
                let registration = Self::parse_registration(item)?;
                Ok(Some(registration))
            }
            None => Ok(None),
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn delete(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<()> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);

        self.client
            .delete_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(())
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn discover(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        let tenant_scoped = !ctx.is_admin() || !ctx.tenant_id().is_empty();
        let namespace_scoped = !ctx.is_admin() || !ctx.namespace().is_empty();

        if !tenant_scoped || !namespace_scoped {
            let mut expression_values: HashMap<String, AttributeValue> = HashMap::new();
            let mut filter_parts = Vec::new();

            if tenant_scoped {
                filter_parts.push("tenant_id = :tenant_id");
                expression_values.insert(
                    ":tenant_id".to_string(),
                    AttributeValue::S(ctx.tenant_id().to_string()),
                );
            }
            if namespace_scoped {
                filter_parts.push("namespace = :namespace");
                expression_values.insert(
                    ":namespace".to_string(),
                    AttributeValue::S(ctx.namespace().to_string()),
                );
            }
            if let Some(ref obj_type) = filter.object_type {
                filter_parts.push("object_type = :object_type");
                expression_values.insert(
                    ":object_type".to_string(),
                    AttributeValue::N((obj_type.clone() as i32).to_string()),
                );
            }
            if let Some(ref category) = filter.object_category {
                filter_parts.push("object_category = :category");
                expression_values
                    .insert(":category".to_string(), AttributeValue::S(category.clone()));
            }
            if let Some(ref node_id) = filter.node_id {
                filter_parts.push("node_id = :node_id");
                expression_values
                    .insert(":node_id".to_string(), AttributeValue::S(node_id.clone()));
            }
            if let Some(ref status) = filter.health_status {
                filter_parts.push("health_status = :health_status");
                expression_values.insert(
                    ":health_status".to_string(),
                    AttributeValue::N((status.clone() as i32).to_string()),
                );
            }
            if let Some(before) = filter.last_heartbeat_before {
                filter_parts.push("last_heartbeat < :hb_before");
                expression_values.insert(
                    ":hb_before".to_string(),
                    AttributeValue::N(before.to_string()),
                );
            }
            if let Some(after) = filter.last_heartbeat_after {
                filter_parts.push("last_heartbeat > :hb_after");
                expression_values.insert(
                    ":hb_after".to_string(),
                    AttributeValue::N(after.to_string()),
                );
            }

            let mut scan = self.client.scan().table_name(&self.table_name);
            if !filter_parts.is_empty() {
                scan = scan
                    .filter_expression(filter_parts.join(" AND "))
                    .set_expression_attribute_values(Some(expression_values));
            }
            scan = scan.limit((offset + limit) as i32);

            let result = scan
                .send()
                .await
                .map_err(|e| RepositoryError::Storage(e.to_string()))?;

            let items = result.items();
            let mut results = Vec::with_capacity(limit);

            for (i, item) in items.iter().enumerate() {
                if i < offset {
                    continue;
                }
                if results.len() >= limit {
                    break;
                }

                let registration = Self::parse_registration(item)?;

                if let Some(ref required_labels) = filter.labels {
                    if !required_labels
                        .iter()
                        .all(|l| registration.labels.contains(l))
                    {
                        continue;
                    }
                }
                if let Some(ref required_caps) = filter.capabilities {
                    if !required_caps
                        .iter()
                        .all(|c| registration.capabilities.contains(c))
                    {
                        continue;
                    }
                }

                results.push(registration);
            }

            return Ok(results);
        }

        let tenant_namespace = Self::make_tenant_namespace(ctx.tenant_id(), ctx.namespace());

        // Choose index based on filter
        let (index_name, key_condition) = if let Some(ref obj_type) = filter.object_type {
            // Use type_index GSI
            let type_prefix = format!("{}#", obj_type.clone() as i32);
            (
                Some("type_index"),
                format!("tenant_namespace = :tn AND begins_with(object_type_id, :tp)"),
            )
        } else if filter.last_heartbeat_before.is_some() || filter.last_heartbeat_after.is_some() {
            // Use heartbeat_index GSI
            let mut cond = "tenant_namespace = :tn".to_string();
            if let Some(before) = filter.last_heartbeat_before {
                cond.push_str(" AND last_heartbeat < :hb_before");
            }
            if let Some(after) = filter.last_heartbeat_after {
                cond.push_str(" AND last_heartbeat > :hb_after");
            }
            (Some("heartbeat_index"), cond)
        } else {
            // Full table scan with partition key
            (Some("type_index"), "tenant_namespace = :tn".to_string())
        };

        let mut expression_values: HashMap<String, AttributeValue> = HashMap::new();
        expression_values.insert(":tn".to_string(), AttributeValue::S(tenant_namespace));

        if let Some(ref obj_type) = filter.object_type {
            let type_prefix = format!("{}#", obj_type.clone() as i32);
            expression_values.insert(":tp".to_string(), AttributeValue::S(type_prefix));
        }

        if let Some(before) = filter.last_heartbeat_before {
            expression_values.insert(
                ":hb_before".to_string(),
                AttributeValue::N(before.to_string()),
            );
        }

        if let Some(after) = filter.last_heartbeat_after {
            expression_values.insert(
                ":hb_after".to_string(),
                AttributeValue::N(after.to_string()),
            );
        }

        // Build filter expression for additional filters
        let mut filter_parts = Vec::new();
        if let Some(ref category) = filter.object_category {
            filter_parts.push("object_category = :cat");
            expression_values.insert(":cat".to_string(), AttributeValue::S(category.clone()));
        }
        if let Some(ref node_id) = filter.node_id {
            filter_parts.push("node_id = :nid");
            expression_values.insert(":nid".to_string(), AttributeValue::S(node_id.clone()));
        }
        if let Some(ref status) = filter.health_status {
            filter_parts.push("health_status = :hs");
            expression_values.insert(
                ":hs".to_string(),
                AttributeValue::N((status.clone() as i32).to_string()),
            );
        }

        let filter_expr = if filter_parts.is_empty() {
            None
        } else {
            Some(filter_parts.join(" AND "))
        };

        let mut query = self
            .client
            .query()
            .table_name(&self.table_name)
            .key_condition_expression(&key_condition)
            .set_expression_attribute_values(Some(expression_values));

        if let Some(index) = index_name {
            query = query.index_name(index);
        }

        if let Some(ref filter_expr) = filter_expr {
            query = query.filter_expression(filter_expr);
        }

        // Note: DynamoDB doesn't support OFFSET directly, would need to use ExclusiveStartKey
        // For simplicity, we fetch more and skip in memory
        query = query.limit((offset + limit) as i32);

        let result = query
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let items = result.items();
        let mut results = Vec::with_capacity(limit);

        for (i, item) in items.iter().enumerate() {
            if i < offset {
                continue;
            }
            if results.len() >= limit {
                break;
            }

            let registration = Self::parse_registration(item)?;

            // Post-filter for labels and capabilities
            if let Some(ref required_labels) = filter.labels {
                if !required_labels
                    .iter()
                    .all(|l| registration.labels.contains(l))
                {
                    continue;
                }
            }
            if let Some(ref required_caps) = filter.capabilities {
                if !required_caps
                    .iter()
                    .all(|c| registration.capabilities.contains(c))
                {
                    continue;
                }
            }

            results.push(registration);
        }

        Ok(results)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        timestamp: i64,
    ) -> RepositoryResult<()> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);
        let now = Self::now_unix();

        let result = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .update_expression("SET last_heartbeat = :hb, updated_at = :ua")
            .expression_attribute_values(":hb", AttributeValue::N(timestamp.to_string()))
            .expression_attribute_values(":ua", AttributeValue::N(now.to_string()))
            .condition_expression("attribute_exists(pk)")
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_code = e.code().unwrap_or("Unknown");
                if error_code == "ConditionalCheckFailedException" {
                    Err(RepositoryError::NotFound(object_id.to_string()))
                } else {
                    Err(RepositoryError::Storage(e.to_string()))
                }
            }
        }
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn update_health_status(
        &self,
        ctx: &RequestContext,
        object_id: &str,
        status: HealthStatus,
    ) -> RepositoryResult<()> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);
        let now = Self::now_unix();

        let result = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .update_expression("SET health_status = :hs, updated_at = :ua")
            .expression_attribute_values(":hs", AttributeValue::N((status as i32).to_string()))
            .expression_attribute_values(":ua", AttributeValue::N(now.to_string()))
            .condition_expression("attribute_exists(pk)")
            .send()
            .await;

        match result {
            Ok(_) => Ok(()),
            Err(e) => {
                let error_code = e.code().unwrap_or("Unknown");
                if error_code == "ConditionalCheckFailedException" {
                    Err(RepositoryError::NotFound(object_id.to_string()))
                } else {
                    Err(RepositoryError::Storage(e.to_string()))
                }
            }
        }
    }

    #[instrument(skip(self, ctx, filter), fields(tenant_id = %ctx.tenant_id()))]
    async fn count(
        &self,
        ctx: &RequestContext,
        filter: &DiscoverFilter,
    ) -> RepositoryResult<usize> {
        // For DynamoDB, we need to query and count
        // This is not efficient for large datasets but works for moderate sizes
        let results = self.discover(ctx, filter, 0, 10000).await?;
        Ok(results.len())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn list_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
        offset: usize,
        limit: usize,
    ) -> RepositoryResult<Vec<String>> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty())
                .then(|| ctx.tenant_id().to_string())
                .into_iter()
                .collect());
        }

        let registrations = self
            .discover(
                ctx,
                &DiscoverFilter {
                    object_type: Some(object_type),
                    ..Default::default()
                },
                0,
                10_000,
            )
            .await?;
        let mut tenant_ids = std::collections::BTreeSet::new();
        for registration in registrations {
            if !registration.tenant_id.is_empty() {
                tenant_ids.insert(registration.tenant_id);
            }
        }
        Ok(tenant_ids.into_iter().skip(offset).take(limit).collect())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_type = ?object_type))]
    async fn count_tenant_ids_by_object_type(
        &self,
        ctx: &RequestContext,
        object_type: ObjectType,
    ) -> RepositoryResult<usize> {
        if ctx.auth_enabled && !ctx.is_admin() {
            return Ok((!ctx.tenant_id().is_empty()) as usize);
        }

        let registrations = self
            .discover(
                ctx,
                &DiscoverFilter {
                    object_type: Some(object_type),
                    ..Default::default()
                },
                0,
                10_000,
            )
            .await?;
        let mut tenant_ids = std::collections::BTreeSet::new();
        for registration in registrations {
            if !registration.tenant_id.is_empty() {
                tenant_ids.insert(registration.tenant_id);
            }
        }
        Ok(tenant_ids.len())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), object_id = %object_id))]
    async fn exists(&self, ctx: &RequestContext, object_id: &str) -> RepositoryResult<bool> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);

        let result = self
            .client
            .get_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .projection_expression("pk")
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        Ok(result.item().is_some())
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), alias = %alias))]
    async fn get_by_alias(
        &self,
        ctx: &RequestContext,
        alias: &str,
    ) -> RepositoryResult<Option<ObjectRegistration>> {
        if alias.is_empty() {
            return Ok(None);
        }
        // Query alias-index GSI, then verify tenant/namespace match to enforce isolation.
        // The GSI has alias as partition key; tenant isolation is enforced in the filter.
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("alias-index")
            .key_condition_expression("#alias_attr = :alias_val")
            .filter_expression("tenant_id = :tid AND #ns = :ns")
            .expression_attribute_names("#alias_attr", "alias")
            .expression_attribute_names("#ns", "namespace")
            .expression_attribute_values(":alias_val", AttributeValue::S(alias.to_string()))
            .expression_attribute_values(":tid", AttributeValue::S(ctx.tenant_id().to_string()))
            .expression_attribute_values(":ns", AttributeValue::S(ctx.namespace().to_string()))
            .limit(1)
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let items = result.items();
        if !items.is_empty() {
            if let Some(blob_attr) = items[0].get("registration_blob") {
                if let Ok(blob) = blob_attr.as_b() {
                    let mut registration =
                        ObjectRegistration::decode(blob.as_ref()).map_err(|e| {
                            RepositoryError::Serialization(e.to_string())
                        })?;
                    // Merge indexed columns from item attributes
                    if let Some(hs) = items[0]
                        .get("health_status")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        registration.health_status = hs;
                    }
                    if let Some(fc) = items[0]
                        .get("heartbeat_failure_count")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u32>().ok())
                    {
                        registration.heartbeat_failure_count = fc;
                    }
                    if let Some(mf) = items[0]
                        .get("max_heartbeat_failures")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u32>().ok())
                    {
                        if mf > 0 {
                            registration.max_heartbeat_failures = mf;
                        }
                    }
                    return Ok(Some(registration));
                }
            }
        }
        Ok(None)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn increment_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<u32> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);
        let result = self
            .client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .update_expression("ADD heartbeat_failure_count :one")
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .return_values(aws_sdk_dynamodb::types::ReturnValue::UpdatedNew)
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let count = result
            .attributes()
            .and_then(|attrs| attrs.get("heartbeat_failure_count"))
            .and_then(|v| v.as_n().ok())
            .and_then(|n| n.parse::<u32>().ok())
            .unwrap_or(1);
        Ok(count)
    }

    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), object_id = %object_id))]
    async fn reset_heartbeat_failures(
        &self,
        ctx: &RequestContext,
        object_id: &str,
    ) -> RepositoryResult<()> {
        let pk = Self::make_pk(ctx.tenant_id(), ctx.namespace(), object_id);
        self.client
            .update_item()
            .table_name(&self.table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("REG".to_string()))
            .update_expression("SET heartbeat_failure_count = :zero")
            .expression_attribute_values(":zero", AttributeValue::N("0".to_string()))
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;
        Ok(())
    }

    /// Mark all live objects on `node_id` as DEAD, scoped to tenant+namespace.
    ///
    /// Queries the `node-index` GSI (partition key: `node_id`) with a filter on
    /// `tenant_id`/`namespace` and `health_status IN (1, 2, 4)`.  Each matching item
    /// is updated individually since DynamoDB has no bulk-update primitive.
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), node_id = %node_id))]
    async fn mark_dead_by_node_id(
        &self,
        ctx: &RequestContext,
        node_id: &str,
    ) -> RepositoryResult<u64> {
        // Query node-index GSI for this node; filter to tenant+namespace and live statuses.
        let result = self
            .client
            .query()
            .table_name(&self.table_name)
            .index_name("node-index")
            .key_condition_expression("node_id = :nid")
            .filter_expression(
                "tenant_id = :tid AND #ns = :ns AND health_status IN (:h1, :h2, :h4)",
            )
            .expression_attribute_names("#ns", "namespace")
            .expression_attribute_values(":nid", AttributeValue::S(node_id.to_string()))
            .expression_attribute_values(":tid", AttributeValue::S(ctx.tenant_id().to_string()))
            .expression_attribute_values(":ns", AttributeValue::S(ctx.namespace().to_string()))
            .expression_attribute_values(":h1", AttributeValue::N("1".to_string()))
            .expression_attribute_values(":h2", AttributeValue::N("2".to_string()))
            .expression_attribute_values(":h4", AttributeValue::N("4".to_string()))
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut updated = 0u64;
        for item in result.items() {
            let pk = item
                .get("pk")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string());
            let sk = item
                .get("sk")
                .and_then(|v| v.as_s().ok())
                .map(|s| s.to_string());
            if let (Some(pk), Some(sk)) = (pk, sk) {
                self.client
                    .update_item()
                    .table_name(&self.table_name)
                    .key("pk", AttributeValue::S(pk))
                    .key("sk", AttributeValue::S(sk))
                    .update_expression("SET health_status = :dead")
                    .expression_attribute_values(":dead", AttributeValue::N("3".to_string()))
                    .send()
                    .await
                    .map_err(|e| RepositoryError::Storage(e.to_string()))?;
                updated += 1;
            }
        }
        Ok(updated)
    }

    /// Find registrations with stale heartbeats.
    ///
    /// Scoped to tenant+namespace when ctx has a non-empty tenant_id.  Pass an admin
    /// context with empty tenant_id to scan all tenants (system heartbeat monitor).
    #[instrument(skip(self, ctx), fields(tenant_id = %ctx.tenant_id(), namespace = %ctx.namespace(), threshold_seconds = %threshold_seconds, limit = %limit))]
    async fn find_stale_heartbeats(
        &self,
        ctx: &RequestContext,
        threshold_seconds: i64,
        limit: usize,
    ) -> RepositoryResult<Vec<ObjectRegistration>> {
        let cutoff = chrono::Utc::now().timestamp() - threshold_seconds;

        // Build scan with conditional tenant filter
        let mut scan = self
            .client
            .scan()
            .table_name(&self.table_name)
            .limit(limit as i32);

        if !ctx.tenant_id().is_empty() {
            scan = scan
                .filter_expression(
                    "tenant_id = :tid AND #ns = :ns AND health_status IN (:h1, :h2) AND (attribute_not_exists(last_heartbeat) OR last_heartbeat < :cutoff)"
                )
                .expression_attribute_names("#ns", "namespace")
                .expression_attribute_values(":tid", AttributeValue::S(ctx.tenant_id().to_string()))
                .expression_attribute_values(":ns", AttributeValue::S(ctx.namespace().to_string()))
                .expression_attribute_values(":h1", AttributeValue::N("1".to_string()))
                .expression_attribute_values(":h2", AttributeValue::N("2".to_string()))
                .expression_attribute_values(":cutoff", AttributeValue::N(cutoff.to_string()));
        } else {
            scan = scan
                .filter_expression(
                    "health_status IN (:h1, :h2) AND (attribute_not_exists(last_heartbeat) OR last_heartbeat < :cutoff)"
                )
                .expression_attribute_values(":h1", AttributeValue::N("1".to_string()))
                .expression_attribute_values(":h2", AttributeValue::N("2".to_string()))
                .expression_attribute_values(":cutoff", AttributeValue::N(cutoff.to_string()));
        }

        let result = scan
            .send()
            .await
            .map_err(|e| RepositoryError::Storage(e.to_string()))?;

        let mut registrations = Vec::new();
        for item in result.items() {
            if let Some(blob_attr) = item.get("registration_blob") {
                if let Ok(blob) = blob_attr.as_b() {
                    let mut registration =
                        ObjectRegistration::decode(blob.as_ref()).map_err(|e| {
                            RepositoryError::Serialization(e.to_string())
                        })?;
                    // Merge indexed columns from item
                    if let Some(hs) = item
                        .get("health_status")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<i32>().ok())
                    {
                        registration.health_status = hs;
                    }
                    if let Some(fc) = item
                        .get("heartbeat_failure_count")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u32>().ok())
                    {
                        registration.heartbeat_failure_count = fc;
                    }
                    if let Some(mf) = item
                        .get("max_heartbeat_failures")
                        .and_then(|v| v.as_n().ok())
                        .and_then(|n| n.parse::<u32>().ok())
                    {
                        if mf > 0 {
                            registration.max_heartbeat_failures = mf;
                        }
                    }
                    registrations.push(registration);
                }
            }
        }
        Ok(registrations)
    }
}
