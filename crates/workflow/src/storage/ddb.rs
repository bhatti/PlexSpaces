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

//! DynamoDB-based WorkflowStorage implementation.
//!
//! ## Purpose
//! Provides a production-grade DynamoDB backend for workflow storage
//! with proper tenant isolation, optimistic locking, and comprehensive observability.
//!
//! ## Design
//! - **Composite Partition Keys**: `{tenant_id}#{namespace}#{entity_id}` for tenant isolation
//! - **Auto-table creation**: Creates tables with proper schema on initialization
//! - **GSI for queries**: Multiple GSIs for efficient querying by status, node, etc.
//! - **Optimistic locking**: Version-based conditional writes
//! - **Production-grade**: Full observability (metrics, tracing, structured logging)
//!
//! ## Table Schema
//!
//! ### workflow_definitions
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{definition_id}#{version}"
//! Sort Key: sk = "DEF"
//! Attributes:
//!   - definition_id: String
//!   - version: String
//!   - name: String
//!   - definition_json: String (serialized WorkflowDefinition)
//!   - created_at: Number (UNIX timestamp)
//!   - updated_at: Number (UNIX timestamp)
//! ```
//!
//! ### workflow_executions
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{execution_id}"
//! Sort Key: sk = "EXEC"
//! Attributes:
//!   - execution_id: String
//!   - definition_id: String
//!   - definition_version: String
//!   - status: String (PENDING, RUNNING, COMPLETED, FAILED, CANCELLED, TIMED_OUT)
//!   - current_step_id: String (optional)
//!   - input_json: String (optional)
//!   - output_json: String (optional)
//!   - error: String (optional)
//!   - node_id: String (optional)
//!   - version: Number (for optimistic locking)
//!   - last_heartbeat: Number (UNIX timestamp, optional)
//!   - created_at: Number (UNIX timestamp)
//!   - started_at: Number (UNIX timestamp, optional)
//!   - completed_at: Number (UNIX timestamp, optional)
//!   - updated_at: Number (UNIX timestamp)
//! ```
//!
//! ### GSI: status_index
//! - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//! - Sort Key: `status_created` = "{status}#{created_at}"
//! - Purpose: Query executions by status
//!
//! ### GSI: node_index
//! - Partition Key: `tenant_namespace` = "{tenant_id}#{namespace}"
//! - Sort Key: `node_heartbeat` = "{node_id}#{last_heartbeat}"
//! - Purpose: Query executions by node for recovery
//!
//! ### step_executions
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{execution_id}"
//! Sort Key: sk = "STEP#{step_execution_id}"
//! Attributes:
//!   - step_execution_id: String
//!   - execution_id: String
//!   - step_id: String
//!   - status: String (RUNNING, COMPLETED, FAILED)
//!   - input_json: String (optional)
//!   - output_json: String (optional)
//!   - error: String (optional)
//!   - attempt: Number
//!   - started_at: Number (UNIX timestamp)
//!   - completed_at: Number (UNIX timestamp, optional)
//! ```
//!
//! ### signals
//! ```
//! Partition Key: pk = "{tenant_id}#{namespace}#{execution_id}"
//! Sort Key: sk = "SIGNAL#{signal_name}#{received_at}"
//! Attributes:
//!   - signal_id: String
//!   - execution_id: String
//!   - signal_name: String
//!   - payload: String (JSON)
//!   - received_at: Number (UNIX timestamp)
//! ```

use crate::types::*;
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
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, error, instrument, warn};

/// Trait for workflow storage with RequestContext support for multi-tenancy.
#[async_trait]
pub trait WorkflowStorageTrait: Send + Sync {
    /// Save workflow definition
    async fn save_definition(
        &self,
        ctx: &RequestContext,
        def: &WorkflowDefinition,
    ) -> Result<(), WorkflowError>;

    /// Get workflow definition
    async fn get_definition(
        &self,
        ctx: &RequestContext,
        id: &str,
        version: &str,
    ) -> Result<WorkflowDefinition, WorkflowError>;

    /// List workflow definitions
    async fn list_definitions(
        &self,
        ctx: &RequestContext,
        name_prefix: Option<&str>,
    ) -> Result<Vec<WorkflowDefinition>, WorkflowError>;

    /// Delete workflow definition
    async fn delete_definition(
        &self,
        ctx: &RequestContext,
        id: &str,
        version: &str,
    ) -> Result<(), WorkflowError>;

    /// Create workflow execution
    async fn create_execution(
        &self,
        ctx: &RequestContext,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        labels: HashMap<String, String>,
    ) -> Result<String, WorkflowError>;

    /// Create workflow execution with node ownership
    async fn create_execution_with_node(
        &self,
        ctx: &RequestContext,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        labels: HashMap<String, String>,
        node_id: Option<&str>,
    ) -> Result<String, WorkflowError>;

    /// Get workflow execution
    async fn get_execution(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
    ) -> Result<WorkflowExecution, WorkflowError>;

    /// Update execution status
    async fn update_execution_status(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        status: ExecutionStatus,
    ) -> Result<(), WorkflowError>;

