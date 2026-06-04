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

use crate::{
    actor_context::{ObjectRegistry as ObjectRegistryTrait, RegisterResult},
    RequestContext, RequestContextExt,
};
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
type DiscoveryCacheStore = Arc<RwLock<DiscoveryCache<CacheKey, Vec<ObjectRegistration>>>>;
static DISCOVERY_CACHE: once_cell::sync::Lazy<DiscoveryCacheStore> =
    once_cell::sync::Lazy::new(|| {
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
        created_at: Some(timestamp),
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
        created_at: Some(timestamp),
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
        created_at: Some(timestamp),
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
            Box::new(std::io::Error::other(e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
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
        created_at: Some(timestamp),
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
            crate::DiscoverOptions {
                object_type: Some(ObjectType::ObjectTypeNode),
                limit: 1000,
                ..Default::default()
            },
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
            crate::DiscoverOptions {
                object_type: Some(ObjectType::ObjectTypeApplication),
                object_category: Some(app_name.to_string()),
                limit: 1000,
                ..Default::default()
            },
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
            crate::DiscoverOptions {
                object_type: Some(ObjectType::ObjectTypeWorkflow),
                object_category: Some(definition_id.to_string()),
                limit: 1000,
                ..Default::default()
            },
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

/// Build the canonical alias key for an actor identity.
///
/// Format: `"{actor_type}:{name}:{namespace}:{tenant_id}"`
///
/// This key is used as the unique placement identifier in the object registry
/// (Orleans grain directory pattern). Two actors with the same identity share
/// this alias, so only one can be HEALTHY/DEGRADED at a time when
/// `enforce_unique_placement=true`.
pub fn build_actor_alias(
    actor_type: &str,
    name: &str,
    namespace: &str,
    tenant_id: &str,
) -> String {
    format!("{}:{}:{}:{}", actor_type, name, namespace, tenant_id)
}

/// Parameters for [`register_actor`].
pub struct RegisterActorParams<'a> {
    /// Actor identifier string (e.g. `"name@type@ns@node"`)
    pub actor_id: &'a str,
    /// Actor type slug (e.g. `"counter"`)
    pub actor_type: &'a str,
    /// Actor instance name
    pub actor_name: &'a str,
    /// Node where the actor is running
    pub node_id: &'a str,
    /// Node's gRPC address
    pub grpc_address: &'a str,
    /// If `true`, reject spawn when another active instance with the same identity alias already exists.
    pub enforce_unique: bool,
}

/// Register an actor in the object registry.
///
/// ## Returns
/// `RegisterResult::Registered` on success or
/// `RegisterResult::AlreadyExists { grpc_address, object_id }` when another
/// live instance holds the alias (only when `enforce_unique=true`).
pub async fn register_actor<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    params: RegisterActorParams<'_>,
) -> Result<RegisterResult, Box<dyn std::error::Error + Send + Sync>> {
    let RegisterActorParams {
        actor_id,
        actor_type,
        actor_name,
        node_id,
        grpc_address,
        enforce_unique,
    } = params;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let timestamp = Timestamp {
        seconds: now.as_secs() as i64,
        nanos: now.subsec_nanos() as i32,
    };

    let alias = build_actor_alias(actor_type, actor_name, ctx.namespace(), ctx.tenant_id());

    let registration = ObjectRegistration {
        object_type: ObjectType::ObjectTypeActor as i32,
        object_id: actor_id.to_string(),
        object_name: format!("{}/{}", actor_type, actor_name),
        object_category: actor_type.to_string(),
        node_id: node_id.to_string(),
        grpc_address: grpc_address.to_string(),
        tenant_id: ctx.tenant_id().to_string(),
        namespace: ctx.namespace().to_string(),
        health_status: HealthStatus::HealthStatusHealthy as i32,
        alias,
        max_heartbeat_failures: 3,
        created_at: Some(timestamp),
        updated_at: Some(timestamp),
        ..Default::default()
    };

    // Invalidate actor cache entries for this tenant/namespace.
    let tenant_ns_suffix = format!(":{}:{}", ctx.tenant_id(), ctx.namespace());
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.remove_matching(|key| {
            key.starts_with("actor:") && key.ends_with(&tenant_ns_suffix)
        });
    }

    object_registry
        .register_with_unique_alias(ctx, registration, enforce_unique)
        .await
}

/// Unregister an actor from the object registry.
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `actor_id` - Actor identifier string
pub async fn unregister_actor<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    actor_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Invalidate actor cache entries for this tenant/namespace.
    let tenant_ns_suffix = format!(":{}:{}", ctx.tenant_id(), ctx.namespace());
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.remove_matching(|key| {
            key.starts_with("actor:") && key.ends_with(&tenant_ns_suffix)
        });
    }

    object_registry
        .unregister(ctx, ObjectType::ObjectTypeActor, actor_id)
        .await
        .map_err(|e| {
            Box::new(std::io::Error::other(e.to_string()))
                as Box<dyn std::error::Error + Send + Sync>
        })
}

/// Discover actors by type (object_category) within a tenant.
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `actor_type` - Actor type slug to filter on (e.g. `"Counter"`)
///
/// ## Returns
/// All HEALTHY + DEGRADED + STARTING actor registrations matching the type.
///
/// ## Caching
/// Results are cached for 60 seconds under a key of the form
/// `"actor:{actor_type}:{tenant_id}:{namespace}"`.
/// Cache is invalidated by `register_actor` and `unregister_actor`.
pub async fn discover_actors_by_type<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    actor_type: &str,
) -> Result<Vec<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    let cache_key = format!(
        "actor:{}:{}:{}",
        actor_type,
        ctx.tenant_id(),
        ctx.namespace()
    );

    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    let registrations = object_registry
        .discover(
            ctx,
            crate::DiscoverOptions {
                object_type: Some(ObjectType::ObjectTypeActor),
                object_category: Some(actor_type.to_string()),
                limit: 10_000,
                ..Default::default()
            },
        )
        .await?;

    {
        let mut cache = DISCOVERY_CACHE.write().await;
        cache.insert(cache_key, registrations.clone());
    }

    Ok(registrations)
}

