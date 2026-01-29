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

//! Distributed Lock Capability Facet
//!
//! ## Purpose
//! Provides distributed lock coordination capabilities to actors as a runtime-attachable facet.
//! This follows the same pattern as KeyValueFacet - actors send messages with specific message types,
//! and the facet intercepts and handles them using the real LockManager backend.
//!
//! ## Design Pattern
//! - **Message Interception**: Facet intercepts messages with types like `"acquire_lock"`, `"release_lock"`, etc.
//! - **Short-Circuit Handling**: Facet handles the operation and returns result without calling the actor
//! - **Works for Rust and WASM**: Both Rust and WASM actors send messages, facet handles them uniformly
//! - **Uses Real Backend**: Wraps LockManager (MemoryLockManager, SQLite, Redis, etc.)
//!
//! ## Message Types
//! - `"acquire_lock"`: Acquire a lock with lease duration
//! - `"release_lock"`: Release a lock (requires version)
//! - `"renew_lock"`: Renew a lock lease (heartbeat)
//! - `"try_acquire_lock"`: Non-blocking lock attempt
//! - `"get_lock"`: Get current lock state
//!
//! ## Usage Example
//! ```rust
//! // Attach LockFacet to actor
//! let lock_facet = LockFacet::new(lock_manager, json!({}), 50);
//! actor.attach_facet(Box::new(lock_facet)).await?;
//!
//! // Actor sends message to acquire lock
//! let msg = Message::json(&json!({
//!     "lock_key": "resource-1",
//!     "holder_id": "actor-1",
//!     "lease_duration_secs": 30
//! }))?.with_message_type("acquire_lock");
//! actor_ref.tell(msg).await?;
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::Instant;

use crate::{Facet, FacetError, InterceptResult};
use plexspaces_common::RequestContext;
use plexspaces_proto::locks::prv::{AcquireLockOptions, Lock, ReleaseLockOptions, RenewLockOptions};
use tracing::{debug, error, info, instrument, warn};

/// Convert Lock proto struct to JSON Value (Lock doesn't implement Serialize)
fn lock_to_json(lock: &Lock) -> Value {
    json!({
        "lock_key": lock.lock_key,
        "holder_id": lock.holder_id,
        "version": lock.version,
        "expires_at": lock.expires_at.as_ref().map(|ts| {
            // Convert Timestamp to seconds since epoch (as f64 for precision)
            ts.seconds as f64 + (ts.nanos as f64 / 1_000_000_000.0)
        }),
        "lease_duration_secs": lock.lease_duration_secs,
        "last_heartbeat": lock.last_heartbeat.as_ref().map(|ts| {
            ts.seconds as f64 + (ts.nanos as f64 / 1_000_000_000.0)
        }),
        "metadata": lock.metadata,
        "locked": lock.locked,
    })
}

/// Default priority for LockFacet
pub const LOCK_FACET_DEFAULT_PRIORITY: i32 = 30;

/// Configuration for lock facet
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LockConfig {
    /// Default lease duration in seconds
    pub default_lease_secs: Option<u32>,
}

impl Default for LockConfig {
    fn default() -> Self {
        LockConfig {
            default_lease_secs: Some(30),
        }
    }
}

/// Trait for lock manager implementations (to avoid circular dependency with plexspaces-core)
#[async_trait]
pub trait LockManager: Send + Sync {
    /// Acquire a lock
    async fn acquire_lock(&self, ctx: &RequestContext, options: AcquireLockOptions) -> Result<Lock, String>;
    /// Renew a lock
    async fn renew_lock(&self, ctx: &RequestContext, options: RenewLockOptions) -> Result<Lock, String>;
    /// Release a lock
    async fn release_lock(&self, ctx: &RequestContext, options: ReleaseLockOptions) -> Result<(), String>;
    /// Get current lock state
    async fn get_lock(&self, ctx: &RequestContext, lock_key: &str) -> Result<Option<Lock>, String>;
}

