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

//! Object Registry Helper Functions
//!
//! Provides convenient wrappers for common object-registry operations
//! to simplify registration and discovery of different object types.
//! Includes LRU caching for discovery operations to reduce registry load.

use crate::{actor_context::ObjectRegistry as ObjectRegistryTrait, RequestContext, RequestContextExt};
use plexspaces_proto::common::v1::Metadata;
use plexspaces_proto::node::v1::{OutboundTransport, ServiceLinkConfig};
use plexspaces_proto::object_registry::v1::{HealthStatus, ObjectRegistration, ObjectType};
use prost_types::Timestamp;
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::RwLock;

/// Simple LRU cache with TTL expiration for discovery results
struct DiscoveryCache<K, V> {
    capacity: usize,
    ttl: Duration,
    map: HashMap<K, (V, SystemTime)>,
    queue: VecDeque<K>,
}

impl<K, V> DiscoveryCache<K, V>
where
    K: Hash + Eq + Clone,
{
    fn new(capacity: usize, ttl: Duration) -> Self {
        Self {
            capacity,
            ttl,
            map: HashMap::with_capacity(capacity),
            queue: VecDeque::with_capacity(capacity),
        }
    }

    fn get(&mut self, key: &K) -> Option<&V> {
        let now = SystemTime::now();

        if !self.map.contains_key(key) {
            return None;
        }

        let expired = self
            .map
            .get(key)
            .map(|(_, timestamp)| now.duration_since(*timestamp).unwrap_or_default() >= self.ttl)
            .unwrap_or(true);

        if expired {
            self.remove(key);
            return None;
        }

        // Update LRU order
        if let Some(pos) = self.queue.iter().position(|k| k == key) {
            self.queue.remove(pos);
        }
        self.queue.push_back(key.clone());

        self.map.get(key).map(|(value, _)| value)
    }

    fn insert(&mut self, key: K, value: V) {
        let now = SystemTime::now();

        if let Some((old_value, timestamp)) = self.map.get_mut(&key) {
            *old_value = value;
            *timestamp = now;

            if let Some(pos) = self.queue.iter().position(|k| k == &key) {
                self.queue.remove(pos);
            }
            self.queue.push_back(key.clone());
            return;
        }

        if self.map.len() >= self.capacity {
            if let Some(lru_key) = self.queue.pop_front() {
                self.map.remove(&lru_key);
            }
        }

        self.queue.push_back(key.clone());
        self.map.insert(key, (value, now));
    }

    fn remove(&mut self, key: &K) -> Option<V> {
        if let Some((value, _)) = self.map.remove(key) {
            if let Some(pos) = self.queue.iter().position(|k| k == key) {
                self.queue.remove(pos);
            }
            Some(value)
        } else {
            None
        }
    }

    fn cleanup_expired(&mut self) {
        let now = SystemTime::now();
        let expired_keys: Vec<K> = self
            .map
            .iter()
            .filter(|(_, (_, timestamp))| {
                now.duration_since(*timestamp).unwrap_or_default() >= self.ttl
            })
            .map(|(key, _)| key.clone())
            .collect();

        for key in expired_keys {
            self.remove(&key);
        }
    }

    fn remove_matching<F>(&mut self, predicate: F) -> usize
    where
        F: Fn(&K) -> bool,
    {
        let matching_keys: Vec<K> = self
            .map
            .keys()
            .filter(|key| predicate(key))
            .cloned()
            .collect();

        let count = matching_keys.len();
        for key in matching_keys {
            self.remove(&key);
        }
        count
    }
}

/// Global discovery cache (shared across all discovery operations)
/// Cache key format: "{object_type}:{category}:{tenant_id}:{namespace}" (e.g., "node:cluster:tenant1:prod", "application:myapp:tenant1:default")
type CacheKey = String;
static DISCOVERY_CACHE: once_cell::sync::Lazy<
    Arc<RwLock<DiscoveryCache<CacheKey, Vec<ObjectRegistration>>>>,