/// Look up a single actor by its identity (actor_type + name), using the alias index.
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `actor_type` - Actor type slug (e.g. `"Counter"`)
/// * `actor_name` - Actor instance name (e.g. `"worker-1"`)
///
/// ## Returns
/// `Ok(Some(registration))` if an active (HEALTHY/DEGRADED/STARTING) instance is found,
/// `Ok(None)` if no registration holds this identity alias.
///
/// ## Note
/// The alias cache inside `ObjectRegistryImpl` (30 s TTL) short-circuits repeated
/// lookups for the same identity without going to the DB.
pub async fn lookup_actor_by_identity<T: ObjectRegistryTrait + ?Sized>(
    object_registry: &Arc<T>,
    ctx: &RequestContext,
    actor_type: &str,
    actor_name: &str,
) -> Result<Option<ObjectRegistration>, Box<dyn std::error::Error + Send + Sync>> {
    let alias = build_actor_alias(actor_type, actor_name, ctx.namespace(), ctx.tenant_id());
    object_registry.lookup_by_alias(ctx, &alias).await
}

// Integration tests: crates/core/tests/object_registry_helpers_integration_tests.rs
// Unit tests below cover the new Phase-8 helpers.

#[cfg(test)]
mod tests {
    use super::*;
    use plexspaces_object_registry::{ObjectRegistryImpl, SqliteObjectRegistryRepository};

    async fn make_registry() -> Arc<ObjectRegistryImpl> {
        let repo = Arc::new(
            SqliteObjectRegistryRepository::new(":memory:")
                .await
                .unwrap(),
        );
        Arc::new(ObjectRegistryImpl::new(repo))
    }

    fn ctx(tenant: &str, ns: &str) -> RequestContext {
        RequestContext::new_without_auth(tenant.into(), ns.into())
    }

    #[tokio::test]
    async fn test_discover_actors_by_type_returns_registered() {
        clear_discovery_cache().await;
        let reg = make_registry().await;
        let ctx = ctx("t1", "ns1");

        register_actor(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            RegisterActorParams {
                actor_id: "actor-1@node1",
                actor_type: "Counter",
                actor_name: "worker-1",
                node_id: "node1",
                grpc_address: "http://node1:8000",
                enforce_unique: false,
            },
        )
        .await
        .unwrap();

        let actors = discover_actors_by_type(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
        )
        .await
        .unwrap();

        assert_eq!(actors.len(), 1);
        assert_eq!(actors[0].object_id, "actor-1@node1");
    }

    #[tokio::test]
    async fn test_discover_actors_by_type_cache_hit() {
        clear_discovery_cache().await;
        let reg = make_registry().await;
        let ctx = ctx("t2", "ns2");

        register_actor(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            RegisterActorParams {
                actor_id: "actor-2@node1",
                actor_type: "Counter",
                actor_name: "worker-2",
                node_id: "node1",
                grpc_address: "http://node1:8000",
                enforce_unique: false,
            },
        )
        .await
        .unwrap();

        // First call populates cache.
        let first = discover_actors_by_type(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
        )
        .await
        .unwrap();
        assert_eq!(first.len(), 1);

        // Second call uses cache (same result, no DB).
        let second = discover_actors_by_type(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
        )
        .await
        .unwrap();
        assert_eq!(second.len(), 1);
        assert_eq!(first[0].object_id, second[0].object_id);
    }

    #[tokio::test]
    async fn test_lookup_actor_by_identity_found() {
        clear_discovery_cache().await;
        let reg = make_registry().await;
        let ctx = ctx("t3", "ns3");

        register_actor(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            RegisterActorParams {
                actor_id: "actor-3@node1",
                actor_type: "Counter",
                actor_name: "worker-3",
                node_id: "node1",
                grpc_address: "http://node1:8000",
                enforce_unique: false,
            },
        )
        .await
        .unwrap();

        let found = lookup_actor_by_identity(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
            "worker-3",
        )
        .await
        .unwrap();

        assert!(found.is_some());
        assert_eq!(found.unwrap().object_id, "actor-3@node1");
    }

    #[tokio::test]
    async fn test_lookup_actor_by_identity_not_found() {
        clear_discovery_cache().await;
        let reg = make_registry().await;
        let ctx = ctx("t4", "ns4");

        let found = lookup_actor_by_identity(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
            "nonexistent",
        )
        .await
        .unwrap();

        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_discover_actors_cache_invalidated_on_unregister() {
        clear_discovery_cache().await;
        let reg = make_registry().await;
        let ctx = ctx("t5", "ns5");

        register_actor(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            RegisterActorParams {
                actor_id: "actor-5@node1",
                actor_type: "Counter",
                actor_name: "worker-5",
                node_id: "node1",
                grpc_address: "http://node1:8000",
                enforce_unique: false,
            },
        )
        .await
        .unwrap();

        // Populate cache.
        let before = discover_actors_by_type(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
        )
        .await
        .unwrap();
        assert_eq!(before.len(), 1);

        // Unregister invalidates the actor discovery cache.
        unregister_actor(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "actor-5@node1",
        )
        .await
        .unwrap();

        // Next discover fetches from DB → empty.
        let after = discover_actors_by_type(
            &(reg.clone() as Arc<dyn ObjectRegistryTrait>),
            &ctx,
            "Counter",
        )
        .await
        .unwrap();
        assert!(after.is_empty());
    }
}