/// Distributed lock capability facet
///
/// ## Purpose
/// Provides distributed lock coordination to actors via message interception.
/// Actors send messages with lock operation types, facet handles them using LockManager.
pub struct LockFacet {
    /// Facet configuration as Value (immutable, for Facet trait)
    config_value: Value,
    /// Facet priority (immutable)
    priority: i32,
    /// Lock manager implementation
    lock_manager: Arc<dyn LockManager>,
    /// Configuration (parsed from config_value)
    config: LockConfig,
}

impl LockFacet {
    /// Create a new lock facet
    ///
    /// ## Arguments
    /// * `lock_manager` - Lock manager backend (implements LockManager trait)
    /// * `config` - Facet configuration JSON
    /// * `priority` - Facet priority
    pub fn new(lock_manager: Arc<dyn LockManager>, config: Value, priority: i32) -> Self {
        let config_clone = config.clone();
        let lock_config = serde_json::from_value::<LockConfig>(config_clone)
            .unwrap_or_else(|_| LockConfig::default());

        LockFacet {
            config_value: config,
            priority,
            lock_manager,
            config: lock_config,
        }
    }

    /// Handle lock operations with observability
    #[instrument(skip(self, args), fields(operation = method))]
    async fn handle_lock_operation(&self, method: &str, args: &[u8]) -> Result<Vec<u8>, FacetError> {
        let start = Instant::now();
        
        // Create request context - locks facet doesn't have access to ServiceLocator or actor context
        // TODO: Extract tenant/namespace from actor context when available (similar to RegistryFacet)
        // For now, use empty strings (locks will work but without tenant/namespace isolation)
        let ctx = RequestContext::new_without_auth(String::new(), String::new());

        let result = match method {
            "acquire_lock" => {
                metrics::counter!("plexspaces_facet_locks_operations_total", "operation" => "acquire_lock").increment(1);
                
                #[derive(Deserialize)]
                struct AcquireArgs {
                    lock_key: String,
                    holder_id: String,
                    lease_duration_secs: Option<u32>,
                    additional_wait_time_ms: Option<u32>,
                    refresh_period_ms: Option<u32>,
                    metadata: Option<std::collections::HashMap<String, String>>,
                }

                let args: AcquireArgs = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "acquire_lock", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                let lease_duration = args.lease_duration_secs
                    .or(self.config.default_lease_secs)
                    .unwrap_or(30);

                let options = AcquireLockOptions {
                    lock_key: args.lock_key.clone(),
                    holder_id: args.holder_id.clone(),
                    lease_duration_secs: lease_duration,
                    additional_wait_time_ms: args.additional_wait_time_ms.unwrap_or(0),
                    refresh_period_ms: args.refresh_period_ms.unwrap_or(100),
                    metadata: args.metadata.unwrap_or_default(),
                };

                match self.lock_manager.acquire_lock(&ctx, options).await {
                    Ok(lock) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "acquire_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "acquire_lock").increment(1);
                        info!(lock_key = %lock.lock_key, holder_id = %lock.holder_id, version = %lock.version, lease_secs = lock.lease_duration_secs, duration_ms = duration.as_millis(), "🔒 LockFacet: Lock acquired");
                        
                        let lock_json = lock_to_json(&lock);
                        serde_json::to_vec(&lock_json)
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        let error_str = e.to_string();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "acquire_lock").record(duration.as_secs_f64());
                        
                        // Check if it's a lock contention error (already held)
                        if error_str.contains("already held") || error_str.contains("LockAlreadyHeld") {
                            let current_holder = if error_str.contains("already held by:") {
                                error_str.split("already held by: ").nth(1).unwrap_or("unknown").trim().to_string()
                            } else {
                                "unknown".to_string()
                            };
                            metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "acquire_lock", "error" => "lock_already_held").increment(1);
                            warn!(lock_key = %args.lock_key, holder_id = %args.holder_id, current_holder = %current_holder, duration_ms = duration.as_millis(), "⚔️  LockFacet: Lock contention - lock already held by another worker");
                        } else {
                            metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "acquire_lock", "error" => "lock_failed").increment(1);
                            error!(lock_key = %args.lock_key, error = %e, duration_ms = duration.as_millis(), "Failed to acquire lock");
                        }
                        Err(FacetError::InterceptionFailed(e))
                    }
                }
            }
            "release_lock" => {
                metrics::counter!("plexspaces_facet_locks_operations_total", "operation" => "release_lock").increment(1);
                
                #[derive(Deserialize)]
                struct ReleaseArgs {
                    lock_key: String,
                    holder_id: String,
                    version: String,
                    delete_lock: Option<bool>,
                }

                let args: ReleaseArgs = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "release_lock", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                let options = ReleaseLockOptions {
                    lock_key: args.lock_key.clone(),
                    holder_id: args.holder_id.clone(),
                    version: args.version.clone(),
                    delete_lock: args.delete_lock.unwrap_or(false),
                };

                match self.lock_manager.release_lock(&ctx, options).await {
                    Ok(()) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "release_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "release_lock").increment(1);
                        info!(lock_key = %args.lock_key, holder_id = %args.holder_id, duration_ms = duration.as_millis(), "🔓 LockFacet: Lock released");
                        
                        serde_json::to_vec(&json!({"status": "ok"}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "release_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "release_lock", "error" => "lock_failed").increment(1);
                        error!(lock_key = %args.lock_key, error = %e, duration_ms = duration.as_millis(), "Failed to release lock");
                        Err(FacetError::InterceptionFailed(e))
                    }
                }
            }
            "renew_lock" => {
                metrics::counter!("plexspaces_facet_locks_operations_total", "operation" => "renew_lock").increment(1);
                
                #[derive(Deserialize)]
                struct RenewArgs {
                    lock_key: String,
                    holder_id: String,
                    version: String,
                    lease_duration_secs: Option<u32>,
                }

                let args: RenewArgs = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "renew_lock", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                let lease_duration = args.lease_duration_secs
                    .or(self.config.default_lease_secs)
                    .unwrap_or(30);

                let options = RenewLockOptions {
                    lock_key: args.lock_key.clone(),
                    holder_id: args.holder_id.clone(),
                    version: args.version.clone(),
                    lease_duration_secs: lease_duration,
                    metadata: std::collections::HashMap::new(),
                };

                match self.lock_manager.renew_lock(&ctx, options).await {
                    Ok(lock) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "renew_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "renew_lock").increment(1);
                        info!(lock_key = %lock.lock_key, holder_id = %lock.holder_id, version = %lock.version, lease_secs = lock.lease_duration_secs, duration_ms = duration.as_millis(), "🔄 LockFacet: Lock renewed (heartbeat)");
                        
                        let lock_json = lock_to_json(&lock);
                        serde_json::to_vec(&lock_json)
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "renew_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "renew_lock", "error" => "lock_failed").increment(1);
                        error!(lock_key = %args.lock_key, error = %e, duration_ms = duration.as_millis(), "Failed to renew lock");
                        Err(FacetError::InterceptionFailed(e))
                    }
                }
            }
            "try_acquire_lock" => {
                metrics::counter!("plexspaces_facet_locks_operations_total", "operation" => "try_acquire_lock").increment(1);
                
                #[derive(Deserialize)]
                struct TryAcquireArgs {
                    lock_key: String,
                    holder_id: String,
                    lease_duration_secs: Option<u32>,
                }

                let args: TryAcquireArgs = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "try_acquire_lock", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                let lease_duration = args.lease_duration_secs
                    .or(self.config.default_lease_secs)
                    .unwrap_or(30);

                let options = AcquireLockOptions {
                    lock_key: args.lock_key.clone(),
                    holder_id: args.holder_id.clone(),
                    lease_duration_secs: lease_duration,
                    additional_wait_time_ms: 0, // Non-blocking
                    refresh_period_ms: 100,
                    metadata: std::collections::HashMap::new(),
                };

                match self.lock_manager.acquire_lock(&ctx, options).await {
                    Ok(lock) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "try_acquire_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "try_acquire_lock").increment(1);
                        info!(lock_key = %lock.lock_key, holder_id = %lock.holder_id, version = %lock.version, lease_secs = lock.lease_duration_secs, duration_ms = duration.as_millis(), "🔒 LockFacet: Lock acquired (try_acquire_lock)");
                        
                        let lock_json = lock_to_json(&lock);
                        serde_json::to_vec(&lock_json)
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "try_acquire_lock").record(duration.as_secs_f64());
                        // Check if error is "lock already held" - that's expected for try_acquire
                        let error_str = e.to_string();
                        if error_str.contains("already held") || error_str.contains("LockAlreadyHeld") {
                            // Extract current holder from error message (format: "Lock already held by: {holder_id}")
                            let current_holder = if error_str.contains("already held by:") {
                                error_str.split("already held by: ").nth(1).unwrap_or("unknown").trim().to_string()
                            } else if error_str.contains("LockAlreadyHeld") {
                                // Try to extract from error string
                                error_str.split("LockAlreadyHeld").nth(1)
                                    .and_then(|s| s.split('"').nth(1))
                                    .unwrap_or("unknown")
                                    .to_string()
                            } else {
                                "unknown".to_string()
                            };
                            
                            metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "try_acquire_lock", "result" => "not_acquired").increment(1);
                            warn!(lock_key = %args.lock_key, holder_id = %args.holder_id, current_holder = %current_holder, duration_ms = duration.as_millis(), "⚔️  LockFacet: Try acquire failed - lock already held by another worker");
                            serde_json::to_vec(&json!({"acquired": false, "reason": error_str}))
                                .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                        } else {
                            metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "try_acquire_lock", "error" => "lock_failed").increment(1);
                            error!(lock_key = %args.lock_key, error = %e, duration_ms = duration.as_millis(), "Failed to try acquire lock");
                            Err(FacetError::InterceptionFailed(e))
                        }
                    }
                }
            }
            "get_lock" => {
                metrics::counter!("plexspaces_facet_locks_operations_total", "operation" => "get_lock").increment(1);
                
                let lock_key: String = serde_json::from_slice(args)
                    .map_err(|e| {
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "get_lock", "error" => "deserialization").increment(1);
                        FacetError::InvalidConfig(e.to_string())
                    })?;

                match self.lock_manager.get_lock(&ctx, &lock_key).await {
                    Ok(Some(lock)) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "get_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "get_lock").increment(1);
                        info!(lock_key = %lock_key, holder_id = %lock.holder_id, version = %lock.version, lease_secs = lock.lease_duration_secs, duration_ms = duration.as_millis(), "🔍 LockFacet: Lock retrieved");
                        
                        let lock_json = lock_to_json(&lock);
                        serde_json::to_vec(&lock_json)
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Ok(None) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "get_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_operations_success_total", "operation" => "get_lock", "result" => "not_found").increment(1);
                        debug!(lock_key = %lock_key, duration_ms = duration.as_millis(), "Lock not found");
                        
                        serde_json::to_vec(&json!({"found": false}))
                            .map_err(|e| FacetError::InterceptionFailed(e.to_string()))
                    }
                    Err(e) => {
                        let duration = start.elapsed();
                        metrics::histogram!("plexspaces_facet_locks_operation_duration_seconds", "operation" => "get_lock").record(duration.as_secs_f64());
                        metrics::counter!("plexspaces_facet_locks_errors_total", "operation" => "get_lock", "error" => "lock_failed").increment(1);
                        error!(lock_key = %lock_key, error = %e, duration_ms = duration.as_millis(), "Failed to get lock");
                        Err(FacetError::InterceptionFailed(e))
                    }
                }
            }
            _ => {
                warn!(method = %method, "Unknown lock operation method");
                Ok(vec![])
            }
        };

        result
    }
}