    /// Update execution status with version check (optimistic locking)
    async fn update_execution_status_with_version(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        status: ExecutionStatus,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError>;

    /// Update execution output
    async fn update_execution_output(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        output: Value,
    ) -> Result<(), WorkflowError>;

    /// Update execution output with version check
    async fn update_execution_output_with_version(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        output: Value,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError>;

    /// Transfer workflow ownership
    async fn transfer_ownership(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        new_node_id: &str,
        expected_version: u64,
    ) -> Result<(), WorkflowError>;

    /// Update heartbeat
    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        node_id: &str,
    ) -> Result<(), WorkflowError>;

    /// Create step execution
    async fn create_step_execution(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        step_id: &str,
        input: Value,
    ) -> Result<String, WorkflowError>;

    /// Get step execution
    async fn get_step_execution(
        &self,
        ctx: &RequestContext,
        step_exec_id: &str,
    ) -> Result<StepExecution, WorkflowError>;

    /// Complete step execution
    async fn complete_step_execution(
        &self,
        ctx: &RequestContext,
        step_exec_id: &str,
        status: StepExecutionStatus,
        output: Option<Value>,
        error: Option<String>,
    ) -> Result<(), WorkflowError>;

    /// Get step execution history
    async fn get_step_execution_history(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
    ) -> Result<Vec<StepExecution>, WorkflowError>;

    /// Send signal
    async fn send_signal(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        signal_name: &str,
        payload: Value,
    ) -> Result<(), WorkflowError>;

    /// Check signal (consumes it)
    async fn check_signal(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        signal_name: &str,
    ) -> Result<Option<Value>, WorkflowError>;

    /// List executions by status
    async fn list_executions_by_status(
        &self,
        ctx: &RequestContext,
        statuses: Vec<ExecutionStatus>,
        node_id: Option<&str>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError>;

    /// List stale executions
    async fn list_stale_executions(
        &self,
        ctx: &RequestContext,
        stale_threshold_seconds: u64,
        statuses: Vec<ExecutionStatus>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError>;
}

/// DynamoDB WorkflowStorage implementation.
#[derive(Clone)]
pub struct DynamoDBWorkflowStorage {
    /// DynamoDB client
    client: DynamoDbClient,
    /// Table name prefix (e.g., "plexspaces-workflow")
    table_prefix: String,
    /// Schema version
    schema_version: u32,
}

impl DynamoDBWorkflowStorage {
    /// Create a new DynamoDB WorkflowStorage.
    ///
    /// ## Arguments
    /// * `region` - AWS region (e.g., "us-east-1")
    /// * `table_prefix` - Table name prefix (tables will be: {prefix}-definitions, {prefix}-executions, etc.)
    /// * `endpoint_url` - Optional endpoint URL (for DynamoDB Local testing)
    #[instrument(skip(region, table_prefix, endpoint_url), fields(region = %region, table_prefix = %table_prefix))]
    pub async fn new(
        region: String,
        table_prefix: String,
        endpoint_url: Option<String>,
    ) -> Result<Self, WorkflowError> {
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
        Self::ensure_definitions_table_exists(&client, &format!("{}-definitions", table_prefix)).await?;
        Self::ensure_executions_table_exists(&client, &format!("{}-executions", table_prefix)).await?;
        Self::ensure_step_executions_table_exists(&client, &format!("{}-step-executions", table_prefix)).await?;
        Self::ensure_signals_table_exists(&client, &format!("{}-signals", table_prefix)).await?;

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_workflow_ddb_init_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());

        debug!(
            table_prefix = %table_prefix,
            region = %region,
            duration_ms = duration.as_millis(),
            "DynamoDB WorkflowStorage initialized"
        );

        Ok(Self {
            client,
            table_prefix,
            schema_version: 1,
        })
    }

    /// Create composite partition key for tenant isolation.
    fn composite_key(ctx: &RequestContext, entity_id: &str) -> String {
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
        format!("{}#{}#{}", tenant_id, namespace, entity_id)
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

    /// Ensure definitions table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_definitions_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), WorkflowError> {
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
                    return Err(WorkflowError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB table");

        // Create table
        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

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
                    Err(WorkflowError::Storage(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Ensure executions table exists with GSIs.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_executions_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), WorkflowError> {
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
                    return Err(WorkflowError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB executions table with GSIs");

        // Create table with GSIs for status and node queries
        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let tenant_namespace_attr = AttributeDefinition::builder()
            .attribute_name("tenant_namespace")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let status_created_attr = AttributeDefinition::builder()
            .attribute_name("status_created")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let node_heartbeat_attr = AttributeDefinition::builder()
            .attribute_name("node_heartbeat")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        // GSI: status_index
        let status_gsi_pk = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let status_gsi_sk = KeySchemaElement::builder()
            .attribute_name("status_created")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let status_gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let status_gsi = GlobalSecondaryIndex::builder()
            .index_name("status_index")
            .key_schema(status_gsi_pk)
            .key_schema(status_gsi_sk)
            .projection(status_gsi_projection)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI: {}", e)))?;

        // GSI: node_index
        let node_gsi_pk = KeySchemaElement::builder()
            .attribute_name("tenant_namespace")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let node_gsi_sk = KeySchemaElement::builder()
            .attribute_name("node_heartbeat")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI key schema: {}", e)))?;

        let node_gsi_projection = Projection::builder()
            .projection_type(ProjectionType::All)
            .build();

        let node_gsi = GlobalSecondaryIndex::builder()
            .index_name("node_index")
            .key_schema(node_gsi_pk)
            .key_schema(node_gsi_sk)
            .projection(node_gsi_projection)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build GSI: {}", e)))?;

        let create_table_result = client
            .create_table()
            .table_name(table_name)
            .billing_mode(BillingMode::PayPerRequest)
            .key_schema(pk_key_schema)
            .key_schema(sk_key_schema)
            .attribute_definitions(pk_attr)
            .attribute_definitions(sk_attr)
            .attribute_definitions(tenant_namespace_attr)
            .attribute_definitions(status_created_attr)
            .attribute_definitions(node_heartbeat_attr)
            .global_secondary_indexes(status_gsi)
            .global_secondary_indexes(node_gsi)
            .send()
            .await;

        match create_table_result {
            Ok(_) => {
                debug!(table_name = %table_name, "DynamoDB executions table created successfully");
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(WorkflowError::Storage(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Ensure step_executions table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_step_executions_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), WorkflowError> {
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
                    return Err(WorkflowError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB step_executions table");

        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

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
                debug!(table_name = %table_name, "DynamoDB step_executions table created successfully");
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(WorkflowError::Storage(format!(
                        "Failed to create DynamoDB table: {}",
                        e
                    )))
                }
            }
        }
    }

    /// Ensure signals table exists.
    #[instrument(skip(client), fields(table_name = %table_name))]
    async fn ensure_signals_table_exists(
        client: &DynamoDbClient,
        table_name: &str,
    ) -> Result<(), WorkflowError> {
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
                    return Err(WorkflowError::Storage(format!(
                        "Failed to check table existence: {} (code: {})",
                        error_message, error_code
                    )));
                }
            }
        }

        debug!(table_name = %table_name, "Creating DynamoDB signals table");

        let pk_key_schema = KeySchemaElement::builder()
            .attribute_name("pk")
            .key_type(KeyType::Hash)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let sk_key_schema = KeySchemaElement::builder()
            .attribute_name("sk")
            .key_type(KeyType::Range)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build key schema: {}", e)))?;

        let pk_attr = AttributeDefinition::builder()
            .attribute_name("pk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

        let sk_attr = AttributeDefinition::builder()
            .attribute_name("sk")
            .attribute_type(ScalarAttributeType::S)
            .build()
            .map_err(|e| WorkflowError::Storage(format!("Failed to build attribute definition: {}", e)))?;

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
                debug!(table_name = %table_name, "DynamoDB signals table created successfully");
                Self::wait_for_table_active(client, table_name).await?;
                Ok(())
            }
            Err(e) => {
                if e.to_string().contains("ResourceInUseException") {
                    debug!(table_name = %table_name, "Table created concurrently, waiting for active");
                    Self::wait_for_table_active(client, table_name).await?;
                    Ok(())
                } else {
                    Err(WorkflowError::Storage(format!(
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
    ) -> Result<(), WorkflowError> {
        use aws_sdk_dynamodb::types::TableStatus;

        let mut attempts = 0;
        let max_attempts = 30;

        loop {
            let describe_result = client
                .describe_table()
                .table_name(table_name)
                .send()
                .await
                .map_err(|e| WorkflowError::Storage(format!("Failed to describe table: {}", e)))?;

            if let Some(status) = describe_result.table().and_then(|t| t.table_status()) {
                match status {
                    TableStatus::Active => {
                        debug!(table_name = %table_name, "Table is now active");
                        return Ok(());
                    }
                    TableStatus::Creating => {
                        attempts += 1;
                        if attempts >= max_attempts {
                            return Err(WorkflowError::Storage(format!(
                                "Table creation timeout after {} attempts",
                                max_attempts
                            )));
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        continue;
                    }
                    _ => {
                        return Err(WorkflowError::Storage(format!(
                            "Table in unexpected status: {:?}",
                            status
                        )));
                    }
                }
            } else {
                return Err(WorkflowError::Storage("Table status not available".to_string()));
            }
        }
    }

    /// Helper: Convert DDB item to WorkflowExecution
    fn item_to_execution(
        item: &HashMap<String, AttributeValue>,
    ) -> Result<WorkflowExecution, WorkflowError> {
        let execution_id = item
            .get("execution_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing execution_id".to_string()))?
            .clone();

        let definition_id = item
            .get("definition_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing definition_id".to_string()))?
            .clone();

        let definition_version = item
            .get("definition_version")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing definition_version".to_string()))?
            .clone();

        let status_str = item
            .get("status")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing status".to_string()))?;
        let status = ExecutionStatus::from_string(status_str)?;

        let current_step_id = item.get("current_step_id").and_then(|v| v.as_s().ok()).cloned();

        let input_json = item.get("input_json").and_then(|v| v.as_s().ok()).cloned();
        let input = input_json.as_ref().and_then(|s| serde_json::from_str(s).ok());

        let output_json = item.get("output_json").and_then(|v| v.as_s().ok()).cloned();
        let output = output_json.as_ref().and_then(|s| serde_json::from_str(s).ok());

        let error = item.get("error").and_then(|v| v.as_s().ok()).cloned();

        let node_id = item.get("node_id").and_then(|v| v.as_s().ok()).cloned();

        let version = item
            .get("version")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(1);

        let last_heartbeat_secs = item
            .get("last_heartbeat")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<i64>().ok());
        let last_heartbeat = last_heartbeat_secs
            .map(|secs| chrono::DateTime::from_timestamp(secs, 0).unwrap_or_else(|| Utc::now()));

        Ok(WorkflowExecution {
            execution_id,
            definition_id,
            definition_version,
            status,
            current_step_id,
            input,
            output,
            error,
            node_id,
            version,
            last_heartbeat,
        })
    }

    /// Helper: Convert DDB item to StepExecution
    fn item_to_step_execution(
        item: &HashMap<String, AttributeValue>,
    ) -> Result<StepExecution, WorkflowError> {
        let step_execution_id = item
            .get("step_execution_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing step_execution_id".to_string()))?
            .clone();

        let execution_id = item
            .get("execution_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing execution_id".to_string()))?
            .clone();

        let step_id = item
            .get("step_id")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing step_id".to_string()))?
            .clone();

        let status_str = item
            .get("status")
            .and_then(|v| v.as_s().ok())
            .ok_or_else(|| WorkflowError::Serialization("Missing status".to_string()))?;
        let status = StepExecutionStatus::from_string(status_str)?;

        let input_json = item.get("input_json").and_then(|v| v.as_s().ok()).cloned();
        let input = input_json.as_ref().and_then(|s| serde_json::from_str(s).ok());

        let output_json = item.get("output_json").and_then(|v| v.as_s().ok()).cloned();
        let output = output_json.as_ref().and_then(|s| serde_json::from_str(s).ok());

        let error = item.get("error").and_then(|v| v.as_s().ok()).cloned();

        let attempt = item
            .get("attempt")
            .and_then(|v| v.as_n().ok())
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(1);

        Ok(StepExecution {
            step_execution_id,
            execution_id,
            step_id,
            status,
            input,
            output,
            error,
            attempt,
        })
    }

    /// Internal helper for creating step execution with attempt
    async fn create_step_execution_with_attempt_internal(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        step_id: &str,
        input: Value,
        attempt: u32,
    ) -> Result<String, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-step-executions", self.table_prefix);
        let step_execution_id = ulid::Ulid::new().to_string();
        let pk = Self::composite_key(ctx, execution_id);
        let sk = format!("STEP#{}", step_execution_id);

        let input_json = serde_json::to_string(&input)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S(sk));
        item.insert("step_execution_id".to_string(), AttributeValue::S(step_execution_id.clone()));
        item.insert("execution_id".to_string(), AttributeValue::S(execution_id.to_string()));
        item.insert("step_id".to_string(), AttributeValue::S(step_id.to_string()));
        item.insert("status".to_string(), AttributeValue::S("RUNNING".to_string()));
        item.insert("input_json".to_string(), AttributeValue::S(input_json));
        item.insert("attempt".to_string(), AttributeValue::N(attempt.to_string()));
        item.insert("started_at".to_string(), AttributeValue::N(now_secs.to_string()));

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
                    "plexspaces_workflow_ddb_create_step_execution_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_create_step_execution_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(step_execution_id)
            }
            Err(e) => {
                error!(error = %e, execution_id = %execution_id, step_id = %step_id, "Failed to create step execution");
                metrics::counter!(
                    "plexspaces_workflow_ddb_create_step_execution_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }
}

#[async_trait]
impl WorkflowStorageTrait for DynamoDBWorkflowStorage {
    async fn save_definition(
        &self,
        ctx: &RequestContext,
        def: &WorkflowDefinition,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-definitions", self.table_prefix);
        let pk = format!("{}#{}", Self::composite_key(ctx, &def.id), def.version);

        let definition_json = serde_json::to_string(def)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let now_secs = Utc::now().timestamp();

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("DEF".to_string()));
        item.insert("definition_id".to_string(), AttributeValue::S(def.id.clone()));
        item.insert("version".to_string(), AttributeValue::S(def.version.clone()));
        item.insert("name".to_string(), AttributeValue::S(def.name.clone()));
        item.insert("definition_json".to_string(), AttributeValue::S(definition_json));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert(
            "schema_version".to_string(),
            AttributeValue::N(self.schema_version.to_string()),
        );

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
                    "plexspaces_workflow_ddb_save_definition_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_save_definition_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, "Failed to save workflow definition");
                metrics::counter!(
                    "plexspaces_workflow_ddb_save_definition_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn get_definition(
        &self,
        ctx: &RequestContext,
        id: &str,
        version: &str,
    ) -> Result<WorkflowDefinition, WorkflowError> {
        let table_name = format!("{}-definitions", self.table_prefix);
        let pk = format!("{}#{}", Self::composite_key(ctx, id), version);

        match self
            .client
            .get_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("DEF".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                if let Some(item) = result.item().cloned() {
                    if let Some(def_json_attr) = item.get("definition_json") {
                        if let Ok(def_json) = def_json_attr.as_s() {
                            let definition: WorkflowDefinition = serde_json::from_str(def_json)
                                .map_err(|e| WorkflowError::Serialization(e.to_string()))?;
                            return Ok(definition);
                        }
                    }
                }
                Err(WorkflowError::NotFound(format!(
                    "Definition {}:{} not found",
                    id, version
                )))
            }
            Err(e) => Err(WorkflowError::Storage(format!("DynamoDB get_item failed: {}", e))),
        }
    }

    async fn list_definitions(
        &self,
        ctx: &RequestContext,
        name_prefix: Option<&str>,
    ) -> Result<Vec<WorkflowDefinition>, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-definitions", self.table_prefix);
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        // Query all definitions for this tenant/namespace
        // Note: We need to scan or use a GSI - for now, we'll scan with filter
        // In production, consider adding a GSI for name queries
        let mut definitions = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut scan = self.client.scan().table_name(&table_name);

            // Filter by tenant_namespace (if we had a GSI, we'd query it)
            // For now, we'll scan and filter client-side (not ideal for large datasets)
            if let Some(lek) = last_evaluated_key {
                scan = scan.set_exclusive_start_key(Some(lek));
            }

            match scan.send().await {
                Ok(result) => {
                    for item in result.items() {
                        // Check tenant isolation
                        if let Some(pk_attr) = item.get("pk") {
                            if let Ok(pk) = pk_attr.as_s() {
                                if !pk.starts_with(&format!("{}#", tenant_namespace)) {
                                    continue; // Skip items from other tenants
                                }
                            }
                        }

                        // Filter by name prefix if specified
                        if let Some(prefix) = name_prefix {
                            if let Some(name_attr) = item.get("name") {
                                if let Ok(name) = name_attr.as_s() {
                                    if !name.starts_with(prefix) {
                                        continue;
                                    }
                                }
                            }
                        }

                        // Parse definition
                        if let Some(def_json_attr) = item.get("definition_json") {
                            if let Ok(def_json) = def_json_attr.as_s() {
                                if let Ok(definition) = serde_json::from_str::<WorkflowDefinition>(def_json) {
                                    definitions.push(definition);
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
                    error!(error = %e, "Failed to scan definitions from DynamoDB");
                    return Err(WorkflowError::Storage(format!("DynamoDB scan failed: {}", e)));
                }
            }
        }

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_workflow_ddb_list_definitions_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_workflow_ddb_list_definitions_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => definitions.len().to_string()
        )
        .increment(1);

        Ok(definitions)
    }

    async fn delete_definition(
        &self,
        ctx: &RequestContext,
        id: &str,
        version: &str,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-definitions", self.table_prefix);
        let pk = format!("{}#{}", Self::composite_key(ctx, id), version);

        match self
            .client
            .delete_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("DEF".to_string()))
            .send()
            .await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_delete_definition_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_delete_definition_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, definition_id = %id, version = %version, "Failed to delete definition");
                metrics::counter!(
                    "plexspaces_workflow_ddb_delete_definition_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB delete_item failed: {}", e)))
            }
        }
    }

