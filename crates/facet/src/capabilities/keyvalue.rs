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

//! Key-Value Store Capability Facet
//!
//! Provides key-value storage capabilities to actors as a runtime-attachable facet.
//! This replaces the need for a separate capability provider system.

#[cfg(feature = "ddb-backend")]
#[path = "keyvalue_ddb.rs"]
pub mod keyvalue_ddb;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{Facet, FacetError, InterceptResult};

#[cfg(feature = "ddb-backend")]
pub use keyvalue_ddb::DynamoDBStore;

/// Key-value store facet
pub struct KeyValueFacet {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    /// Facet priority (immutable)
    priority: i32,
    /// Store implementation
    store: Arc<RwLock<Box<dyn KeyValueStore>>>,
    /// Configuration (parsed from config_value)
    config: KeyValueConfig,
    /// Metrics
    metrics: Arc<RwLock<KeyValueMetrics>>,
    /// Tenant ID from API request (stored when facet is attached to actor, empty if not set)
    tenant_id: std::sync::Mutex<String>,
    /// Namespace from API request (stored when facet is attached to actor, empty if not set)
    namespace: std::sync::Mutex<String>,
}

/// Default priority for KeyValueFacet
pub const KEYVALUE_FACET_DEFAULT_PRIORITY: i32 = 30;

/// Configuration for key-value store facet
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeyValueConfig {
    /// Store type (memory, redis, dynamodb, etc.)
    pub store_type: String,
    /// Connection string (if applicable)
    pub connection_string: Option<String>,
    /// Default TTL for keys
    pub default_ttl: Option<u64>,
    /// Enable caching
    pub enable_cache: bool,
    /// Cache size
    pub cache_size: usize,
}

impl Default for KeyValueConfig {
    fn default() -> Self {
        KeyValueConfig {
            store_type: "memory".to_string(),
            connection_string: None,
            default_ttl: None,
            enable_cache: true,
            cache_size: 1000,
        }
    }
}

#[derive(Default)]
struct KeyValueMetrics {
    gets: u64,
    sets: u64,
    deletes: u64,
    hits: u64,
    misses: u64,
}

// Re-export the unified KeyValueStore trait from common crate.
// This ensures a single trait definition across core, facet, and keyvalue crates.
pub use plexspaces_common::{KeyValueStore, KeyValueStoreError, KeyValueStoreResult};

/// In-memory key-value store (used as default backend for KeyValueFacet)
struct MemoryStore {
    data: Arc<RwLock<HashMap<String, Vec<u8>>>>,
}

#[async_trait]
impl KeyValueStore for MemoryStore {
    async fn get(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
    ) -> KeyValueStoreResult<Option<Vec<u8>>> {
        Ok(self.data.read().await.get(key).cloned())
    }

    async fn put(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
        value: Vec<u8>,
    ) -> KeyValueStoreResult<()> {
        self.data.write().await.insert(key.to_string(), value);
        Ok(())
    }

    async fn put_with_ttl(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
        value: Vec<u8>,
        _ttl: std::time::Duration,
    ) -> KeyValueStoreResult<()> {
        // In-memory store ignores TTL (no background expiry thread)
        self.data.write().await.insert(key.to_string(), value);
        Ok(())
    }

    async fn delete(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
    ) -> KeyValueStoreResult<()> {
        self.data.write().await.remove(key);
        Ok(())
    }

    async fn exists(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
    ) -> KeyValueStoreResult<bool> {
        Ok(self.data.read().await.contains_key(key))
    }

