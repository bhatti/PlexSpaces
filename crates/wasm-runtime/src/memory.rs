// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.

//! WASM memory access utilities for reading/writing data between Rust and WASM

use crate::{WasmError, WasmResult};
use wasmtime::{Memory, Store};

/// Read bytes from WASM linear memory
///
/// ## Arguments
/// * `memory` - WASM memory instance
/// * `store` - Wasmtime store
/// * `ptr` - Pointer in WASM memory
/// * `len` - Length of bytes to read
///
/// ## Returns
/// Bytes read from WASM memory
///
/// ## Errors
/// Returns error if pointer is out of bounds
pub fn read_bytes<T>(
    memory: &Memory,
    store: &mut Store<T>,
    ptr: i32,
    len: i32,
) -> WasmResult<Vec<u8>> {
    if ptr < 0 || len < 0 {
        return Err(WasmError::ActorFunctionError(format!(
            "Invalid pointer or length: ptr={}, len={}",
            ptr, len
        )));
    }

    let ptr = ptr as usize;
    let len = len as usize;

    // Get memory data
    let data = memory.data(store);

    // Check bounds
    if ptr + len > data.len() {
        return Err(WasmError::ActorFunctionError(format!(
            "Memory access out of bounds: ptr={}, len={}, memory_size={}",
            ptr, len, data.len()
        )));
    }

    // Read bytes
    Ok(data[ptr..ptr + len].to_vec())
}

/// Read string from WASM linear memory (UTF-8)
///
/// ## Arguments
/// * `memory` - WASM memory instance
/// * `store` - Wasmtime store
/// * `ptr` - Pointer in WASM memory
/// * `len` - Length of string bytes
///
/// ## Returns
/// String read from WASM memory
///
/// ## Errors
/// Returns error if pointer is out of bounds or string is not valid UTF-8
pub fn read_string<T>(
    memory: &Memory,
    store: &mut Store<T>,
    ptr: i32,
    len: i32,
) -> WasmResult<String> {
    let bytes = read_bytes(memory, store, ptr, len)?;
    String::from_utf8(bytes).map_err(|e| {
        WasmError::ActorFunctionError(format!("Invalid UTF-8 string: {}", e))
    })
}

/// Write bytes to WASM linear memory
///
/// ## Arguments
/// * `memory` - WASM memory instance
/// * `store` - Wasmtime store
/// * `ptr` - Pointer in WASM memory
/// * `bytes` - Bytes to write
///
/// ## Errors
/// Returns error if pointer is out of bounds
pub fn write_bytes<T>(
    memory: &Memory,
    mut store: &mut Store<T>,
    ptr: i32,
    bytes: &[u8],
) -> WasmResult<()> {
    if ptr < 0 {
        return Err(WasmError::ActorFunctionError(format!(
            "Invalid pointer: ptr={}",
            ptr
        )));
    }

    let ptr = ptr as usize;
    let len = bytes.len();

    // Get mutable memory data
    let data = memory.data_mut(&mut store);

    // Check bounds
    if ptr + len > data.len() {
        return Err(WasmError::ActorFunctionError(format!(
            "Memory write out of bounds: ptr={}, len={}, memory_size={}",
            ptr, len, data.len()
        )));
    }

    // Write bytes
    data[ptr..ptr + len].copy_from_slice(bytes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use wasmtime::{Config, Engine, Linker, Module};

    // Helper to create test memory
    async fn create_test_memory<T: Default>() -> WasmResult<(Engine, Store<T>, Memory)> {
        // Create engine without async support for tests (simpler, faster)
        let mut config = Config::default();
        config.async_support(false); // Disable async for test helper
        let engine = Engine::new(&config)?;
        let mut store = Store::new(&engine, T::default());

        // Create module with exported memory
        let wat = r#"
            (module
                (memory (export "memory") 1)
            )
        "#;
        let wasm_bytes = wat::parse_str(wat).map_err(|e| {
            WasmError::CompilationError(format!("WAT parse failed: {}", e))
        })?;
        let module = Module::new(&engine, &wasm_bytes)?;

        let mut linker = Linker::new(&engine);
        // Use synchronous instantiate since we disabled async support for tests
        let instance = linker.instantiate(&mut store, &module)?;
        let memory = instance.get_memory(&mut store, "memory")
            .ok_or_else(|| WasmError::ActorFunctionError("Memory not found".to_string()))?;

        Ok((engine, store, memory))
    }

    #[tokio::test]
    async fn test_read_bytes() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Write some test data
        {
            let data = memory.data_mut(&mut store);
            data[0] = 0x42;
            data[1] = 0x43;
            data[2] = 0x44;
        }

        // Read it back
        let bytes = read_bytes(&memory, &mut store, 0, 3).unwrap();
        assert_eq!(bytes, vec![0x42, 0x43, 0x44]);
    }

    #[tokio::test]
    async fn test_read_string() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Write UTF-8 string "hello"
        {
            let data = memory.data_mut(&mut store);
            data[0..5].copy_from_slice(b"hello");
        }

        // Read it back
        let s = read_string(&memory, &mut store, 0, 5).unwrap();
        assert_eq!(s, "hello");
    }

    #[tokio::test]
    async fn test_write_bytes() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Write bytes
        write_bytes(&memory, &mut store, 0, b"test").unwrap();

        // Read them back
        let data = memory.data(&store);
        assert_eq!(&data[0..4], b"test");
    }

    #[tokio::test]
    async fn test_read_out_of_bounds() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Try to read beyond memory size (1 page = 64KB)
        let result = read_bytes(&memory, &mut store, 70000, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));
    }

    #[tokio::test]
    async fn test_write_out_of_bounds() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Try to write beyond memory size
        let result = write_bytes(&memory, &mut store, 70000, b"test");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("out of bounds"));
    }

    #[tokio::test]
    async fn test_invalid_utf8() {
        let (_engine, mut store, memory) = create_test_memory::<()>().await.unwrap();

        // Write invalid UTF-8
        {
            let data = memory.data_mut(&mut store);
            data[0] = 0xFF; // Invalid UTF-8
            data[1] = 0xFE;
        }

        // Try to read as string
        let result = read_string(&memory, &mut store, 0, 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid UTF-8"));
    }
}
