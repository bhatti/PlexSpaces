// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM runtime core (wasmtime integration)

use crate::{WasmConfig, WasmError, WasmResult};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use wasmtime::{Engine, Module};

/// Loaded WASM module with metadata
#[derive(Clone)]
pub struct WasmModule {
    /// Module name
    pub name: String,

    /// Module version
    pub version: String,

    /// SHA-256 hash of module bytes
    pub hash: String,

    /// Compiled wasmtime module
    pub(crate) module: Arc<Module>,

    /// Module size in bytes
    pub size_bytes: u64,
}

impl std::fmt::Debug for WasmModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmModule")
            .field("name", &self.name)
            .field("version", &self.version)
            .field("hash", &self.hash)
            .field("size_bytes", &self.size_bytes)
            .finish()
    }
}

/// PlexSpaces WASM runtime (wasmtime-based)
#[derive(Clone)]
pub struct WasmRuntime {
    /// wasmtime Engine (shared across all modules)
    engine: Engine,

    /// Module cache with LRU eviction (hash -> module)
    module_cache: Arc<RwLock<crate::module_cache::ModuleCache>>,
}

impl std::fmt::Debug for WasmRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmRuntime")
            .field("module_cache", &"<ModuleCache>")
            .finish()
    }
}

impl WasmRuntime {
    /// Create new WASM runtime with default configuration
    ///
    /// # Errors
    /// Returns error if wasmtime engine initialization fails
    pub async fn new() -> WasmResult<Self> {
        Self::with_config(WasmConfig::default()).await
    }

    /// Create new WASM runtime with custom configuration
    ///
    /// # Errors
    /// Returns error if wasmtime engine initialization fails
    pub async fn with_config(config: WasmConfig) -> WasmResult<Self> {
        // Create wasmtime engine with Component Model support
        let mut wasmtime_config = wasmtime::Config::new();

        // Enable Component Model (User Decision A2)
        #[cfg(feature = "component-model")]
        wasmtime_config.wasm_component_model(true);

        // Enable async support for tokio integration
        wasmtime_config.async_support(true);

        // Enable fuel metering for resource limits (User Decision A6)
        wasmtime_config.consume_fuel(config.limits.max_fuel > 0);

        // Set memory limits
        if config.limits.max_memory_bytes > 0 {
            wasmtime_config.max_wasm_stack(config.limits.max_stack_bytes as usize);
        }

        // Enable ahead-of-time compilation if requested
        if config.enable_aot {
            // AOT compilation happens during module loading
        }

        // Enable instance pooling for warm starts
        if config.enable_pooling {
            let mut pooling = wasmtime::PoolingAllocationConfig::default();
            pooling.max_memory_size(config.limits.max_memory_bytes as usize);
            pooling.total_memories(config.limits.max_pooled_instances);
            wasmtime_config
                .allocation_strategy(wasmtime::InstanceAllocationStrategy::Pooling(pooling));
        }

        let engine = Engine::new(&wasmtime_config)
            .map_err(|e| WasmError::CompilationError(e.to_string()))?;

        Ok(Self {
            engine,
            module_cache: Arc::new(RwLock::new(crate::module_cache::ModuleCache::new(1000))),
        })
    }