    async fn list_keys(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        prefix: &str,
    ) -> KeyValueStoreResult<Vec<String>> {
        Ok(self
            .data
            .read()
            .await
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    async fn cas(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
        expected: Option<Vec<u8>>,
        new_value: Vec<u8>,
    ) -> KeyValueStoreResult<bool> {
        let mut data = self.data.write().await;
        let current = data.get(key).cloned();
        if current == expected {
            data.insert(key.to_string(), new_value);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    async fn increment(
        &self,
        _ctx: &plexspaces_common::RequestContext,
        key: &str,
        delta: i64,
    ) -> KeyValueStoreResult<i64> {
        let mut data = self.data.write().await;
        let current: i64 = data
            .get(key)
            .and_then(|v| String::from_utf8(v.clone()).ok())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let new_val = current + delta;
        data.insert(key.to_string(), new_val.to_string().into_bytes());
        Ok(new_val)
    }
}

impl Default for KeyValueFacet {
    fn default() -> Self {
        Self::new(serde_json::json!({}), KEYVALUE_FACET_DEFAULT_PRIORITY)
    }
}

impl KeyValueFacet {
    /// Create a new key-value facet with default in-memory store
    pub fn new(config: Value, priority: i32) -> Self {
        let config_clone = config.clone();
        let kv_config = serde_json::from_value::<KeyValueConfig>(config_clone)
            .unwrap_or_else(|_| KeyValueConfig::default());
        Self::with_config_internal(kv_config, config, priority).unwrap_or_else(|_| {
            // Fallback to default if config parsing fails
            KeyValueFacet {
                config_value: serde_json::json!({}),
                priority,
                store: Arc::new(RwLock::new(Box::new(MemoryStore {
                    data: Arc::new(RwLock::new(HashMap::new())),
                }))),
                config: KeyValueConfig::default(),
                metrics: Arc::new(RwLock::new(KeyValueMetrics::default())),
                tenant_id: std::sync::Mutex::new(String::new()),
                namespace: std::sync::Mutex::new(String::new()),
            }
        })
    }

    /// Internal helper to create with specific configuration
    fn with_config_internal(
        config: KeyValueConfig,
        config_value: Value,
        priority: i32,
    ) -> Result<Self, FacetError> {
        let store: Box<dyn KeyValueStore> = match config.store_type.as_str() {
            "memory" => Box::new(MemoryStore {
                data: Arc::new(RwLock::new(HashMap::new())),
            }),
            #[cfg(feature = "ddb-backend")]
            "dynamodb" => {
                // DynamoDB store requires async initialization
                // For now, we'll create it synchronously with default config
                // In production, this should be initialized properly
                return Err(FacetError::InvalidConfig(
                    "DynamoDB store requires async initialization. Use DynamoDBStore::new() directly.".to_string()
                ));
            }
            _ => {
                return Err(FacetError::InvalidConfig(format!(
                    "Unknown store type: {}",
                    config.store_type
                )))
            }
        };

        Ok(KeyValueFacet {
            config_value,
            priority,
            store: Arc::new(RwLock::new(store)),
            config,
            metrics: Arc::new(RwLock::new(KeyValueMetrics::default())),
            tenant_id: std::sync::Mutex::new(String::new()),
            namespace: std::sync::Mutex::new(String::new()),
        })
    }

    /// Create with specific configuration
    pub fn with_config(config: KeyValueConfig) -> Result<Self, FacetError> {
        let config_value = serde_json::to_value(&config).unwrap_or(serde_json::json!({}));
        Self::with_config_internal(config, config_value, KEYVALUE_FACET_DEFAULT_PRIORITY)
    }

    /// Handle KV operations
    ///
    /// Uses tenant_id/namespace stored when facet was attached (from API request).
    /// Falls back to empty strings if not set (which will use defaults from node config).
    async fn handle_kv_operation(&self, method: &str, args: &[u8]) -> Result<Vec<u8>, FacetError> {
        // Get tenant_id/namespace from stored values (set during facet attachment from API request)
        let tenant_id = self.tenant_id.lock().unwrap().clone();
        let namespace = self.namespace.lock().unwrap().clone();
        // Use empty strings if not set (will use defaults from node config in RequestContext)
        let ctx = plexspaces_common::RequestContext::new_without_auth(
            if tenant_id.is_empty() {
                String::new()
            } else {
                tenant_id
            },
            if namespace.is_empty() {
                String::new()
            } else {
                namespace
            },
        );

        match method {
            "kv_get" => {
                let key: String = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let mut metrics = self.metrics.write().await;
                metrics.gets += 1;

                let store = self.store.read().await;
                match store.get(&ctx, &key).await {
                    Ok(Some(value)) => {
                        metrics.hits += 1;
                        Ok(value)
                    }
                    Ok(None) => {
                        metrics.misses += 1;
                        Ok(vec![])
                    }
                    Err(e) => Err(FacetError::InterceptionFailed(e.to_string())),
                }
            }
            "kv_set" => {
                #[derive(Deserialize)]
                struct SetArgs {
                    key: String,
                    value: Vec<u8>,
                    ttl: Option<u64>,
                }

                let args: SetArgs = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                self.metrics.write().await.sets += 1;

                let store = self.store.read().await;
                let effective_ttl = args.ttl.or(self.config.default_ttl);
                if let Some(ttl_secs) = effective_ttl {
                    store
                        .put_with_ttl(
                            &ctx,
                            &args.key,
                            args.value,
                            std::time::Duration::from_secs(ttl_secs),
                        )
                        .await
                        .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;
                } else {
                    store
                        .put(&ctx, &args.key, args.value)
                        .await
                        .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;
                }

                Ok(vec![])
            }
            "kv_delete" => {
                let key: String = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                self.metrics.write().await.deletes += 1;

                let store = self.store.read().await;
                store
                    .delete(&ctx, &key)
                    .await
                    .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;

                Ok(serde_json::to_vec(&true).unwrap())
            }
            "kv_exists" => {
                let key: String = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let store = self.store.read().await;
                let exists = store
                    .exists(&ctx, &key)
                    .await
                    .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;

                Ok(serde_json::to_vec(&exists).unwrap())
            }
            "kv_list" => {
                let prefix: String = serde_json::from_slice(args)
                    .map_err(|e| FacetError::InvalidConfig(e.to_string()))?;

                let store = self.store.read().await;
                let keys = store
                    .list_keys(&ctx, &prefix)
                    .await
                    .map_err(|e| FacetError::InterceptionFailed(e.to_string()))?;

                Ok(serde_json::to_vec(&keys).unwrap())
            }
            _ => Ok(vec![]),
        }
    }
}

#[async_trait]
impl Facet for KeyValueFacet {
    fn facet_type(&self) -> &str {
        "keyvalue" // Capability facets use simple names, namespace/contract in metadata if needed
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, config: Value) -> Result<(), FacetError> {
        // Extract tenant_id/namespace from config if available (passed from actor context)
        // These come from the API request (HTTP/gRPC), not ServiceLocator defaults
        if let Some(config_obj) = config.as_object() {
            if let Some(tenant_id_val) = config_obj.get("_tenant_id") {
                if let Some(tenant_id) = tenant_id_val.as_str() {
                    *self.tenant_id.lock().unwrap() = tenant_id.to_string();
                    tracing::debug!(actor_id = %actor_id, tenant_id = %tenant_id, "KeyValueFacet: Stored tenant_id from API request");
                }
            }
            if let Some(namespace_val) = config_obj.get("_namespace") {
                if let Some(namespace) = namespace_val.as_str() {
                    *self.namespace.lock().unwrap() = namespace.to_string();
                    tracing::debug!(actor_id = %actor_id, namespace = %namespace, "KeyValueFacet: Stored namespace from API request");
                }
            }
        }
        tracing::trace!(actor_id = %actor_id, "KeyValue capability attached to actor");
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        // Log metrics before detaching
        let metrics = self.metrics.read().await;
        let hit_rate = if metrics.gets > 0 {
            (metrics.hits as f64 / metrics.gets as f64) * 100.0
        } else {
            0.0
        };
        tracing::debug!(
            actor_id = %actor_id,
            gets = metrics.gets,
            sets = metrics.sets,
            deletes = metrics.deletes,
            hit_rate = %format!("{:.2}%", hit_rate),
            "KeyValue metrics on detach"
        );
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Intercept KV operations
        if method.starts_with("kv_") {
            let result = self.handle_kv_operation(method, args).await?;
            return Ok(InterceptResult::ShortCircuit(result));
        }
        Ok(InterceptResult::Continue)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        // Return metrics as state
        let metrics = self
            .metrics
            .try_read()
            .map_err(|_| FacetError::InterceptionFailed("Failed to read metrics".to_string()))?;

        Ok(serde_json::json!({
            "gets": metrics.gets,
            "sets": metrics.sets,
            "deletes": metrics.deletes,
            "hits": metrics.hits,
            "misses": metrics.misses,
            "hit_rate": if metrics.gets > 0 {
                metrics.hits as f64 / metrics.gets as f64
            } else {
                0.0
            }
        }))
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

// Note: CapabilityFacet trait removed - capabilities are just facets.
// If namespace/contract information is needed, store in facet metadata/config.
// Namespace: "wasi:keyvalue", Contract: "store"

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_keyvalue_facet() {
        let mut facet = KeyValueFacet::new(serde_json::json!({}), 50);

        // Attach to actor
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // Test set operation
        let set_args = serde_json::json!({
            "key": "test_key",
            "value": vec![1, 2, 3],
            "ttl": null
        });

        let result = facet
            .before_method("kv_set", serde_json::to_vec(&set_args).unwrap().as_slice())
            .await
            .unwrap();

        assert!(matches!(result, InterceptResult::ShortCircuit(_)));

        // Test get operation
        let get_args = serde_json::json!("test_key");
        let result = facet
            .before_method("kv_get", serde_json::to_vec(&get_args).unwrap().as_slice())
            .await
            .unwrap();

        match result {
            InterceptResult::ShortCircuit(data) => {
                assert!(!data.is_empty());
            }
            _ => panic!("Expected short circuit"),
        }

        // Check metrics
        let state = facet.get_state().unwrap();
        assert_eq!(state["sets"], 1);
        assert_eq!(state["gets"], 1);
        assert_eq!(state["hits"], 1);
    }

    #[tokio::test]
    async fn test_facet_type() {
        let facet = KeyValueFacet::new(serde_json::json!({}), 50);
        assert_eq!(facet.facet_type(), "keyvalue");
    }
}
