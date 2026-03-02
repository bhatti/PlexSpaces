// SPDX-License-Identifier: LGPL-2.1-or-later
// Copyright (C) 2025 Shahzad A. Bhatti <bhatti@plexobject.com>
//
// This file is part of PlexSpaces.
//
// Shared helper for loading calculator_actor.wasm once and reusing across tests
// This avoids repeatedly loading the 40MB WASM file, significantly speeding up tests

use std::path::PathBuf;
use std::fs;
use std::sync::OnceLock;
use tokio::sync::Mutex;
use std::time::Duration;
use tokio::time::timeout;

/// Get the calculator WASM file path
/// Tries multiple locations:
/// 1. tests/webapps/calculator/app.wasm (preferred, auto-deploy convention)
/// 2. tests/fixtures/calculator_actor.wasm
/// 3. examples path (fallback)
pub fn get_calculator_wasm_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/webapps/calculator/app.wasm");
    if path.exists() {
        return path;
    }
    path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/fixtures/calculator_actor.wasm");
    if path.exists() {
        return path;
    }
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop(); // crates
    path.push("examples");
    path.push("simple");
    path.push("wasm_calculator");
    path.push("wasm-modules");
    path.push("calculator_actor.wasm");
    path
}

/// Get the webapps directory path for auto-deploy testing
pub fn get_webapps_path() -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("tests/webapps");
    path
}

/// Shared WASM bytes cache (loaded once, reused for all tests)
static SHARED_WASM_BYTES: OnceLock<Mutex<Option<Vec<u8>>>> = OnceLock::new();
static INIT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Get or load shared WASM bytes
/// Loads the 40MB WASM file once and caches it for all tests
/// Returns None if the file doesn't exist
pub async fn get_shared_wasm_bytes() -> Option<Vec<u8>> {
    let cache = SHARED_WASM_BYTES.get_or_init(|| Mutex::new(None));
    
    // Fast path: already loaded
    {
        let guard = cache.lock().await;
        if let Some(ref bytes) = *guard {
            return Some(bytes.clone());
        }
    }
    
    // Slow path: need to load (use lock to ensure only one thread loads)
    let _guard = INIT_LOCK.lock().unwrap();
    
    // Double-check after acquiring lock
    {
        let guard = cache.lock().await;
        if let Some(ref bytes) = *guard {
            return Some(bytes.clone());
        }
    }
    
    // Load WASM file (first time only)
    let wasm_path = get_calculator_wasm_path();
    if !wasm_path.exists() {
        eprintln!("⚠️  WASM test fixture not found at {:?}", wasm_path);
        return None;
    }
    
    eprintln!("📦 Loading WASM file (first time, ~40MB): {} (this may take a moment)", wasm_path.display());
    
    // Read file with timeout (for very slow filesystems)
    let bytes = timeout(
        Duration::from_secs(30),
        tokio::task::spawn_blocking(move || fs::read(&wasm_path))
    ).await
        .ok()
        .and_then(|r| r.ok())
        .and_then(|r| r.ok())?;
    
    eprintln!("✅ WASM file loaded: {} bytes", bytes.len());
    
    // Cache the bytes
    {
        let mut guard = cache.lock().await;
        *guard = Some(bytes.clone());
    }
    
    Some(bytes)
}

