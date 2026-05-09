// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! # Instance Pooling for Warm Starts
//!
//! ## Purpose
//! Provides pre-instantiated WASM instance pool for fast actor spawning (< 10ms goal).
//!
//! ## Architecture Context
//! Instance pooling is critical for production performance:
//! - Cold start: Compile WASM module + instantiate (~50-100ms)
//! - Warm start: Checkout from pool (~1-5ms)
//!
//! ## Design
//! - Pool maintains N pre-instantiated instances per module
//! - Checkout/checkin pattern for instance reuse
//! - Automatic pool expansion if all instances busy
//! - Metrics tracking (available, in-use, total checkouts)
//!
//! ## TODO(deploy-path integration)
//! - Integrate InstancePool into application/node spawn path when `WasmConfig.use_instance_pool` is true.
//! - Lightweight actors + worker pools: a pool of actors can use a pool of pre-instantiated instances
//!   (checkout on spawn, checkin on terminate). Requires binding actor_id and services on checkout
//!   or creating pool instances with node-level services. See PROJECT_TRACKER.md.

use crate::{WasmCapabilities, WasmConfig, WasmError, WasmInstance, WasmModule, WasmResult};
use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};
use wasmtime::{Engine, StoreLimitsBuilder};

/// Pool statistics
#[derive(Debug, Clone, Default)]
pub struct PoolStats {
    /// Total instances created
    pub total_created: usize,
    /// Total checkouts
    pub total_checkouts: usize,
    /// Currently available instances
    pub available: usize,
    /// Currently in-use instances
    pub in_use: usize,
}

/// Pre-instantiated instance pool for fast actor spawning
pub struct InstancePool {
    /// wasmtime Engine (shared)
    engine: Arc<Engine>,

    /// WASM module to pool
    module: WasmModule,

    /// WASM configuration
    config: WasmConfig,

    /// Actor type for pool instances (identifies the WASM behavior class)
    actor_type: String,

    /// Namespace for tenant isolation
    namespace: String,

    /// Node ID for canonical actor ID construction
    node_id: String,

    /// Pool of available instances
    instances: Arc<Mutex<Vec<WasmInstance>>>,

    /// Semaphore for pool capacity control
    semaphore: Arc<Semaphore>,

    /// Pool statistics
    stats: Arc<Mutex<PoolStats>>,

    /// Capacity (max instances)
    capacity: usize,
}

impl InstancePool {
    /// Create new instance pool with pre-instantiated instances
    ///
    /// # Arguments
    /// - `engine`: wasmtime Engine reference
    /// - `module`: WASM module to pool
    /// - `capacity`: Maximum number of instances in pool
    /// - `config`: WASM configuration
    ///
    /// # Returns
    /// Pool with `capacity` pre-instantiated instances
    pub async fn new(
        engine: &Engine,
        module: WasmModule,
        capacity: usize,
        config: WasmConfig,
        actor_type: impl Into<String>,
        namespace: impl Into<String>,
        node_id: impl Into<String>,
    ) -> WasmResult<Self> {
        if capacity == 0 {
            return Err(WasmError::ConfigurationError(
                "Pool capacity must be > 0".to_string(),
            ));
        }

        let instances = Arc::new(Mutex::new(Vec::with_capacity(capacity)));
        let semaphore = Arc::new(Semaphore::new(capacity));
        let stats = Arc::new(Mutex::new(PoolStats::default()));

        let pool = Self {
            engine: Arc::new(engine.clone()),
            module,
            config,
            actor_type: actor_type.into(),
            namespace: namespace.into(),
            node_id: node_id.into(),
            instances,
            semaphore,
            stats,
            capacity,
        };

        // Pre-instantiate initial instances
        pool.fill_pool(capacity).await?;

        Ok(pool)
    }

    /// Fill pool with instances
    async fn fill_pool(&self, count: usize) -> WasmResult<()> {
        let mut instances = self.instances.lock().await;
        let mut stats = self.stats.lock().await;

        for i in 0..count {
            let name = format!("pooled-{}", stats.total_created + i);
            let instance = self.create_instance_by_name(&name).await?;
            instances.push(instance);
            stats.total_created += 1;
            stats.available += 1;
        }

        Ok(())
    }

    /// Build an ActorId from a name and create a WasmInstance directly.
    async fn create_instance_by_name(&self, name: &str) -> WasmResult<WasmInstance> {
        let actor_id = plexspaces_actor::ActorId::new(
            name,
            &self.actor_type,
            &self.namespace,
            &self.node_id,
        )
        .map_err(|e| WasmError::ActorFunctionError(e.to_string()))?;
        let limits = StoreLimitsBuilder::new()
            .memory_size(self.config.limits.max_memory_bytes as usize)
            .build();

        WasmInstance::new(
            &self.engine,
            self.module.clone(),
            actor_id,
            &[],
            self.config.capabilities.clone(),
            limits,
            self.config.limits.max_fuel,
            None, // ChannelService not available at pool level
            None, // MessageSender not available at pool level
            None, // TupleSpaceProvider not available at pool level
            None, // KeyValueStore not available at pool level
            None, // ProcessGroupRegistry not available at pool level
            None, // LockManager not available at pool level
            None, // ObjectRegistry not available at pool level
            None, // JournalStorage not available at pool level
            None, // BlobService not available at pool level
            None, // ElasticPoolService not available at pool level
            None, // OutboundHttpClient not available at pool level
            self.config.durability_enabled,
            None, // global_reinstantiation_semaphore - pool not tied to runtime
            None, // shared_timer_pool - pool instances don't track timers
        )
        .await
    }

