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

//! Simplified PlexSpaces host function bindings for Python WASM components
//!
//! This module provides a simplified WIT interface that uses only strings
//! for all complex data. This avoids componentize-py's PyObject_SetItem
//! issues with complex WIT types like `list<u8>`.
//!
//! Key design decisions:
//! - All payloads are JSON strings (not bytes)
//! - Error handling uses string return values ("" = success, "ERROR:..." = failure)
//! - Compatible with componentize-py 0.19.x
//!
//! ## KeyValue WIT compatibility
//! - WIT (plexspaces-simple-actor): `host.kv-get(key) -> string`, `host.kv-put(key, value) -> string`.
//! - Full plexspaces-actor WIT uses `keyvalue.get(ctx, key) -> result<option<payload>, actor-error>`.
//! - Simple host builds `RequestContext` with tenant_id="" and namespace=actor_id for key scoping.
//! - Node must pass keyvalue_store when instantiating; otherwise kv_get/kv_put return "ERROR: KeyValue store not configured".
//! - Debug logs: when store is None (host_functions) and on kv_get/kv_put failure (simple_component_host).
//!
//! See: archived_docs/python-wasm-component-guide.md for details

#[cfg(feature = "component-model")]
use crate::HostFunctions;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use plexspaces_core::{ActorId, RequestContext, TupleSpaceProvider, LockManager};
use plexspaces_tuplespace::{OrderedFloat, Tuple, TupleField, Pattern, PatternField};
use plexspaces_blob::BlobService;
use plexspaces_locks::{AcquireLockOptions, RenewLockOptions};
use std::collections::HashMap;
use std::sync::Arc;
use wasmtime::Result as WasmtimeResult;

// Generate bindings from simplified WIT files
// The bindgen macro generates types like ActorWorld for the "actor-world" world
#[cfg(feature = "component-model")]
wasmtime::component::bindgen!({
    world: "actor-world",
    path: "../../wit/plexspaces-simple-actor",
    async: true,
});

// Note: The bindgen macro generates types directly in this module:
// - ActorWorld: Main instantiation type (available as ActorWorld in this module)
// - plexspaces::simple_actor::*: Interface types
// ActorWorld is used via crate::simple_component_host::ActorWorld

/// Simple host implementation for Python-compatible WASM actors.
///
/// Implements the WIT `host` interface for `plexspaces:simple-actor`.
/// All host functions delegate to the framework's `HostFunctions` service gateway,
/// ensuring the WIT/SDK layer is a thin decorator over the core framework.
#[cfg(feature = "component-model")]
pub struct SimpleHostImpl {
    /// Actor ID of the component instance
    pub actor_id: ActorId,
    /// Host functions implementation (gateway to framework services)
    pub host_functions: Arc<HostFunctions>,
    /// TupleSpace provider for ts_write/read/take
    pub tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
    /// Lock manager for distributed locks
    pub lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
    /// Blob service for object storage
    pub blob_service: Option<Arc<BlobService>>,
    /// Pending send-after timer handles for cleanup on actor stop.
    /// Keyed by timer-id, values are JoinHandles that can be joined/aborted.
    pub pending_timers: Arc<tokio::sync::RwLock<HashMap<String, tokio::task::JoinHandle<()>>>>,
}