#[async_trait]
impl Facet for LockFacet {
    fn facet_type(&self) -> &str {
        "locks"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    async fn on_attach(&mut self, actor_id: &str, _config: Value) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_locks_attached_total").increment(1);
        info!(actor_id = %actor_id, "🔧 LockFacet: Lock capability attached to actor");
        Ok(())
    }

    async fn on_detach(&mut self, actor_id: &str) -> Result<(), FacetError> {
        metrics::counter!("plexspaces_facet_locks_detached_total").increment(1);
        debug!(actor_id = %actor_id, "Lock capability detached from actor");
        Ok(())
    }

    async fn before_method(
        &self,
        method: &str,
        args: &[u8],
    ) -> Result<InterceptResult, FacetError> {
        // Intercept lock operations
        if method == "acquire_lock" || method == "release_lock" || method == "renew_lock"
            || method == "try_acquire_lock" || method == "get_lock"
        {
            info!(method = %method, args_len = args.len(), "🔧 LockFacet: Intercepting lock operation");
            let result = self.handle_lock_operation(method, args).await?;
            info!(method = %method, result_len = result.len(), "✅ LockFacet: Lock operation completed");
            return Ok(InterceptResult::ShortCircuit(result));
        }
        Ok(InterceptResult::Continue)
    }