    /// Load WASM module from bytes
    ///
    /// Parses module bytes, computes hash, and caches the compiled module.
    ///
    /// # Arguments
    /// - `name`: Module name (e.g., "counter-actor")
    /// - `version`: Module version (e.g., "1.0.0")
    /// - `bytes`: WASM module bytes (Component Model format)
    ///
    /// # Returns
    /// Loaded module with computed hash
    ///
    /// # Errors
    /// - [`WasmError::CompilationError`]: Module compilation failed
    /// - [`WasmError::HashMismatch`]: Provided hash doesn't match computed hash
    pub async fn load_module(
        &self,
        name: &str,
        version: &str,
        bytes: &[u8],
    ) -> WasmResult<WasmModule> {
        let start_time = std::time::Instant::now();
        metrics::counter!("plexspaces_wasm_module_load_attempts_total").increment(1);

        // Compute SHA-256 hash
        let hash = Self::compute_hash(bytes);

        // Check cache first
        {
            let mut cache = self.module_cache.write().await;
            if let Some(cached) = cache.get(&hash) {
                let cache_hit_duration = start_time.elapsed();
                metrics::histogram!("plexspaces_wasm_module_load_duration_seconds").record(cache_hit_duration.as_secs_f64());
                metrics::counter!("plexspaces_wasm_module_cache_hits_total").increment(1);
                return Ok((*cached).clone());
            }
        }

        // Compile module
        let compile_start = std::time::Instant::now();
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| {
                metrics::counter!("plexspaces_wasm_module_load_errors_total").increment(1);
                WasmError::CompilationError(e.to_string())
            })?;
        let compile_duration = compile_start.elapsed();

        let wasm_module = WasmModule {
            name: name.to_string(),
            version: version.to_string(),
            hash: hash.clone(),
            module: Arc::new(module),
            size_bytes: bytes.len() as u64,
        };

        // Cache module (with LRU eviction if needed)
        {
            let mut cache = self.module_cache.write().await;
            cache.insert(hash, Arc::new(wasm_module.clone()));
        }

        let total_duration = start_time.elapsed();
        metrics::histogram!("plexspaces_wasm_module_load_duration_seconds").record(total_duration.as_secs_f64());
        metrics::histogram!("plexspaces_wasm_module_compile_duration_seconds").record(compile_duration.as_secs_f64());
        metrics::histogram!("plexspaces_wasm_module_size_bytes").record(bytes.len() as f64);
        metrics::counter!("plexspaces_wasm_module_load_success_total").increment(1);

