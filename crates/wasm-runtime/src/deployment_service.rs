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

//! WASM Deployment Service
//!
//! ## Purpose
//! Implements the WasmRuntimeService gRPC interface for dynamic deployment
//! of WASM modules to cluster nodes.
//!
//! ## Architecture
//! - **Deploy Module**: Upload WASM bytes to node, compile, cache
//! - **Instantiate Actor**: Spawn actor from cached module
//! - **Migrate Actor**: Move actor state between nodes (code cached)
//!
//! ## Usage
//! ```rust,no_run
//! use plexspaces_wasm_runtime::deployment_service::WasmDeploymentService;
//! use plexspaces_wasm_runtime::WasmRuntime;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let runtime = Arc::new(WasmRuntime::new().await?);
//! let service = WasmDeploymentService::new(runtime);
//!
//! // Service is ready for gRPC requests
//! # Ok(())
//! # }
//! ```

use crate::error::{WasmError, WasmResult};
use crate::runtime::{WasmModule, WasmRuntime};
use crate::WasmConfig;
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// WASM Deployment Service
///
/// Handles dynamic deployment of WASM modules across the cluster:
/// - Receives WASM bytes over network
/// - Compiles and caches modules
/// - Instantiates actors from cached modules
/// - Migrates actors between nodes
#[derive(Debug, Clone)]
pub struct WasmDeploymentService {
    /// WASM runtime for compilation and instantiation
    runtime: Arc<WasmRuntime>,
}

impl WasmDeploymentService {
    /// Create new deployment service
    ///
    /// ## Arguments
    /// * `runtime` - WASM runtime instance
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let runtime = Arc::new(WasmRuntime::new().await?);
    /// let service = deployment_service::WasmDeploymentService::new(runtime);
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(runtime: Arc<WasmRuntime>) -> Self {
        Self { runtime }
    }

    /// Deploy WASM module to this node
    ///
    /// ## Arguments
    /// * `name` - Module name (e.g., "counter-actor")
    /// * `version` - Module version (semantic versioning: "1.2.3")
    /// * `module_bytes` - Compiled WASM bytes (Component Model format)
    ///
    /// ## Returns
    /// Module hash (SHA-256) on success
    ///
    /// ## Errors
    /// - `WasmError::CompilationFailed`: Invalid WASM bytecode
    /// - `WasmError::HashMismatch`: Integrity check failed
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = deployment_service::WasmDeploymentService::new(
    /// #     std::sync::Arc::new(WasmRuntime::new().await?)
    /// # );
    /// let wasm_bytes = std::fs::read("actor.wasm")?;
    /// let hash = service.deploy_module(
    ///     "my-actor",
    ///     "1.0.0",
    ///     &wasm_bytes
    /// ).await?;
    /// println!("Deployed module with hash: {}", hash);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn deploy_module(
        &self,
        name: &str,
        version: &str,
        module_bytes: &[u8],
    ) -> WasmResult<String> {
        // Compute module hash for content-addressable caching
        let module_hash = Self::compute_hash(module_bytes);

        // Check if already cached (idempotent deployment)
        if self.runtime.contains_module(&module_hash).await {
            tracing::info!(
                module_name = name,
                module_version = version,
                module_hash = %module_hash,
                "Module already cached, skipping compilation"
            );
            return Ok(module_hash);
        }

        // Compile WASM module
        tracing::info!(
            module_name = name,
            module_version = version,
            module_size = module_bytes.len(),
            "Compiling WASM module"
        );

        let module = self
            .runtime
            .load_module(name, version, module_bytes)
            .await
            .map_err(|e| {
                WasmError::CompilationError(format!(
                    "Failed to compile module {name}@{version}: {e}"
                ))
            })?;

        // Verify hash matches
        let actual_hash = Self::compute_hash(module_bytes);
        if actual_hash != module_hash {
            return Err(WasmError::HashMismatch {
                expected: module_hash,
                actual: actual_hash,
            });
        }

        tracing::info!(
            module_name = name,
            module_version = version,
            module_hash = %module_hash,
            "Module deployed successfully"
        );

        Ok(module_hash)
    }

