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
//! Includes LRU caching for discovery operations to reduce registry load.

use crate::{RequestContext, actor_context::ObjectRegistry as ObjectRegistryTrait};
use plexspaces_proto::object_registry::v1::{
    ObjectRegistration, ObjectType, HealthStatus,
};
use prost_types::Timestamp;
use std::sync::Arc;
use std::time::{SystemTime, Duration};
use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
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
        
        let expired = self.map.get(key)
            .map(|(_, timestamp)| {
                now.duration_since(*timestamp).unwrap_or_default() >= self.ttl
            })
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
}

/// Global discovery cache (shared across all discovery operations)
/// Cache key format: "{object_type}:{category}" (e.g., "node:", "application:myapp", "workflow:def1")
type CacheKey = String;
static DISCOVERY_CACHE: once_cell::sync::Lazy<Arc<RwLock<DiscoveryCache<CacheKey, Vec<ObjectRegistration>>>>> =
    once_cell::sync::Lazy::new(|| {
        Arc::new(RwLock::new(DiscoveryCache::new(
            1000, // capacity
            Duration::from_secs(60), // 60 second TTL
        )))
    });

/// Register a node in object-registry
///
/// ## Arguments
/// * `object_registry` - ObjectRegistry instance
/// * `ctx` - RequestContext for tenant isolation
/// * `node_id` - Node identifier
/// * `grpc_address` - Node's gRPC address (e.g., "http://127.0.0.1:9001")
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
        labels: cluster_name.map(|c| vec![c.to_string()]).unwrap_or_default(),
        ..Default::default()
    };
    
    // Invalidate cache for nodes
    let cache_key = format!("node:");
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove(&cache_key);
    
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
    
    // Invalidate cache for this application
    let cache_key = format!("application:{}", app_name);
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove(&cache_key);
    
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
pub async fn unregister_application(
    object_registry: &Arc<dyn ObjectRegistryTrait>,
    ctx: &RequestContext,
    app_name: &str,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let object_id = format!("{}@{}", app_name, node_id);
    // Use lookup_full to get the registration, then we'd need to call unregister
    // For now, this is a placeholder - unregister would need to be added to the trait
    // Invalidate cache
    let cache_key = format!("application:{}", app_name);
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove(&cache_key);
    
    Err(Box::new(std::io::Error::new(std::io::ErrorKind::Unsupported, "Unregister not yet implemented via trait")))
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
    
    // Invalidate cache for this workflow definition
    let cache_key = format!("workflow:{}", definition_id);
    let mut cache = DISCOVERY_CACHE.write().await;
    cache.remove(&cache_key);
    
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
    let cache_key = "node:".to_string();
    
    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    
    // Cache miss - query registry
    let registrations = object_registry.discover(
        ctx,
        Some(ObjectType::ObjectTypeNode),
        None, // object_category
        None, // capabilities
        None, // labels
        None, // health_status
        0, // offset
        1000, // limit
    ).await?;
    
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
    let cache_key = format!("application:{}", app_name);
    
    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    
    // Cache miss - query registry
    let registrations = object_registry.discover(
        ctx,
        Some(ObjectType::ObjectTypeApplication),
        Some(app_name.to_string()),
        None, // capabilities
        None, // labels
        None, // health_status
        0, // offset
        1000, // limit
    ).await?;
    
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
    let cache_key = format!("workflow:{}", definition_id);
    
    // Check cache first
    {
        let mut cache = DISCOVERY_CACHE.write().await;
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }
    
    // Cache miss - query registry
    let registrations = object_registry.discover(
        ctx,
        Some(ObjectType::ObjectTypeWorkflow),
        Some(definition_id.to_string()),
        None, // capabilities
        None, // labels
        None, // health_status
        0, // offset
        1000, // limit
    ).await?;
    
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
    object_registry: &Arc<dyn ObjectRegistryTrait>,
    ctx: &RequestContext,
    node_id: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Heartbeat is not in the trait - this would need to be added
    // For now, return Ok(())
    // Note: Heartbeat updates don't invalidate cache as they don't change discovery results
    Ok(())
}

// Tests are in crates/core/tests/object_registry_helpers_integration_tests.rs