#[cfg(feature = "component-model")]
impl SimpleHostImpl {
    pub fn new(
        actor_id: ActorId,
        host_functions: Arc<HostFunctions>,
        tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
    ) -> Self {
        // Extract lock_manager and blob_service from host_functions if available
        let lock_manager = host_functions.lock_manager().cloned();
        let blob_service = host_functions.blob_service().cloned();
        Self {
            actor_id,
            host_functions,
            tuplespace_provider,
            lock_manager,
            blob_service,
            pending_timers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    pub fn with_services(
        actor_id: ActorId,
        host_functions: Arc<HostFunctions>,
        tuplespace_provider: Option<Arc<dyn TupleSpaceProvider>>,
        lock_manager: Option<Arc<dyn LockManager + Send + Sync>>,
        blob_service: Option<Arc<BlobService>>,
    ) -> Self {
        Self {
            actor_id,
            host_functions,
            tuplespace_provider,
            lock_manager,
            blob_service,
            pending_timers: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Build a RequestContext for process group operations.
    /// Uses the application namespace (from actor_id format `name:namespace@node`)
    /// so all actors in the same application share the same process group scope.
    fn pg_context(&self) -> RequestContext {
        let actor_str = self.actor_id.to_string();
        // Extract namespace from actor_id format: "name:namespace@node"
        let namespace = actor_str
            .split(':')
            .nth(1)
            .and_then(|rest| rest.split('@').next())
            .unwrap_or("")
            .to_string();
        RequestContext::new_without_auth(String::new(), namespace)
    }

    /// Parse tuple_json (JSON array of strings/numbers) into Tuple for existing TupleSpaceProvider.write
    fn parse_tuple_json(tuple_json: &str) -> Result<Tuple, String> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(tuple_json)
            .map_err(|e| format!("invalid tuple JSON: {}", e))?;
        let mut fields = Vec::with_capacity(arr.len());
        for v in arr {
            let field = match v {
                serde_json::Value::String(s) => TupleField::String(s),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        TupleField::Integer(i)
                    } else if let Some(f) = n.as_f64() {
                        TupleField::Float(OrderedFloat::new(f))
                    } else {
                        TupleField::String(n.to_string())
                    }
                }
                serde_json::Value::Bool(b) => TupleField::Boolean(b),
                serde_json::Value::Null => TupleField::Null,
                _ => TupleField::String(v.to_string()),
            };
            fields.push(field);
        }
        Ok(Tuple::new(fields))
    }

    /// Parse pattern_json (JSON array with wildcards) into Pattern for TupleSpace read/take.
    /// null or "*" matches any value (wildcard).
    fn parse_pattern_json(pattern_json: &str) -> Result<Pattern, String> {
        let arr: Vec<serde_json::Value> = serde_json::from_str(pattern_json)
            .map_err(|e| format!("invalid pattern JSON: {}", e))?;
        let mut fields = Vec::with_capacity(arr.len());
        for v in arr {
            let field = match v {
                serde_json::Value::Null => PatternField::Wildcard,
                serde_json::Value::String(s) if s == "*" => PatternField::Wildcard,
                serde_json::Value::String(s) => PatternField::Exact(TupleField::String(s)),
                serde_json::Value::Number(n) => {
                    if let Some(i) = n.as_i64() {
                        PatternField::Exact(TupleField::Integer(i))
                    } else if let Some(f) = n.as_f64() {
                        PatternField::Exact(TupleField::Float(OrderedFloat::new(f)))
                    } else {
                        PatternField::Exact(TupleField::String(n.to_string()))
                    }
                }
                serde_json::Value::Bool(b) => PatternField::Exact(TupleField::Boolean(b)),
                _ => PatternField::Exact(TupleField::String(v.to_string())),
            };
            fields.push(field);
        }
        Ok(Pattern::new(fields))
    }

    /// Convert Tuple to JSON string for returning to WASM
    fn tuple_to_json(tuple: &Tuple) -> String {
        let arr: Vec<serde_json::Value> = tuple.fields().iter().map(|f| match f {
            TupleField::String(s) => serde_json::Value::String(s.clone()),
            TupleField::Integer(i) => serde_json::Value::Number((*i).into()),
            TupleField::Float(f) => serde_json::Value::Number(serde_json::Number::from_f64(f.get()).unwrap_or(serde_json::Number::from(0))),
            TupleField::Boolean(b) => serde_json::Value::Bool(*b),
            TupleField::Null => serde_json::Value::Null,
            TupleField::Binary(b) => serde_json::Value::String(BASE64_STANDARD.encode(b)),
        }).collect();
        serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
    }
}

/// Implement the simple-host interface
#[cfg(feature = "component-model")]
#[async_trait::async_trait]
impl plexspaces::simple_actor::host::Host for SimpleHostImpl {
    /// Send a message to another actor
    /// Returns empty string on success, error message on failure
    /// Send-to-self is deferred (spawned) to avoid re-entering the component and triggering
    /// "cannot enter component instance" (wasmtime reentrancy trap).
    async fn send(&mut self, to: String, msg_type: String, payload_json: String) -> String {
        metrics::counter!("plexspaces_wasm_simple_send_total").increment(1);
        let start_time = std::time::Instant::now();
        let self_id = self.actor_id.to_string();

        if tracing::enabled!(tracing::Level::DEBUG) {
            tracing::debug!(
                actor_id = %self_id,
                to = %to,
                msg_type = %msg_type,
                payload_len = payload_json.len(),
                "Simple actor sending message"
            );
        }

        // Defer send-to-self to avoid re-entering the same component (wasmtime "cannot enter" trap).
        if to == self_id {
            let host_functions = std::sync::Arc::clone(&self.host_functions);
            let from = self_id.clone();
            let msg_type_clone = msg_type.clone();
            tokio::task::spawn(async move {
                let _ = host_functions.send_message(&from, &from, &msg_type_clone, &payload_json).await;
            });
            metrics::counter!("plexspaces_wasm_simple_send_success_total").increment(1);
            return String::new();
        }

        // Send message using existing host_functions API
        let result = self
            .host_functions
            .send_message(&self_id, &to, &msg_type, &payload_json)
            .await;

        let duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_simple_send_duration_seconds")
            .record(duration.as_secs_f64());

        match result {
            Ok(()) => {
                metrics::counter!("plexspaces_wasm_simple_send_success_total").increment(1);
                String::new()
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_simple_send_errors_total").increment(1);
                tracing::warn!(
                    actor_id = %self.actor_id,
                    to = %to,
                    error = %e,
                    "Simple actor send failed"
                );
                format!("ERROR: {}", e)
            }
        }
    }
    