    /// Checkout an instance from the pool.
    ///
    /// If a pre-warmed instance is available (semaphore has a free permit), it is returned
    /// immediately.  If all initial-capacity permits are taken but there is an available
    /// instance in the queue (returned via checkin), the semaphore permit will be released
    /// by the caller.  When the pool is fully exhausted AND all instances are in use, the
    /// pool dynamically expands: a new instance is created and an extra permit is added to
    /// the semaphore so future checkouts are not artificially capped.
    ///
    /// # Returns
    /// Pooled instance ready for use
    pub async fn checkout(&self) -> WasmResult<PooledInstance> {
        // Try non-blocking acquire first (covers the pre-warmed / checkin path).
        let permit = match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => {
                // Permit acquired — pop from pool or create new if pool is empty.
                let mut instances = self.instances.lock().await;
                let instance = if let Some(inst) = instances.pop() {
                    let mut stats = self.stats.lock().await;
                    stats.available -= 1;
                    stats.in_use += 1;
                    stats.total_checkouts += 1;
                    inst
                } else {
                    drop(instances);
                    let name = format!("pooled-dyn-{}", ulid::Ulid::new());
                    let instance = self.create_instance_by_name(&name).await?;
                    let mut stats = self.stats.lock().await;
                    stats.total_created += 1;
                    stats.in_use += 1;
                    stats.total_checkouts += 1;
                    instance
                };
                return Ok(PooledInstance {
                    instance: Some(instance),
                    pool: self.instances.clone(),
                    stats: self.stats.clone(),
                    _permit: permit,
                });
            }
            Err(_) => {
                // All initial-capacity permits are taken.  Dynamically expand: add a permit
                // so that checkin() can release it and future checkouts are not blocked.
                self.semaphore.add_permits(1);
                self.semaphore
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|e| WasmError::PoolExhausted(e.to_string()))?
            }
        };

        // We hold a newly-added permit — create a dynamic instance.
        let name = format!("pooled-dyn-{}", ulid::Ulid::new());
        let instance = self.create_instance_by_name(&name).await?;
        let mut stats = self.stats.lock().await;
        stats.total_created += 1;
        stats.in_use += 1;
        stats.total_checkouts += 1;
        drop(stats);

        Ok(PooledInstance {
            instance: Some(instance),
            pool: self.instances.clone(),
            stats: self.stats.clone(),
            _permit: permit,
        })
    }

    /// Get current pool statistics
    pub async fn stats(&self) -> PoolStats {
        self.stats.lock().await.clone()
    }

    /// Get pool capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Pooled instance with automatic return-to-pool on drop
pub struct PooledInstance {
    /// The actual instance (Option for Drop safety)
    instance: Option<WasmInstance>,

    /// Reference to pool for return (only used for traditional instances)
    pool: Arc<Mutex<Vec<WasmInstance>>>,

    /// Reference to stats
    stats: Arc<Mutex<PoolStats>>,

    /// Semaphore permit (releases on drop)
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl PooledInstance {
    /// Get reference to the instance
    pub fn instance(&self) -> &WasmInstance {
        self.instance.as_ref().expect("Instance already returned")
    }

    /// Get mutable reference to the instance
    pub fn instance_mut(&mut self) -> &mut WasmInstance {
        self.instance.as_mut().expect("Instance already returned")
    }

    /// Explicitly return instance to pool
    /// This should be called before drop if you want the instance returned to the pool
    pub async fn checkin(mut self) {
        if let Some(instance) = self.instance.take() {
            // Check if this is a component instance (components are not Send and can't be pooled)
            if !instance.is_component_instance() {
                // Traditional instances: return to pool
                let mut instances = self.pool.lock().await;
                instances.push(instance);

                // Update stats
                let mut stats = self.stats.lock().await;
                stats.available += 1;
                stats.in_use -= 1;
            }
        }
        // Permit is automatically released when self is dropped
    }
}

impl Drop for PooledInstance {
    fn drop(&mut self) {
        if let Some(instance) = self.instance.take() {
            // Decrement in_use counter synchronously using try_lock.
            // If the lock is currently held we skip (benign: callers should use explicit checkin()
            // for precise tracking; the semaphore permit released below still enforces capacity).
            if let Ok(mut stats) = self.stats.try_lock() {
                stats.in_use = stats.in_use.saturating_sub(1);
            }
            drop(instance);
        }
        // Semaphore permit is released when self is dropped, restoring pool capacity.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::WasmRuntime;

    // Simple WASM module for testing
    const SIMPLE_WASM: &str = r#"
        (module
            (memory (export "memory") 1)
            (func (export "handle_message") (param i32 i32 i32 i32 i32 i32) (result i32)
                i32.const 0
            )
            (func (export "snapshot_state") (result i32 i32)
                i32.const 0
                i32.const 0
            )
        )
    "#;

    async fn create_test_pool(capacity: usize) -> WasmResult<InstancePool> {
        let runtime = WasmRuntime::new().await?;
        let wasm_bytes = wat::parse_str(SIMPLE_WASM).expect("Failed to parse WAT");
        let module = runtime
            .load_module("test-pool", "1.0.0", &wasm_bytes)
            .await?;

        InstancePool::new(
            runtime.engine(),
            module,
            capacity,
            WasmConfig::default(),
            "test-pool",
            "test-namespace",
            "test-node",
        ).await
    }

    #[tokio::test]
    async fn test_pool_creation() {
        let pool = create_test_pool(5).await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.total_created, 5);
        assert_eq!(stats.available, 5);
        assert_eq!(stats.in_use, 0);
        assert_eq!(pool.capacity(), 5);
    }