> = once_cell::sync::Lazy::new(|| {
    Arc::new(RwLock::new(DiscoveryCache::new(
        1000,                    // capacity
        Duration::from_secs(60), // 60 second TTL
    )))
});

/// Clear the discovery cache (test-only helper)
/// This is useful for tests to ensure clean state between test runs
/// Note: This function is public for use in integration tests but should not be used in production code
pub async fn clear_discovery_cache() {
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.map.clear();
    cache.queue.clear();
}

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
pub async fn register_node<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
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
        object_id: node_id.to_string(),
        object_name: format!("Node {}", node_id),
        node_id: node_id.to_string(),
        grpc_address: grpc_address.to_string(),
        object_category: "Node".to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        health_status: HealthStatus::HealthStatusHealthy as i32,
        created_at: Some(timestamp.clone()),
        updated_at: Some(timestamp),
        labels: cluster_name
            .map(|c| vec![c.to_string()])
            .unwrap_or_default(),
        ..Default::default()
    };

    // Invalidate all node cache entries for this tenant/namespace to prevent stale data
    let tenant_ns_pattern = format!("node:{}:{}", ctx.tenant_id(), ctx.namespace());
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove_matching(|key| key.starts_with(&tenant_ns_pattern));

    object_registry.register(ctx, registration).await
}

/// Register a static outbound service link as [`ObjectType::ObjectTypeService`].
///
/// Call when `ServiceLinkConfig.publish_to_registry` is true at node startup. The primary URI is
/// `base_url` (HTTP or gRPC origin). Metadata labels `plexspaces.link_name` and
/// `plexspaces.transport` support discovery filters.
pub async fn register_outbound_service_link<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    link: &ServiceLinkConfig,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp = Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    };

    let transport = match link.transport {
        x if x == OutboundTransport::OutboundTransportGrpc as i32 => {
            OutboundTransport::OutboundTransportGrpc
        }
        x if x == OutboundTransport::OutboundTransportChannel as i32 => {
            OutboundTransport::OutboundTransportChannel
        }
        x if x == OutboundTransport::OutboundTransportHttp as i32 => {
            OutboundTransport::OutboundTransportHttp
        }
        _ => OutboundTransport::OutboundTransportUnspecified,
    };
    let cap = match transport {
        OutboundTransport::OutboundTransportGrpc => "grpc",
        OutboundTransport::OutboundTransportChannel => "channel",
        OutboundTransport::OutboundTransportHttp
        | OutboundTransport::OutboundTransportUnspecified => "http",
    };
    let mut labels = HashMap::new();
    labels.insert("plexspaces.link_name".to_string(), link.name.clone());
    labels.insert("plexspaces.transport".to_string(), cap.to_string());

    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeService as i32,
        object_id: format!("service-link:{}@{}", link.name, node_id),
        object_name: link.name.clone(),
        version: "1".to_string(),
        node_id: node_id.to_string(),
        grpc_address: link.base_url.clone(),
        object_category: "outbound-service-link".to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        capabilities: vec![cap.to_string()],
        health_status: HealthStatus::HealthStatusHealthy as i32,
        created_at: Some(timestamp.clone()),
        updated_at: Some(timestamp),
        metadata: Some(Metadata {
            labels,
            ..Default::default()
        }),
        ..Default::default()
    };

    let tenant_ns = format!("{}:{}", ctx.tenant_id(), ctx.namespace());
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove_matching(|key| key.starts_with("service_link:") && key.contains(&tenant_ns));

    object_registry.register(ctx, registration).await
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
pub async fn register_application<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
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

    // Invalidate all application cache entries for this tenant/namespace to prevent stale data
    // Cache key format: "application:{app_name}:{tenant_id}:{namespace}"
    let tenant_ns_suffix = format!(":{}:{}", ctx.tenant_id(), ctx.namespace());
    let mut cache = DISCOVERY_CACHE.write().await;
    cache
        .remove_matching(|key| key.starts_with("application:") && key.ends_with(&tenant_ns_suffix));

    object_registry.register(ctx, registration).await
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
pub async fn unregister_application<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    app_name: &str,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Invalidate all application cache entries for this tenant/namespace to prevent stale data
    // Cache key format: "application:{app_name}:{tenant_id}:{namespace}"
    let tenant_ns_suffix = format!(":{}:{}", ctx.tenant_id(), ctx.namespace());
    let mut cache = DISCOVERY_CACHE.write().await;
    cache
        .remove_matching(|key| key.starts_with("application:") && key.ends_with(&tenant_ns_suffix));

    object_registry
        .unregister(
            ctx,
            ObjectType::ObjectTypeApplication,
            &format!("{app_name}@{node_id}"),
        )
        .await
        .map_err(|e| {
            Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                e.to_string(),
            )) as Box<dyn std::error::Error + Send + Sync>
        })
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
pub async fn register_workflow<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
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

    // Invalidate all workflow cache entries for this tenant/namespace to prevent stale data
    // Cache key format: "workflow:{definition_id}:{tenant_id}:{namespace}"
    let tenant_ns_suffix = format!(":{}:{}", ctx.tenant_id(), ctx.namespace());
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove_matching(|key| key.starts_with("workflow:") && key.ends_with(&tenant_ns_suffix));

    object_registry.register(ctx, registration).await
}

