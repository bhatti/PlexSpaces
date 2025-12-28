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

//! Object Registry Helper Functions
//!
//! Provides convenient wrappers for common object-registry operations
//! to simplify registration and discovery of different object types.

use plexspaces_core::RequestContext;
use plexspaces_proto::object_registry::v1::{
    ObjectRegistration, ObjectType, HealthStatus,
};
use prost_types::Timestamp;
use std::sync::Arc;
use std::time::SystemTime;

/// Register a node in object-registry
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `node_id` - Node identifier
/// * `grpc_address` - Node's gRPC address (e.g., "http://127.0.0.1:8000")
/// * `cluster_name` - Optional cluster name
///
/// ## Returns
/// Result indicating success or failure
pub async fn register_node(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    node_id: &str,
    grpc_address: &str,
    cluster_name: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp = Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    };
    
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeNode as i32,
        object_id: node_id.to_string(), // Use node_id directly, no "_node@" prefix needed
        object_name: format!("Node {}", node_id),
        node_id: node_id.to_string(),
        grpc_address: grpc_address.to_string(),
        object_category: "Node".to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        health_status: HealthStatus::HealthStatusHealthy as i32,
        created_at: Some(timestamp.clone()),
        updated_at: Some(timestamp),
        labels: cluster_name.map(|c| vec![c.to_string()]).unwrap_or_default(),
        ..Default::default()
    };
    
    object_registry.register_trait(ctx, registration).await
}

/// Register an application in object-registry
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `app_name` - Application name
/// * `version` - Application version
/// * `node_id` - Node where application is deployed
/// * `grpc_address` - Node's gRPC address
///
/// ## Returns
/// Result indicating success or failure
pub async fn register_application(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    app_name: &str,
    version: &str,
    node_id: &str,
    grpc_address: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp = Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    };
    
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeApplication as i32,
        object_id: format!("{}@{}", app_name, node_id),
        object_name: app_name.to_string(),
        version: version.to_string(),
        node_id: node_id.to_string(),
        grpc_address: grpc_address.to_string(),
        object_category: app_name.to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        health_status: HealthStatus::HealthStatusHealthy as i32,
        created_at: Some(timestamp.clone()),
        updated_at: Some(timestamp),
        ..Default::default()
    };
    
    object_registry.register_trait(ctx, registration).await
}

/// Unregister an application from object-registry
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `app_name` - Application name
/// * `node_id` - Node where application is deployed
///
/// ## Returns
/// Result indicating success or failure
pub async fn unregister_application(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    app_name: &str,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let object_id = format!("{}@{}", app_name, node_id);
    object_registry.unregister(ctx, ObjectType::ObjectTypeApplication, &object_id).await
        .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
}

/// Register a workflow in object-registry
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `workflow_id` - Workflow execution ID
/// * `definition_id` - Workflow definition ID
/// * `node_id` - Node where workflow is running
/// * `grpc_address` - Node's gRPC address
///
/// ## Returns
/// Result indicating success or failure
pub async fn register_workflow(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    workflow_id: &str,
    definition_id: &str,
    node_id: &str,
    grpc_address: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp = Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    };
    
    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeWorkflow as i32,
        object_id: workflow_id.to_string(),
        object_name: format!("Workflow {}", workflow_id),
        node_id: node_id.to_string(),
        grpc_address: grpc_address.to_string(),
        object_category: definition_id.to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        health_status: HealthStatus::HealthStatusHealthy as i32,
        created_at: Some(timestamp.clone()),
        updated_at: Some(timestamp),
        ..Default::default()
    };
    
    object_registry.register_trait(ctx, registration).await
}

/// Discover applications by name across all nodes
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `app_name` - Application name to search for
///
/// ## Returns
/// Vector of ObjectRegistration for all nodes that have this application
pub async fn discover_application_nodes(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    app_name: &str,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    let registrations = object_registry.discover(
        ctx,
        Some(ObjectType::ObjectTypeApplication),
        Some(app_name.to_string()),
        None, // capabilities
        None, // labels
        None, // health_status
        0, // offset
        1000, // limit
    ).await
    .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)?;
    
    Ok(registrations)
}

/// Discover workflows by definition ID across all nodes
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `definition_id` - Workflow definition ID to search for
///
/// ## Returns
/// Vector of ObjectRegistration for all nodes that have workflows with this definition
pub async fn discover_workflow_nodes(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    definition_id: &str,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    let registrations = object_registry.discover(
        ctx,
        Some(ObjectType::ObjectTypeWorkflow),
        Some(definition_id.to_string()),
        None, // capabilities
        None, // labels
        None, // health_status
        0, // offset
        1000, // limit
    ).await
    .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)?;
    
    Ok(registrations)
}

/// Send heartbeat for a node
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `node_id` - Node identifier
///
/// ## Returns
/// Result indicating success or failure
pub async fn heartbeat_node(
    object_registry: &plexspaces_object_registry::ObjectRegistry,
    ctx: &RequestContext,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Use the direct heartbeat method from ObjectRegistry
    object_registry.heartbeat(
        ctx,
        ObjectType::ObjectTypeNode,
        node_id,
    ).await
    .map_err(|e| Box::new(std::io::Error::new(std::io::ErrorKind::Other, e.to_string())) as Box<dyn std::error::Error + Send + Sync>)
}

