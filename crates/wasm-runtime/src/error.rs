// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! Error types for WASM runtime

use thiserror::Error;

/// Result type for WASM operations
pub type WasmResult<T> = Result<T, WasmError>;

/// Errors that can occur during WASM operations
#[derive(Error, Debug)]
pub enum WasmError {
    /// Module compilation failed
    #[error("Module compilation failed: {0}")]
    CompilationError(String),

    /// Module instantiation failed
    #[error("Module instantiation failed: {0}")]
    InstantiationError(String),

    /// Resource limit exceeded (memory, fuel, CPU time)
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    /// Capability denied (missing permission)
    #[error("Capability denied: {0}")]
    CapabilityDenied(String),

    /// Host function call failed
    #[error("Host function call failed: {0}")]
    HostFunctionError(String),

    /// Actor function call failed
    #[error("Actor function call failed: {0}")]
    ActorFunctionError(String),

    /// Module cache error
    #[error("Module cache error: {0}")]
    CacheError(String),

    /// Module hash mismatch (integrity check failed)
    #[error("Module hash mismatch: expected {expected}, got {actual}")]
    HashMismatch {
        /// Expected hash
        expected: String,
        /// Actual hash
        actual: String,
    },

    /// Module not found
    #[error("Module not found: {0}")]
    ModuleNotFound(String),

    /// Serialization/deserialization error
    #[error("Serialization error: {0}")]
    SerializationError(String),

    /// Instance pool exhausted
    #[error("Instance pool exhausted: {0}")]
    PoolExhausted(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigurationError(String),

    /// wasmtime runtime error
    #[error("wasmtime error: {0}")]
    WasmtimeError(#[from] anyhow::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compilation_error() {
        let error = WasmError::CompilationError("invalid syntax".to_string());
        assert!(error.to_string().contains("Module compilation failed"));
        assert!(error.to_string().contains("invalid syntax"));
    }

    #[test]
    fn test_instantiation_error() {
        let error = WasmError::InstantiationError("missing import".to_string());
        assert!(error.to_string().contains("Module instantiation failed"));
        assert!(error.to_string().contains("missing import"));
    }

    #[test]
    fn test_resource_limit_exceeded() {
        let error = WasmError::ResourceLimitExceeded("memory limit".to_string());
        assert!(error.to_string().contains("Resource limit exceeded"));
        assert!(error.to_string().contains("memory limit"));
    }

    #[test]
    fn test_capability_denied() {
        let error = WasmError::CapabilityDenied("filesystem access".to_string());
        assert!(error.to_string().contains("Capability denied"));
        assert!(error.to_string().contains("filesystem access"));
    }

    #[test]
    fn test_host_function_error() {
        let error = WasmError::HostFunctionError("log failed".to_string());
        assert!(error.to_string().contains("Host function call failed"));
        assert!(error.to_string().contains("log failed"));
    }

    #[test]
    fn test_actor_function_error() {
        let error = WasmError::ActorFunctionError("handle_message trap".to_string());
        assert!(error.to_string().contains("Actor function call failed"));
        assert!(error.to_string().contains("handle_message trap"));
    }

    #[test]
    fn test_cache_error() {
        let error = WasmError::CacheError("failed to write".to_string());
        assert!(error.to_string().contains("Module cache error"));
        assert!(error.to_string().contains("failed to write"));
    }

    #[test]
    fn test_hash_mismatch() {
        let error = WasmError::HashMismatch {
            expected: "abc123".to_string(),
            actual: "def456".to_string(),
        };
        let error_str = error.to_string();
        assert!(error_str.contains("Module hash mismatch"));
        assert!(error_str.contains("abc123"));
        assert!(error_str.contains("def456"));
    }

    #[test]
    fn test_module_not_found() {
        let error = WasmError::ModuleNotFound("calculator@1.0.0".to_string());
        assert!(error.to_string().contains("Module not found"));
        assert!(error.to_string().contains("calculator@1.0.0"));
    }

    #[test]
    fn test_serialization_error() {
        let error = WasmError::SerializationError("invalid JSON".to_string());
        assert!(error.to_string().contains("Serialization error"));
        assert!(error.to_string().contains("invalid JSON"));
    }

    #[test]
    fn test_wasmtime_error_conversion() {
        let anyhow_err = anyhow::anyhow!("wasmtime failure");
        let wasm_error: WasmError = anyhow_err.into();
        assert!(wasm_error.to_string().contains("wasmtime error"));
        assert!(wasm_error.to_string().contains("wasmtime failure"));
    }

    #[test]
    fn test_error_is_send_sync() {
        fn assert_send<T: Send>() {}
        fn assert_sync<T: Sync>() {}
        assert_send::<WasmError>();
        assert_sync::<WasmError>();
    }
}