    fn get_state(&self) -> Result<Value, FacetError> {
        Ok(serde_json::json!({}))
    }

    fn get_config(&self) -> Value {
        self.config_value.clone()
    }

    fn get_priority(&self) -> i32 {
        self.priority
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    // In-memory lock manager for testing
    struct TestLockManager {
        locks: Arc<tokio::sync::RwLock<std::collections::HashMap<String, Lock>>>,
    }
    
    impl TestLockManager {
        fn new() -> Self {
            Self {
                locks: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
            }
        }
    }
    
    #[async_trait]
    impl LockManager for TestLockManager {
        async fn acquire_lock(&self, _ctx: &RequestContext, options: AcquireLockOptions) -> Result<Lock, String> {
            let mut locks = self.locks.write().await;
            let lock = Lock {
                lock_key: options.lock_key.clone(),
                holder_id: options.holder_id.clone(),
                version: ulid::Ulid::new().to_string(),
                expires_at: None,
                lease_duration_secs: options.lease_duration_secs,
                last_heartbeat: None,
                metadata: options.metadata,
                locked: true,
            };
            locks.insert(options.lock_key, lock.clone());
            Ok(lock)
        }
        
        async fn renew_lock(&self, _ctx: &RequestContext, options: RenewLockOptions) -> Result<Lock, String> {
            let mut locks = self.locks.write().await;
            let lock = locks.get_mut(&options.lock_key)
                .ok_or_else(|| "Lock not found".to_string())?;
            if lock.version != options.version {
                return Err("Version mismatch".to_string());
            }
            lock.version = ulid::Ulid::new().to_string();
            lock.lease_duration_secs = options.lease_duration_secs;
            Ok(lock.clone())
        }
        
        async fn release_lock(&self, _ctx: &RequestContext, options: ReleaseLockOptions) -> Result<(), String> {
            let mut locks = self.locks.write().await;
            let lock = locks.get(&options.lock_key)
                .ok_or_else(|| "Lock not found".to_string())?;
            if lock.version != options.version {
                return Err("Version mismatch".to_string());
            }
            if options.delete_lock {
                locks.remove(&options.lock_key);
            } else {
                if let Some(lock) = locks.get_mut(&options.lock_key) {
                    lock.locked = false;
                }
            }
            Ok(())
        }
        
        async fn get_lock(&self, _ctx: &RequestContext, lock_key: &str) -> Result<Option<Lock>, String> {
            let locks = self.locks.read().await;
            Ok(locks.get(lock_key).cloned())
        }
    }

    #[tokio::test]
    async fn test_lock_facet_acquire_release() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);