    async fn create_execution(
        &self,
        ctx: &RequestContext,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        _labels: HashMap<String, String>,
    ) -> Result<String, WorkflowError> {
        self.create_execution_with_node(ctx, definition_id, definition_version, input, HashMap::new(), None)
            .await
    }

    async fn create_execution_with_node(
        &self,
        ctx: &RequestContext,
        definition_id: &str,
        definition_version: &str,
        input: Value,
        _labels: HashMap<String, String>,
        node_id: Option<&str>,
    ) -> Result<String, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let execution_id = ulid::Ulid::new().to_string();
        let pk = Self::composite_key(ctx, &execution_id);
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        let input_json = serde_json::to_string(&input)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let now_secs = Utc::now().timestamp();
        let now_ts = chrono::DateTime::from_timestamp(now_secs, 0).unwrap_or_else(|| Utc::now());

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S("EXEC".to_string()));
        item.insert("execution_id".to_string(), AttributeValue::S(execution_id.clone()));
        item.insert("definition_id".to_string(), AttributeValue::S(definition_id.to_string()));
        item.insert("definition_version".to_string(), AttributeValue::S(definition_version.to_string()));
        item.insert("status".to_string(), AttributeValue::S("PENDING".to_string()));
        item.insert("input_json".to_string(), AttributeValue::S(input_json));
        item.insert("version".to_string(), AttributeValue::N("1".to_string()));
        item.insert("tenant_namespace".to_string(), AttributeValue::S(tenant_namespace.clone()));
        item.insert("created_at".to_string(), AttributeValue::N(now_secs.to_string()));
        item.insert("updated_at".to_string(), AttributeValue::N(now_secs.to_string()));

        // For status_index GSI
        let status_created = format!("PENDING#{}", now_secs);
        item.insert("status_created".to_string(), AttributeValue::S(status_created));

        if let Some(node) = node_id {
            item.insert("node_id".to_string(), AttributeValue::S(node.to_string()));
            item.insert("last_heartbeat".to_string(), AttributeValue::N(now_secs.to_string()));
            // For node_index GSI
            let node_heartbeat = format!("{}#{}", node, now_secs);
            item.insert("node_heartbeat".to_string(), AttributeValue::S(node_heartbeat));
        }

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
                    "plexspaces_workflow_ddb_create_execution_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_create_execution_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(execution_id)
            }
            Err(e) => {
                error!(error = %e, execution_id = %execution_id, "Failed to create execution");
                metrics::counter!(
                    "plexspaces_workflow_ddb_create_execution_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn get_execution(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
    ) -> Result<WorkflowExecution, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);

        match self
            .client
            .get_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("EXEC".to_string()))
            .send()
            .await
        {
            Ok(result) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_get_execution_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());

                if let Some(item) = result.item().cloned() {
                    let execution = Self::item_to_execution(&item)?;
                    metrics::counter!(
                        "plexspaces_workflow_ddb_get_execution_total",
                        "backend" => "dynamodb",
                        "result" => "found"
                    )
                    .increment(1);
                    Ok(execution)
                } else {
                    metrics::counter!(
                        "plexspaces_workflow_ddb_get_execution_total",
                        "backend" => "dynamodb",
                        "result" => "not_found"
                    )
                    .increment(1);
                    Err(WorkflowError::NotFound(format!(
                        "Execution {} not found",
                        execution_id
                    )))
                }
            }
            Err(e) => {
                error!(error = %e, execution_id = %execution_id, "Failed to get execution");
                metrics::counter!(
                    "plexspaces_workflow_ddb_get_execution_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB get_item failed: {}", e)))
            }
        }
    }

    async fn update_execution_status(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        status: ExecutionStatus,
    ) -> Result<(), WorkflowError> {
        self.update_execution_status_with_version(ctx, execution_id, status, None).await
    }

    async fn update_execution_status_with_version(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        status: ExecutionStatus,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);
        let status_str = status.to_string();
        let now_secs = Utc::now().timestamp();
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        // Build update expression
        let mut update_expr = "SET #status = :status, version = version + :one, updated_at = :now".to_string();

        // Update status_created for GSI
        let status_created = format!("{}#{}", status_str, now_secs);
        update_expr.push_str(", status_created = :status_created");

        // Update timestamps based on status
        if status == ExecutionStatus::Running {
            update_expr.push_str(", last_heartbeat = :now");
            update_expr.push_str(", started_at = if_not_exists(started_at, :now)");
        }

        if matches!(status, ExecutionStatus::Completed | ExecutionStatus::Failed | ExecutionStatus::Cancelled | ExecutionStatus::TimedOut) {
            update_expr.push_str(", completed_at = :now");
        }

        let mut update = self
            .client
            .update_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("EXEC".to_string()))
            .update_expression(update_expr)
            .expression_attribute_names("#status", "status");

        update = update
            .expression_attribute_values(":status", AttributeValue::S(status_str.clone()))
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .expression_attribute_values(":now", AttributeValue::N(now_secs.to_string()))
            .expression_attribute_values(":status_created", AttributeValue::S(status_created));

        // Add version condition for optimistic locking
        if let Some(version) = expected_version {
            update = update
                .condition_expression("version = :expected_version")
                .expression_attribute_values(":expected_version", AttributeValue::N(version.to_string()));
        }

        match update.send().await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_update_execution_status_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_update_execution_status_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                let error_code = e.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                let error_message = e.message().map(|m| m.to_string()).unwrap_or_else(|| error_str.clone());

                // Check for conditional check failures (optimistic locking)
                if error_code == "ConditionalCheckFailedException" 
                    || error_str.contains("ConditionalCheckFailedException")
                    || error_str.contains("conditional")
                    || error_str.contains("condition") {
                    debug!(
                        execution_id = %execution_id,
                        expected_version = ?expected_version,
                        error_code = %error_code,
                        "Concurrent update detected"
                    );
                    metrics::counter!(
                        "plexspaces_workflow_ddb_update_execution_status_total",
                        "backend" => "dynamodb",
                        "result" => "concurrent_update"
                    )
                    .increment(1);
                    return Err(WorkflowError::ConcurrentUpdate(format!(
                        "Execution {} was modified concurrently (expected version: {:?})",
                        execution_id, expected_version
                    )));
                } else {
                    error!(error = %e, execution_id = %execution_id, "Failed to update execution status");
                    metrics::counter!(
                        "plexspaces_workflow_ddb_update_execution_status_errors_total",
                        "backend" => "dynamodb"
                    )
                    .increment(1);
                    Err(WorkflowError::Storage(format!("DynamoDB update_item failed: {}", e)))
                }
            }
        }
    }

    async fn update_execution_output(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        output: Value,
    ) -> Result<(), WorkflowError> {
        self.update_execution_output_with_version(ctx, execution_id, output, None).await
    }

    async fn update_execution_output_with_version(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        output: Value,
        expected_version: Option<u64>,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);

        let output_json = serde_json::to_string(&output)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let now_secs = Utc::now().timestamp();

        let mut update = self
            .client
            .update_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("EXEC".to_string()))
            .update_expression("SET output_json = :output, version = version + :one, updated_at = :now")
            .expression_attribute_values(":output", AttributeValue::S(output_json))
            .expression_attribute_values(":one", AttributeValue::N("1".to_string()))
            .expression_attribute_values(":now", AttributeValue::N(now_secs.to_string()));

        if let Some(version) = expected_version {
            update = update
                .condition_expression("version = :expected_version")
                .expression_attribute_values(":expected_version", AttributeValue::N(version.to_string()));
        }

        match update.send().await {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_update_execution_output_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_update_execution_output_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("ConditionalCheckFailedException") {
                    Err(WorkflowError::ConcurrentUpdate(format!(
                        "Execution {} version mismatch (concurrent update detected)",
                        execution_id
                    )))
                } else {
                    error!(error = %e, execution_id = %execution_id, "Failed to update execution output");
                    Err(WorkflowError::Storage(format!("DynamoDB update_item failed: {}", e)))
                }
            }
        }
    }

    async fn transfer_ownership(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        new_node_id: &str,
        expected_version: u64,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);
        let now_secs = Utc::now().timestamp();

        let mut expr_values = HashMap::new();
        expr_values.insert(":node_id".to_string(), AttributeValue::S(new_node_id.to_string()));
        expr_values.insert(":one".to_string(), AttributeValue::N("1".to_string()));
        expr_values.insert(":now".to_string(), AttributeValue::N(now_secs.to_string()));
        expr_values.insert(":expected_version".to_string(), AttributeValue::N(expected_version.to_string()));

        // Update node_heartbeat for GSI
        let node_heartbeat = format!("{}#{}", new_node_id, now_secs);
        expr_values.insert(":node_heartbeat".to_string(), AttributeValue::S(node_heartbeat));

        let mut update = self
            .client
            .update_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("EXEC".to_string()))
            .update_expression("SET node_id = :node_id, version = version + :one, last_heartbeat = :now, updated_at = :now, node_heartbeat = :node_heartbeat")
            .condition_expression("version = :expected_version");
        for (k, v) in expr_values {
            update = update.expression_attribute_values(k, v);
        }
        match update.send().await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_transfer_ownership_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_transfer_ownership_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                if error_str.contains("ConditionalCheckFailedException") {
                    metrics::counter!(
                        "plexspaces_workflow_ddb_transfer_ownership_total",
                        "backend" => "dynamodb",
                        "result" => "concurrent_update"
                    )
                    .increment(1);
                    Err(WorkflowError::ConcurrentUpdate(format!(
                        "Execution {} version mismatch (concurrent ownership transfer)",
                        execution_id
                    )))
                } else {
                    error!(error = %e, execution_id = %execution_id, "Failed to transfer ownership");
                    Err(WorkflowError::Storage(format!("DynamoDB update_item failed: {}", e)))
                }
            }
        }
    }

    async fn update_heartbeat(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        node_id: &str,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);
        let now_secs = Utc::now().timestamp();

        let mut expr_values = HashMap::new();
        expr_values.insert(":now".to_string(), AttributeValue::N(now_secs.to_string()));
        expr_values.insert(":node_id".to_string(), AttributeValue::S(node_id.to_string()));

        // Update node_heartbeat for GSI
        let node_heartbeat = format!("{}#{}", node_id, now_secs);
        expr_values.insert(":node_heartbeat".to_string(), AttributeValue::S(node_heartbeat));

        let mut update = self
            .client
            .update_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S("EXEC".to_string()))
            .update_expression("SET last_heartbeat = :now, updated_at = :now, node_heartbeat = :node_heartbeat")
            .condition_expression("attribute_exists(node_id) AND node_id = :node_id");
        for (k, v) in expr_values {
            update = update.expression_attribute_values(k, v);
        }
        match update.send().await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_update_heartbeat_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                Ok(())
            }
            Err(e) => {
                let error_str = e.to_string();
                let error_code = e.code().map(|c| c.to_string()).unwrap_or_else(|| "unknown".to_string());
                let error_message = e.message().map(|m| m.to_string()).unwrap_or_else(|| error_str.clone());

                // Check for conditional check failures
                if error_code == "ConditionalCheckFailedException" 
                    || error_str.contains("ConditionalCheckFailedException")
                    || error_str.contains("conditional")
                    || error_str.contains("condition") {
                    warn!(
                        execution_id = %execution_id,
                        node_id = %node_id,
                        error_code = %error_code,
                        "Heartbeat update failed: condition not met (node_id mismatch or missing)"
                    );
                    return Err(WorkflowError::Storage(format!(
                        "Failed to update heartbeat: condition not met (node_id: {}, error: {})",
                        node_id, error_message
                    )));
                }

                error!(
                    error = %e,
                    execution_id = %execution_id,
                    error_code = %error_code,
                    "Failed to update heartbeat"
                );
                Err(WorkflowError::Storage(format!(
                    "DynamoDB update_item failed: {} (code: {})",
                    error_message, error_code
                )))
            }
        }
    }

    async fn create_step_execution(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        step_id: &str,
        input: Value,
    ) -> Result<String, WorkflowError> {
        self.create_step_execution_with_attempt_internal(ctx, execution_id, step_id, input, 1).await
    }

    async fn get_step_execution(
        &self,
        ctx: &RequestContext,
        step_exec_id: &str,
    ) -> Result<StepExecution, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-step-executions", self.table_prefix);

        // We need to find the step execution by scanning or using a GSI
        // For now, we'll scan with a filter (not ideal, but works)
        // In production, consider adding a GSI on step_execution_id
        let mut last_evaluated_key = None;
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        loop {
            let mut scan = self.client.scan().table_name(&table_name);

            if let Some(lek) = last_evaluated_key {
                scan = scan.set_exclusive_start_key(Some(lek));
            }

            match scan.send().await {
                Ok(result) => {
                    for item in result.items() {
                        // Check tenant isolation
                        if let Some(pk_attr) = item.get("pk") {
                            if let Ok(pk) = pk_attr.as_s() {
                                if !pk.starts_with(&format!("{}#", tenant_namespace)) {
                                    continue;
                                }
                            }
                        }

                        // Check if this is the step execution we're looking for
                        if let Some(se_id_attr) = item.get("step_execution_id") {
                            if let Ok(se_id) = se_id_attr.as_s() {
                                if se_id == step_exec_id {
                                    let duration = start_time.elapsed();
                                    metrics::histogram!(
                                        "plexspaces_workflow_ddb_get_step_execution_duration_seconds",
                                        "backend" => "dynamodb"
                                    )
                                    .record(duration.as_secs_f64());
                                    metrics::counter!(
                                        "plexspaces_workflow_ddb_get_step_execution_total",
                                        "backend" => "dynamodb",
                                        "result" => "found"
                                    )
                                    .increment(1);
                                    return Self::item_to_step_execution(&item);
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
                    error!(error = %e, step_exec_id = %step_exec_id, "Failed to get step execution");
                    return Err(WorkflowError::Storage(format!("DynamoDB scan failed: {}", e)));
                }
            }
        }

        metrics::counter!(
            "plexspaces_workflow_ddb_get_step_execution_total",
            "backend" => "dynamodb",
            "result" => "not_found"
        )
        .increment(1);
        Err(WorkflowError::NotFound(format!(
            "Step execution {} not found",
            step_exec_id
        )))
    }

    async fn complete_step_execution(
        &self,
        ctx: &RequestContext,
        step_exec_id: &str,
        status: StepExecutionStatus,
        output: Option<Value>,
        error: Option<String>,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-step-executions", self.table_prefix);

        // First, find the step execution to get its pk/sk
        let step_exec = self.get_step_execution(ctx, step_exec_id).await?;
        let execution_id = &step_exec.execution_id;
        let pk = Self::composite_key(ctx, execution_id);
        let sk = format!("STEP#{}", step_exec_id);

        let status_str = status.to_string();
        let now_secs = Utc::now().timestamp();

        let mut expr_values = HashMap::new();
        expr_values.insert(":status".to_string(), AttributeValue::S(status_str));
        expr_values.insert(":now".to_string(), AttributeValue::N(now_secs.to_string()));

        let mut update_expr = "SET #status = :status, completed_at = :now".to_string();
        let mut expr_names = HashMap::new();
        expr_names.insert("#status".to_string(), "status".to_string());

        if let Some(output_val) = output {
            let output_json = serde_json::to_string(&output_val)
                .map_err(|e| WorkflowError::Serialization(e.to_string()))?;
            update_expr.push_str(", output_json = :output");
            expr_values.insert(":output".to_string(), AttributeValue::S(output_json));
        }

        if let Some(err) = error {
            update_expr.push_str(", error = :error");
            expr_values.insert(":error".to_string(), AttributeValue::S(err));
        }

        let mut update = self
            .client
            .update_item()
            .table_name(&table_name)
            .key("pk", AttributeValue::S(pk))
            .key("sk", AttributeValue::S(sk))
            .update_expression(update_expr);
        for (k, v) in expr_names {
            update = update.expression_attribute_names(k, v);
        }
        for (k, v) in expr_values {
            update = update.expression_attribute_values(k, v);
        }
        match update.send().await
        {
            Ok(_) => {
                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_complete_step_execution_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_complete_step_execution_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, step_exec_id = %step_exec_id, "Failed to complete step execution");
                Err(WorkflowError::Storage(format!("DynamoDB update_item failed: {}", e)))
            }
        }
    }

    async fn get_step_execution_history(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
    ) -> Result<Vec<StepExecution>, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-step-executions", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);

        let mut step_executions = Vec::new();
        let mut last_evaluated_key = None;

        loop {
            let mut query = self
                .client
                .query()
                .table_name(&table_name)
                .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
                .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
                .expression_attribute_values(":sk_prefix", AttributeValue::S("STEP#".to_string()));

            if let Some(lek) = last_evaluated_key {
                query = query.set_exclusive_start_key(Some(lek));
            }

            match query.send().await {
                Ok(result) => {
                    for item in result.items() {
                        match Self::item_to_step_execution(&item) {
                                Ok(step_exec) => step_executions.push(step_exec),
                                Err(e) => {
                                    warn!(error = %e, "Failed to parse step execution item, skipping");
                                }
                            }
                    }

                    last_evaluated_key = result.last_evaluated_key().cloned();
                    if last_evaluated_key.is_none() {
                        break;
                    }
                }
                Err(e) => {
                    error!(error = %e, execution_id = %execution_id, "Failed to query step executions");
                    return Err(WorkflowError::Storage(format!("DynamoDB query failed: {}", e)));
                }
            }
        }

        // Sort by started_at (we don't have that in the item, so sort by step_execution_id which is ULID)
        step_executions.sort_by(|a, b| a.step_execution_id.cmp(&b.step_execution_id));

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_workflow_ddb_get_step_execution_history_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_workflow_ddb_get_step_execution_history_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => step_executions.len().to_string()
        )
        .increment(1);

        Ok(step_executions)
    }

    async fn send_signal(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        signal_name: &str,
        payload: Value,
    ) -> Result<(), WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-signals", self.table_prefix);
        let signal_id = ulid::Ulid::new().to_string();
        let pk = Self::composite_key(ctx, execution_id);
        let received_at = Utc::now().timestamp();
        let sk = format!("SIGNAL#{}#{}", signal_name, received_at);

        let payload_json = serde_json::to_string(&payload)
            .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

        let mut item = HashMap::new();
        item.insert("pk".to_string(), AttributeValue::S(pk));
        item.insert("sk".to_string(), AttributeValue::S(sk));
        item.insert("signal_id".to_string(), AttributeValue::S(signal_id));
        item.insert("execution_id".to_string(), AttributeValue::S(execution_id.to_string()));
        item.insert("signal_name".to_string(), AttributeValue::S(signal_name.to_string()));
        item.insert("payload".to_string(), AttributeValue::S(payload_json));
        item.insert("received_at".to_string(), AttributeValue::N(received_at.to_string()));

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
                    "plexspaces_workflow_ddb_send_signal_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_send_signal_total",
                    "backend" => "dynamodb",
                    "result" => "success"
                )
                .increment(1);
                Ok(())
            }
            Err(e) => {
                error!(error = %e, execution_id = %execution_id, signal_name = %signal_name, "Failed to send signal");
                metrics::counter!(
                    "plexspaces_workflow_ddb_send_signal_errors_total",
                    "backend" => "dynamodb"
                )
                .increment(1);
                Err(WorkflowError::Storage(format!("DynamoDB put_item failed: {}", e)))
            }
        }
    }

    async fn check_signal(
        &self,
        ctx: &RequestContext,
        execution_id: &str,
        signal_name: &str,
    ) -> Result<Option<Value>, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-signals", self.table_prefix);
        let pk = Self::composite_key(ctx, execution_id);

        // Query for the first signal with this name (ordered by received_at)
        let mut query = self
            .client
            .query()
            .table_name(&table_name)
            .key_condition_expression("pk = :pk AND begins_with(sk, :sk_prefix)")
            .expression_attribute_values(":pk", AttributeValue::S(pk.clone()))
            .expression_attribute_values(":sk_prefix", AttributeValue::S(format!("SIGNAL#{}#", signal_name)))
            .limit(1)
            .scan_index_forward(true); // Ascending order (oldest first)

        match query.send().await {
            Ok(result) => {
                if let Some(item) = result.items().first() {
                    // Get payload
                    if let Some(payload_attr) = item.get("payload") {
                        if let Ok(payload_json) = payload_attr.as_s() {
                            let payload: Value = serde_json::from_str(payload_json)
                                .map_err(|e| WorkflowError::Serialization(e.to_string()))?;

                            // Delete the signal (consume it)
                            if let Some(sk_attr) = item.get("sk") {
                                if let Ok(sk) = sk_attr.as_s() {
                                    match self
                                        .client
                                        .delete_item()
                                        .table_name(&table_name)
                                        .key("pk", AttributeValue::S(pk))
                                        .key("sk", AttributeValue::S(sk.clone()))
                                        .send()
                                        .await
                                    {
                                        Ok(_) => {
                                            let duration = start_time.elapsed();
                                            metrics::histogram!(
                                                "plexspaces_workflow_ddb_check_signal_duration_seconds",
                                                "backend" => "dynamodb"
                                            )
                                            .record(duration.as_secs_f64());
                                            metrics::counter!(
                                                "plexspaces_workflow_ddb_check_signal_total",
                                                "backend" => "dynamodb",
                                                "result" => "found"
                                            )
                                            .increment(1);
                                            return Ok(Some(payload));
                                        }
                                        Err(e) => {
                                            warn!(error = %e, "Failed to delete signal after consumption");
                                            // Still return the payload even if delete failed
                                            return Ok(Some(payload));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                let duration = start_time.elapsed();
                metrics::histogram!(
                    "plexspaces_workflow_ddb_check_signal_duration_seconds",
                    "backend" => "dynamodb"
                )
                .record(duration.as_secs_f64());
                metrics::counter!(
                    "plexspaces_workflow_ddb_check_signal_total",
                    "backend" => "dynamodb",
                    "result" => "not_found"
                )
                .increment(1);
                Ok(None)
            }
            Err(e) => {
                error!(error = %e, execution_id = %execution_id, signal_name = %signal_name, "Failed to check signal");
                Err(WorkflowError::Storage(format!("DynamoDB query failed: {}", e)))
            }
        }
    }

    async fn list_executions_by_status(
        &self,
        ctx: &RequestContext,
        statuses: Vec<ExecutionStatus>,
        node_id: Option<&str>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let tenant_namespace = Self::tenant_namespace_key(ctx);

        let mut executions = Vec::new();
        let mut last_evaluated_key = None;

        // Query using status_index GSI
        for status in &statuses {
            let status_str = status.to_string();
            let status_prefix = format!("{}#", status_str);

            loop {
                let mut query = self
                    .client
                    .query()
                    .table_name(&table_name)
                    .index_name("status_index")
                    .key_condition_expression("tenant_namespace = :tn AND begins_with(status_created, :status_prefix)")
                    .expression_attribute_values(":tn", AttributeValue::S(tenant_namespace.clone()))
                    .expression_attribute_values(":status_prefix", AttributeValue::S(status_prefix.clone()));

                if let Some(lek) = last_evaluated_key {
                    query = query.set_exclusive_start_key(Some(lek));
                }

                match query.send().await {
                    Ok(result) => {
                        for item in result.items() {
                            // Filter by node_id if specified
                            if let Some(nid) = node_id {
                                if let Some(item_node_id_attr) = item.get("node_id") {
                                    if let Ok(item_node_id) = item_node_id_attr.as_s() {
                                        if item_node_id != nid {
                                            continue;
                                        }
                                    } else {
                                        continue; // Item has no node_id but we're filtering by it
                                    }
                                } else {
                                    continue; // Item has no node_id but we're filtering by it
                                }
                            }

                            match Self::item_to_execution(&item) {
                                Ok(exec) => executions.push(exec),
                                Err(e) => {
                                    warn!(error = %e, "Failed to parse execution item, skipping");
                                }
                            }
                        }

                        last_evaluated_key = result.last_evaluated_key().cloned();
                        if last_evaluated_key.is_none() {
                            break;
                        }
                    }
                    Err(e) => {
                        error!(error = %e, "Failed to query executions by status");
                        return Err(WorkflowError::Storage(format!("DynamoDB query failed: {}", e)));
                    }
                }
            }

            last_evaluated_key = None; // Reset for next status
        }

        // Sort by created_at (we can use execution_id which is ULID for approximate ordering)
        executions.sort_by(|a, b| a.execution_id.cmp(&b.execution_id));

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_workflow_ddb_list_executions_by_status_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_workflow_ddb_list_executions_by_status_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => executions.len().to_string()
        )
        .increment(1);

        Ok(executions)
    }

    async fn list_stale_executions(
        &self,
        ctx: &RequestContext,
        stale_threshold_seconds: u64,
        statuses: Vec<ExecutionStatus>,
    ) -> Result<Vec<WorkflowExecution>, WorkflowError> {
        let start_time = std::time::Instant::now();
        let table_name = format!("{}-executions", self.table_prefix);
        let tenant_namespace = Self::tenant_namespace_key(ctx);
        let now_secs = Utc::now().timestamp();
        let threshold_secs = now_secs - stale_threshold_seconds as i64;

        let mut executions = Vec::new();

        // Query using status_index GSI and filter by last_heartbeat
        for status in &statuses {
            let status_str = status.to_string();
            let status_prefix = format!("{}#", status_str);
            let mut last_evaluated_key = None;

            loop {
                let mut query = self
                    .client
                    .query()
                    .table_name(&table_name)
                    .index_name("status_index")
                    .key_condition_expression("tenant_namespace = :tn AND begins_with(status_created, :status_prefix)")
                    .expression_attribute_values(":tn", AttributeValue::S(tenant_namespace.clone()))
                    .expression_attribute_values(":status_prefix", AttributeValue::S(status_prefix.clone()));

                if let Some(lek) = last_evaluated_key {
                    query = query.set_exclusive_start_key(Some(lek));
                }

                match query.send().await {
                    Ok(result) => {
                        for item in result.items() {
                            // Check if stale (last_heartbeat or updated_at is older than threshold)
                            let is_stale = if let Some(heartbeat_attr) = item.get("last_heartbeat") {
                                if let Ok(heartbeat_str) = heartbeat_attr.as_n() {
                                    if let Ok(heartbeat_secs) = heartbeat_str.parse::<i64>() {
                                        heartbeat_secs < threshold_secs
                                    } else {
                                        // No heartbeat, check updated_at
                                        if let Some(updated_attr) = item.get("updated_at") {
                                            if let Ok(updated_str) = updated_attr.as_n() {
                                                if let Ok(updated_secs) = updated_str.parse::<i64>() {
                                                    updated_secs < threshold_secs
                                                } else {
                                                    false
                                                }
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    }
                                } else {
                                    // No heartbeat, check updated_at
                                    if let Some(updated_attr) = item.get("updated_at") {
                                        if let Ok(updated_str) = updated_attr.as_n() {
                                            if let Ok(updated_secs) = updated_str.parse::<i64>() {
                                                updated_secs < threshold_secs
                                            } else {
                                                false
                                            }
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                }
                            } else {
                                // No heartbeat, check updated_at
                                if let Some(updated_attr) = item.get("updated_at") {
                                    if let Ok(updated_str) = updated_attr.as_n() {
                                        if let Ok(updated_secs) = updated_str.parse::<i64>() {
                                            updated_secs < threshold_secs
                                        } else {
                                            false
                                        }
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            };

                            if is_stale {
                                match Self::item_to_execution(&item) {
                                    Ok(exec) => executions.push(exec),
                                    Err(e) => {
                                        warn!(error = %e, "Failed to parse execution item, skipping");
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
                        error!(error = %e, "Failed to query stale executions");
                        return Err(WorkflowError::Storage(format!("DynamoDB query failed: {}", e)));
                    }
                }
            }
        }

        // Sort by last_heartbeat or updated_at (oldest first)
        executions.sort_by(|a, b| {
            let a_time = a.last_heartbeat.map(|h| h.timestamp()).unwrap_or(0);
            let b_time = b.last_heartbeat.map(|h| h.timestamp()).unwrap_or(0);
            a_time.cmp(&b_time)
        });

        let duration = start_time.elapsed();
        metrics::histogram!(
            "plexspaces_workflow_ddb_list_stale_executions_duration_seconds",
            "backend" => "dynamodb"
        )
        .record(duration.as_secs_f64());
        metrics::counter!(
            "plexspaces_workflow_ddb_list_stale_executions_total",
            "backend" => "dynamodb",
            "result" => "success",
            "count" => executions.len().to_string()
        )
        .increment(1);

        Ok(executions)
    }
}