    /// Log a message
    async fn log(&mut self, level: String, message: String) {
        metrics::counter!("plexspaces_wasm_simple_log_total").increment(1);
        
        match level.to_lowercase().as_str() {
            "trace" => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, "[WASM] {}", message);
                }
            },
            "debug" => {
                if tracing::enabled!(tracing::Level::DEBUG) {
                    tracing::debug!(actor_id = %self.actor_id, "[WASM] {}", message);
                }
            },
            "info" => tracing::info!(actor_id = %self.actor_id, "[WASM] {}", message),
            "warn" | "warning" => tracing::warn!(actor_id = %self.actor_id, "[WASM] {}", message),
            "error" => tracing::error!(actor_id = %self.actor_id, "[WASM] {}", message),
            _ => tracing::info!(actor_id = %self.actor_id, level = %level, "[WASM] {}", message),
        }
    }
    
    /// Get current timestamp in milliseconds
    async fn now_ms(&mut self) -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
    
    /// Key-value get (string-only). Returns value or empty if not found.
    /// WIT: plexspaces-simple-actor host.kv-get(key) -> string. Context uses tenant_id="", namespace=actor_id for key scoping.
    async fn kv_get(&mut self, key: String) -> String {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_get entry");
        }
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.get_keyvalue(&ctx, &key).await {
            Ok(Some(bytes)) => {
                let s = String::from_utf8_lossy(&bytes).into_owned();
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, value_len = s.len(), "wasm kv_get ok");
                }
                s
            }
            Ok(None) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_get none");
                }
                String::new()
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_get failed");
                format!("ERROR: {}", e)
            }
        }
    }

    /// Key-value put (string-only). Returns empty on success.
    /// Values are stored as UTF-8 bytes so kv_store remains human-readable for actor keys
    /// (object-registry uses the same table with protobuf for its entries).
    async fn kv_put(&mut self, key: String, value: String) -> String {
        if tracing::enabled!(tracing::Level::TRACE) {
            tracing::trace!(actor_id = %self.actor_id, key = %key, value_len = value.len(), "wasm kv_put entry");
        }
        // Ensure we only store valid UTF-8 so sqlite/kv_store displays as text (no binary/special chars from WASM)
        let bytes = match std::str::from_utf8(value.as_bytes()) {
            Ok(_) => value.into_bytes(),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_put: value not valid UTF-8, rejecting");
                return format!("ERROR: value must be valid UTF-8: {}", e);
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.put_keyvalue(&ctx, &key, bytes).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, key = %key, "wasm kv_put ok");
                }
                String::new()
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_put failed");
                format!("ERROR: {}", e)
            }
        }
    }

    /// TupleSpace write using existing TupleSpaceProvider (same as full plexspaces-actor tuplespace API).
    /// tuple_json: JSON array of strings/numbers, e.g. ["AUDIT","action","id",...].
    async fn ts_write(&mut self, tuple_json: String) -> String {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_write: TupleSpaceProvider not available");
                return "ERROR: TupleSpaceProvider not available".to_string();
            }
        };
        let tuple = match Self::parse_tuple_json(&tuple_json) {
            Ok(t) => t,
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "ts_write: invalid tuple JSON");
                return format!("ERROR: {}", e);
            }
        };
        match provider.write(tuple).await {
            Ok(()) => {
                if tracing::enabled!(tracing::Level::TRACE) {
                    tracing::trace!(actor_id = %self.actor_id, "wasm ts_write ok");
                }
                String::new()
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_write failed");
                format!("ERROR: {}", e)
            }
        }
    }

    // ========================================================================
    // Extended Key-Value Operations
    // ========================================================================

    /// Key-value delete
    async fn kv_delete(&mut self, key: String) -> String {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.delete_keyvalue(&ctx, &key).await {
            Ok(()) => String::new(),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, key = %key, error = %e, "wasm kv_delete failed");
                format!("ERROR: {}", e)
            }
        }
    }

    /// Key-value list keys with prefix
    async fn kv_list(&mut self, prefix: String) -> String {
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        match self.host_functions.list_keyvalue(&ctx, &prefix).await {
            Ok(keys) => serde_json::to_string(&keys).unwrap_or_else(|_| "[]".to_string()),
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, prefix = %prefix, error = %e, "wasm kv_list failed");
                format!("ERROR: {}", e)
            }
        }
    }

    // ========================================================================
    // Extended TupleSpace Operations
    // ========================================================================

    /// TupleSpace read (non-destructive). Returns first matching tuple as JSON array.
    async fn ts_read(&mut self, pattern_json: String) -> String {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_read: TupleSpaceProvider not available");
                return "ERROR: TupleSpaceProvider not available".to_string();
            }
        };
        let pattern = match Self::parse_pattern_json(&pattern_json) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "ts_read: invalid pattern JSON");
                return format!("ERROR: {}", e);
            }
        };
        match provider.read(&pattern).await {
            Ok(tuples) => {
                if let Some(tuple) = tuples.first() {
                    Self::tuple_to_json(tuple)
                } else {
                    String::new() // Not found
                }
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_read failed");
                format!("ERROR: {}", e)
            }
        }
    }

    /// TupleSpace take (destructive read). Returns matched tuple as JSON array and removes it.
    async fn ts_take(&mut self, pattern_json: String) -> String {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_take: TupleSpaceProvider not available");
                return "ERROR: TupleSpaceProvider not available".to_string();
            }
        };
        let pattern = match Self::parse_pattern_json(&pattern_json) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "ts_take: invalid pattern JSON");
                return format!("ERROR: {}", e);
            }
        };
        match provider.take(&pattern).await {
            Ok(Some(tuple)) => Self::tuple_to_json(&tuple),
            Ok(None) => String::new(), // Not found
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_take failed");
                format!("ERROR: {}", e)
            }
        }
    }

    /// TupleSpace read-all matching tuples (non-destructive).
    async fn ts_read_all(&mut self, pattern_json: String) -> String {
        let provider = match &self.tuplespace_provider {
            Some(p) => p,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "ts_read_all: TupleSpaceProvider not available");
                return "ERROR: TupleSpaceProvider not available".to_string();
            }
        };
        let pattern = match Self::parse_pattern_json(&pattern_json) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "ts_read_all: invalid pattern JSON");
                return format!("ERROR: {}", e);
            }
        };
        match provider.read(&pattern).await {
            Ok(tuples) => {
                let arr: Vec<serde_json::Value> = tuples.iter()
                    .map(|t| serde_json::from_str(&Self::tuple_to_json(t)).unwrap_or(serde_json::Value::Null))
                    .collect();
                serde_json::to_string(&arr).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, error = %e, "wasm ts_read_all failed");
                format!("ERROR: {}", e)
            }
        }
    }

    // ========================================================================
    // Distributed Lock Operations
    // ========================================================================
    // API requires tenant-id, namespace, holder-id for all operations (per WIT).

    /// Acquire a distributed lock. Returns JSON with lock_key, version, holder_id, locked, lease_duration_secs, expires_at_ms.
    async fn lock_acquire(
        &mut self,
        tenant_id: String,
        namespace: String,
        holder_id: String,
        lock_name: String,
        lease_duration_secs: u32,
        timeout_ms: u64,
    ) -> String {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_acquire: LockManager not available");
                return "ERROR: LockManager not available".to_string();
            }
        };
        let lease_secs = if lease_duration_secs == 0 { 30 } else { lease_duration_secs };
        tracing::debug!(
            actor_id = %self.actor_id,
            lock_name = %lock_name,
            tenant_id = %tenant_id,
            namespace = %namespace,
            holder_id = %holder_id,
            "lock_acquire attempt"
        );
        let ctx = RequestContext::new_without_auth(tenant_id.clone(), namespace.clone());
        let options = AcquireLockOptions {
            lock_key: lock_name.clone(),
            holder_id: holder_id.clone(),
            lease_duration_secs: lease_secs,
            additional_wait_time_ms: timeout_ms as u32,
            refresh_period_ms: 100,
            metadata: HashMap::new(),
        };
        match lock_manager.acquire_lock(&ctx, options).await {
            Ok(lock) => {
                let expires_at_ms = lock
                    .expires_at
                    .map(|ts| (ts.seconds as u64 * 1000) + (ts.nanos as u64 / 1_000_000))
                    .unwrap_or(0);
                let json = serde_json::json!({
                    "lock_key": lock.lock_key,
                    "version": lock.version,
                    "holder_id": lock.holder_id,
                    "locked": lock.locked,
                    "lease_duration_secs": lock.lease_duration_secs,
                    "expires_at_ms": expires_at_ms,
                });
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_name = %lock_name,
                    version = %lock.version,
                    "lock_acquire success"
                );
                json.to_string()
            }
            Err(e) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_name = %lock_name,
                    error = %e,
                    "lock_acquire failed"
                );
                format!("ERROR: {}", e)
            }
        }
    }

    /// Release a distributed lock. Requires lock-id, tenant-id, namespace, holder-id, lock-version.
    async fn lock_release(
        &mut self,
        lock_id: String,
        tenant_id: String,
        namespace: String,
        holder_id: String,
        lock_version: String,
    ) -> String {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_release: LockManager not available");
                return "ERROR: LockManager not available".to_string();
            }
        };
        let ctx = RequestContext::new_without_auth(tenant_id, namespace);
        let options = plexspaces_locks::ReleaseLockOptions {
            lock_key: lock_id,
            holder_id,
            version: lock_version,
            delete_lock: false,
        };
        match lock_manager.release_lock(&ctx, options).await {
            Ok(()) => String::new(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    /// Renew lease on a held lock (heartbeat). Returns new version on success.
    async fn lock_renew(
        &mut self,
        lock_id: String,
        tenant_id: String,
        namespace: String,
        holder_id: String,
        lock_version: String,
        lease_duration_secs: u32,
    ) -> String {
        let lock_manager = match &self.lock_manager {
            Some(lm) => lm,
            None => {
                tracing::warn!(actor_id = %self.actor_id, "lock_renew: LockManager not available");
                return "ERROR: LockManager not available".to_string();
            }
        };
        let ctx = RequestContext::new_without_auth(tenant_id, namespace);
        let options = RenewLockOptions {
            lock_key: lock_id.clone(),
            holder_id,
            version: lock_version.clone(),
            lease_duration_secs,
            metadata: HashMap::new(),
        };
        match lock_manager.renew_lock(&ctx, options).await {
            Ok(renewed) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_id = %lock_id,
                    new_version = %renewed.version,
                    "lock_renew success"
                );
                renewed.version
            }
            Err(e) => {
                tracing::debug!(
                    actor_id = %self.actor_id,
                    lock_id = %lock_id,
                    error = %e,
                    "lock_renew failed"
                );
                format!("ERROR: {}", e)
            }
        }
    }

    // ========================================================================
    // Blob Storage Operations
    // ========================================================================

    /// Upload blob data (base64-encoded).
    async fn blob_upload(&mut self, blob_id: String, data: String, content_type: String) -> String {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_upload: BlobService not available");
                return "ERROR: BlobService not available".to_string();
            }
        };
        // Decode base64 data
        let decoded = match BASE64_STANDARD.decode(&data) {
            Ok(d) => d,
            Err(e) => {
                tracing::error!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_upload: invalid base64 data");
                return format!("ERROR: invalid base64 data: {}", e);
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        tracing::debug!(actor_id = %self.actor_id, name = %blob_id, data_len = decoded.len(), content_type = %content_type, "blob_upload: starting");
        match blob_service.upload_blob(
            &ctx,
            &blob_id,
            decoded,
            Some(content_type.clone()),
            None, // blob_group
            None, // kind
            HashMap::new(), // metadata
            HashMap::new(), // tags
            None, // expires_after
        ).await {
            Ok(metadata) => {
                tracing::debug!(actor_id = %self.actor_id, name = %blob_id, internal_blob_id = %metadata.blob_id, "blob_upload: success");
                // Return empty string on success (WIT convention)
                // Callers use the name for download; internal blob_id is for debugging
                String::new()
            },
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, name = %blob_id, error = %e, "blob_upload: failed");
                format!("ERROR: {}", e)
            },
        }
    }

    /// Download blob data (returns base64-encoded).
    /// Supports both name (path) and blob_id (ULID) lookup.
    /// First tries by name, then falls back to by ID.
    async fn blob_download(&mut self, blob_id: String) -> String {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_download: BlobService not available");
                return "ERROR: BlobService not available".to_string();
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: starting");
        
        // First try by name (path) - common pattern for WASM actors
        match blob_service.download_blob_by_name(&ctx, &blob_id).await {
            Ok(data) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, data_len = data.len(), "blob_download: success (by name)");
                return BASE64_STANDARD.encode(&data);
            },
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                // Name not found, try by ID
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: not found by name, trying by ID");
            },
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_download: name lookup failed");
                // Continue to try by ID
            },
        }
        
        // Fall back to by ID (ULID) - for callers who have the internal blob_id
        match blob_service.download_blob(&ctx, &blob_id).await {
            Ok(data) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, data_len = data.len(), "blob_download: success (by ID)");
                BASE64_STANDARD.encode(&data)
            },
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_download: not found");
                String::new() // Not found
            },
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_download: failed");
                format!("ERROR: {}", e)
            },
        }
    }

    /// Delete blob.
    /// Supports both name (path) and blob_id (ULID) lookup.
    /// First tries by name, then falls back to by ID.
    async fn blob_delete(&mut self, blob_id: String) -> String {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_delete: BlobService not available");
                return "ERROR: BlobService not available".to_string();
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: starting");
        
        // First try by name (path) - common pattern for WASM actors
        match blob_service.delete_blob_by_name(&ctx, &blob_id).await {
            Ok(()) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: success (by name)");
                return String::new();
            },
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                // Name not found, try by ID
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: not found by name, trying by ID");
            },
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, blob_id = %blob_id, error = %e, "blob_delete: name lookup failed");
                // Continue to try by ID
            },
        }
        
        // Fall back to by ID (ULID) - for callers who have the internal blob_id
        match blob_service.delete_blob(&ctx, &blob_id).await {
            Ok(()) => {
                tracing::debug!(actor_id = %self.actor_id, blob_id = %blob_id, "blob_delete: success (by ID)");
                String::new()
            },
            Err(plexspaces_blob::BlobError::NotFound(_)) => {
                // Blob not found by either name or ID - return error
                format!("ERROR: Blob not found: {}", blob_id)
            },
            Err(e) => {
                format!("ERROR: {}", e)
            },
        }
    }

    /// List blobs with prefix.
    /// Returns blob names (paths) since WASM actors use paths as identifiers.
    async fn blob_list(&mut self, prefix: String) -> String {
        let blob_service = match &self.blob_service {
            Some(bs) => bs,
            None => {
                tracing::error!(actor_id = %self.actor_id, "blob_list: BlobService not available");
                return "ERROR: BlobService not available".to_string();
            }
        };
        let ctx = RequestContext::new_without_auth(String::new(), self.actor_id.to_string());
        tracing::debug!(actor_id = %self.actor_id, prefix = %prefix, "blob_list: starting");
        let filters = plexspaces_blob::repository::ListFilters {
            name_prefix: Some(prefix.clone()),
            ..Default::default()
        };
        match blob_service.list_blobs(&ctx, &filters, 100, 1).await {
            Ok((blobs, _total)) => {
                // Return names (paths) instead of blob_ids since WASM actors use paths
                let names: Vec<String> = blobs.iter().map(|b| b.name.clone()).collect();
                tracing::debug!(actor_id = %self.actor_id, prefix = %prefix, count = names.len(), "blob_list: success");
                serde_json::to_string(&names).unwrap_or_else(|_| "[]".to_string())
            }
            Err(e) => {
                tracing::warn!(actor_id = %self.actor_id, prefix = %prefix, error = %e, "blob_list: failed");
                format!("ERROR: {}", e)
            },
        }
    }

    // ========================================================================
    // Messaging: ask (request-reply)
    // ========================================================================

    /// Send request and wait for response. Delegates to HostFunctions::ask().
    async fn ask(&mut self, to: String, msg_type: String, payload_json: String, timeout_ms: u64) -> String {
        let self_id = self.actor_id.to_string();
        tracing::debug!(
            actor_id = %self_id, to = %to, msg_type = %msg_type,
            timeout_ms = timeout_ms, "simple actor ask"
        );
        match self.host_functions.ask(
            &self_id, &to, &msg_type,
            payload_json.into_bytes(),
            timeout_ms,
        ).await {
            Ok(response_bytes) => String::from_utf8_lossy(&response_bytes).into_owned(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    // ========================================================================
    // Actor Identity
    // ========================================================================

    /// Get own actor ID. Returns the ActorId assigned during actor registration.
    async fn self_id(&mut self) -> String {
        self.actor_id.to_string()
    }

    // ========================================================================
    // Actor Lifecycle: spawn, stop
    // ========================================================================

    /// Spawn a new actor. Delegates to HostFunctions::spawn_actor() which calls
    /// ActorServiceMessageSender → ActorService → ActorFactory::spawn_actor().
    ///
    /// If actor_id is empty, the framework generates a ULID-based ID automatically.
    /// Returns the actual spawned actor ID on success (important when auto-generated),
    /// or "ERROR:message" on failure.
    async fn spawn(&mut self, module_ref: String, actor_id: String, init_config_json: String) -> String {
        metrics::counter!("plexspaces_wasm_spawn_total").increment(1);
        let self_id = self.actor_id.to_string();
        // Pass None for empty actor_id so the framework generates a ULID
        let requested_id = if actor_id.is_empty() { None } else { Some(actor_id.clone()) };
        tracing::debug!(
            actor_id = %self_id, module_ref = %module_ref,
            new_actor_id = %actor_id, "simple actor spawn"
        );
        match self.host_functions.spawn_actor(
            &self_id,
            &module_ref,
            init_config_json.into_bytes(),
            requested_id,
            vec![],  // labels not exposed in simple-actor WIT
            false,   // durability configured at framework level via facets
        ).await {
            Ok(spawned_id) => {
                metrics::counter!("plexspaces_wasm_spawn_success_total").increment(1);
                tracing::debug!(
                    actor_id = %self_id, spawned_id = %spawned_id,
                    "simple actor spawn success"
                );
                // Return the actual spawned actor ID (may differ from requested if auto-generated)
                spawned_id
            }
            Err(e) => {
                metrics::counter!("plexspaces_wasm_spawn_errors_total").increment(1);
                format!("ERROR: {}", e)
            }
        }
    }

    /// Stop an actor. Delegates to HostFunctions::stop_actor().
    async fn stop(&mut self, actor_id: String) -> String {
        let self_id = self.actor_id.to_string();
        tracing::debug!(actor_id = %self_id, target = %actor_id, "simple actor stop");
        match self.host_functions.stop_actor(&self_id, &actor_id, 5000).await {
            Ok(()) => String::new(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    // ========================================================================
    // Actor Linking & Monitoring (Erlang/OTP patterns)
    // ========================================================================

    /// Bidirectional link
    async fn link(&mut self, actor_id: String) -> String {
        let self_id = self.actor_id.to_string();
        match self.host_functions.link_actor(&self_id, &self_id, &actor_id).await {
            Ok(()) => String::new(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    /// Remove bidirectional link
    async fn unlink(&mut self, actor_id: String) -> String {
        let self_id = self.actor_id.to_string();
        match self.host_functions.unlink_actor(&self_id, &self_id, &actor_id).await {
            Ok(()) => String::new(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    /// Unidirectional monitor — returns monitor reference as string
    async fn monitor(&mut self, actor_id: String) -> String {
        let self_id = self.actor_id.to_string();
        match self.host_functions.monitor_actor(&self_id, &actor_id).await {
            Ok(monitor_ref) => monitor_ref.to_string(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    /// Cancel a monitor
    async fn demonitor(&mut self, monitor_ref: String) -> String {
        let self_id = self.actor_id.to_string();
        let ref_id: u64 = match monitor_ref.parse() {
            Ok(id) => id,
            Err(_) => return format!("ERROR: invalid monitor ref: {}", monitor_ref),
        };
        match self.host_functions.demonitor_actor(&self_id, "", ref_id).await {
            Ok(()) => String::new(),
            Err(e) => format!("ERROR: {}", e),
        }
    }

    // ========================================================================
    // Timers (Delayed Messaging)
    // ========================================================================

    /// Send a message to self after a delay.
    /// Spawns a tracked background task that delivers the message after delay_ms.
    /// Returns a timer-id for observability. The JoinHandle is stored in pending_timers
    /// so it can be joined/aborted when the actor stops (cleanup via drop or explicit stop).
    async fn send_after(&mut self, delay_ms: u64, msg_type: String, payload_json: String) -> String {
        metrics::counter!("plexspaces_wasm_send_after_total").increment(1);
        let host_functions = self.host_functions.clone();
        let self_id = self.actor_id.to_string();
        let from = self_id.clone();

        // Generate unique timer ID using actor ID + monotonic nanos
        let timer_id = format!("timer-{}-{}", self_id,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        let timer_id_for_task = timer_id.clone();
        let timer_id_for_cleanup = timer_id.clone();
        let pending_timers = self.pending_timers.clone();

        tracing::debug!(
            actor_id = %self_id, timer_id = %timer_id,
            delay_ms = delay_ms, msg_type = %msg_type,
            "send_after: scheduling delayed message"
        );

        // Spawn tracked background task
        let handle = tokio::task::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
            if let Err(e) = host_functions.send_message(&from, &from, &msg_type, &payload_json).await {
                tracing::warn!(
                    actor_id = %from, timer_id = %timer_id_for_task,
                    error = %e, "send_after: delivery failed"
                );
                metrics::counter!("plexspaces_wasm_send_after_errors_total").increment(1);
            } else {
                tracing::debug!(
                    actor_id = %from, timer_id = %timer_id_for_task,
                    "send_after: message delivered"
                );
            }
            // Self-cleanup: remove from pending_timers after delivery
            pending_timers.write().await.remove(&timer_id_for_cleanup);
        });

        // Store JoinHandle for cleanup on actor stop
        self.pending_timers.write().await.insert(timer_id.clone(), handle);
        timer_id
    }

    // ========================================================================
    // Process Groups
    // ========================================================================

    /// Join a named process group (auto-creates the group if it doesn't exist, like Erlang pg:join/2)
    async fn pg_join(&mut self, group_name: String) -> String {
        let self_id = self.actor_id.clone();
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => {
                match registry.join_group(&ctx, &group_name, &self_id, vec![]).await {
                    Ok(()) => String::new(),
                    Err(plexspaces_process_groups::ProcessGroupError::GroupNotFound(_)) => {
                        // Auto-create group and retry join (Erlang pg semantics)
                        if let Err(e) = registry.create_group(&ctx, &group_name).await {
                            // Ignore GroupAlreadyExists (race condition with another actor)
                            if !matches!(e, plexspaces_process_groups::ProcessGroupError::GroupAlreadyExists(_)) {
                                return format!("ERROR: Failed to create group: {}", e);
                            }
                        }
                        match registry.join_group(&ctx, &group_name, &self_id, vec![]).await {
                            Ok(()) => String::new(),
                            Err(e) => format!("ERROR: {}", e),
                        }
                    }
                    Err(e) => format!("ERROR: {}", e),
                }
            }
            None => "ERROR: ProcessGroupRegistry not configured".to_string(),
        }
    }

    /// Leave a named process group
    async fn pg_leave(&mut self, group_name: String) -> String {
        let self_id = self.actor_id.clone();
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => {
                match registry.leave_group(&ctx, &group_name, &self_id).await {
                    Ok(()) => String::new(),
                    Err(e) => format!("ERROR: {}", e),
                }
            }
            None => "ERROR: ProcessGroupRegistry not configured".to_string(),
        }
    }

    /// List members of a process group
    async fn pg_members(&mut self, group_name: String) -> String {
        let ctx = self.pg_context();
        match self.host_functions.process_group_registry() {
            Some(registry) => {
                match registry.get_members(&ctx, &group_name).await {
                    Ok(members) => {
                        let ids: Vec<String> = members.iter().map(|m| m.to_string()).collect();
                        serde_json::to_string(&ids).unwrap_or_else(|_| "[]".to_string())
                    }
                    Err(e) => format!("ERROR: {}", e),
                }
            }
            None => "ERROR: ProcessGroupRegistry not configured".to_string(),
        }
    }

    /// Broadcast message to all members of a process group
    async fn pg_broadcast(&mut self, group_name: String, _msg_type: String, payload_json: String) -> String {
        let self_id = self.actor_id.to_string();
        let ctx = self.pg_context();
        let registry = match self.host_functions.process_group_registry() {
            Some(r) => r.clone(),
            None => return "ERROR: ProcessGroupRegistry not configured".to_string(),
        };
        let members = match registry.get_members(&ctx, &group_name).await {
            Ok(m) => m,
            Err(e) => return format!("ERROR: {}", e),
        };
        // Extract message_type from payload JSON if available, otherwise use "cast" (fire-and-forget)
        let message_type = if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&payload_json) {
            json_value.get("op")
                .or_else(|| json_value.get("msg_type"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| "cast".to_string())
        } else {
            "cast".to_string()
        };
        for member in &members {
            let member_str = member.to_string();
            if let Err(e) = self.host_functions.send_message(&self_id, &member_str, &message_type, &payload_json).await {
                tracing::warn!(
                    actor_id = %self_id, group = %group_name,
                    target = %member_str, error = %e,
                    "pg_broadcast: failed to send to member"
                );
            }
        }
        String::new()
    }
}

/// Check if a WASM component uses the simple-actor interface by examining its imports.
/// Tolerates different name formats (e.g. with/without version, different separators).
#[cfg(feature = "component-model")]
pub fn is_simple_actor_component(component: &wasmtime::component::Component) -> bool {
    let component_type = component.component_type();
    for (name, _) in component_type.imports(&component.engine()) {
        let n = format!("{}", name);
        if n == "plexspaces:simple-actor/host@0.1.0"
            || n.starts_with("plexspaces:simple-actor@")
            || n.contains("simple-actor")
            || n.contains("simple_actor")
        {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_actor_module_compiles() {
        // This test ensures the module compiles correctly
        // Actual functional tests require WASM components
    }
}