        // Attach to actor
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Acquire lock
        let acquire_args = serde_json::json!({
            "lock_key": "resource-1",
            "holder_id": "actor-1",
            "lease_duration_secs": 30
        });

        let result = facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should short-circuit with lock data
        match result {
            InterceptResult::ShortCircuit(data) => {
                let lock_json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(lock_json["lock_key"], "resource-1");
                assert_eq!(lock_json["holder_id"], "actor-1");
                assert!(!lock_json["version"].as_str().unwrap().is_empty());
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Release lock
        let release_args = serde_json::json!({
            "lock_key": "resource-1",
            "holder_id": "actor-1",
            "version": "test-version",
            "delete_lock": false
        });

        // First acquire to get real version
        let acquire_result = facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        let lock_json: serde_json::Value = match acquire_result {
            InterceptResult::ShortCircuit(data) => serde_json::from_slice(&data).unwrap(),
            _ => panic!("Expected short circuit"),
        };
        let version = lock_json["version"].as_str().unwrap().to_string();

        let release_args_with_version = serde_json::json!({
            "lock_key": "resource-1",
            "holder_id": "actor-1",
            "version": version,
            "delete_lock": false
        });

        let result = facet
            .before_method("release_lock", serde_json::to_vec(&release_args_with_version).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should short-circuit with success
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["status"], "ok");
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_lock_facet_try_acquire() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);

        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Try acquire (should succeed)
        let try_acquire_args = serde_json::json!({
            "lock_key": "resource-2",
            "holder_id": "actor-2",
            "lease_duration_secs": 30
        });

        let result1 = facet
            .before_method("try_acquire_lock", serde_json::to_vec(&try_acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should acquire
        match result1 {
            InterceptResult::ShortCircuit(data) => {
                let lock_json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(lock_json["lock_key"], "resource-2");
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Try acquire again (should fail - already held)
        let result2 = facet
            .before_method("try_acquire_lock", serde_json::to_vec(&try_acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should return not acquired
        match result2 {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["acquired"], false);
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_lock_facet_get_lock() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);

        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Get lock that doesn't exist
        let get_args = serde_json::json!("resource-3");
        let result = facet
            .before_method("get_lock", serde_json::to_vec(&get_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should return not found
        match result {
            InterceptResult::ShortCircuit(data) => {
                let response: Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(response["found"], false);
            }
            _ => panic!("Expected short circuit"),
        }

        // ACT: Acquire lock, then get it
        let acquire_args = serde_json::json!({
            "lock_key": "resource-3",
            "holder_id": "actor-3",
            "lease_duration_secs": 30
        });

        facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        let result = facet
            .before_method("get_lock", serde_json::to_vec(&get_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should return lock
        match result {
            InterceptResult::ShortCircuit(data) => {
                let lock_json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(lock_json["lock_key"], "resource-3");
                assert_eq!(lock_json["holder_id"], "actor-3");
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_facet_type() {
        let lock_manager = Arc::new(TestLockManager::new());
        let facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        assert_eq!(facet.facet_type(), "locks");
    }

    #[tokio::test]
    async fn test_lock_facet_renew_lock() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Acquire lock first
        let acquire_args = serde_json::json!({
            "lock_key": "resource-4",
            "holder_id": "actor-4",
            "lease_duration_secs": 30
        });

        let acquire_result = facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        let lock_json: serde_json::Value = match acquire_result {
            InterceptResult::ShortCircuit(data) => serde_json::from_slice(&data).unwrap(),
            _ => panic!("Expected short circuit"),
        };
        let version = lock_json["version"].as_str().unwrap().to_string();

        // ACT: Renew lock
        let renew_args = serde_json::json!({
            "lock_key": "resource-4",
            "holder_id": "actor-4",
            "version": version,
            "lease_duration_secs": 60
        });

        let result = facet
            .before_method("renew_lock", serde_json::to_vec(&renew_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should return renewed lock with new version
        match result {
            InterceptResult::ShortCircuit(data) => {
                let renewed_lock_json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(renewed_lock_json["lock_key"], "resource-4");
                assert_eq!(renewed_lock_json["lease_duration_secs"], 60);
                assert_ne!(renewed_lock_json["version"].as_str().unwrap(), version.as_str()); // Version should change
            }
            _ => panic!("Expected short circuit"),
        }
    }

    #[tokio::test]
    async fn test_lock_facet_renew_lock_version_mismatch() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Try to renew with wrong version
        let renew_args = serde_json::json!({
            "lock_key": "resource-5",
            "holder_id": "actor-5",
            "version": "wrong-version",
            "lease_duration_secs": 60
        });

        let result = facet
            .before_method("renew_lock", serde_json::to_vec(&renew_args).unwrap().as_slice())
            .await;

        // ASSERT: Should return error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lock_facet_release_lock_version_mismatch() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Acquire lock first
        let acquire_args = serde_json::json!({
            "lock_key": "resource-6",
            "holder_id": "actor-6",
            "lease_duration_secs": 30
        });

        facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        // ACT: Try to release with wrong version
        let release_args = serde_json::json!({
            "lock_key": "resource-6",
            "holder_id": "actor-6",
            "version": "wrong-version",
            "delete_lock": false
        });

        let result = facet
            .before_method("release_lock", serde_json::to_vec(&release_args).unwrap().as_slice())
            .await;

        // ASSERT: Should return error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lock_facet_invalid_message_type() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Send non-lock message
        let result = facet
            .before_method("other_message", b"{}")
            .await
            .unwrap();

        // ASSERT: Should continue (not short-circuit)
        match result {
            InterceptResult::Continue => {
                // Expected - facet doesn't handle this message type
            }
            _ => panic!("Expected Continue for non-lock messages"),
        }
    }

    #[tokio::test]
    async fn test_lock_facet_invalid_json() {
        // ARRANGE
        let lock_manager = Arc::new(TestLockManager::new());
        let mut facet = LockFacet::new(lock_manager, serde_json::json!({}), 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Send invalid JSON
        let result = facet
            .before_method("acquire_lock", b"invalid json")
            .await;

        // ASSERT: Should return error
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_lock_facet_config_priority() {
        // ARRANGE: Create facet with custom priority
        let lock_manager = Arc::new(TestLockManager::new());
        let facet = LockFacet::new(lock_manager, serde_json::json!({}), 75);

        // ASSERT: Priority should be set correctly
        assert_eq!(facet.get_priority(), 75);
    }

    #[tokio::test]
    async fn test_lock_facet_default_lease_duration() {
        // ARRANGE: Create facet with default lease duration in config
        let lock_manager = Arc::new(TestLockManager::new());
        let config = serde_json::json!({
            "default_lease_secs": 60
        });
        let mut facet = LockFacet::new(lock_manager, config, 50);
        facet.on_attach("test-actor", Value::Null).await.unwrap();

        // ACT: Acquire lock without specifying lease_duration_secs
        let acquire_args = serde_json::json!({
            "lock_key": "resource-7",
            "holder_id": "actor-7"
            // No lease_duration_secs - should use default
        });

        let result = facet
            .before_method("acquire_lock", serde_json::to_vec(&acquire_args).unwrap().as_slice())
            .await
            .unwrap();

        // ASSERT: Should use default lease duration
        match result {
            InterceptResult::ShortCircuit(data) => {
                let lock_json: serde_json::Value = serde_json::from_slice(&data).unwrap();
                assert_eq!(lock_json["lease_duration_secs"], 60); // Should use default from config
            }
            _ => panic!("Expected short circuit"),
        }
    }
}

// Note: LockFacetFactory is created in actor crate (plexspaces-actor) to avoid circular dependency
// The factory needs ServiceLocator which is in core, but facet can't depend on core.
// See actor::facet_helpers for LockFacetFactory implementation.