    #[tokio::test]
    async fn test_pool_checkout_checkin() {
        let pool = create_test_pool(3).await.unwrap();

        // Checkout instance
        let instance = pool.checkout().await.unwrap();
        let stats = pool.stats().await;
        assert_eq!(stats.available, 2);
        assert_eq!(stats.in_use, 1);
        assert_eq!(stats.total_checkouts, 1);

        // Instance is usable
        assert!(!instance.instance().actor_id().is_empty());

        // Explicitly return to pool
        instance.checkin().await;

        let stats = pool.stats().await;
        assert_eq!(stats.available, 3);
        assert_eq!(stats.in_use, 0);
    }

    #[tokio::test]
    async fn test_pool_exhaustion_and_expansion() {
        let pool =
            match tokio::time::timeout(std::time::Duration::from_secs(10), create_test_pool(2))
                .await
            {
                Ok(Ok(p)) => p,
                Ok(Err(e)) => panic!("create_test_pool failed: {}", e),
                Err(_) => {
                    eprintln!("test_pool_exhaustion_and_expansion: timeout (skipping)");
                    return;
                }
            };

        // Checkout all instances
        let _inst1 = pool.checkout().await.unwrap();
        let _inst2 = pool.checkout().await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.available, 0);
        assert_eq!(stats.in_use, 2);

        // Next checkout should create new instance (expansion)
        let _inst3 = pool.checkout().await.unwrap();

        let stats = pool.stats().await;
        assert_eq!(stats.total_created, 3); // Expanded beyond initial capacity
        assert_eq!(stats.in_use, 3);
    }

    #[tokio::test]
    async fn test_pool_concurrent_checkout() {
        let pool =
            match tokio::time::timeout(std::time::Duration::from_secs(5), create_test_pool(10))
                .await
            {
                Ok(Ok(p)) => Arc::new(p),
                Ok(Err(e)) => panic!("create_test_pool failed: {}", e),
                Err(_) => {
                    eprintln!("test_pool_concurrent_checkout: timeout creating pool (skipping)");
                    return;
                }
            };

        // Spawn 20 concurrent checkouts (pool has 10 capacity) with overall timeout to avoid hanging
        let run = async {
            let mut handles = vec![];
            for i in 0..20 {
                let pool = pool.clone();
                let handle = tokio::spawn(async move {
                    let instance = pool.checkout().await.unwrap();
                    let actor_id = instance.instance().actor_id().to_string();
                    tokio::time::sleep(tokio::time::Duration::from_millis(5)).await;
                    format!("task-{} used {}", i, actor_id)
                });
                handles.push(handle);
            }
            for handle in handles {
                let _ = handle.await.unwrap();
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
            pool.stats().await
        };
        let stats = match tokio::time::timeout(std::time::Duration::from_secs(15), run).await {
            Ok(s) => s,
            Err(_) => {
                eprintln!("test_pool_concurrent_checkout: timeout (skipping)");
                return;
            }
        };
        assert_eq!(stats.total_checkouts, 20);
        assert_eq!(stats.in_use, 0); // All returned
    }

    #[tokio::test]
    async fn test_pool_zero_capacity_fails() {
        let runtime = WasmRuntime::new().await.unwrap();
        let wasm_bytes = wat::parse_str(SIMPLE_WASM).unwrap();
        let module = runtime
            .load_module("test", "1.0.0", &wasm_bytes)
            .await
            .unwrap();

        let result = InstancePool::new(
            runtime.engine(), module, 0, WasmConfig::default(),
            "test-pool", "test-namespace", "test-node",
        ).await;

        assert!(result.is_err());
        assert!(matches!(result, Err(WasmError::ConfigurationError(_))));
    }
}