        Ok(wasm_module)
    }

    /// Load WASM module from bytes with hash verification
    ///
    /// # Arguments
    /// - `name`: Module name
    /// - `version`: Module version
    /// - `bytes`: WASM module bytes
    /// - `expected_hash`: Expected SHA-256 hash (hex-encoded)
    ///
    /// # Errors
    /// Returns [`WasmError::HashMismatch`] if hash doesn't match
    pub async fn load_module_verified(
        &self,
        name: &str,
        version: &str,
        bytes: &[u8],
        expected_hash: &str,
    ) -> WasmResult<WasmModule> {
        let actual_hash = Self::compute_hash(bytes);

        if actual_hash != expected_hash {
            return Err(WasmError::HashMismatch {
                expected: expected_hash.to_string(),
                actual: actual_hash,
            });
        }

        self.load_module(name, version, bytes).await
    }

    /// Get cached module by hash
    ///
    /// # Arguments
    /// - `hash`: SHA-256 hash (hex-encoded)
    ///
    /// # Returns
    /// Cached module if found, None otherwise
    pub async fn get_module(&self, hash: &str) -> Option<WasmModule> {
        let mut cache = self.module_cache.write().await;
        cache.get(hash).map(|m| (*m).clone())
    }

    /// Compute SHA-256 hash of bytes
    ///
    /// # Arguments
    /// - `bytes`: Data to hash
    ///
    /// # Returns
    /// Hex-encoded SHA-256 hash
    pub fn compute_hash(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    /// Get number of cached modules
    pub async fn module_count(&self) -> usize {
        let cache = self.module_cache.read().await;
        cache.len()
    }

    /// Clear module cache
    pub async fn clear_cache(&self) {
        let mut cache = self.module_cache.write().await;
        cache.clear();
    }

    /// Get reference to wasmtime Engine
    ///
    /// Used by WasmInstance for creating stores
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Instantiate WASM module with actor ID and configuration
    ///
    /// High-level method that creates a WasmInstance ready to execute.
    ///
    /// # Arguments
    /// - `module`: Compiled WASM module
    /// - `actor_id`: Unique actor identifier
    /// - `initial_state`: Initial state bytes (empty for new actors)
    /// - `config`: WASM configuration (limits + capabilities)
    ///
    /// # Returns
    /// Ready-to-execute WasmInstance
    ///
    /// # Arguments
    /// * `channel_service` - Optional ChannelService for queue/topic operations
    ///
    /// # Errors
    /// Returns error if instantiation fails
    pub async fn instantiate(
        &self,
        module: WasmModule,
        actor_id: String,
        initial_state: &[u8],
        config: crate::WasmConfig,
        channel_service: Option<std::sync::Arc<dyn plexspaces_core::ChannelService>>,
    ) -> WasmResult<crate::WasmInstance> {
        use wasmtime::StoreLimitsBuilder;

        let limits = StoreLimitsBuilder::new()
            .memory_size(config.limits.max_memory_bytes as usize)
            .build();

        crate::WasmInstance::new(
            &self.engine,
            module,
            actor_id,
            initial_state,
            config.capabilities,
            limits,
            channel_service,
        )
        .await
    }

    /// Check if module is cached
    ///
    /// # Arguments
    /// - `hash`: SHA-256 hash to check
    ///
    /// # Returns
    /// true if module is in cache, false otherwise
    pub async fn contains_module(&self, hash: &str) -> bool {
        let mut cache = self.module_cache.write().await;
        cache.get(hash).is_some()
    }

    /// List all cached modules
    ///
    /// # Returns
    /// Vector of (name, version, hash) tuples
    pub async fn list_modules(&self) -> Vec<(String, String, String)> {
        let cache = self.module_cache.read().await;
        cache.iter()
    }

    /// Remove module from cache
    ///
    /// # Arguments
    /// - `hash`: SHA-256 hash of module to evict
    ///
    /// # Returns
    /// true if module was removed, false if not found
    pub async fn evict_module(&self, hash: &str) -> bool {
        let mut cache = self.module_cache.write().await;
        cache.evict(hash)
    }

    /// Resolve module by reference (name@version or hash)
    ///
    /// # Arguments
    /// - `module_ref`: Either "name@version" or SHA-256 hash
    ///
    /// # Returns
    /// Cached module if found, None otherwise
    ///
    /// # Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let runtime = WasmRuntime::new().await?;
    /// // By name@version
    /// let module1 = runtime.resolve_module("counter-actor@1.0.0").await;
    ///
    /// // By hash
    /// let module2 = runtime.resolve_module("a1b2c3d4...").await;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn resolve_module(&self, module_ref: &str) -> Option<WasmModule> {
        let mut cache = self.module_cache.write().await;

        // Try direct hash lookup first
        if let Some(module) = cache.get(module_ref) {
            return Some((*module).clone());
        }

        // Try name@version format
        // Note: ModuleCache is hash-based, so name@version lookup requires
        // maintaining a separate index or iterating
        // For now, only hash-based lookup is supported
        // TODO: Add name@version index if needed for this use case
        if let Some((name, version)) = module_ref.split_once('@') {
            tracing::debug!(
                name = %name,
                version = %version,
                "name@version lookup not yet supported, use hash instead"
            );
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ResourceLimits;

    // Simple WASM module: (module (func (export "test") (result i32) i32.const 42))
    const SIMPLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // Export section
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code section
    ];

    #[tokio::test]
    async fn test_runtime_creation_default() {
        let runtime = WasmRuntime::new().await;
        assert!(
            runtime.is_ok(),
            "Failed to create runtime: {:?}",
            runtime.err()
        );
    }

    #[tokio::test]
    async fn test_runtime_creation_with_config() {
        let config = WasmConfig {
            limits: ResourceLimits {
                max_memory_bytes: 8 * 1024 * 1024, // 8MB
                max_stack_bytes: 512 * 1024,
                max_fuel: 1_000_000,
                max_execution_time: None,
                max_table_elements: 10_000,
                max_pooled_instances: 10,
            },
            enable_pooling: false,
            enable_aot: false,
            ..Default::default()
        };

        let runtime = WasmRuntime::with_config(config).await;
        assert!(
            runtime.is_ok(),
            "Failed to create runtime with config: {:?}",
            runtime.err()
        );
    }

    #[tokio::test]
    async fn test_compute_hash() {
        let hash = WasmRuntime::compute_hash(SIMPLE_WASM);

        // Hash should be 64 hex characters (32 bytes * 2)
        assert_eq!(hash.len(), 64);

        // Hash should be deterministic
        let hash2 = WasmRuntime::compute_hash(SIMPLE_WASM);
        assert_eq!(hash, hash2);

        // Different data should produce different hash
        let hash3 = WasmRuntime::compute_hash(b"different data");
        assert_ne!(hash, hash3);
    }

    #[tokio::test]
    async fn test_load_module_simple() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        let module = runtime
            .load_module("test-module", "1.0.0", SIMPLE_WASM)
            .await;
        assert!(module.is_ok(), "Failed to load module: {:?}", module.err());

        let module = module.unwrap();
        assert_eq!(module.name, "test-module");
        assert_eq!(module.version, "1.0.0");
        assert_eq!(module.size_bytes, SIMPLE_WASM.len() as u64);
        assert_eq!(module.hash.len(), 64); // SHA-256 hex = 64 chars
    }

    #[tokio::test]
    async fn test_module_caching() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        // Load module first time
        let module1 = runtime
            .load_module("test", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        assert_eq!(runtime.module_count().await, 1);

        // Load same module again (should use cache)
        let module2 = runtime
            .load_module("test", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        // Should still only have 1 cached module (same hash)
        assert_eq!(runtime.module_count().await, 1);

        // Modules should have same hash
        assert_eq!(module1.hash, module2.hash);
    }

    #[tokio::test]
    async fn test_get_module_by_hash() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        // Load module
        let module = runtime
            .load_module("test", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        let hash = module.hash.clone();

        // Get module by hash
        let cached = runtime.get_module(&hash).await;
        assert!(cached.is_some());

        let cached = cached.unwrap();
        assert_eq!(cached.name, "test");
        assert_eq!(cached.hash, hash);

        // Try non-existent hash
        let not_found = runtime.get_module("nonexistent").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_load_module_verified_success() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        // Compute expected hash
        let expected_hash = WasmRuntime::compute_hash(SIMPLE_WASM);

        // Load with verification
        let module = runtime
            .load_module_verified("test", "1.0.0", SIMPLE_WASM, &expected_hash)
            .await;

        assert!(
            module.is_ok(),
            "Verification should succeed with correct hash"
        );
    }

    #[tokio::test]
    async fn test_load_module_verified_hash_mismatch() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        // Use wrong hash
        let wrong_hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // Load with verification
        let result = runtime
            .load_module_verified("test", "1.0.0", SIMPLE_WASM, wrong_hash)
            .await;

        assert!(result.is_err(), "Verification should fail with wrong hash");

        match result {
            Err(WasmError::HashMismatch { expected, actual }) => {
                assert_eq!(expected, wrong_hash);
                assert_ne!(actual, wrong_hash);
            }
            _ => panic!("Expected HashMismatch error"),
        }
    }

    #[tokio::test]
    async fn test_clear_cache() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        // Load module
        runtime
            .load_module("test", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Failed to load module");

        assert_eq!(runtime.module_count().await, 1);

        // Clear cache
        runtime.clear_cache().await;

        assert_eq!(runtime.module_count().await, 0);
    }

    #[tokio::test]
    async fn test_load_invalid_wasm_fails() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");

        let invalid_wasm = b"not valid wasm";

        let result = runtime.load_module("invalid", "1.0.0", invalid_wasm).await;

        assert!(result.is_err(), "Loading invalid WASM should fail");

        match result {
            Err(WasmError::CompilationError(_)) => { /* Expected */ }
            _ => panic!("Expected CompilationError"),
        }
    }
}