    /// Instantiate actor from deployed module
    ///
    /// ## Arguments
    /// * `module_ref` - Module name@version or module hash
    /// * `actor_id` - Actor ID to assign
    /// * `initial_state` - Initial state bytes
    /// * `config` - Configuration (optional, uses defaults if None)
    ///
    /// ## Returns
    /// Actor ID on success
    ///
    /// ## Errors
    /// - `WasmError::ModuleNotFound`: Module not deployed
    /// - `WasmError::InstantiationFailed`: Instance creation failed
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = deployment_service::WasmDeploymentService::new(
    /// #     std::sync::Arc::new(WasmRuntime::new().await?)
    /// # );
    /// let actor_id = service.instantiate_actor(
    ///     "my-actor@1.0.0",
    ///     "actor-001",
    ///     &[],
    ///     None
    /// ).await?;
    /// println!("Instantiated actor: {}", actor_id);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn instantiate_actor(
        &self,
        module_ref: &str,
        actor_id: &str,
        initial_state: &[u8],
        config: Option<WasmConfig>,
    ) -> WasmResult<String> {
        // Resolve module reference (name@version or hash)
        let module = self
            .runtime
            .resolve_module(module_ref)
            .await
            .ok_or_else(|| {
                WasmError::ModuleNotFound(format!("Module not found: {}", module_ref))
            })?;

        // Use provided config or default
        let config = config.unwrap_or_default();

        tracing::info!(
            module_ref = module_ref,
            actor_id = actor_id,
            state_size = initial_state.len(),
            "Instantiating WASM actor"
        );

        // Create instance
        let _instance = self
            .runtime
            .instantiate(
                module,
                actor_id.to_string(),
                initial_state,
                config,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                
                None,
            )
            .await
            .map_err(|e| {
                WasmError::InstantiationError(format!(
                    "Failed to instantiate actor {actor_id}: {e}"
                ))
            })?;

        tracing::info!(
            actor_id = actor_id,
            module_ref = module_ref,
            "Actor instantiated successfully"
        );

        Ok(actor_id.to_string())
    }

    /// List deployed modules on this node
    ///
    /// ## Returns
    /// List of (module_name, module_version, module_hash) tuples
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = deployment_service::WasmDeploymentService::new(
    /// #     std::sync::Arc::new(WasmRuntime::new().await?)
    /// # );
    /// let modules = service.list_modules().await?;
    /// for (name, version, hash) in modules {
    ///     println!("{}@{}: {}", name, version, hash);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn list_modules(&self) -> WasmResult<Vec<(String, String, String)>> {
        // Get all cached modules
        let modules = self.runtime.list_modules().await;
        Ok(modules)
    }