/// Discover nodes across all nodes
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
///
/// ## Returns
/// Vector of ObjectRegistration for all nodes
///
/// ## Caching
/// Results are cached for 60 seconds to reduce registry load.
pub async fn discover_nodes<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    // Tenant-aware cache key to ensure proper isolation
    let cache_key = format!("node:{}:{}", ctx.tenant_id(), ctx.namespace());

    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    // Cache miss - query registry
    let registrations = object_registry
        .discover(
            ctx,
            Some(ObjectType::ObjectTypeNode),
            None, // object_category
            None, // capabilities
            None, // labels
            None, // health_status
            0,    // offset
            1000, // limit
        )
        .await?;

    // Store in cache
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.insert(cache_key, registrations.clone());
    }

    Ok(registrations)
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
///
/// ## Caching
/// Results are cached for 60 seconds to reduce registry load.
pub async fn discover_application_nodes<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    app_name: &str,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    // Tenant-aware cache key to ensure proper isolation
    let cache_key = format!(
        "application:{}:{}:{}",
        app_name,
        ctx.tenant_id(),
        ctx.namespace()
    );

    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    // Cache miss - query registry
    let registrations = object_registry
        .discover(
            ctx,
            Some(ObjectType::ObjectTypeApplication),
            Some(app_name.to_string()),
            None, // capabilities
            None, // labels
            None, // health_status
            0,    // offset
            1000, // limit
        )
        .await?;

    // Store in cache
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.insert(cache_key, registrations.clone());
    }

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
///
/// ## Caching
/// Results are cached for 60 seconds to reduce registry load.
pub async fn discover_workflow_nodes<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    definition_id: &str,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    // Tenant-aware cache key to ensure proper isolation
    let cache_key = format!(
        "workflow:{}:{}:{}",
        definition_id,
        ctx.tenant_id(),
        ctx.namespace()
    );

    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    // Cache miss - query registry
    let registrations = object_registry
        .discover(
            ctx,
            Some(ObjectType::ObjectTypeWorkflow),
            Some(definition_id.to_string()),
            None, // capabilities
            None, // labels
            None, // health_status
            0,    // offset
            1000, // limit
        )
        .await?;

    // Store in cache
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.insert(cache_key, registrations.clone());
    }

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
    _object_registry: &Arc<dyn ObjectRegistryTrait>,
    _ctx: &RequestContext,
    _node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Heartbeat is not in the trait - this would need to be added
    // For now, return Ok(())
    // Note: Heartbeat updates don't invalidate cache as they don't change discovery results
    Ok(())
}

// Tests are in crates/core/tests/object_registry_helpers_integration_tests.rs