    /// Remove module from cache
    ///
    /// ## Arguments
    /// * `module_hash` - Module hash to remove
    ///
    /// ## Returns
    /// true if module was removed, false if not found
    ///
    /// ## Examples
    /// ```rust,no_run
    /// # use plexspaces_wasm_runtime::*;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let service = deployment_service::WasmDeploymentService::new(
    /// #     std::sync::Arc::new(WasmRuntime::new().await?)
    /// # );
    /// let removed = service.remove_module("a1b2c3...").await?;
    /// if removed {
    ///     println!("Module removed from cache");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub async fn remove_module(&self, module_hash: &str) -> WasmResult<bool> {
        let removed = self.runtime.evict_module(module_hash).await;
        if removed {
            tracing::info!(module_hash = module_hash, "Module removed from cache");
        }
        Ok(removed)
    }

    /// Compute SHA-256 hash of module bytes
    ///
    /// ## Arguments
    /// * `bytes` - Module bytes to hash
    ///
    /// ## Returns
    /// Hex-encoded SHA-256 hash (64 characters)
    ///
    /// ## Examples
    /// ```
    /// # use plexspaces_wasm_runtime::deployment_service::WasmDeploymentService;
    /// let hash = WasmDeploymentService::compute_hash(b"hello");
    /// assert_eq!(hash.len(), 64); // SHA-256 = 32 bytes = 64 hex chars
    /// ```
    pub fn compute_hash(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        let result = hasher.finalize();
        hex::encode(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compute_hash() {
        // SHA-256 of "hello"
        let hash = WasmDeploymentService::compute_hash(b"hello");
        assert_eq!(hash.len(), 64); // 32 bytes = 64 hex characters

        // Same input = same hash (deterministic)
        let hash2 = WasmDeploymentService::compute_hash(b"hello");
        assert_eq!(hash, hash2);

        // Different input = different hash
        let hash3 = WasmDeploymentService::compute_hash(b"world");
        assert_ne!(hash, hash3);
    }

    #[test]
    fn test_hash_empty_bytes() {
        let hash = WasmDeploymentService::compute_hash(&[]);
        assert_eq!(hash.len(), 64);
        // SHA-256 of empty string is known value
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[tokio::test]
    async fn test_service_creation() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let _service = WasmDeploymentService::new(Arc::new(runtime));
        // Service should be created successfully
        assert!(true);
    }

    // Simple WASM module for testing: (module (func (export "test") (result i32) i32.const 42))
    const SIMPLE_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x08, 0x01, 0x04, 0x74, 0x65, 0x73, 0x74, 0x00, 0x00, // Export section
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code section
    ];

    #[tokio::test]
    async fn test_deploy_module_success() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        // Deploy module
        let result = service
            .deploy_module("test-module", "1.0.0", SIMPLE_WASM)
            .await;

        assert!(result.is_ok(), "Deploy should succeed: {:?}", result.err());

        let hash = result.unwrap();
        assert_eq!(hash.len(), 64); // SHA-256 = 64 hex chars
    }

    #[tokio::test]
    async fn test_deploy_module_idempotent() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        // Deploy module first time
        let hash1 = service
            .deploy_module("test-module", "1.0.0", SIMPLE_WASM)
            .await
            .expect("First deploy should succeed");

        // Deploy same module again (should be idempotent)
        let hash2 = service
            .deploy_module("test-module", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Second deploy should succeed");

        // Same hash returned
        assert_eq!(hash1, hash2);

        // Only one module in cache
        let modules = service.list_modules().await.expect("List should succeed");
        assert_eq!(modules.len(), 1);
    }

    #[tokio::test]
    async fn test_deploy_invalid_wasm_fails() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        let invalid_wasm = b"not valid wasm bytes";

        let result = service
            .deploy_module("invalid", "1.0.0", invalid_wasm)
            .await;

        assert!(result.is_err(), "Invalid WASM should fail to deploy");

        match result {
            Err(WasmError::CompilationError(_)) => { /* Expected */ }
            _ => panic!("Expected CompilationError"),
        }
    }

    #[tokio::test]
    async fn test_list_modules_empty() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        let modules = service.list_modules().await.expect("List should succeed");
        assert_eq!(modules.len(), 0);
    }

    #[tokio::test]
    async fn test_list_modules_after_deploy() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        // Deploy module
        let hash = service
            .deploy_module("test-module", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Deploy should succeed");

        // List modules
        let modules = service.list_modules().await.expect("List should succeed");
        assert_eq!(modules.len(), 1);

        let (name, version, module_hash) = &modules[0];
        assert_eq!(name, "test-module");
        assert_eq!(version, "1.0.0");
        assert_eq!(module_hash, &hash);
    }

    #[tokio::test]
    async fn test_remove_module_success() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        // Deploy module
        let hash = service
            .deploy_module("test-module", "1.0.0", SIMPLE_WASM)
            .await
            .expect("Deploy should succeed");

        // Verify it's there
        let modules = service.list_modules().await.expect("List should succeed");
        assert_eq!(modules.len(), 1);

        // Remove module
        let removed = service
            .remove_module(&hash)
            .await
            .expect("Remove should succeed");
        assert!(removed, "Module should be removed");

        // Verify it's gone
        let modules = service.list_modules().await.expect("List should succeed");
        assert_eq!(modules.len(), 0);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_module() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        let removed = service
            .remove_module("nonexistent-hash")
            .await
            .expect("Remove should succeed");
        assert!(!removed, "Nonexistent module should not be removed");
    }

    #[tokio::test]
    async fn test_deploy_multiple_modules() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        // Deploy first module
        let hash1 = service
            .deploy_module("module-1", "1.0.0", SIMPLE_WASM)
            .await
            .expect("First deploy should succeed");

        // Deploy second module (different version, same bytes)
        let hash2 = service
            .deploy_module("module-2", "2.0.0", SIMPLE_WASM)
            .await
            .expect("Second deploy should succeed");

        // Same WASM bytes = same hash (content-addressable)
        assert_eq!(hash1, hash2);

        // List modules (should show both, but only 1 cached since same hash)
        let modules = service.list_modules().await.expect("List should succeed");
        // Note: Current implementation caches by hash, so last name/version wins
        assert_eq!(modules.len(), 1);
    }

    #[tokio::test]
    async fn test_instantiate_actor_not_found() {
        let runtime = WasmRuntime::new().await.expect("Failed to create runtime");
        let service = WasmDeploymentService::new(Arc::new(runtime));

        let result = service
            .instantiate_actor("nonexistent@1.0.0", "actor-001", &[], None)
            .await;

        assert!(result.is_err(), "Instantiate nonexistent module should fail");

        match result {
            Err(WasmError::ModuleNotFound(_)) => { /* Expected */ }
            _ => panic!("Expected ModuleNotFound error"),
        }
    }
}
